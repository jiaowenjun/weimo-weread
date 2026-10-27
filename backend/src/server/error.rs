//! 统一错误——信封 `{"error":{"code","message",...}}`，映射设计文档第 9 节的错误码表。

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::client::GatewayError;

pub type AppResult<T> = Result<T, AppError>;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("{0}")]
    InvalidArgument(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("任务不存在或已过期: {0}")]
    TaskNotFound(String),
    #[error("任务未完成（当前状态: {0}）")]
    TaskNotFinished(String),
    #[error("请求过于频繁，请稍后再试")]
    TooManyRequests,
    #[error("队列已满或运行中任务过多，请稍后再试")]
    TooManyTasks,
    #[error("{0}")]
    Internal(String),
    #[error("网关业务错误 [errcode={errcode}] {message}")]
    UpstreamApi { errcode: String, message: String },
    #[error("网关超时")]
    UpstreamTimeout,
    #[error("网关不可用: {0}")]
    UpstreamUnavailable(String),
    #[error("官方 skill 需要升级: {0}")]
    SkillUpgrade(String),
    #[error("熔断中，抓取已暂停")]
    CircuitOpen,
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::InvalidArgument(_) => "INVALID_ARGUMENT",
            AppError::Unauthorized(_) => "UNAUTHORIZED",
            AppError::TaskNotFound(_) => "TASK_NOT_FOUND",
            AppError::TaskNotFinished(_) => "TASK_NOT_FINISHED",
            AppError::TooManyRequests => "TOO_MANY_REQUESTS",
            AppError::TooManyTasks => "TOO_MANY_TASKS",
            AppError::Internal(_) => "INTERNAL",
            AppError::UpstreamApi { .. } => "UPSTREAM_ERROR",
            AppError::UpstreamTimeout => "UPSTREAM_TIMEOUT",
            AppError::UpstreamUnavailable(_) => "UPSTREAM_UNAVAILABLE",
            AppError::SkillUpgrade(_) => "SKILL_UPGRADE_REQUIRED",
            AppError::CircuitOpen => "CIRCUIT_OPEN",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            AppError::InvalidArgument(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::TaskNotFound(_) => StatusCode::NOT_FOUND,
            AppError::TaskNotFinished(_) => StatusCode::CONFLICT,
            AppError::TooManyRequests | AppError::TooManyTasks => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::UpstreamApi { .. }
            | AppError::UpstreamTimeout
            | AppError::UpstreamUnavailable(_) => StatusCode::BAD_GATEWAY,
            AppError::SkillUpgrade(_) | AppError::CircuitOpen => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// 信封附带的机器可读补充字段
    fn extra(&self) -> Option<Value> {
        match self {
            AppError::UpstreamApi { errcode, .. } => Some(json!({ "errcode": errcode })),
            _ => None,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let code = self.code();
        let message = self.to_string();
        let extra = self.extra();
        let mut body = json!({"code": code, "message": message});
        if let Some(Value::Object(extra)) = extra {
            if let Value::Object(map) = &mut body {
                for (k, v) in extra {
                    map.insert(k, v);
                }
            }
        }
        (self.status(), Json(json!({ "error": body }))).into_response()
    }
}

impl From<GatewayError> for AppError {
    fn from(e: GatewayError) -> Self {
        match e {
            GatewayError::Upgrade(m) => AppError::SkillUpgrade(m),
            GatewayError::Api { errcode, message } => AppError::UpstreamApi { errcode, message },
            GatewayError::Timeout(_) => AppError::UpstreamTimeout,
            GatewayError::MissingAuth => {
                AppError::UpstreamUnavailable("上游 API Key 未配置".into())
            }
            GatewayError::Network { message, .. } => AppError::UpstreamUnavailable(message),
            GatewayError::Http { status, body } => {
                AppError::UpstreamUnavailable(format!("HTTP {status}: {body}"))
            }
            GatewayError::InvalidJson(m) => AppError::UpstreamUnavailable(m),
            GatewayError::Io(m) => AppError::Internal(m),
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error = %e, "database error");
        AppError::Internal("数据库错误".to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        tracing::error!(error = %e, "io error");
        AppError::Internal("文件系统错误".to_string())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!(error = %e, "internal error");
        AppError::Internal(e.to_string())
    }
}
