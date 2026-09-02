// audience: internal
// # personal-service-local-save-encryption-tests
//
// 该文件验证大存档加密导出, 重启后导入, 本地密钥隔离和篡改拒绝.

#[path = "support/cn.rs"]
mod cn_support;
#[path = "support/local_saves.rs"]
mod local_save_support;
mod support;

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use local_save_support::{
    activate_local_save, assert_status, authorized_request, export_local_save, list_local_saves,
    load, response_body, signup, update_slot_player_snapshot,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

// //// 读取本地存档加密密钥状态 [@x380kkm 2026-07-23] ////
fn read_encryption_key(root: &std::path::Path) -> (String, Vec<u8>) {
    let database = Connection::open(root.join("personal-service.sqlite3"))
        .expect("service database is opened");
    database
        .query_row(
            "SELECT key_id, key_bytes FROM local_save_encryption_keys WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("local save encryption key exists")
}
// //// /读取本地存档加密密钥状态 ////

// //// 生成和读取旧版加密存档封装 [@x380kkm 2026-08-03] ////
fn encrypt_legacy_save(data: &Value, key_id: &str, key_bytes: &[u8]) -> Value {
    let cipher = Aes256Gcm::new_from_slice(key_bytes).expect("legacy key length is valid");
    let nonce = [17_u8; 12];
    let associated_data = format!("starpoint-encrypted-save:1:AES-256-GCM:{key_id}");
    let plaintext = serde_json::to_vec(data).expect("legacy save is encoded");
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: associated_data.as_bytes(),
            },
        )
        .expect("legacy save is encrypted");
    json!({
        "format": "starpoint-encrypted-save",
        "version": 1,
        "algorithm": "AES-256-GCM",
        "keyId": key_id,
        "nonce": URL_SAFE_NO_PAD.encode(nonce),
        "ciphertext": URL_SAFE_NO_PAD.encode(ciphertext),
    })
}

fn decrypt_exported_save(envelope: &Value, key_bytes: &[u8]) -> Value {
    let key_id = envelope["keyId"].as_str().expect("key id is text");
    let nonce = URL_SAFE_NO_PAD
        .decode(envelope["nonce"].as_str().expect("nonce is text"))
        .expect("nonce is base64url");
    let ciphertext = URL_SAFE_NO_PAD
        .decode(envelope["ciphertext"].as_str().expect("ciphertext is text"))
        .expect("ciphertext is base64url");
    let cipher = Aes256Gcm::new_from_slice(key_bytes).expect("export key length is valid");
    let associated_data = format!("starpoint-encrypted-save:1:AES-256-GCM:{key_id}");
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: associated_data.as_bytes(),
            },
        )
        .expect("exported save is decrypted");
    serde_json::from_slice(&plaintext).expect("exported save is JSON")
}
// //// /生成和读取旧版加密存档封装 ////

