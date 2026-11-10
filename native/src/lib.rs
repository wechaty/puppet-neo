use anyhow::{anyhow, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use napi::{bindgen_prelude::AsyncTask, Env, Task};
use napi_derive::napi;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use wechat_protocol::{
    wire, AccountPushConnection, DeviceProfile, LoginAttempt, LoginStatus, NativeClient,
    NativeOptions, TextSendOptions,
};

const DEVICE_NAME: &str = "Puppet Neo";
/// Error text TypeScript receives when WeChat no longer accepts this session.
const SESSION_ENDED: &str = "Neo session ended by WeChat";
/// Service errors that carry WeChat's code for rejected credentials (-13).
const AUTHENTICATED_REPLIES: [&str; 3] = [
    "Native envelope returned ",
    "Heartbeat returned ",
    "Sync service returned ",
];

/// WeChat rejected the session's credentials and rejected one renewal.
#[derive(Debug)]
struct SessionEnded;

impl std::fmt::Display for SessionEnded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(SESSION_ENDED)
    }
}

impl std::error::Error for SessionEnded {}

#[derive(Default)]
struct Session {
    attempt: Option<LoginAttempt>,
    /// Owns the accepted credentials after login; account requests share it.
    account: Option<NativeClient>,
    incoming: Option<AccountPushConnection>,
    received_cursor: Vec<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TextMessage {
    id: String,
    from: String,
    to: String,
    text: String,
    timestamp: u32,
}

#[derive(Default, Serialize)]
struct SelfProfile {
    nickname: Option<String>,
    alias: Option<String>,
    avatar: Option<String>,
}

enum Operation {
    StartLogin,
    PollLogin,
    Heartbeat,
    PollIncoming,
    Sync { scene: u32 },
    SendText { destination: String, text: String },
    LookupSelf,
    Logout,
    Close,
}

#[napi]
#[derive(Default)]
pub struct NeoSession {
    session: Arc<Mutex<Session>>,
}

#[napi]
impl NeoSession {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    #[napi]
    pub fn start_login(&self) -> AsyncTask<NeoTask> {
        self.task(Operation::StartLogin)
    }

    #[napi]
    pub fn poll_login(&self) -> AsyncTask<NeoTask> {
        self.task(Operation::PollLogin)
    }

    #[napi]
    pub fn heartbeat(&self) -> AsyncTask<NeoTask> {
        self.task(Operation::Heartbeat)
    }

    #[napi]
    pub fn sync(&self, scene: u32) -> AsyncTask<NeoTask> {
        self.task(Operation::Sync { scene })
    }

    #[napi]
    pub fn poll_incoming(&self) -> AsyncTask<NeoTask> {
        self.task(Operation::PollIncoming)
    }

    #[napi]
    pub fn send_text(&self, destination: String, text: String) -> AsyncTask<NeoTask> {
        self.task(Operation::SendText { destination, text })
    }

    #[napi]
    pub fn lookup_self(&self) -> AsyncTask<NeoTask> {
        self.task(Operation::LookupSelf)
    }

    #[napi]
    pub fn logout(&self) -> AsyncTask<NeoTask> {
        self.task(Operation::Logout)
    }

    #[napi]
    pub fn close(&self) -> AsyncTask<NeoTask> {
        self.task(Operation::Close)
    }
}

impl NeoSession {
    fn task(&self, operation: Operation) -> AsyncTask<NeoTask> {
        AsyncTask::new(NeoTask {
            session: self.session.clone(),
            operation: Some(operation),
        })
    }
}

pub struct NeoTask {
    session: Arc<Mutex<Session>>,
    operation: Option<Operation>,
}

impl Task for NeoTask {
    type Output = Value;
    type JsValue = String;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| napi::Error::from_reason("Neo session unavailable"))?;
        let operation = self
            .operation
            .take()
            .ok_or_else(|| napi::Error::from_reason("Operation already completed"))?;
        let stage = match &operation {
            Operation::StartLogin => "start QR login",
            Operation::PollLogin => "check or complete QR login",
            Operation::Heartbeat => "check session heartbeat",
            Operation::PollIncoming => "wait for incoming message notifications",
            Operation::Sync { .. } => "read a sync page",
            Operation::SendText { .. } => "prepare a text send",
            Operation::LookupSelf => "look up the account profile",
            Operation::Logout => "log out",
            Operation::Close => "close the session",
        };
        run(&mut session, operation).map_err(|error| {
            if error.is::<SessionEnded>() {
                napi::Error::from_reason(SESSION_ENDED)
            } else {
                napi::Error::from_reason(format!("Neo could not {stage}"))
            }
        })
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.to_string())
    }
}

fn options() -> NativeOptions {
    NativeOptions {
        timeout: Duration::from_secs(10),
        ..Default::default()
    }
}

