// audience: internal | external
// # local-save-encryption
//
// 该模块使用 AES-256-GCM 加密本地存档. 认证数据固定绑定格式, 版本, 算法和 keyId.

use crate::database::LocalSaveEncryptionKey;
use crate::PersonalServiceError;
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

const ENCRYPTED_SAVE_FORMAT: &str = "starpoint-encrypted-save";
const ENCRYPTED_SAVE_VERSION: i64 = 1;
const ENCRYPTED_SAVE_ALGORITHM: &str = "AES-256-GCM";
const NONCE_BYTES: usize = 12;
const AUTHENTICATION_TAG_BYTES: usize = 16;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EncryptedSaveEnvelope {
    format: String,
    version: i64,
    algorithm: String,
    key_id: String,
    nonce: String,
    ciphertext: String,
}

// //// 加密本地存档 JSON [@x380kkm 2026-07-23] ////
pub(super) fn encrypt_player_data(
    data_json: &str,
    key: &LocalSaveEncryptionKey,
) -> Result<EncryptedSaveEnvelope, PersonalServiceError> {
    let cipher = Aes256Gcm::new_from_slice(&key.key_bytes).map_err(|_| {
        PersonalServiceError::new("local save encryption key has an invalid length")
    })?;
    let mut nonce = [0_u8; NONCE_BYTES];
    getrandom::getrandom(&mut nonce).map_err(|error| {
        PersonalServiceError::new(format!("failed to generate local save nonce: {error}"))
    })?;
    let associated_data = associated_data(&key.key_id);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: data_json.as_bytes(),
                aad: associated_data.as_bytes(),
            },
        )
        .map_err(|_| PersonalServiceError::new("failed to encrypt local save"))?;
    Ok(EncryptedSaveEnvelope {
        format: ENCRYPTED_SAVE_FORMAT.to_owned(),
        version: ENCRYPTED_SAVE_VERSION,
        algorithm: ENCRYPTED_SAVE_ALGORITHM.to_owned(),
        key_id: key.key_id.clone(),
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
    })
}
// //// /加密本地存档 JSON ////

// //// 验证并解密本地存档封装 [@x380kkm 2026-07-23] ////
pub(super) fn decrypt_player_data(
    envelope: &EncryptedSaveEnvelope,
    key: &LocalSaveEncryptionKey,
) -> Option<String> {
    if envelope.format != ENCRYPTED_SAVE_FORMAT
        || envelope.version != ENCRYPTED_SAVE_VERSION
        || envelope.algorithm != ENCRYPTED_SAVE_ALGORITHM
        || envelope.key_id != key.key_id
    {
        return None;
    }
    let nonce = decode_canonical_base64(&envelope.nonce)?;
    if nonce.len() != NONCE_BYTES {
        return None;
    }
    let ciphertext = decode_canonical_base64(&envelope.ciphertext)?;
    if ciphertext.len() < AUTHENTICATION_TAG_BYTES {
        return None;
    }
    let cipher = Aes256Gcm::new_from_slice(&key.key_bytes).ok()?;
    let associated_data = associated_data(&key.key_id);
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: associated_data.as_bytes(),
            },
        )
        .ok()?;
    String::from_utf8(plaintext).ok()
}
// //// /验证并解密本地存档封装 ////

fn associated_data(key_id: &str) -> String {
    format!("{ENCRYPTED_SAVE_FORMAT}:{ENCRYPTED_SAVE_VERSION}:{ENCRYPTED_SAVE_ALGORITHM}:{key_id}")
}

fn decode_canonical_base64(value: &str) -> Option<Vec<u8>> {
    let decoded = URL_SAFE_NO_PAD.decode(value).ok()?;
    (URL_SAFE_NO_PAD.encode(&decoded) == value).then_some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    // //// 验证加密往返和认证失败 [@x380kkm 2026-07-23] ////
    #[test]
    fn round_trips_and_rejects_tampering() {
        let key = LocalSaveEncryptionKey {
            key_id: "test-key".to_owned(),
            key_bytes: [7_u8; 32],
        };
        let plaintext = r#"{"user_info":{"name":"Encrypted"}}"#;
        let mut envelope = encrypt_player_data(plaintext, &key).expect("save is encrypted");
        assert_eq!(
            decrypt_player_data(&envelope, &key).as_deref(),
            Some(plaintext)
        );

        let mut ciphertext = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .expect("ciphertext is base64url");
        ciphertext[0] ^= 1;
        envelope.ciphertext = URL_SAFE_NO_PAD.encode(ciphertext);
        assert!(decrypt_player_data(&envelope, &key).is_none());
    }

    #[test]
    fn rejects_a_different_key_id() {
        let key = LocalSaveEncryptionKey {
            key_id: "first-key".to_owned(),
            key_bytes: [9_u8; 32],
        };
        let envelope = encrypt_player_data("{}", &key).expect("save is encrypted");
        let other_key = LocalSaveEncryptionKey {
            key_id: "other-key".to_owned(),
            key_bytes: key.key_bytes,
        };
        assert!(decrypt_player_data(&envelope, &other_key).is_none());
    }
    // //// /验证加密往返和认证失败 ////
}
