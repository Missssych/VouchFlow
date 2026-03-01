#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

//! App-Voucher - Voucher Transaction Application
//!
//! Main entry point that initializes and wires all components:
//! - Database with migrations
//! - Central Store (hydrated from DB)
//! - Channel infrastructure
//! - DB Writer Actor
//! - Orchestrator
//! - Axum Gateway
//! - Slint UI with event bridge

use i_slint_backend_winit::WinitWindowAccessor;
use slint::Model; // For ModelRc::iter()

// Re-export library
use vouchflow::{
    application::{CentralStore, Orchestrator, store::MenuType},
    config::AppConfig,
    infrastructure::{
        channels::{create_command_bus, create_db_command_queue, create_event_bus},
        database::Database,
        database::DbWriter,
        provider::ProviderClient,
    },
    presentation::{gateway::Gateway, ui::bridge::BridgeCommand},
    utils::init_tracing,
};

slint::include_modules!();
mod callbacks;

fn build_logs_table_model(
    logs: &[vouchflow::domain::LogEntry],
    level_filter_index: i32,
) -> slint::ModelRc<slint::ModelRc<slint::StandardListViewItem>> {
    let selected_level = match level_filter_index {
        1 => Some("INFO"),
        2 => Some("WARN"),
        3 => Some("ERROR"),
        _ => None,
    };

    let slint_logs: Vec<slint::ModelRc<slint::StandardListViewItem>> = logs
        .iter()
        .filter(|log| selected_level.map_or(true, |level| log.level.eq_ignore_ascii_case(level)))
        .map(|log| {
            let time_str = log.created_at.format("%Y-%m-%d %H:%M:%S").to_string();
            let trace_id = log.trace_id.clone().unwrap_or_default();
            let items: Vec<slint::StandardListViewItem> = vec![
                slint::StandardListViewItem::from(slint::SharedString::from(time_str)),
                slint::StandardListViewItem::from(slint::SharedString::from(log.level.clone())),
                slint::StandardListViewItem::from(slint::SharedString::from(log.message.clone())),
                slint::StandardListViewItem::from(slint::SharedString::from(trace_id)),
            ];
            slint::ModelRc::new(slint::VecModel::from(items))
        })
        .collect();

    slint::ModelRc::new(slint::VecModel::from(slint_logs))
}

