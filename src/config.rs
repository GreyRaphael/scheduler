use clap::Parser;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    listen: Option<String>,
    db: Option<String>,
    token: Option<String>,
    log_level: Option<String>,
}

#[derive(Parser, Debug, Clone)]
#[command(name = "scheduler", about = "A task scheduler with web UI")]
pub struct Config {
    #[arg(long, env = "SCHEDULER_LISTEN")]
    pub listen: Option<SocketAddr>,

    #[arg(long, env = "SCHEDULER_DB")]
    pub db: Option<PathBuf>,

    #[arg(long, env = "SCHEDULER_TOKEN")]
    pub token: Option<String>,

    #[arg(long, env = "SCHEDULER_LOG_LEVEL")]
    pub log_level: Option<String>,

    #[arg(long, env = "SCHEDULER_MAX_HISTORY")]
    pub max_history: Option<usize>,

    #[arg(long, env = "SCHEDULER_DEFAULT_TIMEOUT")]
    pub default_timeout: Option<u64>,

    #[arg(long, env = "SCHEDULER_CONFIG", default_value = "config.toml")]
    pub config: PathBuf,
}

impl Config {
    pub fn load() -> Self {
        let cli = Config::parse();
        let file_cfg = if cli.config.exists() {
            std::fs::read_to_string(&cli.config)
                .ok()
                .and_then(|s| toml::from_str::<FileConfig>(&s).ok())
                .unwrap_or_default()
        } else {
            FileConfig::default()
        };

        let listen = cli.listen
            .or_else(|| file_cfg.listen.as_ref().and_then(|s| s.parse().ok()))
            .unwrap_or("0.0.0.0:7070".parse().unwrap());

        let db = cli.db
            .or_else(|| file_cfg.db.map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("./scheduler.db"));

        let token = cli.token.or(file_cfg.token).filter(|s| !s.is_empty());

        let log_level = cli.log_level
            .or(file_cfg.log_level)
            .unwrap_or_else(|| "info".to_string());

        let max_history = cli.max_history.unwrap_or(1000);
        let default_timeout = cli.default_timeout.unwrap_or(3600);

        Config {
            listen: Some(listen),
            db: Some(db),
            token,
            log_level: Some(log_level),
            max_history: Some(max_history),
            default_timeout: Some(default_timeout),
            config: cli.config,
        }
    }

    pub fn listen_addr(&self) -> SocketAddr {
        self.listen.unwrap_or_else(|| "0.0.0.0:7070".parse().unwrap())
    }

    pub fn db_path(&self) -> PathBuf {
        self.db.clone().unwrap_or_else(|| PathBuf::from("./scheduler.db"))
    }

    pub fn log_level_str(&self) -> &str {
        self.log_level.as_deref().unwrap_or("info")
    }
}
