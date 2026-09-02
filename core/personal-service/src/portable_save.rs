// audience: internal
// # portable-save
//
// 该模块定义本地实例和远端实例共同使用的身份无关 CN 存档包.
// 数据摘要使用类型标记, UTF-8 键排序和 IEEE 754 数字编码, 导入方据此拒绝损坏或被替换的载荷.
// 创建存档包时递归删除实例身份, 关系和权限字段.
// 解析旧版存档包时先验证原始摘要, 再删除实例数据并生成规范摘要.

use crate::database::parse_iso_timestamp;
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) const FORMAT: &str = "starpoint-save-package";
pub(crate) const VERSION: i64 = 1;

const INSTANCE_IDENTITY_FIELDS: &[&str] = &[
    "account_id",
    "associate_token",
    "data_headers",
    "device_id",
    "keychain",
    "management_token",
    "player_id",
    "session",
    "session_id",
    "shell_credential",
    "shell_id",
    "token",
    "transfer_token",
    "viewer_id",
];

const INSTANCE_RELATIONSHIP_FIELDS: &[&str] = &[
    "block_list",
    "follow_info",
    "follow_list",
    "followed_count",
    "follower_list",
    "friend_list",
    "friends",
];

const INSTANCE_PERMISSION_FIELDS: &[&str] = &["management_role", "permissions"];

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableSaveSource {
    pub(crate) instance_kind: String,
    pub(crate) slot_id: Option<String>,
    pub(crate) slot_name: Option<String>,
    pub(crate) revision_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableSaveClient {
    pub(crate) platform: String,
    pub(crate) version: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StarpointSavePackage {
    pub(crate) format: String,
    pub(crate) version: i64,
    pub(crate) game: String,
    pub(crate) region: String,
    pub(crate) created_at: String,
    pub(crate) source: PortableSaveSource,
    pub(crate) source_client: PortableSaveClient,
    pub(crate) payload_sha256: String,
    pub(crate) data: Value,
}

// //// 生成跨运行时稳定的类型化数据摘要 [@x380kkm 2026-07-27] ////
fn write_canonical_string(output: &mut String, value: &str) {
    output.push_str(&format!("s{}:", value.len()));
    output.push_str(value);
}

fn write_canonical_json(output: &mut String, value: &Value) -> Result<(), PersonalServiceError> {
    match value {
        Value::Null => output.push('z'),
        Value::Bool(value) => output.push_str(if *value { "b1" } else { "b0" }),
        Value::Number(value) => {
            let value = value.as_f64().ok_or_else(|| {
                PersonalServiceError::new("portable save number is outside the supported range")
            })?;
            if value.fract() == 0.0 && value.abs() > 9_007_199_254_740_991.0 {
                return Err(PersonalServiceError::new(
                    "portable save data contains an unsafe integer",
                ));
            }
            let normalized = if value == 0.0 { 0.0 } else { value };
            output.push_str(&format!("n{:016x}", normalized.to_bits()));
        }
        Value::String(value) => write_canonical_string(output, value),
        Value::Array(values) => {
            output.push_str(&format!("a{}[", values.len()));
            for value in values {
                write_canonical_json(output, value)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
            output.push_str(&format!("o{}{{", keys.len()));
            for key in keys {
                write_canonical_string(output, key);
                write_canonical_json(output, &values[key])?;
            }
            output.push('}');
        }
    }
    Ok(())
}

pub(crate) fn calculate_payload_sha256(data: &Value) -> Result<String, PersonalServiceError> {
    let mut canonical = String::new();
    write_canonical_json(&mut canonical, data)?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}
// //// /生成跨运行时稳定的类型化数据摘要 ////

// //// 创建和验证身份无关的 CN 存档包 [@x380kkm 2026-07-27] ////
fn is_instance_owned_field(key: &str) -> bool {
    INSTANCE_IDENTITY_FIELDS.contains(&key)
        || INSTANCE_RELATIONSHIP_FIELDS.contains(&key)
        || INSTANCE_PERMISSION_FIELDS.contains(&key)
}

fn contains_instance_owned_data(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_instance_owned_data),
        Value::Object(values) => values.iter().any(|(key, value)| {
            is_instance_owned_field(key) || contains_instance_owned_data(value)
        }),
        _ => false,
    }
}

fn remove_instance_owned_data(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(remove_instance_owned_data),
        Value::Object(values) => {
            values.retain(|key, _| !is_instance_owned_field(key));
            values.values_mut().for_each(remove_instance_owned_data);
        }
        _ => {}
    }
}

