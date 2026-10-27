//! 网关注册表——BYOK（Bring Your Own Key）模型的核心。
//!
//! 服务器不持有任何默认 Key：每个请求经 `X-Weread-Key` 头（或 `?key=` 参数）
//! 携带微信读书 API Key，注册表按 Key 解析（并缓存）对应的网关客户端；
//! 节流锁全局共享——不管多少个 Key，到官方网关侧恒为低速串行流。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::Rng;
use serde_json::Value;

use crate::client::{GatewayApi, GatewayError, WereadClient};

/// 按 Key 解析的网关句柄：独占一个 WereadClient，节流锁与其他 Key 共享。
pub struct ThrottledGateway {
    client: WereadClient,
    throttle: Arc<tokio::sync::Mutex<Option<Instant>>>,
    min_interval: Duration,
}

impl ThrottledGateway {
    fn new(
        key: &str,
        base_url: &str,
        timeout_secs: u64,
        min_interval: Duration,
        throttle: Arc<tokio::sync::Mutex<Option<Instant>>>,
    ) -> Result<Self, GatewayError> {
        Ok(Self {
            client: WereadClient::new(Some(key.to_string()), base_url, timeout_secs)?,
            throttle,
            min_interval,
        })
    }

    async fn throttle(&self) {
        let mut last = self.throttle.lock().await;
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

#[async_trait::async_trait]
impl GatewayApi for ThrottledGateway {
    async fn call(&self, api_name: &str, params: Value) -> Result<Value, GatewayError> {
        self.throttle().await;
        self.client.call(api_name, params).await
    }
}

/// Key → 网关客户端注册表（含测试注入入口）。
pub struct GatewayRegistry {
    base_url: String,
    timeout_secs: u64,
    min_interval: Duration,
    /// 全局节流锁：所有 Key 共享，保证网关侧恒低速
    throttle: Arc<tokio::sync::Mutex<Option<Instant>>>,
    cache: Mutex<HashMap<String, Arc<dyn GatewayApi>>>,
}

impl GatewayRegistry {
    pub fn new(base_url: &str, timeout_secs: u64, min_interval_ms: u64) -> Self {
        Self {
            base_url: base_url.to_string(),
            timeout_secs,
            min_interval: Duration::from_millis(min_interval_ms),
            throttle: Arc::new(tokio::sync::Mutex::new(None)),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// 按 Key 解析网关客户端（首次创建后缓存复用）。
    pub fn for_key(&self, key: &str) -> Result<Arc<dyn GatewayApi>, GatewayError> {
        if let Some(g) = self.cache.lock().unwrap().get(key) {
            return Ok(g.clone());
        }
        let g: Arc<dyn GatewayApi> = Arc::new(ThrottledGateway::new(
            key,
            &self.base_url,
            self.timeout_secs,
            self.min_interval,
            self.throttle.clone(),
        )?);
        self.cache
            .lock()
            .unwrap()
            .insert(key.to_string(), g.clone());
        Ok(g)
    }

    /// 显式注入网关实现（测试用；生产路径不会调用）
    pub fn set(&self, key: &str, gw: Arc<dyn GatewayApi>) {
        self.cache.lock().unwrap().insert(key.to_string(), gw);
    }
}
