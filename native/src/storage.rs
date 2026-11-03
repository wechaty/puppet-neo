use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use openssl::{
    rand::rand_bytes,
    symm::{decrypt_aead, encrypt_aead, Cipher},
};
use serde::{Deserialize, Serialize};
use wechat_protocol::{AccountState, DeviceProfile};
use zeroize::{Zeroize, Zeroizing};

const AAD: &[u8] = b"wechaty-puppet-neo/session/v1";
const MAX_RECORD: usize = 16 * 1024 * 1024;
pub const PROTOCOL_VERSION: &str = "1.4.0";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    version: u8,
    nonce: String,
    ciphertext: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    protocol_crate_version: String,
    account: Identity,
    device: DeviceProfile,
    credentials: Credentials,
    routing: Routing,
    sync: Checkpoint,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    uin: String,
    username: String,
}

#[derive(Serialize, Deserialize, Zeroize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Credentials {
    cookie: String,
    session_key: String,
    request_key: String,
    response_key: String,
    auto_auth_key: String,
    key_info: String,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Routing {
    long_host: String,
    short_host: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    cursor: Option<String>,
}

pub fn key_from_env() -> Result<Zeroizing<[u8; 32]>> {
    let encoded = Zeroizing::new(
        std::env::var("PUPPET_NEO_SESSION_KEY").context("PUPPET_NEO_SESSION_KEY is required")?,
    );
    parse_key(&encoded)
}

pub fn parse_key(encoded: &str) -> Result<Zeroizing<[u8; 32]>> {
    ensure!(
        encoded.len() == 44,
        "Session key must be 32 bytes in Base64"
    );
    let key = Zeroizing::new(STANDARD.decode(encoded)?);
    ensure!(key.len() == 32, "Session key must be 32 bytes in Base64");
    let mut result = Zeroizing::new([0; 32]);
    result.copy_from_slice(&key);
    Ok(result)
}

pub fn seal(state: &AccountState, cursor: &[u8], key: &[u8; 32]) -> Result<Envelope> {
    state.validate()?;
    let snapshot = Snapshot {
        protocol_crate_version: PROTOCOL_VERSION.into(),
        account: Identity {
            uin: state.uin.to_string(),
            username: state.own_username.clone(),
        },
        device: state.device.clone(),
        credentials: Credentials {
            cookie: STANDARD.encode(&state.cookie),
            session_key: STANDARD.encode(&state.session_key),
            request_key: STANDARD.encode(&state.request_key),
            response_key: STANDARD.encode(&state.response_key),
            auto_auth_key: STANDARD.encode(&state.auto_auth_key),
            key_info: STANDARD.encode(&state.key_info),
        },
        routing: Routing {
            long_host: state.long_host.clone(),
            short_host: state.short_host.clone(),
        },
        sync: Checkpoint {
            cursor: (!cursor.is_empty()).then(|| STANDARD.encode(cursor)),
        },
    };
    let plain = Zeroizing::new(serde_json::to_vec(&snapshot)?);
    let mut nonce = [0; 12];
    rand_bytes(&mut nonce)?;
    let mut tag = [0; 16];
    let mut ciphertext = encrypt_aead(
        Cipher::aes_256_gcm(),
        key,
        Some(&nonce),
        AAD,
        &plain,
        &mut tag,
    )?;
    ciphertext.extend(tag);
    Ok(Envelope {
        version: 1,
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    })
}

