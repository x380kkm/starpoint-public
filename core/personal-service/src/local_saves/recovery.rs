// audience: internal | external
// # local-save-recovery
//
// 该模块用密码保护跨设备恢复包. 恢复包只包含加密后的本地密钥和玩家远端作用域.

use super::{json_error, parse_json, serialize_json};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use pbkdf2::pbkdf2_hmac;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

const RECOVERY_EXPORT_PATH: &str = "/v1/player/recovery/export";
const RECOVERY_IMPORT_PATH: &str = "/v1/player/recovery/import";
const ADMIN_RECOVERY_PREFIX: &str = "/v1/player-recovery/";
const RECOVERY_FORMAT: &str = "starpoint-save-recovery";
const RECOVERY_VERSION: i64 = 1;
const RECOVERY_KDF: &str = "PBKDF2-HMAC-SHA256";
const RECOVERY_ALGORITHM: &str = "AES-256-GCM";
const RECOVERY_ITERATIONS: u32 = 120_000;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;
const KEY_BYTES: usize = 32;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordRequest {
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportRequest {
    password: String,
    package: RecoveryPackage,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryPackage {
    format: String,
    version: i64,
    kdf: String,
    algorithm: String,
    iterations: u32,
    salt: String,
    nonce: String,
    ciphertext: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryMaterial {
    key_id: String,
    key_bytes: String,
    remote_scope: String,
}

#[derive(Serialize)]
struct ExportResponse {
    exported: bool,
    package: RecoveryPackage,
}

pub(super) fn is_player_path(path: &str) -> bool {
    path == RECOVERY_EXPORT_PATH || path == RECOVERY_IMPORT_PATH
}

pub(super) fn is_admin_path(path: &str) -> bool {
    path.starts_with(ADMIN_RECOVERY_PREFIX)
}

//// 分派管理员代玩家执行恢复包请求 [@x380kkm 2026-07-24] ////
pub(super) fn admin_route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let segments = request
        .path()
        .strip_prefix(ADMIN_RECOVERY_PREFIX)
        .unwrap_or_default()
        .split('/')
        .collect::<Vec<_>>();
    let viewer_id = segments
        .first()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0);
    let response = match (request.method(), viewer_id, segments.get(1).copied()) {
        ("POST", Some(viewer_id), Some("export")) => {
            match database.player_account_id_for_viewer(viewer_id) {
                Ok(Some(account_id)) => export_recovery_package(request, database, account_id),
                Ok(None) => Ok(json_error("404 Not Found", "viewer_not_found")),
                Err(error) => Err(error),
            }
        }
        ("POST", Some(viewer_id), Some("import")) => {
            match database.player_account_id_for_viewer(viewer_id) {
                Ok(Some(account_id)) => import_recovery_package(request, database, account_id),
                Ok(None) => Ok(json_error("404 Not Found", "viewer_not_found")),
                Err(error) => Err(error),
            }
        }
        (_, Some(_), Some("export" | "import")) => {
            Ok(json_error("405 Method Not Allowed", "method_not_allowed"))
        }
        _ => Ok(json_error("404 Not Found", "recovery_route_not_found")),
    };
    Some(response)
}
//// /分派管理员代玩家执行恢复包请求 ////

//// 分派跨设备恢复包请求 [@x380kkm 2026-07-24] ////
pub(super) fn player_route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match (request.method(), request.path()) {
        ("POST", RECOVERY_EXPORT_PATH) => export_recovery_package(request, database, account_id),
        ("POST", RECOVERY_IMPORT_PATH) => import_recovery_package(request, database, account_id),
        (_, RECOVERY_EXPORT_PATH | RECOVERY_IMPORT_PATH) => {
            Ok(json_error("405 Method Not Allowed", "method_not_allowed"))
        }
        _ => return None,
    };
    Some(response)
}
//// /分派跨设备恢复包请求 ////

//// 导出密码保护的恢复包 [@x380kkm 2026-07-24] ////
fn export_recovery_package(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(body) = parse_json::<PasswordRequest>(request) else {
        return Ok(json_error("400 Bad Request", "invalid_recovery_request"));
    };
    if !is_valid_password(&body.password) {
        return Ok(json_error("400 Bad Request", "recovery_password_invalid"));
    }
    let key = database.get_or_create_local_save_encryption_key()?;
    let remote_scope = database.get_or_create_player_remote_scope(account_id)?;
    let material = RecoveryMaterial {
        key_id: key.key_id,
        key_bytes: URL_SAFE_NO_PAD.encode(key.key_bytes),
        remote_scope,
    };
    let plaintext = serde_json::to_vec(&material).map_err(|error| {
        PersonalServiceError::new(format!("failed to encode recovery material: {error}"))
    })?;
    let package = encrypt_package(&body.password, &plaintext)?;
    serialize_json(
        "200 OK",
        ExportResponse {
            exported: true,
            package,
        },
    )
}
//// /导出密码保护的恢复包 ////

