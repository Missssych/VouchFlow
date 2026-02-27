//! Dashboard Admin Actions Callbacks
//!
//! Registers all DashboardState callback handlers for the Slint UI.

use super::model_helpers::{TableId, with_table};
use crate::{AppWindow, DashboardState, StokState};
use slint::{ComponentHandle, Global, Model};
use vouchflow::domain::DomainError;
use vouchflow::infrastructure::channels::{CommandSender, DbCommandSender};
use vouchflow::infrastructure::database::Database;

fn format_timestamp_to_local_dashboard(s: &str) -> String {
    let ts = s.trim();
    if ts.is_empty() {
        return String::new();
    }

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
    }

    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(ts, fmt) {
            let utc = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                naive,
                chrono::Utc,
            );
            return utc
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string();
        }
    }

    if ts.len() >= 19 {
        ts[0..19].replace("T", " ")
    } else {
        ts.replace("T", " ")
    }
}

/// Set current date on DashboardState and StokState date pickers
pub fn init_dates(ui: &AppWindow) {
    let now = chrono::Local::now();
    let year = now.format("%Y").to_string().parse::<i32>().unwrap_or(2026);
    let month = now.format("%-m").to_string().parse::<i32>().unwrap_or(1);
    let day = now.format("%-d").to_string().parse::<i32>().unwrap_or(1);
    let today_iso = now.format("%Y-%m-%d").to_string();

    let dashboard_state = DashboardState::get(ui);
    dashboard_state.set_current_year(year);
    dashboard_state.set_current_month(month);
    dashboard_state.set_current_day(day);
    dashboard_state.set_date_from(today_iso.clone().into());
    dashboard_state.set_date_to(today_iso.clone().into());

    let stok_state = StokState::get(ui);
    stok_state.set_current_year(year);
    stok_state.set_current_month(month);
    stok_state.set_current_day(day);
    stok_state.set_date_from(today_iso.clone().into());
    stok_state.set_date_to(today_iso.into());
}

