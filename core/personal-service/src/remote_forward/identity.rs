// audience: internal
// # personal-service-remote-identity
//
// 该文件记录远端 signup 返回的 viewer, 并把后续请求中的顶层 viewer_id 和 keychain
// 替换为当前服务器配置选择的 viewer. 其他 MessagePack 字段保持原值.

use crate::database::{ServerProfileIdentity, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{Number, Value};

const SIGNUP_PATH: &str = "/api/index.php/tool/signup";

pub(super) enum IdentityPreparationError {
    InvalidMessage,
    Storage(PersonalServiceError),
}

pub(super) struct PreparedRequest {
    pub(super) body: Vec<u8>,
    signup_device_id: Option<i64>,
}

// //// 准备当前服务器身份使用的请求正文 [@x380kkm 2026-07-23] ////
pub(super) fn prepare_request(
    request: &HttpRequest,
    profile_id: i64,
    database: &ServiceDatabase,
) -> Result<PreparedRequest, IdentityPreparationError> {
    if request.path() == SIGNUP_PATH {
        return Ok(PreparedRequest {
            body: request.body().to_vec(),
            signup_device_id: decode_message(request.body())
                .and_then(|message| positive_integer_field(&message, "device_id")),
        });
    }
    let Some(viewer_id) = database
        .active_server_profile_viewer_id(profile_id)
        .map_err(IdentityPreparationError::Storage)?
    else {
        return Ok(PreparedRequest {
            body: request.body().to_vec(),
            signup_device_id: None,
        });
    };
    let mut message =
        decode_message(request.body()).ok_or(IdentityPreparationError::InvalidMessage)?;
    let object = message
        .as_object_mut()
        .ok_or(IdentityPreparationError::InvalidMessage)?;
    let viewer_id = Value::Number(Number::from(viewer_id));
    let mut replaced = false;
    for field in ["viewer_id", "keychain"] {
        if let Some(value) = object.get_mut(field) {
            *value = viewer_id.clone();
            replaced = true;
        }
    }
    let body = if replaced {
        encode_message(&message).ok_or(IdentityPreparationError::InvalidMessage)?
    } else {
        request.body().to_vec()
    };
    Ok(PreparedRequest {
        body,
        signup_device_id: None,
    })
}
// //// /准备当前服务器身份使用的请求正文 ////

// //// 保存成功 signup 响应中的服务器身份 [@x380kkm 2026-07-23] ////
pub(super) fn capture_signup_identity(
    request: &PreparedRequest,
    profile_id: i64,
    response: &HttpResponse,
    database: &mut ServiceDatabase,
) -> Result<(), PersonalServiceError> {
    let Some(device_id) = request.signup_device_id else {
        return Ok(());
    };
    if !response.is_success()
        || !response
            .header("content-type")
            .is_some_and(|value| value.starts_with("application/x-msgpack"))
    {
        return Ok(());
    }
    let Some(message) = decode_message(response.body()) else {
        return Ok(());
    };
    let Some(viewer_id) = message
        .get("data_headers")
        .and_then(|headers| positive_integer_field(headers, "viewer_id"))
    else {
        return Ok(());
    };
    database.save_and_activate_server_profile_identity(&ServerProfileIdentity {
        profile_id,
        device_id,
        viewer_id,
    })
}
// //// /保存成功 signup 响应中的服务器身份 ////

fn decode_message(body: &[u8]) -> Option<Value> {
    let wrapped = std::str::from_utf8(body)
        .ok()
        .map(str::trim)
        .and_then(|encoded| STANDARD.decode(encoded).ok())
        .and_then(|packed| rmp_serde::from_slice(&packed).ok());
    wrapped.or_else(|| rmp_serde::from_slice(body).ok())
}

fn encode_message(message: &Value) -> Option<Vec<u8>> {
    rmp_serde::to_vec_named(message)
        .ok()
        .map(|packed| STANDARD.encode(packed).into_bytes())
}

fn positive_integer_field(message: &Value, field: &str) -> Option<i64> {
    message.get(field)?.as_i64().filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::decode_message;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde_json::json;

    // //// 解码原始和 base64 包装的 MessagePack [@x380kkm 2026-07-23] ////
    #[test]
    fn decodes_raw_and_wrapped_messagepack() {
        let message = json!({ "data_headers": { "viewer_id": 765_432_100 } });
        let packed = rmp_serde::to_vec_named(&message).expect("test message is encoded");
        let wrapped = STANDARD.encode(&packed);

        assert_eq!(decode_message(&packed), Some(message.clone()));
        assert_eq!(decode_message(wrapped.as_bytes()), Some(message));
    }
    // //// /解码原始和 base64 包装的 MessagePack ////
}
