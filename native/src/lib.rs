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
    wire, AccountState, DeviceProfile, LoginAttempt, LoginStatus, NativeClient, NativeOptions,
};

const DEVICE_NAME: &str = "Puppet Neo";

#[derive(Default)]
struct Session {
    attempt: Option<LoginAttempt>,
    account: Option<AccountState>,
    incoming: Option<wechat_protocol::push::Connection>,
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

enum Operation {
    StartLogin,
    PollLogin,
    Heartbeat,
    PollIncoming,
    Sync { scene: u32 },
    SendText { destination: String, text: String },
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
        };
        run(&mut session, operation)
            .map_err(|_| napi::Error::from_reason(format!("Neo could not {stage}")))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.to_string())
    }
}

fn run(session: &mut Session, operation: Operation) -> Result<Value> {
    let client = NativeClient::new(NativeOptions {
        timeout: Duration::from_secs(10),
        ..Default::default()
    })?;
    match operation {
        Operation::StartLogin => {
            ensure!(
                session.account.is_none() && session.attempt.is_none(),
                "Login already started"
            );
            let device = DeviceProfile::generate("UnifiedPCLinux x86_64".into())?;
            let attempt = client.start_qr_login(device, DEVICE_NAME)?;
            let output = json!({ "png": STANDARD.encode(&attempt.qr_png) });
            session.attempt = Some(attempt);
            Ok(output)
        }
        Operation::PollLogin => {
            let attempt = session
                .attempt
                .as_mut()
                .ok_or_else(|| anyhow!("No login attempt"))?;
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
                session.account = Some(state);
                return Ok(result);
            }
            Ok(json!({ "status": status }))
        }
        Operation::Heartbeat => {
            let state = session
                .account
                .as_mut()
                .ok_or_else(|| anyhow!("No active session"))?;
            if session.incoming.is_none() {
                session.incoming = Some(client.push_connection(state)?);
            }
            let connection = session
                .incoming
                .as_mut()
                .context("No incoming connection")?;
            let beat = connection.heartbeat(state, &session.received_cursor)?;
            Ok(
                json!({ "sync": beat.selector & 7 != 0, "intervalMs": beat.next_interval.clamp(30, 180) * 1000 }),
            )
        }
        Operation::PollIncoming => {
            let connection = session
                .incoming
                .as_mut()
                .context("No incoming connection")?;
            let notification = connection.poll(Duration::from_secs(1))?;
            Ok(json!(notification.is_some_and(
                |hint| hint.command == 24 && hint.selector & 7 != 0
            )))
        }
        Operation::Sync { scene } => {
            let state = session
                .account
                .as_mut()
                .ok_or_else(|| anyhow!("No active session"))?;
            let connection = session
                .incoming
                .as_mut()
                .context("No incoming connection")?;
            let batch = connection.sync(state, &session.received_cursor, scene)?;
            let messages = text_messages(batch.commands, &state.own_username);
            session.received_cursor = batch.cursor;
            Ok(json!({ "messages": messages, "continuation": batch.continuation }))
        }
        Operation::SendText { destination, text } => {
            let state = session
                .account
                .as_mut()
                .ok_or_else(|| anyhow!("No active session"))?;
            let created_at =
                u32::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
            let mut random = [0; 4];
            openssl::rand::rand_bytes(&mut random)?;
            let client_id = u32::from_le_bytes(random).max(1);
            // Validate locally before the operation is allowed to reach transport.
            wire::text_send_request(&destination, &text, created_at, client_id)?;
            Ok(submit_once(|| {
                client.send_text(state, &destination, &text, created_at, client_id)
            }))
        }
    }
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
