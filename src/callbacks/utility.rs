//! Utility Page Callbacks (Cek & Redeem)
//!
//! Registers all UtilityState callback handlers for the Slint UI.

use crate::{AppWindow, UtilityState};
use slint::{ComponentHandle, Global, Model, ModelRc, VecModel};

/// Register all utility-related callbacks on the UI
pub fn register(ui: &AppWindow, rt: &tokio::runtime::Handle) {
    use vouchflow::application::providers::{
        ByuProvider, ProviderApi, SmartfrenProvider, TelkomselProvider,
    };

    let rt_handle = rt.clone();
    let redeem_dialog_session = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    // --- Check voucher single ---
    {
        let ui_handle = ui.as_weak();
        let rth = rt_handle.clone();
        UtilityState::get(ui).on_check_voucher_single(move |barcode, provider| {
            let ui_weak = ui_handle.clone();
            let barcode = barcode.to_string();
            let provider = provider.to_string();

            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak2.upgrade() {
                    UtilityState::get(&ui).set_single_checking(true);
                    UtilityState::get(&ui).set_single_result_status("".into());
                    UtilityState::get(&ui).set_single_result_description("".into());
                    UtilityState::get(&ui).set_single_result_expiry("".into());
                    UtilityState::get(&ui).set_single_result_message("".into());
                }
            });

            rth.spawn(async move {
                let result = match provider.as_str() {
                    "Byu" => {
                        let p = ByuProvider::new();
                        p.check_voucher(&barcode).await
                    }
                    "Telkomsel" => {
                        let p = TelkomselProvider::new();
                        p.check_voucher(&barcode).await
                    }
                    _ => Err(
                        vouchflow::application::providers::ProviderError::UnknownProvider(format!(
                            "Provider {} tidak mendukung cek voucher",
                            provider
                        )),
                    ),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        UtilityState::get(&ui).set_single_checking(false);
                        match result {
                            Ok(resp) => {
                                UtilityState::get(&ui).set_single_result_status(resp.status.into());
                                UtilityState::get(&ui).set_single_result_description(
                                    resp.product_name.unwrap_or_default().into(),
                                );
                                UtilityState::get(&ui).set_single_result_expiry(
                                    resp.expiry_date.unwrap_or_default().into(),
                                );
                                if let Some(raw) = &resp.raw_response {
                                    UtilityState::get(&ui).set_single_result_raw(
                                        serde_json::to_string_pretty(raw)
                                            .unwrap_or_default()
                                            .into(),
                                    );
                                }
                            }
                            Err(e) => {
                                UtilityState::get(&ui).set_single_result_status("ERROR".into());
                                UtilityState::get(&ui)
                                    .set_single_result_message(format!("{}", e).into());
                            }
                        }
                    }
                });
            });
        });
    }

    // --- Check voucher bulk ---
    {
        let ui_handle = ui.as_weak();
        let rth = rt_handle.clone();
        UtilityState::get(ui).on_check_voucher_bulk(move |barcodes, provider| {
            use futures::stream::{self, StreamExt};

            let ui_weak = ui_handle.clone();
            let barcodes: Vec<String> = barcodes
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let provider = provider.to_string();

            if barcodes.is_empty() { return; }

            let initial_results: Vec<(String, String, String)> = barcodes
                .iter()
                .map(|barcode| (barcode.clone(), "PENDING".to_string(), "Menunggu response API...".to_string()))
                .collect();

            let ui_weak2 = ui_weak.clone();
            let initial_rows = initial_results.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak2.upgrade() {
                    UtilityState::get(&ui).set_bulk_checking(true);
                    let rows: Vec<slint::ModelRc<slint::StandardListViewItem>> = initial_rows
                        .iter()
                        .map(|(barcode, status, desc)| {
                            let items: Vec<slint::StandardListViewItem> = vec![
                                slint::StandardListViewItem::from(barcode.as_str()),
                                slint::StandardListViewItem::from(status.as_str()),
                                slint::StandardListViewItem::from(desc.as_str()),
                            ];
                            slint::ModelRc::new(slint::VecModel::from(items))
                        })
                        .collect();
                    UtilityState::get(&ui).set_bulk_results(slint::ModelRc::new(slint::VecModel::from(rows)));
                }
            });

            rth.spawn(async move {
                let shared_results = std::sync::Arc::new(tokio::sync::Mutex::new(initial_results));
                let ui_done = ui_weak.clone();

                stream::iter(barcodes.into_iter().enumerate())
                    .for_each_concurrent(None, |(idx, barcode)| {
                        let provider = provider.clone();
                        let shared_results = shared_results.clone();
                        let ui_weak = ui_weak.clone();
                        async move {
                            let check_result = match provider.as_str() {
                                "Byu" => { let p = ByuProvider::new(); p.check_voucher(&barcode).await }
                                "Telkomsel" => { let p = TelkomselProvider::new(); p.check_voucher(&barcode).await }
                                _ => Err(vouchflow::application::providers::ProviderError::UnknownProvider("Unsupported provider".into())),
                            };
                            let (status, description) = match check_result {
                                Ok(resp) => (resp.status, resp.product_name.unwrap_or_default()),
                                Err(e) => ("ERROR".to_string(), format!("{}", e)),
                            };
                            let snapshot = {
                                let mut rows = shared_results.lock().await;
                                if let Some(row) = rows.get_mut(idx) {
                                    row.1 = status;
                                    row.2 = description;
                                }
                                rows.clone()
                            };
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_weak.upgrade() {
                                    let rows: Vec<slint::ModelRc<slint::StandardListViewItem>> = snapshot
                                        .iter()
                                        .map(|(barcode, status, desc)| {
                                            let items: Vec<slint::StandardListViewItem> = vec![
                                                slint::StandardListViewItem::from(barcode.as_str()),
                                                slint::StandardListViewItem::from(status.as_str()),
                                                slint::StandardListViewItem::from(desc.as_str()),
                                            ];
                                            slint::ModelRc::new(slint::VecModel::from(items))
                                        })
                                        .collect();
                                    UtilityState::get(&ui).set_bulk_results(slint::ModelRc::new(slint::VecModel::from(rows)));
                                }
                            });
                        }
                    })
                    .await;

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_done.upgrade() {
                        UtilityState::get(&ui).set_bulk_checking(false);
                    }
                });
            });
        });
    }

    // --- Redeem voucher ---
    {
        let ui_handle = ui.as_weak();
        let rth = rt_handle.clone();
        let countdown_rt = rt_handle.clone();
        let redeem_dialog_session_ref = redeem_dialog_session.clone();
        UtilityState::get(ui).on_redeem_voucher(move |provider, msisdn, voucher_code| {
            let ui_weak = ui_handle.clone();
            let provider = provider.to_string();
            let msisdn = msisdn.to_string();
            let voucher_code = voucher_code.to_string();
            let countdown_rt = countdown_rt.clone();
            let redeem_dialog_session_ref = redeem_dialog_session_ref.clone();

            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak2.upgrade() {
                    UtilityState::get(&ui).set_redeem_processing(true);
                }
            });

            rth.spawn(async move {
                let result = match provider.as_str() {
                    "Byu" => {
                        let p = ByuProvider::new();
                        p.redeem_voucher(&msisdn, &voucher_code).await
                    }
                    "Telkomsel" => {
                        let p = TelkomselProvider::new();
                        p.redeem_voucher(&msisdn, &voucher_code).await
                    }
                    "Smartfren" => {
                        let p = SmartfrenProvider::new();
                        p.redeem_voucher(&msisdn, &voucher_code).await
                    }
                    _ => Err(
                        vouchflow::application::providers::ProviderError::UnknownProvider(format!(
                            "Provider {} tidak dikenal",
                            provider
                        )),
                    ),
                };

                let ui_weak3 = ui_weak.clone();
                let countdown_rt = countdown_rt.clone();
                let redeem_dialog_session_ref = redeem_dialog_session_ref.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        let state = UtilityState::get(&ui);
                        state.set_redeem_processing(false);
                        match result {
                            Ok(resp) => {
                                state.set_redeem_result_success(resp.success);
                                state.set_redeem_result_message(
                                    resp.message
                                        .unwrap_or_else(|| {
                                            if resp.success {
                                                "Redeem berhasil".to_string()
                                            } else {
                                                "Redeem gagal".to_string()
                                            }
                                        })
                                        .into(),
                                );
                                if let Some(raw) = &resp.raw_response {
                                    state.set_redeem_result_raw(
                                        serde_json::to_string_pretty(raw)
                                            .unwrap_or_default()
                                            .into(),
                                    );
                                }
                            }
                            Err(e) => {
                                state.set_redeem_result_success(false);
                                state.set_redeem_result_message(format!("{}", e).into());
                            }
                        }
                        state.set_redeem_dialog_countdown(5);
                        state.set_redeem_dialog_open(true);

                        let session_id = redeem_dialog_session_ref
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                            + 1;
                        let ui_weak_countdown = ui_weak3.clone();
                        let redeem_dialog_session_task = redeem_dialog_session_ref.clone();
                        countdown_rt.spawn(async move {
                            for remaining in (0..5).rev() {
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                if redeem_dialog_session_task
                                    .load(std::sync::atomic::Ordering::SeqCst)
                                    != session_id
                                {
                                    break;
                                }
                                let ui_weak_tick = ui_weak_countdown.clone();
                                let redeem_dialog_session_tick = redeem_dialog_session_task.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if redeem_dialog_session_tick
                                        .load(std::sync::atomic::Ordering::SeqCst)
                                        != session_id
                                    {
                                        return;
                                    }
                                    if let Some(ui) = ui_weak_tick.upgrade() {
                                        let state = UtilityState::get(&ui);
                                        if !state.get_redeem_dialog_open() {
                                            return;
                                        }
                                        state.set_redeem_dialog_countdown(remaining);
                                        if remaining == 0 {
                                            state.set_redeem_dialog_open(false);
                                        }
                                    }
                                });
                                if remaining == 0 {
                                    break;
                                }
                            }
                        });
                    }
                });
            });
        });
    }

    // --- Close redeem dialog ---
    {
        let ui_handle = ui.as_weak();
        let redeem_dialog_session_ref = redeem_dialog_session.clone();
        UtilityState::get(ui).on_close_redeem_dialog(move || {
            redeem_dialog_session_ref.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if let Some(ui) = ui_handle.upgrade() {
                let state = UtilityState::get(&ui);
                state.set_redeem_dialog_open(false);
                state.set_redeem_dialog_countdown(5);
            }
        });
    }

    // --- Sort bulk results ---
    {
        let ui_handle = ui.as_weak();
        UtilityState::get(ui).on_sort_bulk_results(move |column_index, ascending| {
            let column_index = column_index as usize;
            let ui_handle_clone = ui_handle.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle_clone.upgrade() {
                    let state = UtilityState::get(&ui);
                    let model = state.get_bulk_results();

                    let mut data: Vec<slint::ModelRc<slint::StandardListViewItem>> = Vec::new();
                    let count = model.row_count();
                    for i in 0..count {
                        if let Some(row) = model.row_data(i) {
                            data.push(row);
                        }
                    }

                    if data.len() <= 1 {
                        return; // Nothing to sort
                    }

                    data.sort_by(|a, b| {
                        let text_a = a
                            .row_data(column_index)
                            .map(|item| item.text.to_string())
                            .unwrap_or_default();
                        let text_b = b
                            .row_data(column_index)
                            .map(|item| item.text.to_string())
                            .unwrap_or_default();

                        // Try parsing as float for numeric sorting
                        let cmp = match (text_a.parse::<f64>(), text_b.parse::<f64>()) {
                            (Ok(num_a), Ok(num_b)) => num_a
                                .partial_cmp(&num_b)
                                .unwrap_or(std::cmp::Ordering::Equal),
                            _ => text_a.cmp(&text_b),
                        };

                        if ascending { cmp } else { cmp.reverse() }
                    });

                    // Create a new VecModel and set it (simpler than modifying in-place for this case as it doesn't have IDs)
                    state.set_bulk_results(slint::ModelRc::new(slint::VecModel::from(data)));
                }
            });
        });
    }
}
