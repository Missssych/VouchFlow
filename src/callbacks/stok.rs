//! Stock Voucher CRUD Callbacks
//!
//! Registers all StokState callback handlers for the Slint UI.
use super::model_helpers::{TableId, with_table};
use crate::{AppWindow, StokState};
use slint::{ComponentHandle, Global, Model};
use vouchflow::infrastructure::channels::DbCommandSender;
use vouchflow::infrastructure::database::Database;

fn format_date_cell(s: &str) -> String {
    if s.len() >= 16 {
        s[0..16].replace("T", " ")
    } else {
        s.to_string()
    }
}

fn format_timestamp_to_local(s: &str) -> String {
    let ts = s.trim();
    if ts.is_empty() {
        return String::new();
    }

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
    }

    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(ts, fmt) {
            let utc =
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc);
            return utc
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string();
        }
    }

    format_date_cell(ts)
}

/// Register all stok-related callbacks on the UI
pub fn register(
    ui: &AppWindow,
    db: &Database,
    db_cmd_tx: &DbCommandSender,
    rt: &tokio::runtime::Handle,
) {
    use vouchflow::application::services;

    let db_clone = db.clone();
    let db_cmd_tx_clone = db_cmd_tx.clone();
    let rt_handle = rt.clone();

    // --- Load active stocks ---
    {
        let ui_handle = ui.as_weak();
        let db_ref = db_clone.clone();
        let rth = rt_handle.clone();
        StokState::get(ui).on_load_active_stocks(move || {
            tracing::info!("Loading active stocks...");
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            let (provider, search) = {
                if let Some(ui) = ui_weak.upgrade() {
                    let state = StokState::get(&ui);
                    let idx = state.get_provider_filter_index();
                    let providers = state.get_providers();
                    let prov = if idx > 0 && (idx as usize) < providers.iter().count() {
                        providers
                            .row_data(idx as usize)
                            .unwrap_or("Semua".into())
                            .to_string()
                    } else {
                        "Semua".to_string()
                    };
                    let sq = state.get_search_query_tab0().to_string();
                    (prov, sq)
                } else {
                    ("Semua".to_string(), String::new())
                }
            };
            rth.spawn(async move {
                let search_opt = if search.is_empty() {
                    None
                } else {
                    Some(search.as_str())
                };
                match services::get_active_stocks(&db, Some(provider.as_str()), search_opt).await {
                    Ok(stocks) => {
                        tracing::info!("Loaded {} active stocks", stocks.len());
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let ids: Vec<i32> = stocks.iter().map(|s| s.id as i32).collect();
                                let rows: Vec<slint::ModelRc<slint::StandardListViewItem>> = stocks
                                    .iter()
                                    .map(|s| {
                                        let items: Vec<slint::StandardListViewItem> = vec![
                                            slint::StandardListViewItem::from(s.provider.as_str()),
                                            slint::StandardListViewItem::from(
                                                s.kode_addon.as_str(),
                                            ),
                                            slint::StandardListViewItem::from(s.barcode.as_str()),
                                            slint::StandardListViewItem::from(
                                                s.serial_number.as_str(),
                                            ),
                                            slint::StandardListViewItem::from(
                                                format_date_cell(&s.expired_date).as_str(),
                                            ),
                                            slint::StandardListViewItem::from(s.status.as_str()),
                                            slint::StandardListViewItem::from(
                                                format_timestamp_to_local(&s.created_at).as_str(),
                                            ),
                                        ];
                                        slint::ModelRc::new(slint::VecModel::from(items))
                                    })
                                    .collect();
                                let state = StokState::get(&ui);
                                let (rows_model, ids_model) =
                                    with_table(TableId::StokActive, |m| m.set_all(rows, ids));
                                state.set_active_stocks(rows_model);
                                state.set_active_stock_ids(ids_model);
                            }
                        });
                    }
                    Err(e) => tracing::error!("Failed to load active stocks: {}", e),
                }
            });
        });
    }

    // --- Load used stocks ---
    {
        let ui_handle = ui.as_weak();
        let db_ref = db_clone.clone();
        let rth = rt_handle.clone();
        StokState::get(ui).on_load_used_stocks(move |date_from, date_to| {
            tracing::info!("Loading used stocks from {} to {}", date_from, date_to);
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            let from = if date_from.is_empty() {
                None
            } else {
                Some(date_from.to_string())
            };
            let to = if date_to.is_empty() {
                None
            } else {
                Some(date_to.to_string())
            };
            let (provider, search) = {
                if let Some(ui) = ui_weak.upgrade() {
                    let state = StokState::get(&ui);
                    let idx = state.get_provider_filter_index();
                    let providers = state.get_providers();
                    let prov = if idx > 0 && (idx as usize) < providers.iter().count() {
                        providers
                            .row_data(idx as usize)
                            .unwrap_or("Semua".into())
                            .to_string()
                    } else {
                        "Semua".to_string()
                    };
                    let sq = state.get_search_query_tab1().to_string();
                    (prov, sq)
                } else {
                    ("Semua".to_string(), String::new())
                }
            };
            rth.spawn(async move {
                let search_opt = if search.is_empty() {
                    None
                } else {
                    Some(search.as_str())
                };
                match services::get_used_stocks(
                    &db,
                    Some(provider.as_str()),
                    from.as_deref(),
                    to.as_deref(),
                    search_opt,
                )
                .await
                {
                    Ok(stocks) => {
                        tracing::info!("Loaded {} used stocks", stocks.len());
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let ids: Vec<i32> = stocks.iter().map(|s| s.id as i32).collect();
                                let rows: Vec<slint::ModelRc<slint::StandardListViewItem>> = stocks
                                    .iter()
                                    .map(|s| {
                                        let items: Vec<slint::StandardListViewItem> = vec![
                                            slint::StandardListViewItem::from(s.provider.as_str()),
                                            slint::StandardListViewItem::from(
                                                s.kode_addon.as_str(),
                                            ),
                                            slint::StandardListViewItem::from(s.barcode.as_str()),
                                            slint::StandardListViewItem::from(
                                                s.serial_number.as_str(),
                                            ),
                                            slint::StandardListViewItem::from(
                                                format_date_cell(&s.expired_date).as_str(),
                                            ),
                                            slint::StandardListViewItem::from(s.status.as_str()),
                                            slint::StandardListViewItem::from(
                                                format_timestamp_to_local(&s.created_at).as_str(),
                                            ),
                                        ];
                                        slint::ModelRc::new(slint::VecModel::from(items))
                                    })
                                    .collect();
                                let state = StokState::get(&ui);
                                let (rows_model, ids_model) =
                                    with_table(TableId::StokUsed, |m| m.set_all(rows, ids));
                                state.set_used_stocks(rows_model);
                                state.set_used_stock_ids(ids_model);
                            }
                        });
                    }
                    Err(e) => tracing::error!("Failed to load used stocks: {}", e),
                }
            });
        });
    }

    // --- Save stock ---
    {
        let ui_handle = ui.as_weak();
        let tx = db_cmd_tx_clone.clone();
        let rth = rt_handle.clone();
        StokState::get(ui).on_save_stock(
            move |id, provider, kode_addon, barcode, serial, expired| {
                tracing::info!(
                    "Saving stock: id={}, provider={}, kode_addon={}",
                    id,
                    provider,
                    kode_addon
                );
                let ui_weak = ui_handle.clone();
                let cmd_tx = tx.clone();
                let provider_str = provider.to_string();
                let kode_addon_str = kode_addon.to_string();
                let barcode_str = barcode.to_string();
                let serial_str = serial.to_string();
                let expired_input = expired.to_string();
                let expired_opt =
                    match vouchflow::utils::normalize_expired_date_optional(&expired_input) {
                        Ok(v) => v,
                        Err(e) => {
                            if let Some(ui) = ui_weak.upgrade() {
                                let state = StokState::get(&ui);
                                state.set_form_error_message(e.into());
                            }
                            return;
                        }
                    };
                let expired_str = expired_opt.clone().unwrap_or_default();
                let is_create = id < 0;
                let row_id = id;
                if let Some(ui) = ui_weak.upgrade() {
                    let state = StokState::get(&ui);
                    state.set_form_error_message("".into());
                    state.set_form_expired_date(expired_str.clone().into());
                }

                rth.spawn(async move {
                    let cmd = if is_create {
                        vouchflow::domain::DbCommand::CreateStokVoucher {
                            provider: provider_str.clone(),
                            kode_addon: kode_addon_str.clone(),
                            barcode: barcode_str.clone(),
                            serial_number: serial_str.clone(),
                            expired_date: expired_opt,
                        }
                    } else {
                        vouchflow::domain::DbCommand::UpdateStokVoucher {
                            id: row_id as i64,
                            provider: provider_str.clone(),
                            kode_addon: kode_addon_str.clone(),
                            barcode: barcode_str.clone(),
                            serial_number: serial_str.clone(),
                            expired_date: expired_opt,
                        }
                    };
                    if let Err(e) = cmd_tx.send(cmd).await {
                        tracing::error!("Failed to save stock: {}", e);
                    } else {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
                                let row = slint::ModelRc::new(slint::VecModel::from(vec![
                                    slint::StandardListViewItem::from(provider_str.as_str()),
                                    slint::StandardListViewItem::from(kode_addon_str.as_str()),
                                    slint::StandardListViewItem::from(barcode_str.as_str()),
                                    slint::StandardListViewItem::from(serial_str.as_str()),
                                    slint::StandardListViewItem::from(
                                        format_date_cell(&expired_str).as_str(),
                                    ),
                                    slint::StandardListViewItem::from("ACTIVE"),
                                    slint::StandardListViewItem::from(now.as_str()),
                                ]));

                                if is_create {
                                    let state = StokState::get(&ui);
                                    state.invoke_next_barcode();
                                    state.set_form_serial_number("".into());
                                    with_table(TableId::StokActive, |m| m.push_front(-1, row));
                                    // Reload after delay to get real DB ID
                                    let ui_weak2 = ui.as_weak();
                                    slint::Timer::single_shot(
                                        std::time::Duration::from_millis(300),
                                        move || {
                                            if let Some(ui) = ui_weak2.upgrade() {
                                                StokState::get(&ui).invoke_load_active_stocks();
                                            }
                                        },
                                    );
                                } else {
                                    with_table(TableId::StokActive, |m| m.update_row(row_id, row));
                                }
                            }
                        });
                    }
                });
            },
        );
    }

    // --- Increment form barcode for next input ---
    {
        let ui_handle = ui.as_weak();
        StokState::get(ui).on_next_barcode(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let state = StokState::get(&ui);
                let current = state.get_form_barcode().to_string();
                let trimmed = current.trim();
                if trimmed.is_empty() {
                    return;
                }

                match trimmed.parse::<u64>().ok().and_then(|n| n.checked_add(1)) {
                    Some(next) => state.set_form_barcode(next.to_string().into()),
                    None => tracing::warn!(
                        "Cannot increment barcode '{}': not numeric or overflow",
                        current
                    ),
                }
            }
        });
    }

    // --- Delete stocks ---
    {
        let ui_handle = ui.as_weak();
        let tx = db_cmd_tx_clone.clone();
        let rth = rt_handle.clone();
        StokState::get(ui).on_delete_stocks(move |ids| {
            let id_vec: Vec<i64> = ids.iter().map(|id| id as i64).collect();
            let id_i32s: Vec<i32> = ids.iter().collect();
            tracing::info!("Deleting stocks: {:?}", id_vec);
            let ui_weak = ui_handle.clone();
            let cmd_tx = tx.clone();
            rth.spawn(async move {
                let cmd = vouchflow::domain::DbCommand::DeleteStokVouchers { ids: id_vec };
                if let Err(e) = cmd_tx.send(cmd).await {
                    tracing::error!("Failed to delete stocks: {}", e);
                } else {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(_ui) = ui_weak.upgrade() {
                            // Remove rows directly — no DB re-query
                            with_table(TableId::StokActive, |m| m.remove_by_ids(&id_i32s));
                        }
                    });
                }
            });
        });
    }

    // --- Change status ---
    {
        let ui_handle = ui.as_weak();
        let tx = db_cmd_tx_clone.clone();
        let rth = rt_handle.clone();
        StokState::get(ui).on_change_status(move |ids, new_status| {
            let id_vec: Vec<i64> = ids.iter().map(|id| id as i64).collect();
            let id_i32s: Vec<i32> = ids.iter().collect();
            let status = new_status.to_string();
            tracing::info!("Changing status to {} for stocks: {:?}", status, id_vec);
            let ui_weak = ui_handle.clone();
            let cmd_tx = tx.clone();
            let status_clone = status.clone();
            rth.spawn(async move {
                let cmd = vouchflow::domain::DbCommand::ChangeStokStatus {
                    ids: id_vec,
                    new_status: status,
                };
                if let Err(e) = cmd_tx.send(cmd).await {
                    tracing::error!("Failed to change status: {}", e);
                } else {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(_ui) = ui_weak.upgrade() {
                            // Remove from current tab — items moved to the other tab
                            if status_clone == "USED" {
                                with_table(TableId::StokActive, |m| m.remove_by_ids(&id_i32s));
                            } else {
                                with_table(TableId::StokUsed, |m| m.remove_by_ids(&id_i32s));
                            }
                        }
                    });
                }
            });
        });
    }

    // --- Load stock for edit ---
    {
        let ui_handle = ui.as_weak();
        let db_ref = db_clone.clone();
        let rth = rt_handle.clone();
        StokState::get(ui).on_load_stock_for_edit(move |id| {
            tracing::info!("Loading stock for edit: id={}", id);
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            rth.spawn(async move {
                match services::get_stock_by_id(&db, id as i64).await {
                    Ok(Some(stock)) => {
                        let provider_idx = match stock.provider.as_str() {
                            "Byu" => 0,
                            "Smartfren" => 1,
                            "Telkomsel" => 2,
                            _ => 0,
                        };
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let state = StokState::get(&ui);
                                state.set_edit_stock_id(stock.id as i32);
                                state.set_form_provider_index(provider_idx);
                                state.set_form_kode_addon(stock.kode_addon.into());
                                state.set_form_barcode(stock.barcode.into());
                                state.set_form_serial_number(stock.serial_number.into());
                                state.set_form_expired_date(stock.expired_date.into());
                                state.set_form_error_message("".into());
                                state.set_form_panel_open(true);
                            }
                        });
                    }
                    Ok(None) => tracing::warn!("Stock not found: {}", id),
                    Err(e) => tracing::error!("Failed to load stock: {}", e),
                }
            });
        });
    }

    // --- Load stock summary ---
    {
        let ui_handle = ui.as_weak();
        let db_ref = db_clone.clone();
        let rth = rt_handle.clone();
        StokState::get(ui).on_load_stock_summary(move || {
            tracing::info!("Loading stock summary...");
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            rth.spawn(async move {
                match services::get_stock_addon_summary(&db).await {
                    Ok(summary) => {
                        tracing::info!("Loaded stock summary: {} items", summary.len());
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let rows: Vec<slint::ModelRc<slint::StandardListViewItem>> =
                                    summary
                                        .iter()
                                        .map(|s| {
                                            let items: Vec<slint::StandardListViewItem> = vec![
                                                slint::StandardListViewItem::from(
                                                    s.kode_addon.as_str(),
                                                ),
                                                slint::StandardListViewItem::from(
                                                    format!("{}", s.count).as_str(),
                                                ),
                                            ];
                                            slint::ModelRc::new(slint::VecModel::from(items))
                                        })
                                        .collect();
                                StokState::get(&ui).set_stock_summary(slint::ModelRc::new(
                                    slint::VecModel::from(rows),
                                ));
                            }
                        });
                    }
                    Err(e) => tracing::error!("Failed to load stock summary: {}", e),
                }
            });
        });
    }

    // --- Range-based delete ---
    {
        let tx = db_cmd_tx_clone.clone();
        StokState::get(ui).on_delete_stocks_by_range(move |start, end, all_ids| {
            let start = start as usize;
            let end = end as usize;
            let mut ids = Vec::new();
            let mut id_i32s = Vec::new();
            let row_count = all_ids.row_count();
            if start <= end && end < row_count {
                for i in start..=end {
                    if let Some(id) = all_ids.row_data(i) {
                        ids.push(id as i64);
                        id_i32s.push(id);
                    }
                }
            }
            tracing::info!(
                "Deleting stocks by range {}-{} ({} items): {:?}",
                start,
                end,
                ids.len(),
                ids
            );
            if !ids.is_empty() {
                match tx.try_send(vouchflow::domain::DbCommand::DeleteStokVouchers { ids }) {
                    Ok(()) => {
                        // Remove rows directly — no timer delay or DB re-query
                        with_table(TableId::StokActive, |m| m.remove_by_ids(&id_i32s));
                    }
                    Err(e) => tracing::error!("Failed to send delete command: {}", e),
                }
            }
        });
    }

    // --- Range-based change status ---
    {
        let tx = db_cmd_tx_clone.clone();
        StokState::get(ui).on_change_status_by_range(move |start, end, all_ids, new_status| {
            let start = start as usize;
            let end = end as usize;
            let mut ids = Vec::new();
            let mut id_i32s = Vec::new();
            let row_count = all_ids.row_count();
            if start <= end && end < row_count {
                for i in start..=end {
                    if let Some(id) = all_ids.row_data(i) {
                        ids.push(id as i64);
                        id_i32s.push(id);
                    }
                }
            }
            tracing::info!(
                "Changing status to {} for stocks range {}-{} ({} items): {:?}",
                new_status,
                start,
                end,
                ids.len(),
                ids
            );
            if !ids.is_empty() {
                let new_status_str = new_status.to_string();
                match tx.try_send(vouchflow::domain::DbCommand::ChangeStokStatus {
                    ids,
                    new_status: new_status.to_string(),
                }) {
                    Ok(()) => {
                        // Remove from current tab — items moved to other tab
                        if new_status_str == "USED" {
                            with_table(TableId::StokActive, |m| m.remove_by_ids(&id_i32s));
                        } else {
                            with_table(TableId::StokUsed, |m| m.remove_by_ids(&id_i32s));
                        }
                    }
                    Err(e) => tracing::error!("Failed to send change status command: {}", e),
                }
            }
        });
    }

    // --- Check voucher ---
    {
        let ui_handle = ui.as_weak();
        let rth = rt_handle.clone();
        let countdown_rt = rt_handle.clone();
        let stok_check_dialog_session = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let stok_check_dialog_session_ref = stok_check_dialog_session.clone();
        StokState::get(ui).on_check_voucher(move |barcode, provider| {
            use vouchflow::application::providers::{
                ByuProvider, ProviderApi, SmartfrenProvider, TelkomselProvider,
            };

            let barcode = barcode.to_string();
            let provider = provider.to_string();
            tracing::info!(
                "Checking voucher: barcode={}, provider={}",
                barcode,
                provider
            );

            let ui_weak = ui_handle.clone();
            let countdown_rt = countdown_rt.clone();
            let stok_check_dialog_session_ref = stok_check_dialog_session_ref.clone();

            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak2.upgrade() {
                    let state = StokState::get(&ui);
                    state.set_is_checking(true);
                    state.set_check_dialog_open(true);
                    state.set_check_dialog_countdown(5);
                    state.set_check_result_product("".into());
                    state.set_check_result_expiry("".into());
                    state.set_check_result_status("".into());
                    state.set_check_result_message("".into());
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
                    "Smartfren" => {
                        let p = SmartfrenProvider::new();
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
                        let state = StokState::get(&ui);
                        state.set_is_checking(false);
                        match result {
                            Ok(resp) => {
                                state.set_check_result_product(
                                    resp.product_name.unwrap_or_default().into(),
                                );
                                let expiry_date = resp.expiry_date.clone().unwrap_or_default();
                                let normalized_expiry =
                                    vouchflow::utils::normalize_expired_date_optional(&expiry_date)
                                        .ok()
                                        .flatten()
                                        .unwrap_or_else(|| {
                                            expiry_date
                                                .split(|c| c == ' ' || c == 'T')
                                                .next()
                                                .unwrap_or(&expiry_date)
                                                .to_string()
                                        });
                                state.set_check_result_expiry(normalized_expiry.into());
                                state.set_check_result_status(resp.status.into());
                                state.set_check_result_message("Voucher berhasil dicek".into());
                            }
                            Err(e) => {
                                state.set_check_result_status("ERROR".into());
                                state.set_check_result_message(format!("{}", e).into());
                            }
                        }
                        state.set_check_dialog_countdown(5);
                        let session_id = stok_check_dialog_session_ref
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                            + 1;
                        let ui_weak_countdown = ui.as_weak();
                        let stok_check_dialog_session_task = stok_check_dialog_session_ref.clone();
                        countdown_rt.spawn(async move {
                            for remaining in (0..5).rev() {
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                if stok_check_dialog_session_task
                                    .load(std::sync::atomic::Ordering::SeqCst)
                                    != session_id
                                {
                                    break;
                                }
                                let ui_weak_tick = ui_weak_countdown.clone();
                                let stok_check_dialog_session_tick =
                                    stok_check_dialog_session_task.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if stok_check_dialog_session_tick
                                        .load(std::sync::atomic::Ordering::SeqCst)
                                        != session_id
                                    {
                                        return;
                                    }
                                    if let Some(ui) = ui_weak_tick.upgrade() {
                                        let state = StokState::get(&ui);
                                        if !state.get_check_dialog_open() || state.get_is_checking()
                                        {
                                            return;
                                        }
                                        state.set_check_dialog_countdown(remaining);
                                        if remaining == 0 {
                                            state.set_check_dialog_open(false);
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

    // --- Check expired ---
    {
        let ui_handle = ui.as_weak();
        let rth = rt_handle.clone();
        StokState::get(ui).on_check_expired(move |provider, barcode| {
            use vouchflow::application::providers::{
                ByuProvider, ProviderApi, SmartfrenProvider, TelkomselProvider,
            };

            let provider = provider.to_string();
            let barcode = barcode.to_string();
            tracing::info!(
                "Checking expired: provider={}, barcode={}",
                provider,
                barcode
            );
            let ui_weak = ui_handle.clone();

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
                    "Smartfren" => {
                        let p = SmartfrenProvider::new();
                        p.check_voucher(&barcode).await
                    }
                    _ => Err(
                        vouchflow::application::providers::ProviderError::UnknownProvider(format!(
                            "Provider {} tidak mendukung cek expired",
                            provider
                        )),
                    ),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        match result {
                            Ok(resp) => {
                                if let Some(expiry) = resp.expiry_date {
                                    match vouchflow::utils::normalize_expired_date_optional(&expiry)
                                    {
                                        Ok(Some(normalized)) => {
                                            tracing::info!(
                                                "Setting form-expired-date to: {}",
                                                normalized
                                            );
                                            let state = StokState::get(&ui);
                                            state.set_form_expired_date(normalized.into());
                                            state.set_form_error_message("".into());
                                        }
                                        Ok(None) => {
                                            let state = StokState::get(&ui);
                                            state.set_form_expired_date("".into());
                                            state.set_form_error_message("".into());
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "Unsupported provider expiry format: {} ({})",
                                                expiry,
                                                e
                                            );
                                            StokState::get(&ui).set_form_error_message(
                                                format!(
                                                    "Format expiry dari provider tidak dikenali: {}",
                                                    expiry
                                                )
                                                .into(),
                                            );
                                        }
                                    }
                                } else {
                                    tracing::warn!("No expiry_date in response");
                                }
                            }
                            Err(e) => tracing::error!("Failed to check expired: {}", e),
                        }
                    }
                });
            });
        });
    }

    // --- Sort active stocks ---
    {
        StokState::get(ui).on_sort_active_stocks(move |column_index, ascending| {
            let _ = slint::invoke_from_event_loop(move || {
                with_table(TableId::StokActive, |m| {
                    m.sort_by_column(column_index as usize, ascending)
                });
            });
        });
    }

    // --- Sort used stocks ---
    {
        StokState::get(ui).on_sort_used_stocks(move |column_index, ascending| {
            let _ = slint::invoke_from_event_loop(move || {
                with_table(TableId::StokUsed, |m| {
                    m.sort_by_column(column_index as usize, ascending)
                });
            });
        });
    }
}