fn run(session: &mut Session, operation: Operation) -> Result<Value> {
    match operation {
        Operation::StartLogin => {
            ensure!(
                session.account.is_none() && session.attempt.is_none(),
                "Login already started"
            );
            let device = DeviceProfile::generate("UnifiedPCLinux x86_64".into())?;
            let attempt = NativeClient::new(options())?.start_qr_login(device, DEVICE_NAME)?;
            let output = json!({ "png": STANDARD.encode(&attempt.qr_png) });
            session.attempt = Some(attempt);
            Ok(output)
        }
        Operation::PollLogin => {
            let attempt = session
                .attempt
                .as_mut()
                .ok_or_else(|| anyhow!("No login attempt"))?;
            let client = NativeClient::new(options())?;
            let status = match client.poll_login(attempt) {
                Ok(status) => status,
                Err(error) if wechat_protocol::is_retryable_login_poll_error(&error) => {
                    return Ok(json!({ "status": "retry" }))
                }
                Err(error) => return Err(error),
            };
            if status == LoginStatus::Approved {
                let mut attempt = session.attempt.take().expect("issued challenge");
                let state = client.complete_login(&mut attempt)?;
                let result = json!({ "status": "completed", "username": state.own_username });
                session.account = Some(NativeClient::from_account(state, options())?);
                return Ok(result);
            }
            Ok(json!({ "status": status }))
        }
        Operation::Heartbeat => {
            let beat = with_renewal(session, heartbeat)?;
            Ok(
                json!({ "sync": beat.selector & 7 != 0, "intervalMs": beat.next_interval.clamp(30, 180) * 1000 }),
            )
        }
        Operation::PollIncoming => {
            let connection = session
                .incoming
                .as_mut()
                .context("No incoming connection")?;
            match connection.poll(Duration::from_secs(1)) {
                Ok(notification) => Ok(json!(
                    notification.is_some_and(|hint| hint.command == 24 && hint.selector & 7 != 0)
                )),
                Err(_) => {
                    // The notification socket closed. Reconnect and let one
                    // heartbeat decide whether WeChat still accepts the session.
                    session.incoming = None;
                    let beat = with_renewal(session, heartbeat)?;
                    Ok(json!(beat.selector & 7 != 0))
                }
            }
        }
        Operation::Sync { scene } => with_renewal(session, |session| sync(session, scene)),
        Operation::SendText { destination, text } => {
            let account = session
                .account
                .as_ref()
                .ok_or_else(|| anyhow!("No active session"))?;
            let created_at =
                u32::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
            let mut random = [0; 4];
            openssl::rand::rand_bytes(&mut random)?;
            let client_id = u32::from_le_bytes(random).max(1);
            // Validate locally before the operation is allowed to reach transport.
            wire::text_send_request(&destination, &text, created_at, client_id)?;
            let send = TextSendOptions {
                created_at,
                client_message_id: client_id,
                source: None,
            };
            Ok(submit_once(|| {
                account.request_blocking(|request| request.send_text(&destination, &text, &send))
            }))
        }
        Operation::LookupSelf => lookup_self(session),
        Operation::Logout => {
            session.incoming = None;
            let account = session.account.take().context("No active session")?;
            // Reserving the logout stops new account requests before submission.
            let pending = account.prepare_logout()?;
            let reply = tokio::runtime::Builder::new_current_thread()
                .build()?
                .block_on(pending.submit())?;
            Ok(json!({ "accepted": reply.accepted }))
        }
        Operation::Close => {
            if let Some(account) = session.account.take() {
                account.stop()?;
            }
            *session = Session::default();
            Ok(Value::Null)
        }
    }
}

fn connection(session: &mut Session) -> Result<&mut AccountPushConnection> {
    if session.incoming.is_none() {
        let account = session.account.as_ref().context("No active session")?;
        session.incoming = Some(account.open_push_connection_blocking()?);
    }
    session.incoming.as_mut().context("No incoming connection")
}

fn heartbeat(session: &mut Session) -> Result<wire::Heartbeat> {
    let cursor = session.received_cursor.clone();
    connection(session)?.heartbeat(&cursor)
}

fn sync(session: &mut Session, scene: u32) -> Result<Value> {
    let own = session
        .account
        .as_ref()
        .context("No active session")?
        .account_identity()?
        .own_username;
    let cursor = session.received_cursor.clone();
    let batch = connection(session)?.sync(&cursor, scene)?;
    let messages = text_messages(batch.commands, &own);
    session.received_cursor = batch.cursor;
    Ok(json!({ "messages": messages, "continuation": batch.continuation }))
}

/// Read-only session work can repeat after one renewal. Sends never use this.
fn with_renewal<T>(
    session: &mut Session,
    mut operation: impl FnMut(&mut Session) -> Result<T>,
) -> Result<T> {
    match operation(session) {
        Err(error) if credentials_rejected(&error) => {
            let account = session.account.as_ref().context("No active session")?;
            if let Err(renewal) = account.renew_session_blocking(DEVICE_NAME) {
                return Err(if renewal_rejected(&renewal) {
                    renewal.context(SessionEnded)
                } else {
                    renewal
                });
            }
            // Renewal supersedes the notification socket's credentials.
            session.incoming = None;
            operation(session).map_err(|error| {
                if credentials_rejected(&error) {
                    error.context(SessionEnded)
                } else {
                    error
                }
            })
        }
        result => result,
    }
}

