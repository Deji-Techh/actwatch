mod args;
mod hyprland;
mod models;
mod sampler;
mod store;

use args::Config;
use chrono::Utc;
use hyprland::HyprlandManager;
use models::{ActiveWindowInfo, AppEvent};
use sampler::ResourceSampler;
use store::Store;
use std::time::{Duration, Instant};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tracing::{error, info, warn};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let config = match Config::parse() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("actwatch-daemon error: {err}");
            std::process::exit(1);
        }
    };

    // Initialize tracing
    let filter = if config.foreground {
        "actwatch_daemon=info"
    } else {
        "actwatch_daemon=warn"
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .with_writer(std::io::stderr)
        .try_init();

    info!(
        db_path = %config.db_path.display(),
        foreground = config.foreground,
        "Starting actwatch-daemon"
    );

    let mut store = Store::open(&config.db_path)?;
    let mut sampler = ResourceSampler::new();

    // Signal handlers for graceful shutdown
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    // Channel for active window events
    let (tx, mut rx) = mpsc::channel::<ActiveWindowInfo>(64);
    let hypr_mgr = HyprlandManager::new();
    tokio::spawn(async move {
        hypr_mgr.run(tx).await;
    });

    // Active window tracking state
    let mut current_window = ActiveWindowInfo::idle();
    let mut window_start = Instant::now();
    let mut window_start_ts = Utc::now().timestamp();

    // 5s resource sampling ticker
    let mut sample_ticker = tokio::time::interval(Duration::from_secs(5));
    sample_ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    // 60s SQLite commit batch ticker
    let mut commit_ticker = tokio::time::interval(Duration::from_secs(60));
    commit_ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    info!("Collector event loop initialized");

    loop {
        tokio::select! {
            // Receive active window changes
            maybe_win = rx.recv() => {
                match maybe_win {
                    Some(new_win) => {
                        if new_win != current_window {
                            let now = Instant::now();
                            let dur_secs = now.duration_since(window_start).as_secs() as i64;

                            if dur_secs > 0 {
                                store.push_event(AppEvent {
                                    ts: window_start_ts,
                                    app_id: current_window.app_id.clone(),
                                    title: current_window.title.clone(),
                                    dur_secs,
                                });
                            }

                            current_window = new_win;
                            window_start = now;
                            window_start_ts = Utc::now().timestamp();
                        }
                    }
                    None => {
                        warn!("Window event channel closed; continuing tracking");
                    }
                }
            }

            // 5s resource sampler for tracked PID
            _ = sample_ticker.tick() => {
                let sample = sampler.sample(&current_window.app_id, current_window.pid);
                store.push_sample(sample);
            }

            // 60s SQLite batch commit
            _ = commit_ticker.tick() => {
                if let Err(e) = store.flush() {
                    warn!(error = %e, "Batch commit deferred to next cycle");
                }
            }

            // SIGINT graceful shutdown
            _ = sigint.recv() => {
                info!("Received SIGINT, shutting down cleanly");
                break;
            }

            // SIGTERM graceful shutdown
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down cleanly");
                break;
            }
        }
    }

    // Record remaining active window duration
    let dur_secs = Instant::now().duration_since(window_start).as_secs() as i64;
    if dur_secs > 0 {
        store.push_event(AppEvent {
            ts: window_start_ts,
            app_id: current_window.app_id,
            title: current_window.title,
            dur_secs,
        });
    }

    // Flush any pending events and samples
    if let Err(e) = store.flush() {
        error!(error = %e, "Failed to flush pending data during shutdown");
    } else {
        info!("Successfully flushed SQLite store on shutdown");
    }

    Ok(())
}
