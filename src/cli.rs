use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "log-ex-tui", about = "k9s-style TUI for GCP Cloud Logging")]
#[command(version)]
pub struct Cli {
    #[arg(short, long, env = "LOG_EX_TUI_PROJECT")]
    pub project: Option<String>,

    #[arg(short, long)]
    pub debug: bool,

    #[arg(long, env = "LOG_EX_TUI_TAIL_INTERVAL_SECONDS", default_value_t = 30)]
    pub tail_interval_seconds: u64,

    #[arg(long, env = "LOG_EX_TUI_TAIL_PAGE_SIZE", default_value_t = 50)]
    pub tail_page_size: u32,
}