pub fn open(envelope: Envelope, key: &[u8; 32]) -> Result<(AccountState, Vec<u8>)> {
    ensure!(envelope.version == 1, "Unsupported snapshot version");
    ensure!(
        envelope.nonce.len() == 16 && envelope.ciphertext.len() <= MAX_RECORD,
        "Invalid envelope size"
    );
    let nonce = STANDARD.decode(envelope.nonce)?;
    ensure!(nonce.len() == 12, "Invalid nonce length");
    let sealed = STANDARD.decode(envelope.ciphertext)?;
    ensure!(sealed.len() > 16, "Invalid ciphertext size");
    let (ciphertext, tag) = sealed.split_at(sealed.len() - 16);
    let plain = Zeroizing::new(
        decrypt_aead(
            Cipher::aes_256_gcm(),
            key,
            Some(&nonce),
            AAD,
            ciphertext,
            tag,
        )
        .context("Snapshot authentication failed")?,
    );
    let snapshot: Snapshot = serde_json::from_slice(&plain)?;
    ensure!(
        snapshot.protocol_crate_version == PROTOCOL_VERSION,
        "Snapshot was written by an incompatible protocol version"
    );
    let state = AccountState {
        schema: 1,
        device: snapshot.device,
        uin: snapshot.account.uin.parse()?,
        own_username: snapshot.account.username,
        cookie: STANDARD.decode(&snapshot.credentials.cookie)?,
        session_key: STANDARD.decode(&snapshot.credentials.session_key)?,
        request_key: STANDARD.decode(&snapshot.credentials.request_key)?,
        response_key: STANDARD.decode(&snapshot.credentials.response_key)?,
        auto_auth_key: STANDARD.decode(&snapshot.credentials.auto_auth_key)?,
        key_info: STANDARD.decode(&snapshot.credentials.key_info)?,
        database_password: vec![],
        auth_snapshot: Default::default(),
        long_host: snapshot.routing.long_host,
        short_host: snapshot.routing.short_host,
    };
    state.validate()?;
    let cursor = snapshot
        .sync
        .cursor
        .map(|s| STANDARD.decode(s))
        .transpose()?
        .unwrap_or_default();
    if !cursor.is_empty() {
        // The public request builder validates the cursor's input bounds.
        wechat_protocol::wire::sync_request(&cursor, &state.device.device_type, 6)?;
    }
    Ok((state, cursor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_preserves_session_and_cursor_and_rejects_wrong_key_or_tampering() -> Result<()> {
        let mut state = AccountState {
            schema: 1,
            device: DeviceProfile::generate("UnifiedPCLinux x86_64".into())?,
            uin: 42,
            own_username: "fixture-user".into(),
            cookie: vec![5; 15],
            session_key: vec![6; 16],
            request_key: vec![7; 24],
            response_key: vec![8; 24],
            auto_auth_key: vec![9; 32],
            key_info: vec![10; 16],
            database_password: vec![11; 32],
            auth_snapshot: Default::default(),
            long_host: "long.weixin.qq.com".into(),
            short_host: "short.weixin.qq.com".into(),
        };
        state.auth_snapshot.response = b"profile-only fixture".to_vec();
        let key = [7; 32];
        let cursor = [0];
        let envelope = seal(&state, &cursor, &key)?;
        let record = serde_json::to_string(&envelope)?;
        assert!(!record.contains("fixture-user"));
        assert_ne!(envelope.nonce, seal(&state, &cursor, &key)?.nonce);
        let (restored, restored_cursor) = open(serde_json::from_str(&record)?, &key)?;
        assert_eq!(restored_cursor, cursor);
        assert_eq!(restored.device.device_id, state.device.device_id);
        assert_eq!(restored.own_username, state.own_username);
        assert_eq!(restored.uin, state.uin);
        assert_eq!(restored.cookie, state.cookie);
        assert_eq!(restored.session_key, state.session_key);
        assert_eq!(restored.request_key, state.request_key);
        assert_eq!(restored.response_key, state.response_key);
        assert_eq!(restored.auto_auth_key, state.auto_auth_key);
        assert_eq!(restored.key_info, state.key_info);
        assert!(restored.database_password.is_empty());
        assert!(restored.auth_snapshot.response.is_empty());
        assert!(open(serde_json::from_str(&record)?, &[8; 32]).is_err());

        let mut envelope = envelope;
        let mut sealed = STANDARD.decode(&envelope.ciphertext)?;
        sealed[0] ^= 1;
        envelope.ciphertext = STANDARD.encode(sealed);
        assert!(open(envelope, &key).is_err());
        Ok(())
    }
}