fn service_code(error: &anyhow::Error, prefixes: &[&str]) -> Option<i32> {
    error.chain().find_map(|cause| {
        let message = cause.to_string();
        prefixes
            .iter()
            .find_map(|prefix| message.strip_prefix(prefix)?.parse::<i32>().ok())
    })
}

fn credentials_rejected(error: &anyhow::Error) -> bool {
    service_code(error, &AUTHENTICATED_REPLIES) == Some(-13)
}

fn renewal_rejected(error: &anyhow::Error) -> bool {
    credentials_rejected(error)
        || service_code(error, &["Automatic authentication returned "])
            .is_some_and(|code| code != 0)
}

fn lookup_self(session: &mut Session) -> Result<Value> {
    let account = session.account.as_ref().context("No active session")?;
    let own = account.account_identity()?.own_username;
    let lookup =
        account.request_blocking(|request| request.lookup_contacts(std::slice::from_ref(&own)))?;
    let present = |value: Option<String>| value.filter(|text| !text.is_empty());
    let profile = lookup
        .contacts
        .into_iter()
        .find(|contact| contact.username == own)
        .map(|contact| SelfProfile {
            nickname: present(contact.nickname),
            alias: present(contact.alias),
            avatar: present(contact.large_avatar_url).or(present(contact.small_avatar_url)),
        })
        .unwrap_or_default();
    Ok(json!(profile))
}

fn submit_once(send: impl FnOnce() -> Result<wire::TextSendResult>) -> Value {
    match send() {
        Ok(result)
            if result.accepted
                && result.returned_destination_matches
                && result.returned_client_id_matches =>
        {
            json!({ "status": "accepted", "serverMessageId": result.server_message_id })
        }
        Ok(result)
            if !result.accepted
                && (result.outer_code != 0
                    || result.returned_destination_matches
                        && result.returned_client_id_matches
                        && result.item_code.is_some_and(|code| code != 0)) =>
        {
            json!({ "status": "rejected" })
        }
        _ => json!({ "status": "uncertain" }),
    }
}

fn text_messages(commands: Vec<wire::SyncCommand>, username: &str) -> Vec<TextMessage> {
    let mut messages = vec![];
    for command in commands {
        if command.id == 5 {
            if let Ok(message) = wire::decode_add_message(&command.payload, username) {
                if message.message_type == 1 && !message.peer.ends_with("@chatroom") {
                    if let (Some(id), Some(text)) =
                        (message.server_message_id, message.content_text)
                    {
                        if !message.outgoing {
                            messages.push(TextMessage {
                                id,
                                from: message.from,
                                to: message.to,
                                text,
                                timestamp: message.created_at,
                            });
                        }
                    }
                }
            }
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lost_send_response_is_uncertain_after_one_attempt() {
        let mut attempts = 0;
        let result = submit_once(|| {
            attempts += 1;
            Err(anyhow!("lost acknowledgment"))
        });
        assert_eq!(attempts, 1);
        assert_eq!(result["status"], "uncertain");
    }

    #[test]
    fn only_whole_credential_rejections_end_the_session() {
        for prefix in AUTHENTICATED_REPLIES {
            assert!(credentials_rejected(
                &anyhow!("{prefix}-13").context("read a sync page")
            ));
            for other in ["-130", "-1317", "-13 extra", "0"] {
                assert!(!credentials_rejected(&anyhow!("{prefix}{other}")));
            }
        }
        assert!(!credentials_rejected(&anyhow!("network failed -13")));
        assert!(renewal_rejected(&anyhow!(
            "Automatic authentication returned -100"
        )));
        assert!(!renewal_rejected(&anyhow!("connection reset")));
        let ended = anyhow!("Heartbeat returned -13").context(SessionEnded);
        assert!(ended.is::<SessionEnded>());
    }

    #[test]
    fn closing_without_a_session_resets_state() {
        let mut session = Session {
            received_cursor: vec![1, 2, 3],
            ..Default::default()
        };
        assert_eq!(run(&mut session, Operation::Close).unwrap(), Value::Null);
        assert!(session.received_cursor.is_empty());
        assert!(run(&mut session, Operation::LookupSelf).is_err());
        assert!(run(&mut session, Operation::Logout).is_err());
    }

    #[test]
    fn unhandled_sync_commands_are_skipped() {
        let messages = text_messages(
            vec![wire::SyncCommand {
                id: 9999,
                payload: vec![42],
            }],
            "fixture-self",
        );
        assert!(messages.is_empty());
    }
}