// //// 在运行时验证来源和客户端元数据的语义 [@x380kkm 2026-08-13] ////
fn is_valid_source(source: &PortableSaveSource) -> bool {
    matches!(source.instance_kind.as_str(), "local" | "remote")
}

fn is_valid_client(client: &PortableSaveClient) -> bool {
    matches!(client.platform.as_str(), "android" | "ios" | "unknown")
}
// //// /在运行时验证来源和客户端元数据的语义 ////

pub(crate) fn sanitize_game_data(mut data: Value) -> Option<Value> {
    remove_instance_owned_data(&mut data);
    is_portable_game_data(&data).then_some(data)
}

pub(crate) fn is_portable_game_data(data: &Value) -> bool {
    if !data.is_object() {
        return false;
    }
    data.get("user_info").is_some_and(Value::is_object)
        && data
            .get("user_character_list")
            .is_some_and(Value::is_object)
        && !contains_instance_owned_data(data)
}

pub(crate) fn create_package(
    data: Value,
    created_at: String,
    source: PortableSaveSource,
) -> Result<StarpointSavePackage, PersonalServiceError> {
    if !is_valid_source(&source) {
        return Err(PersonalServiceError::new("portable save source is invalid"));
    }
    let data = sanitize_game_data(data)
        .ok_or_else(|| PersonalServiceError::new("portable save data is invalid"))?;
    if parse_iso_timestamp(&created_at).is_none() {
        return Err(PersonalServiceError::new(
            "portable save creation time is invalid",
        ));
    }
    let payload_sha256 = calculate_payload_sha256(&data)?;
    Ok(StarpointSavePackage {
        format: FORMAT.to_string(),
        version: VERSION,
        game: "starpoint".to_string(),
        region: "cn".to_string(),
        created_at,
        source,
        source_client: PortableSaveClient {
            platform: "unknown".to_string(),
            version: None,
        },
        payload_sha256,
        data,
    })
}

