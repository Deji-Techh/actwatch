mod args;
mod away;
mod hyprland;
mod models;
mod sampler;
mod store;

use actwatch_core::TitlePolicy;
use anyhow::Result;
use args::Args;
use away::AwayDetector;
use chrono::Utc;
use hyprland::{query_active_window_oneshot, HyprlandManager};
use models::{ActiveWindowInfo, AppEvent};
use sampler::ResourceSampler;
use std::time::{Duration, Instant};
use store::Store;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse().map_err(|e| anyhow::anyhow!(e))?;

    // 1. Logging setup
    let log_level = if args.foreground {
        "actwatch_daemon=debug,info"
    } else {
        "actwatch_daemon=info,warn"
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_level.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!(
        db_path = %args.db_path.display(),
        foreground = args.foreground,
        "Starting actwatch-daemon v2"
    );

    // 2. Load TitlePolicy from categories.toml [privacy]
    let title_policy = TitlePolicy::load();
    info!(policy = %title_policy, "Loaded window title redaction policy");

    // 3. Initialize SQLite store wrapping core::Store (enforces 0600)
    let mut store = Store::open(&args.db_path)?;

    // 4. Initialize samplers, managers, detectors
    let mut sampler = ResourceSampler::new();
    let mut away_detector = AwayDetector::new();
    let (tx, mut rx) = mpsc::channel::<ActiveWindowInfo>(64);
    let hypr_mgr = HyprlandManager::new();

    tokio::spawn(async move {
        hypr_mgr.run(tx).await;
    });

    // Initial state
    let mut current_window = query_active_window_oneshot().await;
    let mut is_away = current_window.is_away();
    let mut window_start = Instant::now();
    let mut window_start_ts = Utc::now().timestamp();

    // Tickers
    let mut sample_ticker = tokio::time::interval(Duration::from_secs(5));
    sample_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut commit_ticker = tokio::time::interval(Duration::from_secs(60));
    commit_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Away detection poll interval (polled <= every 30s)
    let mut away_ticker = tokio::time::interval(Duration::from_secs(15));
    away_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Signals
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    info!("actwatch-daemon v2 event loop entered");

    loop {
        tokio::select! {
            _ = sigint.recv() => {
                info!("Received SIGINT, shutting down cleanly...");
                break;
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down cleanly...");
                break;
            }

            // 5s resource sampling ticker
            _ = sample_ticker.tick() => {
                let sample = sampler.sample(&current_window.app_id, current_window.pid);
                store.push_sample(sample);
            }

            // Away detection ticker (≤ 30s)
            _ = away_ticker.tick() => {
                let away_status = away_detector.check_away().await;
                if away_status != is_away {
                    let now = Instant::now();
                    let dur_secs = now.duration_since(window_start).as_secs() as i64;

                    if away_status {
                        // User went away
                        if dur_secs > 0 && !current_window.is_away() {
                            let title = title_policy.redact(&current_window.title);
                            store.push_event(AppEvent::new(
                                window_start_ts,
                                current_window.app_id.clone(),
                                title,
                                dur_secs,
                            ));
                        }
                        current_window = ActiveWindowInfo::away();
                        is_away = true;
                        window_start = now;
                        window_start_ts = Utc::now().timestamp();
                    } else {
                        // User returned from away
                        if dur_secs > 0 {
                            store.push_event(AppEvent::new(
                                window_start_ts,
                                "away",
                                None,
                                dur_secs,
                            ));
                        }
                        current_window = query_active_window_oneshot().await;
                        is_away = current_window.is_away();
                        window_start = now;
                        window_start_ts = Utc::now().timestamp();
                    }
                }
            }

            // 60s SQLite batch commit + in-progress checkpoint
            _ = commit_ticker.tick() => {
                let now = Instant::now();
                let dur_secs = now.duration_since(window_start).as_secs() as i64;
                if dur_secs > 0 {
                    let title = if current_window.is_away() {
                        None
                    } else {
                        title_policy.redact(&current_window.title)
                    };
                    store.push_event(AppEvent::new(
                        window_start_ts,
                        current_window.app_id.clone(),
                        title,
                        dur_secs,
                    ));
                    window_start = now;
                    window_start_ts = Utc::now().timestamp();
                }

                let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                if let Err(e) = store.flush(&today) {
                    warn!(error = %e, "Batch commit deferred to next cycle");
                }
            }

            // Active window change events from Hyprland
            maybe_win = rx.recv() => {
                match maybe_win {
                    Some(new_win) => {
                        let now = Instant::now();

                        if new_win.is_away() {
                            // Lockscreen or no window focused
                            if !is_away {
                                is_away = true;
                                let dur_secs = now.duration_since(window_start).as_secs() as i64;
                                if dur_secs > 0 && !current_window.is_away() {
                                    let title = title_policy.redact(&current_window.title);
                                    store.push_event(AppEvent::new(
                                        window_start_ts,
                                        current_window.app_id.clone(),
                                        title,
                                        dur_secs,
                                    ));
                                }
                                current_window = ActiveWindowInfo::away();
                                window_start = now;
                                window_start_ts = Utc::now().timestamp();
                            }
                        } else {
                            // Active application window focused -> user is present
                            if is_away {
                                is_away = false;
                                let dur_secs = now.duration_since(window_start).as_secs() as i64;
                                if dur_secs > 0 {
                                    store.push_event(AppEvent::new(
                                        window_start_ts,
                                        "away",
                                        None,
                                        dur_secs,
                                    ));
                                }
                                current_window = new_win;
                                window_start = now;
                                window_start_ts = Utc::now().timestamp();
                            } else if new_win != current_window {
                                let dur_secs = now.duration_since(window_start).as_secs() as i64;
                                if dur_secs > 0 {
                                    let title = title_policy.redact(&current_window.title);
                                    store.push_event(AppEvent::new(
                                        window_start_ts,
                                        current_window.app_id.clone(),
                                        title,
                                        dur_secs,
                                    ));
                                }
                                current_window = new_win;
                                window_start = now;
                                window_start_ts = Utc::now().timestamp();
                            }
                        }
                    }
                    None => {
                        warn!("Hyprland event channel closed");
                    }
                }
            }
        }
    }

    // Flush remaining state on exit
    let now = Instant::now();
    let dur_secs = now.duration_since(window_start).as_secs() as i64;
    if dur_secs > 0 {
        let title = if current_window.is_away() {
            None
        } else {
            title_policy.redact(&current_window.title)
        };
        store.push_event(AppEvent::new(
            window_start_ts,
            current_window.app_id,
            title,
            dur_secs,
        ));
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if let Err(e) = store.flush(&today) {
        error!(error = %e, "Failed to flush SQLite store on shutdown");
    } else {
        info!("Successfully flushed SQLite store on shutdown");
    }

    Ok(())
}
