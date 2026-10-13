// audience: internal
// # client-reports
// 故障记录保留最近 32 条客户端白名单字段或服务端请求异常, 管理 API 与 HTTP 状态历史一起读取.
// CN CrashUtil 通过表单的 info 字段发送 JSON, 设备身份字段由此边界过滤.

use super::{database_error, ServiceDatabase};
use crate::PersonalServiceError;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Map, Value};

const REPORT_LIMIT: i64 = 32;
const REPORT_NUMBER_FIELDS: [&str; 2] = ["screenWidth", "screenHeight"];
const REPORT_FIELDS: [&str; 9] = [
    "code",
    "error",
    "stackTrace",
    "date",
    "appVersion",
    "resourceVersion",
    "osName",
    "environment",
    "buildVersionNumber",
];

#[derive(Serialize)]
pub(crate) struct ClientReport {
    id: i64,
    received_at: String,
    details: Value,
}

// //// 创建客户端故障记录表 [@x380kkm 2026-09-20] ////
pub(super) fn migrate(connection: &Connection) -> Result<(), PersonalServiceError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS client_reports (
            id INTEGER PRIMARY KEY,
            received_at TEXT NOT NULL,
            details TEXT NOT NULL
        );",
        )
        .map_err(database_error)
}

impl ServiceDatabase {
    // //// 保存客户端故障诊断字段 [@x380kkm 2026-09-20] ////
    pub(crate) fn record_client_report(&mut self, body: &[u8]) -> Result<(), PersonalServiceError> {
        self.store_diagnostic_report(report_details(body))
    }

    // //// 保存服务端请求异常的路径和有界错误正文 [@x380kkm 2026-09-27] ////
    pub(crate) fn record_server_error(
        &mut self,
        method: &str,
        path: &str,
        error: &str,
    ) -> Result<(), PersonalServiceError> {
        self.store_diagnostic_report(json!({
            "source": "server",
            "code": "HTTP 500",
            "method": method.chars().take(16).collect::<String>(),
            "path": path.chars().take(512).collect::<String>(),
            "error": error.chars().take(4_096).collect::<String>(),
        }))
    }

    // //// 在同一事务内保存诊断并保留最近记录 [@x380kkm 2026-09-27] ////
    fn store_diagnostic_report(&mut self, details: Value) -> Result<(), PersonalServiceError> {
        let transaction = self.connection.transaction().map_err(database_error)?;
        transaction
            .execute(
                "INSERT INTO client_reports (received_at, details)
             VALUES (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?1)",
                params![details.to_string()],
            )
            .map_err(database_error)?;
        transaction
            .execute(
                "DELETE FROM client_reports WHERE id NOT IN
             (SELECT id FROM client_reports ORDER BY id DESC LIMIT ?1)",
                params![REPORT_LIMIT],
            )
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)
    }

    // //// 读取最近的客户端与服务端故障诊断 [@x380kkm 2026-09-27] ////
    pub(crate) fn client_reports(&self) -> Result<Vec<ClientReport>, PersonalServiceError> {
        let mut statement = self
            .connection
            .prepare("SELECT id, received_at, details FROM client_reports ORDER BY id DESC")
            .map_err(database_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(database_error)?;
        rows.map(|row| {
            let (id, received_at, text) = row.map_err(database_error)?;
            let details = serde_json::from_str(&text).map_err(|error| {
                PersonalServiceError::new(format!("failed to decode client report: {error}"))
            })?;
            Ok(ClientReport {
                id,
                received_at,
                details,
            })
        })
        .collect()
    }
}

// //// 解码 CN 故障表单并筛选有界字段 [@x380kkm 2026-09-20] ////
fn report_details(body: &[u8]) -> Value {
    let decoded = serde_json::from_slice::<Value>(body).ok().or_else(|| {
        url::form_urlencoded::parse(body)
            .find(|(key, _)| key == "info")
            .and_then(|(_, value)| serde_json::from_str(&value).ok())
    });
    let decoded = decoded.and_then(|value| match value.get("info") {
        Some(Value::String(info)) => serde_json::from_str(info).ok(),
        Some(Value::Object(info)) => Some(Value::Object(info.clone())),
        _ => Some(value),
    });
    let mut details = Map::new();
    if let Some(Value::Object(report)) = decoded {
        for field in REPORT_FIELDS {
            if let Some(Value::String(value)) = report.get(field) {
                let limit = match field {
                    "stackTrace" => 16_384,
                    "error" => 4_096,
                    _ => 256,
                };
                details.insert(
                    field.to_owned(),
                    Value::String(value.chars().take(limit).collect()),
                );
            }
        }
        for field in REPORT_NUMBER_FIELDS {
            if let Some(value @ Value::Number(_)) = report.get(field) {
                details.insert(field.to_owned(), value.clone());
            }
        }
    }
    if details.is_empty() {
        details.insert(
            "error".to_owned(),
            Value::String("未识别的客户端故障格式".to_owned()),
        );
    }
    Value::Object(details)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // //// 保存资源错误并隔离设备身份字段 [@x380kkm 2026-09-20] ////
    #[test]
    fn retains_resource_failure_without_device_identity() {
        let root = tempfile::tempdir().unwrap();
        let mut database = ServiceDatabase::open(root.path()).unwrap();
        let report = json!({"code":"C8601", "error":"missing event/banner.png",
            "resourceVersion":"1.4.56", "stackTrace":"AssetLoader.load",
            "screenWidth":1024, "screenHeight":1366,
            "deviceId":"private-device", "imei":"private-imei"});
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("info", &report.to_string())
            .finish();
        for _ in 0..REPORT_LIMIT + 1 {
            database.record_client_report(body.as_bytes()).unwrap();
        }
        drop(database);
        let database = ServiceDatabase::open(root.path()).unwrap();
        let reports = database.client_reports().unwrap();
        assert_eq!(reports.len(), REPORT_LIMIT as usize);
        assert_eq!(reports[0].details["code"], "C8601");
        assert_eq!(reports[0].details["error"], "missing event/banner.png");
        assert_eq!(reports[0].details["resourceVersion"], "1.4.56");
        assert_eq!(reports[0].details["screenWidth"], 1024);
        assert_eq!(reports[0].details["screenHeight"], 1366);
        assert!(reports[0].details.get("deviceId").is_none());
        assert!(reports[0].details.get("imei").is_none());
        assert!(reports[0].id > reports[1].id);
    }
}
