//! 服务器配置——全部经 clap 参数 + 环境变量。
//!
//! BYOK 模型：服务器不持有任何默认凭据。微信读书 API Key 由前端
//! 逐请求携带（`X-Weread-Key` 头 / `?key=` 参数），此处只保留网络与存储配置。

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "weimo-weread-server", about = "微信读书 Agent Gateway 代理/笔记同步服务器")]
pub struct ServerConfig {
    /// 监听地址
    #[arg(long, env = "WEREAD_SERVER_BIND", default_value = "0.0.0.0:8081")]
    pub bind: String,

    /// 数据目录（SQLite 数据库）
    #[arg(long, env = "WEREAD_SERVER_DATA_DIR", default_value = "data")]
    pub data_dir: PathBuf,

    /// 网关地址（调试用）
    #[arg(long, env = "WEREAD_GATEWAY_URL", default_value = "https://i.weread.qq.com/api/agent/gateway")]
    pub gateway_url: String,

    /// 网关请求超时（秒）
    #[arg(long, env = "WEREAD_GATEWAY_TIMEOUT_SECS", default_value_t = 30)]
    pub gateway_timeout_secs: u64,

    /// 允许的 CORS 来源（逗号分隔，留空关闭）
    #[arg(long, env = "WEREAD_SERVER_CORS_ORIGINS", default_value = "")]
    pub cors_origins: String,

    /// 代理缓存 TTL（秒）
    #[arg(long, env = "WEREAD_CACHE_TTL_SECS", default_value_t = 3600)]
    pub cache_ttl_secs: i64,

    /// 网关最小调用间隔（毫秒）——全局限速（跨所有 Key），官方限额未知取保守值
    #[arg(long, env = "WEREAD_GATEWAY_MIN_INTERVAL_MS", default_value_t = 500)]
    pub gateway_min_interval_ms: u64,

    /// 熔断阈值（连续失败书次数）
    #[arg(long, env = "WEREAD_CIRCUIT_THRESHOLD", default_value_t = 5)]
    pub circuit_threshold: u32,

    /// 熔断冷却（秒）
    #[arg(long, env = "WEREAD_CIRCUIT_COOLDOWN_SECS", default_value_t = 600)]
    pub circuit_cooldown_secs: i64,

    /// 任务记录保留天数（notes 数据不随任务过期）
    #[arg(long, env = "WEREAD_TASK_RETENTION_DAYS", default_value_t = 30)]
    pub task_retention_days: i64,

    /// 队列长度上限（排队中的书数）
    #[arg(long, env = "WEREAD_QUEUE_LIMIT", default_value_t = 1000)]
    pub queue_limit: i64,

    /// 运行中任务上限
    #[arg(long, env = "WEREAD_MAX_RUNNING_TASKS", default_value_t = 4)]
    pub max_running_tasks: i64,

    /// 每分钟每 IP 请求上限
    #[arg(long, env = "WEREAD_RATE_PER_MINUTE", default_value_t = 60)]
    pub rate_per_minute: u32,
}

impl ServerConfig {
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("weread_server.db")
    }

    /// 现在的 epoch 毫秒
    pub fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}
