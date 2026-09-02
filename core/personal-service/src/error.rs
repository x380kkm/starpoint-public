// audience: internal
// # personal-service-error
//
// 该类型把线程、网络和 SQLite 错误收敛为个人服务边界错误.

use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub struct PersonalServiceError(String);

impl PersonalServiceError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for PersonalServiceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PersonalServiceError {}
