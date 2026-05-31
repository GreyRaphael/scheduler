use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "scheduler", about = "A task scheduler with web UI")]
pub struct Config {
    #[arg(long, default_value = "0.0.0.0:6060", env = "SCHEDULER_LISTEN")]
    pub listen: SocketAddr,

    #[arg(long, default_value = "./scheduler.db", env = "SCHEDULER_DB")]
    pub db: PathBuf,

    #[arg(long, env = "SCHEDULER_TOKEN")]
    pub token: Option<String>,

    #[arg(long, default_value = "info", env = "SCHEDULER_LOG_LEVEL")]
    pub log_level: String,

    #[arg(long, default_value_t = 1000, env = "SCHEDULER_MAX_HISTORY")]
    pub max_history: usize,

    #[arg(long, default_value_t = 3600, env = "SCHEDULER_DEFAULT_TIMEOUT")]
    pub default_timeout: u64,
}
