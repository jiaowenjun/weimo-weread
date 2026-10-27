//! 生产网关句柄——全局节流（最小间隔 + 抖动）后走 WereadClient 协议逻辑。
//!
//! 节流对代理端点与导出 worker 统一生效：间隔从上次调用释放时起算，
//! 因此即使缓存未命中并发回源，网关侧看到的仍是恒定低速串行流。

use std::time::{Duration, Instant};

use rand::Rng;
use serde_json::Value;

use crate::client::{GatewayApi, GatewayError, WereadClient};

use super::config::ServerConfig;

pub struct ThrottledGateway {
    client: WereadClient,
    last_call: tokio::sync::Mutex<Option<Instant>>,
    min_interval: Duration,
}

impl ThrottledGateway {
    pub fn new(config: &ServerConfig) -> Result<Self, GatewayError> {
        let client = WereadClient::new(
            Some(config.api_key.clone()),
            &config.gateway_url,
            config.gateway_timeout_secs,
        )?;
        Ok(Self {
            client,
            last_call: tokio::sync::Mutex::new(None),
            min_interval: Duration::from_millis(config.gateway_min_interval_ms),
        })
    }

    async fn throttle(&self) {
        let mut last = self.last_call.lock().await;
        if let Some(prev) = *last {
            let jitter = rand::thread_rng().gen_range(0.0..0.2);
            let min = self.min_interval + Duration::from_secs_f64(jitter);
            let elapsed = prev.elapsed();
            if elapsed < min {
                tokio::time::sleep(min - elapsed).await;
            }
        }
        *last = Some(Instant::now());
    }
}

impl GatewayApi for ThrottledGateway {
    async fn call(&self, api_name: &str, params: Value) -> Result<Value, GatewayError> {
        self.throttle().await;
        self.client.call(api_name, params).await
    }
}