fn sync_server_state(ui: &AppWindow, is_running: bool, status: &str) {
    let app_state = AppState::get(ui);
    app_state.set_server_connected(is_running);
    app_state.set_server_status(status.into());

    let settings_state = SettingsState::get(ui);
    settings_state.set_server_running(is_running);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize tracing
    init_tracing();
    tracing::info!("Starting App-Voucher");

    // 2. Load configuration
    let config = AppConfig::from_env();
    tracing::info!("Configuration loaded: {:?}", config);

    // 3. Initialize Tokio runtime for async components
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // 4. Initialize database and run migrations
    let db = rt.block_on(async {
        let db = Database::new(&config.db_path)?;
        db.run_migrations().await?;
        tracing::info!("Database initialized");
        Ok::<Database, Box<dyn std::error::Error>>(db)
    })?;

    // 4b. Crash recovery: clean up stale states from previous unclean shutdown
    rt.block_on(async {
        let reserved = db.with_writer(|conn| {
            let count = conn.execute(
                "UPDATE stok_voucher SET status = 'ACTIVE', updated_at = datetime('now') WHERE status = 'RESERVED'",
                [],
            )?;
            Ok(count)
        }).await?;
        if reserved > 0 {
            tracing::warn!("Crash recovery: reset {} RESERVED vouchers ? ACTIVE", reserved);
        }

        let stuck = db.with_writer(|conn| {
            let count = conn.execute(
                "UPDATE transactions SET status = 'FAILED', result_code = 'SYS_CRASH', result_payload = 'Recovered from crash - was PROCESSING', updated_at = datetime('now') WHERE status IN ('PROCESSING', 'PENDING')",
                [],
            )?;
            Ok(count)
        }).await?;
        if stuck > 0 {
            tracing::warn!("Crash recovery: marked {} stuck PROCESSING/PENDING transactions ? FAILED", stuck);
        }

        Ok::<(), Box<dyn std::error::Error>>(())
    })?;

    // 5. Create communication channels
    let (command_tx, command_rx) = create_command_bus(config.command_bus_capacity);
    let (db_cmd_tx, db_cmd_rx) = create_db_command_queue(config.db_command_capacity);
    let (event_tx, _event_rx) = create_event_bus(config.event_bus_capacity);

    // 6. Initialize Central Store and hydrate from DB
    let store = rt.block_on(async {
        let store = CentralStore::new();
        store.hydrate_from_db(&db).await?;
        tracing::info!("Central Store hydrated");
        Ok::<CentralStore, Box<dyn std::error::Error>>(store)
    })?;

    // 7. Start DB Writer actor (in background)
    let db_writer = DbWriter::new(db.clone(), db_cmd_rx, event_tx.clone());
    rt.spawn(async move {
        db_writer.run().await;
    });
    tracing::info!("DB Writer started");

    // 7b. Start periodic log retention purge (single-writer via DbCommand)
    let db_cmd_tx_for_retention = db_cmd_tx.clone();
    rt.spawn(async move {
        const LOG_RETENTION_DAYS: i64 = 30;
        const PURGE_INTERVAL_SECS: u64 = 3600; // every 1 hour

        let _ = db_cmd_tx_for_retention
            .send(vouchflow::domain::DbCommand::PurgeOldLogs {
                retention_days: LOG_RETENTION_DAYS,
            })
            .await;

        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(PURGE_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(e) = db_cmd_tx_for_retention
                .send(vouchflow::domain::DbCommand::PurgeOldLogs {
                    retention_days: LOG_RETENTION_DAYS,
                })
                .await
            {
                tracing::warn!("Stopping log retention task: {}", e);
                break;
            }
        }
    });
    tracing::info!("Log retention task started (30 days, interval 1 hour)");

    // 7c. Start periodic WAL checkpoint (single-writer via DbCommand)
    let db_cmd_tx_for_checkpoint = db_cmd_tx.clone();
    rt.spawn(async move {
        const CHECKPOINT_INTERVAL_SECS: u64 = 300; // every 5 minutes

        let _ = db_cmd_tx_for_checkpoint
            .send(vouchflow::domain::DbCommand::WalCheckpoint { truncate: false })
            .await;

        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(CHECKPOINT_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(e) = db_cmd_tx_for_checkpoint
                .send(vouchflow::domain::DbCommand::WalCheckpoint { truncate: false })
                .await
            {
                tracing::warn!("Stopping WAL checkpoint task: {}", e);
                break;
            }
        }
    });
    tracing::info!("WAL checkpoint task started (interval 5 minutes, mode PASSIVE)");

    // 8. Start Orchestrator (in background)
    let provider_url = config.terminal_addr();
    let orchestrator = Orchestrator::new(
        command_rx,
        db_cmd_tx.clone(),
        db.clone(),
        format!("http://{}", provider_url),
    );
    rt.spawn(async move {
        orchestrator.run().await;
    });
    tracing::info!("Orchestrator started");

    // 9. Server control state (lazy start - NOT auto-started)
    // Server will be started when user clicks sidebar trigger
    let server_running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_task =
        std::sync::Arc::new(std::sync::Mutex::new(None::<tokio::task::JoinHandle<()>>));
    let command_tx_for_server = command_tx.clone();
    let server_addr = config.server_addr();
    tracing::info!(
        "Server configured on {} (waiting for user to start)",
        server_addr
    );

    // 10. Initialize Slint UI (before event bridge so we can pass weak reference)
    let ui = AppWindow::new()?;

    // Initial backend-driven state
    sync_server_state(&ui, false, "Stopped");
    AppState::get(&ui).set_current_time(chrono::Local::now().format("%H:%M:%S").to_string().into());

    // Keep footer clock updated from Rust backend every second.
    // Also detects midnight date rollover to refresh dashboard date filters.
    let ui_weak_for_clock = ui.as_weak();
    let clock_timer = slint::Timer::default();
    let last_date = std::cell::RefCell::new(
        chrono::Local::now().format("%Y-%m-%d").to_string(),
    );
    clock_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_secs(1),
        move || {
            if let Some(ui) = ui_weak_for_clock.upgrade() {
                let now = chrono::Local::now();
                AppState::get(&ui).set_current_time(
                    now.format("%H:%M:%S").to_string().into(),
                );

                // Midnight rollover detection — cost: ~60ns/tick (string cmp only)
                // DB query fires ONCE per day transition, not every tick
                let today = now.format("%Y-%m-%d").to_string();
                let mut prev = last_date.borrow_mut();
                if *prev != today {
                    tracing::info!("Date changed: {} → {}", *prev, today);
                    *prev = today;
                    drop(prev); // release borrow before calling init_dates
                    callbacks::dashboard::init_dates(&ui);
                    if DashboardState::get(&ui).get_auto_refresh() {
                        DashboardState::get(&ui).invoke_load_transactions();
                    }
                }
            }
        },
    );

    // 11. Start UI event bridge (in background) - connects DomainEvents to Dashboard UI
    let (bridge_tx, bridge_rx) = tokio::sync::mpsc::channel(32);
    let logs_cache = std::sync::Arc::new(std::sync::Mutex::new(
        Vec::<vouchflow::domain::LogEntry>::new(),
    ));
    let event_rx = event_tx.subscribe();
    let store_clone = store.clone();
    let logs_cache_for_events = logs_cache.clone();
    let ui_weak_for_events = ui.as_weak();
    rt.spawn(async move {
        use vouchflow::presentation::ui::UiBridge;
        let bridge = UiBridge::new(store_clone, event_rx, bridge_rx);
        bridge
            .run(move |ui_event| {
                // Trigger Dashboard refresh on relevant events
                let ui_weak = ui_weak_for_events.clone();
                let logs_cache = logs_cache_for_events.clone();
                match ui_event {
                    vouchflow::domain::UiEvent::RefreshMonitoring {
                        total_transactions,
                        success_count,
                        failed_count,
                        ..
                    } => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let state = DashboardState::get(&ui);
                                if !state.get_auto_refresh() {
                                    return;
                                }
                                // Only update counters — no full table reload
                                state.set_total_transactions(total_transactions as i32);
                                state.set_success_count(success_count as i32);
                                state.set_failed_count(failed_count as i32);
                                state.set_pending_count(
                                    total_transactions as i32
                                        - success_count as i32
                                        - failed_count as i32,
                                );
                            }
                        });
                    }
                    vouchflow::domain::UiEvent::TransactionsUpdated(_) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let state = DashboardState::get(&ui);
                                if state.get_auto_refresh() {
                                    // Calls load_transactions which uses TableModel.set_all()
                                    // internally — reuses the persistent VecModel
                                    state.invoke_load_transactions();
                                }
                            }
                        });
                    }
                    vouchflow::domain::UiEvent::LogsUpdated(logs) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let state = LogsState::get(&ui);

                                if let Ok(mut cache) = logs_cache.lock() {
                                    *cache = logs.clone();
                                }

                                if state.get_paused() && !logs.is_empty() {
                                    return;
                                }

                                state.set_logs(build_logs_table_model(
                                    &logs,
                                    state.get_level_filter_index(),
                                ));
                            }
                        });
                    }
                    vouchflow::domain::UiEvent::ConfigLoaded { key, value } => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let state = SettingsState::get(&ui);
                                match key.as_str() {
                                    "server_addr" => state.set_server_address(value.into()),
                                    "server_port" => state.set_server_port(value.into()),
                                    "webhook_addr" => state.set_webhook_address(value.into()),
                                    "webhook_port" => state.set_webhook_port(value.into()),
                                    _ => {}
                                }
                            }
                        });
                    }
                    _ => {}
                }
            })
            .await;
    });

    // Handle tab changes to update active menu in store
    let bridge_tx_clone = bridge_tx.clone();
    let rt_handle = rt.handle().clone();
    AppState::get(&ui).on_tab_changed(move |index| {
        let menu = match index {
            0 => MenuType::Dashboard,
            1 => MenuType::Produk,
            2 => MenuType::MasterData,
            3 => MenuType::Utility,
            4 => MenuType::Logs,
            5 => MenuType::Settings,
            _ => MenuType::Dashboard,
        };

        let tx = bridge_tx_clone.clone();
        rt_handle.spawn(async move {
            let _ = tx.send(BridgeCommand::SetMenu(menu)).await;
        });
    });

    // ===== Logs Callback Setup =====
    {
        let bridge_tx_for_load = bridge_tx.clone();
        let rt_handle = rt.handle().clone();
        let ui_handle = ui.as_weak();
        let logs_cache_ref = logs_cache.clone();
        LogsState::get(&ui).on_load_logs(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let state = LogsState::get(&ui);
                if state.get_paused() {
                    if let Ok(cache) = logs_cache_ref.lock() {
                        state.set_logs(build_logs_table_model(
                            &cache,
                            state.get_level_filter_index(),
                        ));
                    }
                    return;
                }
            }

            let tx = bridge_tx_for_load.clone();
            rt_handle.spawn(async move {
                let _ = tx.send(BridgeCommand::RequestLogs).await;
            });
        });

        let bridge_tx_for_pause = bridge_tx.clone();
        let rt_handle = rt.handle().clone();
        let ui_handle = ui.as_weak();
        LogsState::get(&ui).on_toggle_pause(move || {
            let ui = match ui_handle.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            let state = LogsState::get(&ui);
            let next_paused = !state.get_paused();
            state.set_paused(next_paused);

            if !next_paused {
                let tx = bridge_tx_for_pause.clone();
                rt_handle.spawn(async move {
                    let _ = tx.send(BridgeCommand::RequestLogs).await;
                });
            }
        });

        let db_cmd_tx = db_cmd_tx.clone();
        let bridge_tx_for_clear = bridge_tx.clone();
        let rt_handle = rt.handle().clone();
        let logs_cache_ref = logs_cache.clone();
        LogsState::get(&ui).on_clear_logs(move || {
            let tx = db_cmd_tx.clone();
            let bridge = bridge_tx_for_clear.clone();
            let logs_cache = logs_cache_ref.clone();
            rt_handle.spawn(async move {
                if let Err(e) = tx.send(vouchflow::domain::DbCommand::ClearLogs).await {
                    tracing::error!("Failed to clear logs: {}", e);
                    return;
                }

                if let Ok(mut cache) = logs_cache.lock() {
                    cache.clear();
                }

                let _ = bridge.send(BridgeCommand::RequestLogs).await;
            });
        });
    }

    // 12. Setup window controls
    ui.on_close_window(move || {
        let _ = slint::quit_event_loop();
    });

    let ui_handle = ui.as_weak();
    ui.on_minimize_window(move || {
        if let Some(ui) = ui_handle.upgrade() {
            ui.window().set_minimized(true);
        }
    });

    let ui_handle = ui.as_weak();
    ui.on_maximize_window(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let window = ui.window();
            let new_state = !window.is_maximized();
            window.set_maximized(new_state);
            AppState::get(&ui).set_is_maximized(new_state);
        }
    });

    let ui_handle = ui.as_weak();
    ui.on_move_window(move || {
        if let Some(ui) = ui_handle.upgrade() {
            ui.window().with_winit_window(|winit_window| {
                let _ = winit_window.drag_window();
            });
        }
    });

    // ===== Server Start/Stop Callback =====
    {
        let ui_handle = ui.as_weak();
        let rt_handle = rt.handle().clone();
        let server_running_clone = server_running.clone();
        let server_task_clone = server_task.clone();
        let command_tx_clone = command_tx_for_server.clone();
        let server_addr_clone = server_addr.clone();
        let db_for_server = db.clone();
        let store_server = store.clone();

        AppState::get(&ui).on_toggle_server(move || {
            let ui = match ui_handle.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            let is_running = server_running_clone.load(std::sync::atomic::Ordering::SeqCst);

            if is_running {
                // Stop server
                tracing::info!("Stopping server...");
                sync_server_state(&ui, true, "Stopping");

                if let Ok(mut task_slot) = server_task_clone.lock() {
                    if let Some(task) = task_slot.take() {
                        task.abort();
                    }
                } else {
                    tracing::error!("Failed to acquire server task lock while stopping");
                }

                server_running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
                sync_server_state(&ui, false, "Stopped");
                tracing::info!("Server stopped");
            } else {
                // Start server
                // tracing::info!("Starting server..."); // Logged inside spawn
                sync_server_state(&ui, false, "Starting");
                server_running_clone.store(true, std::sync::atomic::Ordering::SeqCst);

                let gateway = Gateway::new(command_tx_clone.clone(), db_for_server.clone());
                let running_flag = server_running_clone.clone();
                let server_task_cleanup = server_task_clone.clone();
                let ui_weak = ui.as_weak();
                let store = store_server.clone();
                let default_addr = server_addr_clone.clone();

                let task_handle = rt_handle.spawn(async move {
                    // Fetch dynamic config
                    let host = store.get_config("server_addr").await;
                    let port = store.get_config("server_port").await;

                    let addr = if let (Some(h), Some(p)) = (host, port) {
                        format!("{}:{}", h, p)
                    } else {
                        default_addr
                    };

                    tracing::info!("Starting server on {}...", addr);

                    // Update UI to show server is running
                    let ui_weak_for_running = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_for_running.upgrade() {
                            sync_server_state(&ui, true, "Running");
                        }
                    });

                    tracing::info!("API Gateway listening on {}", addr);
                    if let Err(e) = gateway.serve(&addr).await {
                        tracing::error!("Gateway error: {}", e);
                    }

                    running_flag.store(false, std::sync::atomic::Ordering::SeqCst);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            sync_server_state(&ui, false, "Stopped");
                        }
                    });

                    if let Ok(mut task_slot) = server_task_cleanup.lock() {
                        task_slot.take();
                    }
                });

                if let Ok(mut task_slot) = server_task_clone.lock() {
                    *task_slot = Some(task_handle);
                } else {
                    tracing::error!("Failed to acquire server task lock while starting");
                }
            }
        });
    }

    // ===== Register UI Callbacks (extracted modules) =====
    {
        let rt_handle = rt.handle().clone();
        callbacks::produk::register(&ui, &db, &db_cmd_tx, &rt_handle);
        callbacks::stok::register(&ui, &db, &db_cmd_tx, &rt_handle);
        callbacks::utility::register(&ui, &rt_handle);
        callbacks::settings::register(&ui, &store, &db_cmd_tx, &rt_handle);
        callbacks::dashboard::init_dates(&ui);
        callbacks::dashboard::register(&ui, &db, &db_cmd_tx, &command_tx, &rt_handle);
    }

    // Log startup complete
    let _ = db_cmd_tx.try_send(vouchflow::domain::DbCommand::AppendLog {
        level: "INFO".to_string(),
        message: "Application started successfully".to_string(),
        trace_id: None,
    });

    tracing::info!("App-Voucher ready");

    // 12. Run Slint event loop (blocks until UI closes)
    ui.run()?;

    // ===== Graceful Shutdown =====
    tracing::info!("App-Voucher shutting down...");

    // Stop HTTP server if running
    if server_running.load(std::sync::atomic::Ordering::SeqCst) {
        if let Ok(mut task_slot) = server_task.lock() {
            if let Some(task) = task_slot.take() {
                task.abort();
            }
        }
        tracing::info!("HTTP server stopped");
    }

    // Drop command sender to signal orchestrator to stop
    // (command_tx and command_tx_for_server are the only senders)
    drop(command_tx);
    drop(command_tx_for_server);

    // Give background tasks a moment to drain pending DB writes
    rt.block_on(async {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    });

    // Best-effort checkpoint to shrink WAL before writer shutdown
    let _ = rt.block_on(async {
        db_cmd_tx
            .send(vouchflow::domain::DbCommand::WalCheckpoint { truncate: true })
            .await
    });

    // Drop DB command sender to signal DB writer to stop
    drop(db_cmd_tx);

    // Let DB writer finish remaining commands
    rt.block_on(async {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });

    tracing::info!("App-Voucher shutdown complete");

    Ok(())
}
