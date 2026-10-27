use clap::{Parser, Subcommand};

use weread_notes::client::{GatewayError, WereadClient, GATEWAY_URL};
use weread_notes::commands;

#[derive(Parser)]
#[command(
    name = "weread-notes",
    version,
    about = "微信读书官方 Agent Gateway 读书笔记工具 (Rust 版)"
)]
struct Cli {
    /// API Key (默认读环境变量 WEREAD_API_KEY, wrk- 前缀)
    #[arg(long, env = "WEREAD_API_KEY")]
    key: Option<String>,
    /// 覆盖网关地址(调试用)
    #[arg(long, default_value = GATEWAY_URL)]
    base_url: String,
    /// 请求超时(秒)
    #[arg(long, default_value_t = 30)]
    timeout: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 笔记本概览: 哪些书做过笔记
    Notebooks {
        #[arg(long, default_value_t = 30)]
        limit: usize,
        #[arg(long, default_value_t = 100)]
        page_size: u64,
    },
    /// 任意 api_name 直调, 参数用 k=v 形式
    Raw {
        api_name: String,
        /// 如 count=3 lastSort=100
        params: Vec<String>,
    },
    /// 网关接口自描述(/_list)
    Apis,
}

fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() > 16 {
        let head: String = chars[..8].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{}...{}", head, tail)
    } else {
        "***".to_string()
    }
}

/// 统一错误出口: (用户可读信息, 进退出码)。
fn gw_err(e: GatewayError) -> (String, i32) {
    let code = match &e {
        GatewayError::MissingAuth => 2,
        GatewayError::Upgrade(_) => 3,
        _ => 1,
    };
    (e.to_string(), code)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .with_target(false)
        .init();
    let cli = Cli::parse();
    let code = match run(cli).await {
        Ok(()) => 0,
        Err((msg, code)) => {
            if !msg.is_empty() {
                eprintln!("{}", msg);
            }
            code
        }
    };
    std::process::exit(code);
}

async fn run(cli: Cli) -> Result<(), (String, i32)> {
    let client = WereadClient::new(cli.key.clone(), &cli.base_url, cli.timeout).map_err(gw_err)?;
    eprintln!("[key {} -> {}]", mask(client.api_key()), client.base_url());
    match cli.command {
        Command::Notebooks { limit, page_size } => {
            commands::cmd_notebooks(&client, limit, page_size)
                .await
                .map_err(gw_err)
        }
        Command::Raw { api_name, params } => {
            commands::cmd_raw(&client, &api_name, &params)
                .await
                .map_err(gw_err)
        }
        Command::Apis => commands::cmd_apis(&client).await.map_err(gw_err),
    }
}
