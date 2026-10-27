use std::fmt;
use std::thread::sleep;
use std::time::Duration;

use serde_json::{json, Value};

pub const GATEWAY_URL: &str = "https://i.weread.qq.com/api/agent/gateway";
pub const SKILL_VERSION: &str = "1.0.5"; // 官方 Skill 当前版本(2026-10 实测); 升级时服务端会随响应返回 upgrade_info
pub const KEY_PAGE: &str = "https://weread.qq.com/r/weread-skills";

/// 网关错误。退出码约定: MissingAuth=2, Upgrade=3, 其余=1。
#[derive(Debug)]
pub enum GatewayError {
    MissingAuth,
    /// 服务端要求升级 skill_version(响应仅含 upgrade_info 无业务数据时才阻断)
    Upgrade(String),
    Api { errcode: String, message: String },
    Http { status: u16, body: String },
    InvalidJson(String),
    Network { message: String, attempts: u32 },
    Io(String),
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GatewayError::MissingAuth => write!(
                f,
                "缺少 API Key: 请设置 WEREAD_API_KEY 环境变量或用 --key 传入 (获取页面: {})",
                KEY_PAGE
            ),
            GatewayError::Upgrade(m) => write!(f, "需要升级: {}", m),
            GatewayError::Api { errcode, message } => write!(f, "[errcode={}] {}", errcode, message),
            GatewayError::Http { status, body } => write!(f, "[http_{}] {}", status, body),
            GatewayError::InvalidJson(m) => write!(f, "[invalid_json] {}", m),
            GatewayError::Network { message, attempts } => {
                write!(f, "[network] {} (attempts={})", message, attempts)
            }
            GatewayError::Io(m) => write!(f, "[io] {}", m),
        }
    }
}

impl std::error::Error for GatewayError {}

pub struct WereadClient {
    http: reqwest::blocking::Client,
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
        let http = reqwest::blocking::Client::builder()
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
    pub fn call(&self, api_name: &str, params: Value) -> Result<Value, GatewayError> {
        let mut body = json!({ "api_name": api_name, "skill_version": SKILL_VERSION });
        if let Value::Object(src) = params {
            if let Value::Object(dst) = &mut body {
                for (k, v) in src {
                    dst.insert(k, v);
                }
            }
        }
        self.call_with_retry(api_name, body)
    }

    fn call_with_retry(&self, api_name: &str, body: Value) -> Result<Value, GatewayError> {
        const MAX_ATTEMPTS: u32 = 3;
        let mut last_err = String::new();
        for attempt in 1..=MAX_ATTEMPTS {
            let resp = self
                .http
                .post(&self.base_url)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send();
            match resp {
                Err(e) => {
                    let msg = e.to_string();
                    let retryable = e.is_timeout() || e.is_connect() || e.is_request();
                    last_err = msg;
                    if attempt < MAX_ATTEMPTS && retryable {
                        sleep(Duration::from_millis(150 * attempt as u64));
                        continue;
                    }
                    return Err(GatewayError::Network { message: last_err, attempts: attempt });
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let text = resp
                        .text()
                        .map_err(|e| GatewayError::Network { message: e.to_string(), attempts: attempt })?;
                    if status == 499 {
                        // 上游超时专用状态码, 可重试
                        if attempt < MAX_ATTEMPTS {
                            sleep(Duration::from_millis(150 * attempt as u64));
                            continue;
                        }
                        return Err(GatewayError::Network {
                            message: "upstream timeout (HTTP 499)".into(),
                            attempts: attempt,
                        });
                    }
                    return self.parse_response(status, text);
                }
            }
        }
        let _ = api_name;
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
            eprintln!("[!] 升级提示(已忽略, 数据正常返回): {}", msg);
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