pub(crate) fn parse_package(value: Value) -> Option<StarpointSavePackage> {
    let mut package = serde_json::from_value::<StarpointSavePackage>(value).ok()?;
    if package.format != FORMAT
        || package.version != VERSION
        || package.game != "starpoint"
        || package.region != "cn"
        || parse_iso_timestamp(&package.created_at).is_none()
        || !is_valid_source(&package.source)
        || !is_valid_client(&package.source_client)
        || !package
            .payload_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || package.payload_sha256.len() != 64
    {
        return None;
    }
    let expected_sha256 = calculate_payload_sha256(&package.data).ok()?;
    if expected_sha256 != package.payload_sha256 {
        return None;
    }
    package.data = sanitize_game_data(std::mem::take(&mut package.data))?;
    package.payload_sha256 = calculate_payload_sha256(&package.data).ok()?;
    Some(package)
}
// //// /创建和验证身份无关的 CN 存档包 ////

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    // //// 验证跨运行时摘要, 格式和篡改拒绝 [@x380kkm 2026-07-27] ////
    #[test]
    fn creates_parses_and_rejects_tampered_packages() {
        let data = json!({
            "user_character_list": {},
            "user_info": { "name": "Portable", "rate": 0.25 }
        });
        let package = create_package(
            data,
            "2026-07-27T00:00:00.000Z".to_string(),
            PortableSaveSource {
                instance_kind: "local".to_string(),
                slot_id: Some("1".to_string()),
                slot_name: Some("First".to_string()),
                revision_id: None,
            },
        )
        .expect("package is created");
        let invalid_source = create_package(
            json!({
                "user_character_list": {},
                "user_info": { "name": "Portable" }
            }),
            "2026-07-27T00:00:00.000Z".to_string(),
            PortableSaveSource {
                instance_kind: "sandbox".to_string(),
                slot_id: None,
                slot_name: None,
                revision_id: None,
            },
        );
        assert!(invalid_source.is_err());
        assert_eq!(
            package.payload_sha256,
            "a1817b16f66c8d8d2880ac9634a0653a902dd48270dd9c4f9b3e95d6e8797337"
        );
        assert_eq!(
            calculate_payload_sha256(&json!({
                "user_character_list": {},
                "user_info": { "numbers": [1, 1.0, -0.0, 1.25e-7, 1e-7, 1e-6] }
            }))
            .expect("numeric vector is hashed"),
            "fb6110301d62dd2725bcb487dd1bc6ca4ef6f646294c614b4c450234691a2c01"
        );

        let prototype_key_data = json!({
            "__proto__": { "portable_marker": true },
            "user_character_list": {},
            "user_info": { "name": "Prototype key" }
        });
        let prototype_key_package = create_package(
            prototype_key_data,
            "2026-08-03T00:00:00.000Z".to_string(),
            PortableSaveSource {
                instance_kind: "local".to_string(),
                slot_id: None,
                slot_name: None,
                revision_id: None,
            },
        )
        .expect("prototype key package is created");
        assert_eq!(
            prototype_key_package.payload_sha256,
            "44de641c5c74646295fcad9cd3896dd229691d7fbf6f0962ef1e06957d8a654a"
        );
        let prototype_key_value =
            serde_json::to_value(prototype_key_package).expect("prototype key package is encoded");
        assert!(parse_package(prototype_key_value).is_some());

        let identity_data = json!({
            "account_id": 1,
            "associate_token": "source-associate-token",
            "block_list": [{ "viewer_id": 2 }],
            "data_headers": { "viewer_id": 3 },
            "device_id": 4,
            "follow_info": [{ "viewer_id": 5 }],
            "follow_list": [{ "viewer_id": 6 }],
            "followed_count": 1,
            "follower_list": [{ "viewer_id": 7 }],
            "friend_list": [{ "viewer_id": 8 }],
            "friends": [{ "viewer_id": 9 }],
            "keychain": "source-keychain",
            "management_role": "admin",
            "management_token": "source-management-token",
            "permissions": ["manage"],
            "player_id": 10,
            "session": { "id": "source-session" },
            "session_id": "source-session-id",
            "shell_credential": "source-shell-credential",
            "shell_id": "source-shell-id",
            "token": "source-token",
            "transfer_token": "source-transfer-token",
            "user_character_list": {},
            "user_info": { "name": "身份迁移", "bond_token": 7 },
            "user_tutorial": { "viewer_id": 11, "tutorial_step": 4 },
            "viewer_id": 12,
            "nested": [{ "viewer_id": 13, "bond_token": 9 }]
        });
        let identity_package = create_package(
            identity_data.clone(),
            "2026-08-03T00:00:00.000Z".to_string(),
            PortableSaveSource {
                instance_kind: "remote".to_string(),
                slot_id: Some("12".to_string()),
                slot_name: Some("Source".to_string()),
                revision_id: Some("revision-source".to_string()),
            },
        )
        .expect("identity fields are removed");
        assert_eq!(
            identity_package.payload_sha256,
            "536876fe1aee54176779e0dd84d5fde67c580764787de5dd3dad5e4a02686b65"
        );
        assert_eq!(
            identity_package.data,
            json!({
                "user_character_list": {},
                "user_info": { "name": "身份迁移", "bond_token": 7 },
                "user_tutorial": { "tutorial_step": 4 },
                "nested": [{ "bond_token": 9 }]
            })
        );

        let mut legacy_identity_package =
            serde_json::to_value(&identity_package).expect("identity package is encoded");
        legacy_identity_package["data"] = identity_data;
        legacy_identity_package["payloadSha256"] = Value::String(
            calculate_payload_sha256(&legacy_identity_package["data"])
                .expect("legacy identity-bearing payload is hashed"),
        );
        let migrated_legacy_package = parse_package(legacy_identity_package.clone())
            .expect("valid legacy identity-bearing package is migrated");
        assert_eq!(migrated_legacy_package.data, identity_package.data);
        assert_eq!(
            migrated_legacy_package.payload_sha256,
            identity_package.payload_sha256
        );

        legacy_identity_package["data"]["user_info"]["name"] =
            Value::from("Tampered legacy package");
        assert!(parse_package(legacy_identity_package).is_none());

        let encoded = serde_json::to_value(package).expect("package is encoded");
        assert!(parse_package(encoded.clone()).is_some());

        let mut unexpected = encoded.clone();
        unexpected["unexpected"] = Value::Bool(true);
        assert!(parse_package(unexpected).is_none());

        let mut unexpected_source = encoded.clone();
        unexpected_source["source"]["unexpected"] = Value::Bool(true);
        assert!(parse_package(unexpected_source).is_none());

        let mut unexpected_client = encoded.clone();
        unexpected_client["sourceClient"]["unexpected"] = Value::Bool(true);
        assert!(parse_package(unexpected_client).is_none());

        let mut invalid_timestamp = encoded.clone();
        invalid_timestamp["createdAt"] = Value::String("not-a-time".to_string());
        assert!(parse_package(invalid_timestamp).is_none());

        let mut invalid_source = encoded.clone();
        invalid_source["source"]["instanceKind"] = Value::String("sandbox".to_string());
        assert!(parse_package(invalid_source).is_none());

        let mut invalid_client = encoded.clone();
        invalid_client["sourceClient"]["platform"] = Value::String("desktop".to_string());
        assert!(parse_package(invalid_client).is_none());

        let mut impossible_timestamp = encoded.clone();
        impossible_timestamp["createdAt"] = Value::String("2026-02-30T00:00:00.000Z".to_string());
        assert!(parse_package(impossible_timestamp).is_none());

        assert!(calculate_payload_sha256(&json!({
            "user_character_list": {},
            "user_info": { "value": 9_007_199_254_740_992_u64 }
        }))
        .is_err());

        let mut tampered = encoded;
        tampered["data"]["user_info"]["name"] = Value::from("Tampered");
        assert!(parse_package(tampered).is_none());
    }

    // //// 通过临时 JSON 文件验证 Node 和 Rust 存档包往返 [@x380kkm 2026-08-13] ////
    #[test]
    #[ignore]
    fn portable_save_json_roundtrip_probe() {
        let input_path = PathBuf::from(
            std::env::var("STARPOINT_PORTABLE_SAVE_ROUNDTRIP_INPUT")
                .expect("roundtrip input path is required"),
        );
        let output_path = PathBuf::from(
            std::env::var("STARPOINT_PORTABLE_SAVE_ROUNDTRIP_OUTPUT")
                .expect("roundtrip output path is required"),
        );
        let input: Value =
            serde_json::from_slice(&fs::read(&input_path).expect("roundtrip input is readable"))
                .expect("roundtrip input is JSON");
        let should_reject = std::env::var("STARPOINT_PORTABLE_SAVE_ROUNDTRIP_EXPECT_REJECTED")
            .ok()
            .as_deref()
            == Some("1");
        let parsed = parse_package(input);
        if should_reject {
            assert!(parsed.is_none(), "tampered package was accepted");
            fs::write(&output_path, b"{\"accepted\":false}\n")
                .expect("roundtrip rejection result is writable");
            return;
        }
        let package = parsed.expect("roundtrip package is valid");
        fs::write(
            &output_path,
            serde_json::to_vec_pretty(&package).expect("roundtrip package is serializable"),
        )
        .expect("roundtrip output is writable");
    }
    // //// /验证跨运行时摘要, 格式和篡改拒绝 ////
}
