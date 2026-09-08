use serde::ser::SerializeStruct;

/// 统一错误类型。序列化为 `{ message, code? }` 传给前端，
/// code 为 sub2api 的错误码字符串（如 ADMIN_COMPLIANCE_ACK_REQUIRED）。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{message}")]
    Api {
        message: String,
        code: Option<String>,
        status: Option<u16>,
    },
    #[error("未登录，请先在设置中登录")]
    NotLoggedIn,
    #[error("网络请求失败: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn other(message: impl Into<String>) -> Self {
        AppError::Other(message.into())
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            AppError::Api { code, .. } => code.as_deref(),
            _ => None,
        }
    }

    pub fn is_unauthorized(&self) -> bool {
        match self {
            AppError::Api { status, code, .. } => {
                *status == Some(401) || code.as_deref() == Some("UNAUTHORIZED")
            }
            _ => false,
        }
    }
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("message", &self.to_string())?;
        s.serialize_field("code", &self.code().map(str::to_string))?;
        s.end()
    }
}
