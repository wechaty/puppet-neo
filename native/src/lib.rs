mod storage;

use anyhow::{anyhow, ensure, Result};
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
use zeroize::Zeroizing;

const DEVICE_NAME: &str = "Puppet Neo";
const MAX_UNHANDLED_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
struct Session {
    attempt: Option<LoginAttempt>,
    account: Option<AccountState>,
    /// Network progress can run ahead of the saved cursor while unknown commands remain in memory.
    received_cursor: Vec<u8>,
    checkpoint: Vec<u8>,
    unhandled: Vec<wire::SyncCommand>,
    unhandled_bytes: usize,
    pending_checkpoint: bool,
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
    Restore(storage::Envelope),
    Snapshot,
    Heartbeat,
    Sync { initial: bool },
    FinishPage,
    SendText { destination: String, text: String },
}

#[napi]
pub struct NeoSession {
    session: Arc<Mutex<Session>>,
    key: Arc<Zeroizing<[u8; 32]>>,
}

#[napi]
impl NeoSession {
    #[napi(constructor)]
    pub fn new() -> napi::Result<Self> {
        let key = storage::key_from_env().map_err(|_| {
            napi::Error::from_reason("Set PUPPET_NEO_SESSION_KEY to a Base64-encoded 32-byte key")
        })?;
        Ok(Self {
            session: Default::default(),
            key: Arc::new(key),
        })
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
    pub fn restore(&self, envelope: Value) -> napi::Result<AsyncTask<NeoTask>> {
        let envelope = serde_json::from_value(envelope)
            .map_err(|_| napi::Error::from_reason("Invalid Neo MemoryCard envelope"))?;
        Ok(self.task(Operation::Restore(envelope)))
    }

    #[napi]
    pub fn snapshot(&self) -> AsyncTask<NeoTask> {
        self.task(Operation::Snapshot)
    }

    #[napi]
    pub fn heartbeat(&self) -> AsyncTask<NeoTask> {
        self.task(Operation::Heartbeat)
    }

    #[napi]
    pub fn sync(&self, initial: bool) -> AsyncTask<NeoTask> {
        self.task(Operation::Sync { initial })
    }

    #[napi]
    pub fn finish_page(&self) -> AsyncTask<NeoTask> {
        self.task(Operation::FinishPage)
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
            key: self.key.clone(),
            operation: Some(operation),
        })
    }
}

pub struct NeoTask {
    session: Arc<Mutex<Session>>,
    key: Arc<Zeroizing<[u8; 32]>>,
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
            Operation::Restore(_) => "restore the saved session",
            Operation::Snapshot => "seal the session snapshot",
            Operation::Heartbeat => "check session heartbeat",
            Operation::Sync { .. } => "read a sync page",
            Operation::FinishPage => "finish a sync page",
            Operation::SendText { .. } => "prepare a text send",
        };
        run(&mut session, &self.key, operation)
            .map_err(|_| napi::Error::from_reason(format!("Neo could not {stage}")))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.to_string())
    }
}

fn run(session: &mut Session, key: &[u8; 32], operation: Operation) -> Result<Value> {
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
        Operation::Restore(envelope) => {
            ensure!(
                session.account.is_none() && session.attempt.is_none(),
                "Session already started"
            );
            let (previous, cursor) = storage::open(envelope, key)?;
            let restored = client.renew_session(&previous, DEVICE_NAME)?;
            ensure!(
                restored.uin == previous.uin && restored.own_username == previous.own_username,
                "Restored account changed"
            );
            let result = json!({ "username": restored.own_username });
            session.account = Some(restored);
            session.received_cursor = cursor.clone();
            session.checkpoint = cursor;
            Ok(result)
        }
        Operation::Snapshot => {
            let state = session
                .account
                .as_ref()
                .ok_or_else(|| anyhow!("No active session"))?;
            Ok(serde_json::to_value(storage::seal(
                state,
                &session.checkpoint,
                key,
            )?)?)
        }
        Operation::Heartbeat => {
            let state = session
                .account
                .as_mut()
                .ok_or_else(|| anyhow!("No active session"))?;
            let beat = client.heartbeat(state, &session.received_cursor)?;
            Ok(
                json!({ "sync": beat.selector & 7 != 0, "intervalMs": beat.next_interval.clamp(5, 30) * 1000 }),
            )
        }
        Operation::Sync { initial } => {
            ensure!(
                !session.pending_checkpoint,
                "Previous sync page was not handed off"
            );
            let state = session
                .account
                .as_mut()
                .ok_or_else(|| anyhow!("No active session"))?;
            let batch =
                client.sync_scene(state, &session.received_cursor, if initial { 6 } else { 7 })?;
            let page = classify_page(batch.commands, &state.own_username);
            let page_bytes = page
                .unhandled
                .iter()
                .map(|command| command.payload.len().max(1))
                .sum::<usize>();
            ensure!(
                page_bytes <= MAX_UNHANDLED_BYTES - session.unhandled_bytes,
                "Unhandled sync command limit reached"
            );
            let unsupported: Vec<_> = page.unhandled.iter().map(|command| command.id).collect();
            session.unhandled_bytes += page_bytes;
            session.unhandled.extend(page.unhandled);
            session.received_cursor = batch.cursor;
            session.pending_checkpoint = true;
            Ok(
                json!({ "messages": page.messages, "unsupported": unsupported, "continuation": batch.continuation }),
            )
        }
        Operation::FinishPage => {
            ensure!(session.pending_checkpoint, "No pending page");
            if session.unhandled.is_empty() {
                session.checkpoint.clone_from(&session.received_cursor);
            }
            session.pending_checkpoint = false;
            Ok(Value::Null)
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

struct Page {
    messages: Vec<TextMessage>,
    unhandled: Vec<wire::SyncCommand>,
}

fn classify_page(commands: Vec<wire::SyncCommand>, username: &str) -> Page {
    let mut page = Page {
        messages: vec![],
        unhandled: vec![],
    };
    for command in commands {
        if command.id == 5 {
            if let Ok(message) = wire::decode_add_message(&command.payload, username) {
                if message.message_type == 1 && !message.peer.ends_with("@chatroom") {
                    if let (Some(id), Some(text)) =
                        (message.server_message_id, message.content_text)
                    {
                        if !message.outgoing {
                            page.messages.push(TextMessage {
                                id,
                                from: message.from,
                                to: message.to,
                                text,
                                timestamp: message.created_at,
                            });
                        }
                        continue;
                    }
                }
            }
        }
        page.unhandled.push(command);
    }
    page
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
    fn unknown_sync_command_keeps_saved_cursor_behind_received_cursor() -> Result<()> {
        let mut session = Session {
            received_cursor: vec![2],
            checkpoint: vec![1],
            unhandled: vec![wire::SyncCommand {
                id: 9999,
                payload: vec![42],
            }],
            pending_checkpoint: true,
            ..Default::default()
        };
        run(&mut session, &[0; 32], Operation::FinishPage)?;
        assert_eq!(session.checkpoint, [1]);
        assert_eq!(session.unhandled[0].payload, [42]);
        assert!(!session.pending_checkpoint);
        Ok(())
    }
}
