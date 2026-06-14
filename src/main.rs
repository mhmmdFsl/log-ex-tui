pub mod app;
pub mod auth;
pub mod cli;
pub mod config;
pub mod error;
pub mod gcp;
pub mod model;
pub mod tui;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use clap::Parser;
use cli::Cli;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use app::{App, Message};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.debug {
        setup_tracing()?;
    }

    tui::set_panic_hook();
    let mut terminal = tui::init()?;

    let result = run_app(&mut terminal, cli).await;

    tui::restore()?;
    result
}

fn setup_tracing() -> anyhow::Result<()> {
    let log_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("log-ex-tui");
    std::fs::create_dir_all(&log_dir)?;
    let log_file = std::fs::File::create(log_dir.join("debug.log"))?;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(tracing::level_filters::LevelFilter::DEBUG.into()),
        )
        .with_writer(move || log_file.try_clone().expect("log file clone"))
        .with_ansi(false)
        .init();
    Ok(())
}

struct TailConfig {
    active: bool,
    project: Option<String>,
    filter: Option<String>,
}

async fn run_app(terminal: &mut tui::Term, cli: Cli) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let startup_project = cli.project.clone();
    let tail_interval = Duration::from_secs(cli.tail_interval_seconds.max(1));
    let tail_page_size = cli.tail_page_size.clamp(1, 1000);

    tui::event_stream(tx.clone());
    let mut app = App::new(tx.clone());

    if let Some(project) = cli.project {
        app.init_project(project);
    }

    // Tail config shared between main loop and tail task
    let tail_config = Arc::new(tokio::sync::RwLock::new(TailConfig {
        active: false,
        project: None,
        filter: None,
    }));

    // Auth + project loading in background
    let bg_tx = tx.clone();
    let tail_cfg = tail_config.clone();
    let tail_tx = tx.clone();

    tokio::spawn(async move {
        match auth::TokenCache::new().await {
            Ok(token_cache) => {
                let gcp_client = gcp::Client::new(token_cache);
                let client_for_tail = gcp_client.clone();
                let _ = bg_tx.send(Message::GcpReady(gcp_client));
                match client_for_tail.list_projects().await {
                    Ok(projects) => {
                        let _ = bg_tx.send(Message::ProjectsLoaded(projects));
                    }
                    Err(e) => {
                        if startup_project.is_none() {
                            let _ = bg_tx.send(Message::AuthError(format!(
                                "{e}. Project listing failed; pass --project=<id> to skip the picker."
                            )));
                        } else {
                            tracing::warn!("project listing skipped after error: {e}");
                        }
                    }
                }

                // Start tail polling task now that we have a client
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(tail_interval);
                    interval.tick().await;
                    let mut last_ts: Option<DateTime<Utc>> = None;
                    let mut seen = HashSet::<String>::new();
                    let mut backoff = tail_interval;

                    loop {
                        interval.tick().await;

                        let cfg = tail_cfg.read().await;
                        if !cfg.active {
                            last_ts = None;
                            seen.clear();
                            drop(cfg);
                            continue;
                        }
                        let project = cfg.project.clone();
                        let filter = cfg.filter.clone();
                        drop(cfg);

                        let project = match project {
                            Some(p) => p,
                            None => continue,
                        };

                        let filter_str = if let Some(ts) = last_ts {
                            if let Some(f) = filter {
                                format!("({f}) AND timestamp>=\"{}\"", ts.to_rfc3339())
                            } else {
                                format!("timestamp>=\"{}\"", ts.to_rfc3339())
                            }
                        } else {
                            filter.unwrap_or_default()
                        };

                        match client_for_tail
                            .list_entries(&project, Some(&filter_str), tail_page_size)
                            .await
                        {
                            Ok(entries) => {
                                backoff = tail_interval;
                                let mut new = Vec::new();
                                for e in entries {
                                    if seen.insert(e.insert_id.clone()) {
                                        new.push(e);
                                    }
                                }
                                if !new.is_empty() {
                                    if let Some(ref ts_str) = new[0].timestamp {
                                        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
                                            last_ts = Some(dt.with_timezone(&Utc));
                                        }
                                    }
                                    let _ = tail_tx.send(Message::TailEntries(new));
                                }
                                if seen.len() > 10000 {
                                    let keep: Vec<_> =
                                        seen.iter().skip(seen.len() - 5000).cloned().collect();
                                    seen = keep.into_iter().collect();
                                }
                            }
                            Err(e) => {
                                tracing::warn!("tail poll error: {e}");
                                if e.is_rate_limited() {
                                    tokio::time::sleep(backoff).await;
                                    backoff = (backoff * 2).min(Duration::from_secs(300));
                                }
                            }
                        }
                    }
                });
            }
            Err(e) => {
                let _ = bg_tx.send(Message::AuthError(e.to_string()));
            }
        }
    });

    let mut tick_interval = tokio::time::interval(Duration::from_millis(200));

    while app.running {
        tokio::select! {
            Some(msg) = rx.recv() => {
                app.update(msg);
            }
            _ = tick_interval.tick() => {
                app.update(Message::Tick);
            }
        }

        // Sync tail config with app state
        {
            let mut cfg = tail_config.write().await;
            cfg.active = app.tail_on;
            cfg.project = app.project.clone();
            cfg.filter = app.build_filter_string();
        }

        terminal.draw(|f| app.render(f))?;
    }

    Ok(())
}