/// Register all dashboard-related callbacks on the UI
pub fn register(
    ui: &AppWindow,
    db: &Database,
    db_cmd_tx: &DbCommandSender,
    command_tx: &CommandSender,
    rt: &tokio::runtime::Handle,
) {
    let db_dashboard = db.clone();
    let db_cmd_tx_dashboard = db_cmd_tx.clone();
    let command_tx_dashboard = command_tx.clone();
    let rt_dashboard = rt.clone();
    let voucher_dialog_session = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    // --- Get transaction detail ---
    {
        let ui_handle = ui.as_weak();
        let db_ref = db_dashboard.clone();
        let rth = rt_dashboard.clone();
        DashboardState::get(ui).on_get_transaction_detail(move |tx_id| {
            tracing::info!("Get transaction detail requested for rowid: {}", tx_id);
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            let tx_rowid = tx_id as i64;
            rth.spawn(async move {
                let result = db.with_reader(|conn| {
                    let tx = conn.query_row(
                        "SELECT tx_id, request_id, provider, kategori, harga, produk, nomor, sn,
                                status, result_code, result_payload,
                                COALESCE(created_at, '')
                         FROM transactions WHERE rowid = ?",
                        [tx_rowid],
                        |row| Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, f64>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, Option<String>>(9)?,
                            row.get::<_, Option<String>>(10)?,
                            row.get::<_, String>(11)?,
                        ))
                    )?;
                    let logs = conn.prepare(
                        "SELECT attempt, seq, level, stage, COALESCE(status, ''), message, COALESCE(payload, ''), COALESCE(created_at, ''),
                                latency_ms
                         FROM transaction_logs
                         WHERE tx_id = ?
                         ORDER BY attempt ASC, seq ASC, id ASC
                         LIMIT 400"
                    ).and_then(|mut stmt| {
                        let r: Vec<(i32, i32, String, String, String, String, String, String, Option<i64>)> = stmt.query_map(
                            [&tx.0],
                            |row| Ok((
                                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                                row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?,
                            ))
                        )?.filter_map(|r| r.ok()).collect();
                        Ok(r)
                    }).unwrap_or_default();
                    Ok((tx, logs))
                }).await;

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        let state = DashboardState::get(&ui);
                        match result {
                            Ok((tx, logs)) => {
                                state.set_detail_tx_id(tx.0.as_str().into());
                                state.set_detail_request_id(tx.1.as_str().into());
                                state.set_detail_provider(tx.2.as_str().into());
                                state.set_detail_kategori(tx.3.as_str().into());
                                state.set_detail_produk(tx.5.as_str().into());
                                state.set_detail_nomor(tx.6.as_str().into());
                                state.set_detail_status(tx.8.as_str().into());
                                state.set_detail_result_code(tx.9.as_deref().unwrap_or("").into());
                                state.set_detail_result_payload(tx.10.as_deref().unwrap_or("").into());
                                state.set_detail_waktu(
                                    format_timestamp_to_local_dashboard(&tx.11).into(),
                                );
                                let log_rows: Vec<slint::ModelRc<slint::StandardListViewItem>> = logs.iter().map(|l| {
                                    let status_part = if l.4.is_empty() { String::new() } else { format!(" [{}]", l.4) };
                                    let latency_part = l.8.map(|ms| format!(" ({}ms)", ms)).unwrap_or_default();
                                    let log_time = format_timestamp_to_local_dashboard(&l.7);
                                    let header = format!("[{}] A{}#{} {} {}{}", log_time, l.0, l.1, l.2, l.3, status_part);
                                    let log_line = if l.6.is_empty() {
                                        format!("{} - {}{}", header, l.5, latency_part)
                                    } else {
                                        format!("{} - {}{} | {}", header, l.5, latency_part, l.6)
                                    };
                                    let items: Vec<slint::StandardListViewItem> = vec![
                                        slint::StandardListViewItem::from(log_line.as_str()),
                                    ];
                                    slint::ModelRc::new(slint::VecModel::from(items))
                                }).collect();
                                state.set_detail_flow_logs(slint::ModelRc::new(slint::VecModel::from(log_rows)));
                            }
                            _ => {
                                tracing::error!("Failed to get transaction detail for rowid: {}", tx_id);
                            }
                        }
                    }
                });
            });
        });
    }

    // --- Check voucher ---
    {
        let ui_handle = ui.as_weak();
        let db_ref = db_dashboard.clone();
        let rth = rt_dashboard.clone();
        let countdown_rt = rt_dashboard.clone();
        let voucher_dialog_session_ref = voucher_dialog_session.clone();
        DashboardState::get(ui).on_check_voucher(move |tx_id| {
            use vouchflow::application::providers::ProviderRouter;

            tracing::info!("Check voucher requested for rowid: {}", tx_id);
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            let tx_rowid = tx_id as i64;
            let countdown_rt = countdown_rt.clone();
            let voucher_dialog_session_ref = voucher_dialog_session_ref.clone();
            rth.spawn(async move {
                let parse_payload_barcode = |result_payload: Option<&str>| -> Option<String> {
                    let payload = serde_json::from_str::<serde_json::Value>(result_payload?).ok()?;
                    payload.get("barcode").and_then(|v| v.as_str()).map(str::trim)
                        .filter(|s| !s.is_empty()).map(ToOwned::to_owned)
                        .or_else(|| {
                            payload.get("barcodes").and_then(|v| v.as_array())
                                .and_then(|arr| arr.first()).and_then(|v| v.as_str())
                                .map(str::trim).filter(|s| !s.is_empty()).map(ToOwned::to_owned)
                        })
                };
                let resolve_voucher_code = |kategori: &str, nomor: &str, sn: Option<&str>, result_payload: Option<&str>| -> Option<String> {
                    let kategori_upper = kategori.trim().to_uppercase();
                    if kategori_upper == "FIS" || kategori_upper == "RDM" {
                        parse_payload_barcode(result_payload).or_else(|| sn.map(str::trim).filter(|s| !s.is_empty()).map(ToOwned::to_owned))
                    } else {
                        Some(nomor).map(str::trim).filter(|s| !s.is_empty()).map(ToOwned::to_owned)
                    }
                };

                let tx_info = db.with_reader(|conn| {
                    let info = conn.query_row(
                        "SELECT nomor, sn, provider, kategori, harga, produk, result_payload FROM transactions WHERE rowid = ?",
                        [tx_rowid],
                        |row| Ok((
                            row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?,
                            row.get::<_, String>(2)?, row.get::<_, String>(3)?,
                            row.get::<_, f64>(4)?, row.get::<_, String>(5)?,
                            row.get::<_, Option<String>>(6)?,
                        ))
                    )?;
                    Ok(info)
                }).await;

                let fallback_voucher_code = match &tx_info {
                    Ok((nomor, sn, _provider, kategori, _harga, _produk, result_payload)) => {
                        resolve_voucher_code(kategori, nomor, sn.as_deref(), result_payload.as_deref())
                    }
                    Err(_) => None,
                };

                let check_result = match &tx_info {
                    Ok((nomor, sn, provider, kategori, _harga, _produk, result_payload)) => {
                        if let Some(voucher_code) = resolve_voucher_code(kategori, nomor, sn.as_deref(), result_payload.as_deref()) {
                            let router = ProviderRouter::new();
                            match router.get_provider(provider) {
                                Ok(p) => p.check_voucher(&voucher_code).await
                                    .map(|resp| (voucher_code, resp)).map_err(|e| e.to_string()),
                                Err(e) => Err(format!("Provider {} tidak mendukung cek voucher: {}", provider, e)),
                            }
                        } else {
                            Err(format!("Kode voucher kosong untuk kategori {}", kategori))
                        }
                    }
                    Err(e) => Err(format!("Gagal membaca transaksi: {}", e)),
                };

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        let state = DashboardState::get(&ui);
                        match (tx_info, check_result) {
                            (Ok((_nomor, _sn, provider, _kategori, _harga, _produk, _result_payload)), Ok((used_voucher_code, resp))) => {
                                let expiry = resp.expiry_date.unwrap_or_else(|| "-".to_string());
                                let expiry_date = expiry.split(|c| c == ' ' || c == 'T').next().unwrap_or(&expiry).to_string();
                                let serial_display = if !resp.serial_number.is_empty() { resp.serial_number } else { used_voucher_code };
                                state.set_voucher_status(resp.status.as_str().into());
                                state.set_voucher_expired(expiry_date.into());
                                state.set_voucher_provider(provider.as_str().into());
                                state.set_voucher_barcode(serial_display.as_str().into());
                            }
                            (Ok((nomor, sn, provider, kategori, _harga, produk, _result_payload)), Err(err)) => {
                                let voucher_display = fallback_voucher_code.or(sn).unwrap_or(nomor);
                                state.set_voucher_status(format!("ERROR ({})", kategori).into());
                                state.set_voucher_expired(produk.as_str().into());
                                state.set_voucher_provider(provider.as_str().into());
                                state.set_voucher_barcode(voucher_display.as_str().into());
                                tracing::warn!("Check voucher failed: {}", err);
                            }
                            _ => {
                                state.set_voucher_status("ERROR".into());
                                state.set_voucher_expired("-".into());
                                state.set_voucher_provider("-".into());
                                state.set_voucher_barcode("Not found".into());
                            }
                        }
                        state.set_voucher_dialog_countdown(5);
                        state.set_voucher_dialog_visible(true);
                        let session_id = voucher_dialog_session_ref.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        let ui_weak2 = ui.as_weak();
                        let voucher_dialog_session_task = voucher_dialog_session_ref.clone();
                        countdown_rt.spawn(async move {
                            for remaining in (0..5).rev() {
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                if voucher_dialog_session_task.load(std::sync::atomic::Ordering::SeqCst) != session_id { break; }
                                let ui_weak_tick = ui_weak2.clone();
                                let voucher_dialog_session_tick = voucher_dialog_session_task.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if voucher_dialog_session_tick.load(std::sync::atomic::Ordering::SeqCst) != session_id { return; }
                                    if let Some(ui) = ui_weak_tick.upgrade() {
                                        let state = DashboardState::get(&ui);
                                        if !state.get_voucher_dialog_visible() { return; }
                                        state.set_voucher_dialog_countdown(remaining);
                                        if remaining == 0 { state.set_voucher_dialog_visible(false); }
                                    }
                                });
                                if remaining == 0 { break; }
                            }
                        });
                    }
                });
            });
        });
    }

    // --- Mark success ---
    {
        let ui_handle = ui.as_weak();
        let db_cmd_tx = db_cmd_tx_dashboard.clone();
        let db_ref = db_dashboard.clone();
        let rth = rt_dashboard.clone();
        DashboardState::get(ui).on_mark_success(move |tx_id| {
            tracing::info!("Mark success requested for rowid: {}", tx_id);
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            let cmd_tx = db_cmd_tx.clone();
            rth.spawn(async move {
                let tx_info = db
                    .with_reader(|conn| {
                        let ids: (String, String) = conn.query_row(
                            "SELECT tx_id, request_id FROM transactions WHERE rowid = ?",
                            [tx_id as i64],
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                        )?;
                        let webhook_addr: Option<String> = conn
                            .query_row(
                                "SELECT value FROM configurations WHERE key = 'webhook_addr'",
                                [],
                                |row| row.get(0),
                            )
                            .ok();
                        let webhook_port: Option<String> = conn
                            .query_row(
                                "SELECT value FROM configurations WHERE key = 'webhook_port'",
                                [],
                                |row| row.get(0),
                            )
                            .ok();
                        let addr = webhook_addr.unwrap_or_else(|| "127.0.0.1".to_string());
                        let port = webhook_port.unwrap_or_else(|| "8081".to_string());
                        let webhook_url = build_webhook_url(&addr, &port);
                        Ok((ids.0, ids.1, webhook_url))
                    })
                    .await;

                if let Ok((tx_id_str, request_id, webhook_url)) = tx_info {
                    let _ = cmd_tx
                        .send(vouchflow::domain::DbCommand::ManualSuccess {
                            tx_id: tx_id_str.clone(),
                            result_code: "00".to_string(),
                            result_payload: Some("Manual success by admin".to_string()),
                        })
                        .await;
                    vouchflow::utils::send_webhook(
                        &webhook_url,
                        &request_id,
                        &tx_id_str,
                        "SUCCESS",
                        Some("00"),
                        Some("Manual success by admin"),
                    )
                    .await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            DashboardState::get(&ui).invoke_load_transactions();
                        }
                    });
                }
            });
        });
    }

    // --- Mark failed ---
    {
        let ui_handle = ui.as_weak();
        let db_cmd_tx = db_cmd_tx_dashboard.clone();
        let db_ref = db_dashboard.clone();
        let rth = rt_dashboard.clone();
        DashboardState::get(ui).on_mark_failed(move |tx_id| {
            tracing::info!("Mark failed requested for rowid: {}", tx_id);
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            let cmd_tx = db_cmd_tx.clone();
            rth.spawn(async move {
                let tx_info = db
                    .with_reader(|conn| {
                        let ids: (String, String) = conn.query_row(
                            "SELECT tx_id, request_id FROM transactions WHERE rowid = ?",
                            [tx_id as i64],
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                        )?;
                        let webhook_addr: Option<String> = conn
                            .query_row(
                                "SELECT value FROM configurations WHERE key = 'webhook_addr'",
                                [],
                                |row| row.get(0),
                            )
                            .ok();
                        let webhook_port: Option<String> = conn
                            .query_row(
                                "SELECT value FROM configurations WHERE key = 'webhook_port'",
                                [],
                                |row| row.get(0),
                            )
                            .ok();
                        let addr = webhook_addr.unwrap_or_else(|| "127.0.0.1".to_string());
                        let port = webhook_port.unwrap_or_else(|| "8081".to_string());
                        let webhook_url = build_webhook_url(&addr, &port);
                        Ok((ids.0, ids.1, webhook_url))
                    })
                    .await;

                if let Ok((tx_id_str, request_id, webhook_url)) = tx_info {
                    let _ = cmd_tx
                        .send(vouchflow::domain::DbCommand::UpdateTransaction {
                            tx_id: tx_id_str.clone(),
                            status: vouchflow::domain::TransactionStatus::Failed,
                            sn: None,
                            result_code: Some("99".to_string()),
                            result_payload: Some("Marked as failed by admin".to_string()),
                        })
                        .await;
                    vouchflow::utils::send_webhook(
                        &webhook_url,
                        &request_id,
                        &tx_id_str,
                        "FAILED",
                        Some("99"),
                        Some("Marked as failed by admin"),
                    )
                    .await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            DashboardState::get(&ui).invoke_load_transactions();
                        }
                    });
                }
            });
        });
    }

    // --- Retry transaction ---
    {
        let ui_handle = ui.as_weak();
        let db_cmd_tx = db_cmd_tx_dashboard.clone();
        let command_tx = command_tx_dashboard.clone();
        let db_ref = db_dashboard.clone();
        let rth = rt_dashboard.clone();
        DashboardState::get(ui).on_retry_transaction(move |tx_id| {
            tracing::info!("Retry transaction requested for rowid: {}", tx_id);
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            let db_cmd_tx = db_cmd_tx.clone();
            let command_tx = command_tx.clone();
            rth.spawn(async move {
                let tx_data = db.with_reader(|conn| {
                    let data: (String, String, String, String, String, f64, String, String) = conn.query_row(
                        "SELECT tx_id, request_id, provider, kode_produk, kategori, harga, produk, nomor
                         FROM transactions WHERE rowid = ?",
                        [tx_id as i64],
                        |row| Ok((
                            row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?, row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?, row.get::<_, f64>(5)?,
                            row.get::<_, String>(6)?, row.get::<_, String>(7)?,
                        ))
                    )?;
                    let kode_addon: Option<String> = conn.query_row(
                        "SELECT kode_addon FROM produk WHERE kode_produk = ? AND aktif = 1", [&data.3], |row| row.get(0)
                    ).ok();
                    let webhook_addr: Option<String> = conn.query_row("SELECT value FROM configurations WHERE key = 'webhook_addr'", [], |row| row.get(0)).ok();
                    let webhook_port: Option<String> = conn.query_row("SELECT value FROM configurations WHERE key = 'webhook_port'", [], |row| row.get(0)).ok();
                    let addr = webhook_addr.unwrap_or_else(|| "127.0.0.1".to_string());
                    let port = webhook_port.unwrap_or_else(|| "8081".to_string());
                    let webhook_url = build_webhook_url(&addr, &port);
                    Ok((data.0, data.1, data.2, data.3, data.4, data.5, data.6, data.7, kode_addon, webhook_url))
                }).await;

                if let Ok((old_tx_id, old_request_id, provider, kode_produk, kategori, harga, produk, nomor, kode_addon, webhook_url)) = tx_data {
                    let tx_type = match vouchflow::domain::TransactionType::from_kategori(&kategori) {
                        Some(t) => t,
                        None => { tracing::error!("Retry failed: invalid kategori {}", kategori); return; }
                    };
                    let command = vouchflow::domain::Command::with_product_info(
                        old_request_id.clone(), tx_type, provider, kode_produk, kategori, harga, kode_addon, produk, nomor, None,
                    ).with_retry_target(old_tx_id.clone());

                    if command_tx.send(command).await.is_ok() {
                        let db_for_poll = db.clone();
                        let _db_cmd_for_poll = db_cmd_tx.clone();
                        let retry_tx_id = old_tx_id.clone();
                        let retry_request_id = old_request_id.clone();
                        tokio::spawn(async move {
                            for _ in 0..60 {
                                let status_row = db_for_poll.with_reader(|conn| {
                                    let row: Option<(String, Option<String>, Option<String>)> = conn.query_row(
                                        "SELECT status, result_code, result_payload FROM transactions WHERE tx_id = ?",
                                        [&retry_tx_id],
                                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?)),
                                    ).ok();
                                    Ok(row)
                                }).await;
                                if let Ok(Some((status, result_code, result_payload))) = status_row {
                                    let status_upper = status.to_uppercase();
                                    if status_upper == "SUCCESS" || status_upper == "FAILED" || status_upper == "EXPIRED" {
                                        vouchflow::utils::send_webhook(
                                            &webhook_url, &retry_request_id, &retry_tx_id,
                                            &status_upper, result_code.as_deref(), result_payload.as_deref(),
                                        ).await;
                                        break;
                                    }
                                }
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            }
                        });
                    } else {
                        tracing::error!("Retry failed: command bus send failed for old tx_id={}", old_tx_id);
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() { DashboardState::get(&ui).invoke_load_transactions(); }
                    });
                }
            });
        });
    }

    // --- Load transactions ---
    {
        let ui_handle = ui.as_weak();
        let db_for_load = db_dashboard.clone();
        let rt_handle = rt_dashboard.clone();
        DashboardState::get(ui).on_load_transactions(move || {
            tracing::info!("Load transactions requested");
            let ui_weak = ui_handle.clone();
            let db = db_for_load.clone();
            let (status, provider, kategori, date_from, date_to, limit, search) =
                read_dashboard_filters(&ui_weak);

            rt_handle.spawn(async move {
                let transactions = query_transactions(
                    &db, &status, &provider, &kategori, &date_from, &date_to, limit, &search,
                )
                .await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        apply_transactions_to_ui(&ui, transactions);
                    }
                });
            });
        });
    }

    // --- Filter transactions ---
    {
        let ui_handle = ui.as_weak();
        let db_ref = db_dashboard.clone();
        let rth = rt_dashboard.clone();
        DashboardState::get(ui).on_filter_transactions(move |status, provider, kategori, date_from, date_to, limit, search| {
            tracing::info!(
                "Filter transactions: status={}, provider={}, kategori={}, from={}, to={}, limit={}, search={}",
                status, provider, kategori, date_from, date_to, limit, search
            );
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            let status = status.to_string();
            let provider = provider.to_string();
            let kategori = kategori.to_string();
            let date_from = date_from.to_string();
            let date_to = date_to.to_string();
            let search = search.to_string();

            rth.spawn(async move {
                let transactions = query_transactions(&db, &status, &provider, &kategori, &date_from, &date_to, limit, &search).await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        apply_transactions_to_ui(&ui, transactions);
                    }
                });
            });
        });
    }

    // --- Load stock summary (dashboard version) ---
    {
        let ui_handle = ui.as_weak();
        let db_ref = db_dashboard.clone();
        let rth = rt_dashboard.clone();
        DashboardState::get(ui).on_load_stock_summary(move || {
            tracing::info!("Load stock summary requested");
            let ui_weak = ui_handle.clone();
            let db = db_ref.clone();
            rth.spawn(async move {
                use vouchflow::application::services;
                match services::get_stock_addon_summary(&db).await {
                    Ok(summaries) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(_ui) = ui_weak.upgrade() {
                                tracing::info!("Loaded {} stock summary items", summaries.len());
                            }
                        });
                    }
                    Err(e) => tracing::error!("Failed to load stock summary: {}", e),
                }
            });
        });
    }

    // --- Sort transactions ---
    {
        DashboardState::get(ui).on_sort_transactions(move |column_index, ascending| {
            let _ = slint::invoke_from_event_loop(move || {
                with_table(TableId::Dashboard, |m| {
                    m.sort_by_column(column_index as usize, ascending)
                });
            });
        });
    }
}

