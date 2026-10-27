//! 微信读书 Agent Gateway 客户端——async 协议实现 + 可注入的调用抽象。
//!
//! CLI 与服务器共享本实现；服务器另在 `server::gateway` 包一层全局节流，
//! 测试经 [`GatewayApi`] trait 注入 fake。

use std::fmt;
use std::time::Duration;

use serde_json::{json, Value};

pub const GATEWAY_URL: &str = "https://i.weread.qq.com/api/agent/gateway";
pub const SKILL_VERSION: &str = "1.0.5"; // 官方 Skill 当前版本(2026-10 实测); 升级时服务端会随响应返回 upgrade_info
pub const KEY_PAGE: &str = "https://weread.qq.com/r/weread-skills";

/// 网关错误。
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("缺少 API Key: 请设置 WEREAD_API_KEY 环境变量或用 --key 传入 (获取页面: {KEY_PAGE})")]
    MissingAuth,
    /// 服务端要求升级 skill_version(响应仅含 upgrade_info 无业务数据时才阻断)
    #[error("需要升级: {0}")]
    Upgrade(String),
    #[error("[errcode={errcode}] {message}")]
    Api { errcode: String, message: String },
    #[error("[http_{status}] {body}")]
    Http { status: u16, body: String },
    #[error("[invalid_json] {0}")]
    InvalidJson(String),
    /// 上游超时(网关 499 或请求超时)
    #[error("[timeout] 上游超时 (attempts={0})")]
    Timeout(u32),
    #[error("[network] {message} (attempts={attempts})")]
    Network { message: String, attempts: u32 },
    #[error("[io] {0}")]
    Io(String),
}

impl GatewayError {
    /// 网络/超时/5xx/429 类可重试；业务错误(无效 bookId 等)与鉴权问题不重试。
    pub fn is_transient(&self) -> bool {
        match self {
            GatewayError::Network { .. }
            | GatewayError::Timeout(_)
            | GatewayError::InvalidJson(_) => true,
            GatewayError::Http { status, .. } => *status == 499 || *status == 429 || *status >= 500,
            _ => false,
        }
    }
}

/// 网关调用抽象——生产实现为 [`WereadClient`]（服务器经 `server::gateway` 的注册表按
/// Key 解析），测试注入 fake。async_trait 以支持 `Arc<dyn GatewayApi>` 动态分发。
#[async_trait::async_trait]
pub trait GatewayApi: Send + Sync {
    async fn call(&self, api_name: &str, params: Value) -> Result<Value, GatewayError>;
}

pub struct WereadClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl WereadClient {
    pub fn new(
        api_key: Option<String>,
        base_url: &str,
        timeout_secs: u64,
    ) -> Result<Self, GatewayError> {
        let api_key = api_key
            .or_else(|| std::env::var("WEREAD_API_KEY").ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or(GatewayError::MissingAuth)?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("weread-notes-rs/0.1")
            .build()
            .map_err(|e| GatewayError::Network { message: e.to_string(), attempts: 1 })?;
        Ok(Self {
            http,
            api_key,
            base_url: base_url.to_string(),
        })
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 调用一个 api_name。params 需为 JSON 对象(或 Null), 业务参数平铺合并进请求体顶层。
    pub async fn call(&self, api_name: &str, params: Value) -> Result<Value, GatewayError> {
        let mut body = json!({ "api_name": api_name, "skill_version": SKILL_VERSION });
        if let Value::Object(src) = params {
            if let Value::Object(dst) = &mut body {
                for (k, v) in src {
                    dst.insert(k, v);
                }
            }
        }
        self.call_with_retry(body).await
    }

    async fn call_with_retry(&self, body: Value) -> Result<Value, GatewayError> {
        const MAX_ATTEMPTS: u32 = 3;
        let mut last_err = String::new();
        for attempt in 1..=MAX_ATTEMPTS {
            let resp = self
                .http
                .post(&self.base_url)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await;
            match resp {
                Err(e) => {
                    let is_timeout = e.is_timeout();
                    let retryable = is_timeout || e.is_connect() || e.is_request();
                    last_err = e.to_string();
                    if attempt < MAX_ATTEMPTS && retryable {
                        tokio::time::sleep(Duration::from_millis(150 * attempt as u64)).await;
                        continue;
                    }
                    return Err(if is_timeout {
                        GatewayError::Timeout(attempt)
                    } else {
                        GatewayError::Network { message: last_err, attempts: attempt }
                    });
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let text = resp.text().await.map_err(|e| GatewayError::Network {
                        message: e.to_string(),
                        attempts: attempt,
                    })?;
                    if status == 499 {
                        // 上游超时专用状态码, 可重试
                        if attempt < MAX_ATTEMPTS {
                            tokio::time::sleep(Duration::from_millis(150 * attempt as u64)).await;
                            continue;
                        }
                        return Err(GatewayError::Timeout(attempt));
                    }
                    return self.parse_response(status, text);
                }
            }
        }
        Err(GatewayError::Network { message: last_err, attempts: MAX_ATTEMPTS })
    }

    fn parse_response(&self, status: u16, text: String) -> Result<Value, GatewayError> {
        let data: Value = if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&text).map_err(|_| {
                let head: String = text.chars().take(200).collect();
                GatewayError::InvalidJson(format!("HTTP {} 返回非 JSON: {}", status, head))
            })?
        };

        // 实测(2026-10): 升级提示为伴随性信号, 服务端仍会同时返回业务数据;
        // 仅当响应除 upgrade_info/errcode 外无任何内容时才视为阻断。
        if let Some(info) = data.get("upgrade_info") {
            let msg = info
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("skill 需要升级");
            let has_payload = data
                .as_object()
                .map(|o| o.keys().any(|k| k.as_str() != "upgrade_info" && k.as_str() != "errcode"))
                .unwrap_or(false);
            if !has_payload {
                return Err(GatewayError::Upgrade(msg.to_string()));
            }
            tracing::warn!("升级提示(已忽略, 数据正常返回): {}", msg);
        }

        let errcode = data.get("errcode");
        let nonzero = match errcode {
            Some(Value::Number(n)) => n.as_i64().map(|v| v != 0).unwrap_or(false),
            Some(Value::String(s)) => s.parse::<i64>().map(|v| v != 0).unwrap_or(true),
            _ => false,
        };
        if nonzero {
            let message = ["errmsg", "message", "errMsg", "msg"]
                .iter()
                .find_map(|k| data.get(*k).and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_else(|| "未知错误".into());
            let code = match errcode {
                Some(Value::Number(n)) => n.to_string(),
                Some(Value::String(s)) => s.clone(),
                _ => "?".into(),
            };
            return Err(GatewayError::Api { errcode: code, message });
        }

        if status >= 400 {
            let body: String = text.chars().take(200).collect();
            return Err(GatewayError::Http { status, body });
        }
        Ok(data)
    }
}

#[async_trait::async_trait]
impl GatewayApi for WereadClient {
    async fn call(&self, api_name: &str, params: Value) -> Result<Value, GatewayError> {
        WereadClient::call(self, api_name, params).await
    }
}

impl fmt::Display for WereadClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WereadClient({})", self.base_url)
    }
}
