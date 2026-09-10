use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("序列化错误: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("操作被取消")]
    Cancelled,

    /// The requested action is unsafe given what we found on disk. These are
    /// expected outcomes, not bugs, and are shown to the user verbatim.
    #[error("{0}")]
    Unsafe(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Invalid(String),

    #[error("{0}")]
    Platform(String),
}

impl AppError {
    pub fn unsafe_op(message: impl Into<String>) -> Self {
        AppError::Unsafe(message.into())
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        AppError::Invalid(message.into())
    }

    pub fn platform(message: impl Into<String>) -> Self {
        AppError::Platform(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        AppError::NotFound(message.into())
    }

    pub fn code(&self) -> &'static str {
        match self {
            AppError::Io(_) => "io",
            AppError::Database(_) => "database",
            AppError::Serde(_) => "serde",
            AppError::Cancelled => "cancelled",
            AppError::Unsafe(_) => "unsafe",
            AppError::NotFound(_) => "notFound",
            AppError::Invalid(_) => "invalid",
            AppError::Platform(_) => "platform",
        }
    }
}

/// Wire shape for command failures. Keeping it structured means the UI can
/// branch on `code` instead of matching on translated text.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

impl Serialize for AppErrorWire {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ErrorPayload {
            code: self.0.code().to_string(),
            message: self.0.to_string(),
        }
        .serialize(serializer)
    }
}

pub struct AppErrorWire(pub AppError);

impl From<AppError> for AppErrorWire {
    fn from(value: AppError) -> Self {
        AppErrorWire(value)
    }
}

impl std::fmt::Display for AppErrorWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl AppErrorWire {
    /// Wraps a third-party error that has no place in `AppError`'s taxonomy.
    pub fn from_display(error: impl std::fmt::Display) -> Self {
        AppErrorWire(AppError::Platform(error.to_string()))
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// Command-facing result. Tauri requires the error type to be `Serialize`.
pub type CommandResult<T> = Result<T, AppErrorWire>;