// ---- Helpers ----

/// Build webhook URL from address and port
fn build_webhook_url(addr: &str, port: &str) -> String {
    if addr.starts_with("http://") || addr.starts_with("https://") {
        if addr
            .rsplit(':')
            .next()
            .map(|v| v.parse::<u16>().is_ok())
            .unwrap_or(false)
        {
            addr.to_string()
        } else {
            format!("{}:{}", addr.trim_end_matches('/'), port)
        }
    } else if addr.contains(':') {
        format!("http://{}", addr)
    } else {
        format!("http://{}:{}", addr, port)
    }
}

/// Read current filter values from the DashboardState UI
fn read_dashboard_filters(
    ui_weak: &slint::Weak<AppWindow>,
) -> (String, String, String, String, String, i32, String) {
    if let Some(ui) = ui_weak.upgrade() {
        let state = DashboardState::get(&ui);
        let status_idx = state.get_status_filter_index() as usize;
        let provider_idx = state.get_provider_filter_index() as usize;
        let kategori_idx = state.get_kategori_filter_index() as usize;

        let selected_status = state
            .get_status_options()
            .row_data(status_idx)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "Semua".to_string());
        let selected_provider = state
            .get_provider_options()
            .row_data(provider_idx)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "Semua".to_string());
        let selected_kategori = state
            .get_kategori_options()
            .row_data(kategori_idx)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "Semua".to_string());

        let mut from = state.get_date_from().to_string();
        let mut to = state.get_date_to().to_string();
        if from.is_empty() || to.is_empty() {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            if from.is_empty() {
                from = today.clone();
            }
            if to.is_empty() {
                to = today;
            }
        }
        (
            selected_status,
            selected_provider,
            selected_kategori,
            from,
            to,
            state.get_limit_index(),
            state.get_search_query().to_string(),
        )
    } else {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        (
            "Semua".to_string(),
            "Semua".to_string(),
            "Semua".to_string(),
            today.clone(),
            today,
            0,
            String::new(),
        )
    }
}