// //// 加密大存档并在重启后恢复 [@x380kkm 2026-07-23] ////
#[test]
fn encrypts_and_restores_large_save_after_restart() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    assert!(support::request(service.port(), "GET", "/health").starts_with("HTTP/1.1 200 OK"));
    let device_id = 91;
    signup(service.port(), device_id);
    let initial_slot_id = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .expect("signup slot has an id");
    let mut source_data = export_local_save(&service, initial_slot_id)["data"].clone();
    source_data["user_info"]["name"] = Value::from("Encrypted Source");
    source_data["large_test_payload"] = Value::from("private-large-marker".repeat(2_048));
    let source_import = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import",
        Some(&json!({ "name": "Encryption source", "data": source_data })),
    );
    assert_status(&source_import, "201 Created");
    let source_slot_id = response_body(&source_import)["id"]
        .as_i64()
        .expect("source slot has an id");
    update_slot_player_snapshot(root.path(), source_slot_id, |data| {
        data["user_info"]["bond_token"] = Value::from(7);
        data["associate_token"] = Value::from("legacy-associate-token");
        data["user_tutorial"]["viewer_id"] = Value::from(314);
        data["follow_info"] = json!([{ "viewer_id": 2718 }]);
        data["permissions"] = json!(["manage"]);
    });

    let encrypted_response = authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{source_slot_id}/encrypted-export"),
        None,
    );
    assert_status(&encrypted_response, "200 OK");
    let envelope = response_body(&encrypted_response);
    assert_eq!(
        envelope["format"].as_str(),
        Some("starpoint-encrypted-save")
    );
    assert_eq!(envelope["version"].as_i64(), Some(1));
    assert_eq!(envelope["algorithm"].as_str(), Some("AES-256-GCM"));
    assert_eq!(
        URL_SAFE_NO_PAD
            .decode(envelope["nonce"].as_str().expect("nonce is text"))
            .expect("nonce is base64url")
            .len(),
        12,
    );
    let envelope_json = envelope.to_string();
    assert!(envelope_json.len() > 16 * 1024);
    assert!(!envelope_json.contains("Encrypted Source"));
    assert!(!envelope_json.contains("private-large-marker"));
    service.stop().expect("service stops cleanly");

    let (key_id, key_bytes) = read_encryption_key(root.path());
    assert_eq!(key_bytes.len(), 32);
    assert_eq!(envelope["keyId"].as_str(), Some(key_id.as_str()));
    assert!(!envelope_json.contains(&URL_SAFE_NO_PAD.encode(&key_bytes)));
    let exported_data = decrypt_exported_save(&envelope, &key_bytes);
    assert!(exported_data.get("associate_token").is_none());
    assert!(exported_data["user_tutorial"].get("viewer_id").is_none());
    assert!(exported_data.get("follow_info").is_none());
    assert!(exported_data.get("permissions").is_none());
    assert_eq!(exported_data["user_info"]["bond_token"].as_i64(), Some(7));

    let mut legacy_data = exported_data.clone();
    legacy_data["associate_token"] = Value::from("legacy-associate-token");
    legacy_data["user_tutorial"]["viewer_id"] = Value::from(314);
    legacy_data["follow_info"] = json!([{ "viewer_id": 2718 }]);
    legacy_data["permissions"] = json!(["manage"]);
    let legacy_envelope = encrypt_legacy_save(&legacy_data, &key_id, &key_bytes);

    let service = PersonalService::start(root.path(), 0).expect("service restarts");
    let encrypted_import = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import-encrypted",
        Some(&json!({ "name": "Encrypted restore", "envelope": envelope.clone() })),
    );
    assert_status(&encrypted_import, "201 Created");
    let restored_slot_id = response_body(&encrypted_import)["id"]
        .as_i64()
        .expect("restored slot has an id");
    activate_local_save(&service, restored_slot_id, device_id);
    let restored_signup = signup(service.port(), device_id);
    let restored = load(
        service.port(),
        restored_signup.data_headers.viewer_id,
        "1.4.99-encrypted-restore",
    );
    assert_eq!(
        restored.data["user_info"]["name"].as_str(),
        Some("Encrypted Source"),
    );
    assert_eq!(
        restored.data["associate_token"].as_str(),
        Some("associate_token"),
    );
    assert_eq!(
        restored.data["large_test_payload"].as_str().map(str::len),
        Some("private-large-marker".len() * 2_048),
    );

    let legacy_import = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import-encrypted",
        Some(&json!({ "name": "Legacy encrypted restore", "envelope": legacy_envelope })),
    );
    assert_status(&legacy_import, "201 Created");
    let legacy_slot_id = response_body(&legacy_import)["id"]
        .as_i64()
        .expect("legacy restored slot has an id");
    let legacy_export = export_local_save(&service, legacy_slot_id);
    assert!(legacy_export["data"].get("associate_token").is_none());
    assert!(legacy_export["data"]["user_tutorial"]
        .get("viewer_id")
        .is_none());
    assert!(legacy_export["data"].get("follow_info").is_none());
    assert!(legacy_export["data"].get("permissions").is_none());
    assert_eq!(
        legacy_export["data"]["user_info"]["bond_token"].as_i64(),
        Some(7),
    );

    let mut tampered_envelope = envelope.clone();
    let ciphertext = tampered_envelope["ciphertext"]
        .as_str()
        .expect("ciphertext is text")
        .to_owned();
    let replacement = if ciphertext.starts_with('A') {
        "B"
    } else {
        "A"
    };
    tampered_envelope["ciphertext"] = Value::from(format!("{replacement}{}", &ciphertext[1..]));
    let tampered_import = authorized_request(
        &service,
        "POST",
        "/v1/local-saves/import-encrypted",
        Some(&json!({ "name": "Tampered", "envelope": tampered_envelope })),
    );
    assert_status(&tampered_import, "400 Bad Request");
    assert_eq!(
        list_local_saves(&service)["slots"].as_array().map(Vec::len),
        Some(4),
    );
    service.stop().expect("service stops cleanly");
}
// //// /加密大存档并在重启后恢复 ////
