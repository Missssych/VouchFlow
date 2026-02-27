//! Settings Callbacks
//!
//! Registers all SettingsState callback handlers for the Slint UI.

use crate::{AppState, AppWindow, SettingsState};
use slint::{ComponentHandle, Global};
use vouchflow::application::CentralStore;
use vouchflow::infrastructure::channels::DbCommandSender;

/// Register all settings-related callbacks on the UI
pub fn register(
    ui: &AppWindow,
    store: &CentralStore,
    db_cmd_tx: &DbCommandSender,
    rt: &tokio::runtime::Handle,
) {
    // --- Load config ---
    {
        let store = store.clone();
        let rt_handle = rt.clone();
        let ui_handle = ui.as_weak();
        SettingsState::get(ui).on_load_config(move || {
            let store = store.clone();
            let ui_weak = ui_handle.clone();
            rt_handle.spawn(async move {
                let server_addr = store.get_config("server_addr").await;
                let server_port = store.get_config("server_port").await;
                let webhook_addr = store.get_config("webhook_addr").await;
                let webhook_port = store.get_config("webhook_port").await;

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        let state = SettingsState::get(&ui);
                        if let Some(v) = server_addr {
                            state.set_server_address(v.into());
                        }
                        if let Some(v) = server_port {
                            state.set_server_port(v.into());
                        }
                        if let Some(v) = webhook_addr {
                            state.set_webhook_address(v.into());
                        }
                        if let Some(v) = webhook_port {
                            state.set_webhook_port(v.into());
                        }
                    }
                });
            });
        });
    }

    // --- Save config ---
    {
        let db_cmd_tx = db_cmd_tx.clone();
        let rt_handle = rt.clone();
        let ui_handle = ui.as_weak();
        SettingsState::get(ui).on_save_config(
            move |server_addr, server_port, webhook_addr, webhook_port| {
                let tx = db_cmd_tx.clone();
                let ui_weak = ui_handle.clone();

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        let state = SettingsState::get(&ui);
                        state.set_is_saving(true);
                        state.set_save_status("Saving...".into());
                    }
                });

                let ui_weak = ui_handle.clone();
                let server_addr = server_addr.to_string();
                let server_port = server_port.to_string();
                let webhook_addr = webhook_addr.to_string();
                let webhook_port = webhook_port.to_string();

                rt_handle.spawn(async move {
                    let _ = tx
                        .send(vouchflow::domain::DbCommand::SaveConfig {
                            key: "server_addr".into(),
                            value: server_addr,
                            category: "NETWORK".into(),
                        })
                        .await;
                    let _ = tx
                        .send(vouchflow::domain::DbCommand::SaveConfig {
                            key: "server_port".into(),
                            value: server_port,
                            category: "NETWORK".into(),
                        })
                        .await;
                    let _ = tx
                        .send(vouchflow::domain::DbCommand::SaveConfig {
                            key: "webhook_addr".into(),
                            value: webhook_addr,
                            category: "NETWORK".into(),
                        })
                        .await;
                    let _ = tx
                        .send(vouchflow::domain::DbCommand::SaveConfig {
                            key: "webhook_port".into(),
                            value: webhook_port,
                            category: "NETWORK".into(),
                        })
                        .await;

                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            let state = SettingsState::get(&ui);
                            state.set_is_saving(false);
                            state.set_save_status("Saved".into());

                            let ui_weak2 = ui.as_weak();
                            let _ = slint::Timer::single_shot(
                                std::time::Duration::from_secs(2),
                                move || {
                                    if let Some(ui) = ui_weak2.upgrade() {
                                        SettingsState::get(&ui).set_save_status("".into());
                                    }
                                },
                            );
                        }
                    });
                });
            },
        );
    }

    // --- Stop server ---
    {
        let ui_handle = ui.as_weak();
        SettingsState::get(ui).on_stop_server(move || {
            let ui_weak = ui_handle.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    AppState::get(&ui).invoke_toggle_server();
                }
            });
        });
    }

    // --- Start server ---
    {
        let ui_handle = ui.as_weak();
        SettingsState::get(ui).on_start_server(move || {
            let ui_weak = ui_handle.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    AppState::get(&ui).invoke_toggle_server();
                }
            });
        });
    }
}