/// Query transactions from DB with filters
async fn query_transactions(
    db: &Database,
    status: &str,
    provider: &str,
    kategori: &str,
    date_from: &str,
    date_to: &str,
    limit: i32,
    search: &str,
) -> Result<
    (
        Vec<(
            i64,
            String,
            String,
            String,
            String,
            f64,
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            String,
        )>,
        usize,
        usize,
        usize,
        usize,
    ),
    DomainError,
> {
    let status = status.to_string();
    let provider = provider.to_string();
    let kategori = kategori.to_string();
    let date_from = date_from.to_string();
    let date_to = date_to.to_string();
    let search = search.to_string();

    db.with_reader(move |conn| {
        let local_date_expr =
            "COALESCE(date(created_at, 'localtime'), substr(replace(created_at, 'T', ' '), 1, 10))";
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<String> = Vec::new();

        if !status.is_empty() && status != "Semua" {
            let status_upper = status.to_uppercase();
            if status_upper == "PENDING" {
                conditions.push("UPPER(status) IN ('PENDING', 'PROCESSING')".to_string());
            } else {
                conditions.push("UPPER(status) = ?".to_string());
                params.push(status_upper);
            }
        }
        if !provider.is_empty() && provider != "Semua" {
            conditions.push("provider = ?".to_string());
            params.push(provider.clone());
        }
        if !kategori.is_empty() && kategori != "Semua" {
            conditions.push("kategori = ?".to_string());
            params.push(kategori.clone());
        }
        if !date_from.is_empty() {
            conditions.push(format!("{} >= ?", local_date_expr));
            params.push(date_from.clone());
        }
        if !date_to.is_empty() {
            conditions.push(format!("{} <= ?", local_date_expr));
            params.push(date_to.clone());
        }
        if !search.is_empty() {
            conditions.push("(request_id LIKE ? OR nomor LIKE ? OR kode_produk LIKE ? OR COALESCE(sn, '') LIKE ?)".to_string());
            let pattern = format!("%{}%", search);
            params.push(pattern.clone());
            params.push(pattern.clone());
            params.push(pattern.clone());
            params.push(pattern);
        }

        let where_clause = if conditions.is_empty() { String::new() } else { format!("WHERE {}", conditions.join(" AND ")) };
        let limit_val = match limit {
            0 => 100, 1 => 200, 2 => 300, 3 => 500, _ => 10000,
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT rowid, tx_id, request_id, provider, kategori, harga, kode_produk, nomor, sn, status, result_code,
                    COALESCE(created_at, '')
             FROM transactions {} ORDER BY created_at DESC LIMIT {}",
            where_clause, limit_val
        ))?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
        let rows: Vec<(i64, String, String, String, String, f64, String, String, Option<String>, String, Option<String>, String)> =
            stmt.query_map(param_refs.as_slice(), |row| Ok((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
                row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?
            )))?.filter_map(|r| r.ok()).collect();

        let total = rows.len();
        let success = rows.iter().filter(|r| r.9 == "SUCCESS").count();
        let pending = rows.iter().filter(|r| r.9 == "PROCESSING" || r.9 == "PENDING").count();
        let failed = rows.iter().filter(|r| r.9 == "FAILED").count();
        Ok((rows, total, success, pending, failed))
    }).await
}