//// 导入并验证密码保护的恢复包 [@x380kkm 2026-07-24] ////
fn import_recovery_package(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    account_id: i64,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(body) = parse_json::<ImportRequest>(request) else {
        return Ok(json_error("400 Bad Request", "invalid_recovery_request"));
    };
    if !is_valid_password(&body.password) {
        return Ok(json_error("400 Bad Request", "recovery_password_invalid"));
    }
    let Some(material) = decrypt_package(&body.password, &body.package) else {
        return Ok(json_error("400 Bad Request", "recovery_package_invalid"));
    };
    let Some(key_bytes) = decode_canonical(&material.key_bytes) else {
        return Ok(json_error("400 Bad Request", "recovery_package_invalid"));
    };
    let Ok(key_bytes) = <[u8; KEY_BYTES]>::try_from(key_bytes) else {
        return Ok(json_error("400 Bad Request", "recovery_package_invalid"));
    };
    if !is_valid_key_id(&material.key_id) || !is_valid_remote_scope(&material.remote_scope) {
        return Ok(json_error("400 Bad Request", "recovery_package_invalid"));
    }
    match database.import_player_recovery_material(
        account_id,
        &material.key_id,
        key_bytes,
        &material.remote_scope,
    ) {
        Ok(()) => serialize_json("200 OK", serde_json::json!({ "imported": true })),
        Err(error) if error.to_string().contains("encryption key conflict") => {
            Ok(json_error("409 Conflict", "recovery_key_conflict"))
        }
        Err(error) if error.to_string().contains("remote scope conflict") => {
            Ok(json_error("409 Conflict", "recovery_scope_conflict"))
        }
        Err(error) => Err(error),
    }
}
//// /导入并验证密码保护的恢复包 ////

fn encrypt_package(
    password: &str,
    plaintext: &[u8],
) -> Result<RecoveryPackage, PersonalServiceError> {
    let mut salt = [0_u8; SALT_BYTES];
    let mut nonce = [0_u8; NONCE_BYTES];
    getrandom::getrandom(&mut salt).map_err(|error| {
        PersonalServiceError::new(format!("failed to generate recovery salt: {error}"))
    })?;
    getrandom::getrandom(&mut nonce).map_err(|error| {
        PersonalServiceError::new(format!("failed to generate recovery nonce: {error}"))
    })?;
    let wrapping_key = derive_key(password, &salt);
    let cipher = Aes256Gcm::new_from_slice(&wrapping_key)
        .map_err(|_| PersonalServiceError::new("failed to initialize recovery encryption"))?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: recovery_associated_data().as_bytes(),
            },
        )
        .map_err(|_| PersonalServiceError::new("failed to encrypt recovery package"))?;
    Ok(RecoveryPackage {
        format: RECOVERY_FORMAT.to_owned(),
        version: RECOVERY_VERSION,
        kdf: RECOVERY_KDF.to_owned(),
        algorithm: RECOVERY_ALGORITHM.to_owned(),
        iterations: RECOVERY_ITERATIONS,
        salt: URL_SAFE_NO_PAD.encode(salt),
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
    })
}

fn decrypt_package(password: &str, package: &RecoveryPackage) -> Option<RecoveryMaterial> {
    if package.format != RECOVERY_FORMAT
        || package.version != RECOVERY_VERSION
        || package.kdf != RECOVERY_KDF
        || package.algorithm != RECOVERY_ALGORITHM
        || package.iterations != RECOVERY_ITERATIONS
    {
        return None;
    }
    let salt = decode_canonical(&package.salt)?;
    if salt.len() != SALT_BYTES {
        return None;
    }
    let nonce = decode_canonical(&package.nonce)?;
    if nonce.len() != NONCE_BYTES {
        return None;
    }
    let ciphertext = decode_canonical(&package.ciphertext)?;
    let wrapping_key = derive_key(password, &salt);
    let cipher = Aes256Gcm::new_from_slice(&wrapping_key).ok()?;
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: recovery_associated_data().as_bytes(),
            },
        )
        .ok()?;
    serde_json::from_slice(&plaintext).ok()
}

fn derive_key(password: &str, salt: &[u8]) -> [u8; KEY_BYTES] {
    let mut key = [0_u8; KEY_BYTES];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, RECOVERY_ITERATIONS, &mut key);
    key
}

fn recovery_associated_data() -> String {
    format!("{RECOVERY_FORMAT}:{RECOVERY_VERSION}:{RECOVERY_KDF}:{RECOVERY_ALGORITHM}")
}

fn decode_canonical(value: &str) -> Option<Vec<u8>> {
    let decoded = URL_SAFE_NO_PAD.decode(value).ok()?;
    (URL_SAFE_NO_PAD.encode(&decoded) == value).then_some(decoded)
}

fn is_valid_password(password: &str) -> bool {
    (8..=128).contains(&password.chars().count()) && !password.chars().any(char::is_control)
}

fn is_valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn is_valid_remote_scope(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}