/// Apply transaction query results to the DashboardState UI
fn apply_transactions_to_ui(
    ui: &AppWindow,
    transactions: Result<
        (
            Vec<(
                i64,
                String,
                String,
                String,
                String,
                f64,
                String,
                String,
                Option<String>,
                String,
                Option<String>,
                String,
            )>,
            usize,
            usize,
            usize,
            usize,
        ),
        DomainError,
    >,
) {
    let state = DashboardState::get(ui);
    match transactions {
        Ok((rows, total, success, pending, failed)) => {
            let table_rows: Vec<slint::ModelRc<slint::StandardListViewItem>> = rows
                .iter()
                .map(|r| {
                    let items: Vec<slint::StandardListViewItem> = vec![
                        slint::StandardListViewItem::from(slint::format!("{}", r.2)),
                        slint::StandardListViewItem::from(slint::format!("{}", r.3)),
                        slint::StandardListViewItem::from(slint::format!("{}", r.7)),
                        slint::StandardListViewItem::from(slint::format!("{}", r.6)),
                        slint::StandardListViewItem::from(slint::format!("{}", r.4)),
                        slint::StandardListViewItem::from(slint::format!("{:.0}", r.5)),
                        slint::StandardListViewItem::from(slint::format!(
                            "{}",
                            r.8.clone().unwrap_or_default()
                        )),
                        slint::StandardListViewItem::from(slint::format!("{}", r.9)),
                        slint::StandardListViewItem::from(slint::format!(
                            "{}",
                            format_timestamp_to_local_dashboard(&r.11)
                        )),
                    ];
                    slint::ModelRc::new(slint::VecModel::from(items))
                })
                .collect();
            let ids: Vec<i32> = rows.iter().map(|r| r.0 as i32).collect();

            let (rows_model, ids_model) =
                with_table(TableId::Dashboard, |m| m.set_all(table_rows, ids));
            state.set_transactions(rows_model);
            state.set_transaction_ids(ids_model);
            state.set_total_transactions(total as i32);
            state.set_success_count(success as i32);
            state.set_pending_count(pending as i32);
            state.set_failed_count(failed as i32);
        }
        Err(e) => {
            tracing::error!("Failed to load/filter transactions: {}", e);
        }
    }
}
