//! Product CRUD Callbacks
//!
//! Registers all ProdukState callback handlers for the Slint UI.

use slint::{ComponentHandle, Global, Model};
use vouchflow::application::services::ProductService;
use vouchflow::infrastructure::channels::DbCommandSender;
use vouchflow::infrastructure::database::Database;
use crate::{AppWindow, ProdukState};

/// Register all product-related callbacks on the UI
pub fn register(
    ui: &AppWindow,
    db: &Database,
    db_cmd_tx: &DbCommandSender,
    rt: &tokio::runtime::Handle,
) {
    let product_service = ProductService::new(db.clone());
    let db_cmd_tx_clone = db_cmd_tx.clone();
    let rt_handle = rt.clone();

    // --- Load products ---
    {
        let ui_handle = ui.as_weak();
        let service = product_service.clone();
        let rth = rt_handle.clone();
        ProdukState::get(ui).on_load_products(move || {
            let ui_weak = ui_handle.clone();
            let svc = service.clone();
            rth.spawn(async move {
                match svc.get_all_products().await {
                    Ok(products) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let rows: Vec<slint::ModelRc<slint::StandardListViewItem>> = products.iter().map(|p| {
                                    let items: Vec<slint::StandardListViewItem> = vec![
                                        slint::StandardListViewItem::from(p.provider.as_str()),
                                        slint::StandardListViewItem::from(p.nama_produk.as_str()),
                                        slint::StandardListViewItem::from(p.kode_produk.as_str()),
                                        slint::StandardListViewItem::from(p.kategori.as_str()),
                                        slint::StandardListViewItem::from(p.harga.as_str()),
                                        slint::StandardListViewItem::from(p.kode_addon.as_str()),
                                    ];
                                    slint::ModelRc::new(slint::VecModel::from(items))
                                }).collect();
                                let ids: Vec<i32> = products.iter().map(|p| p.id as i32).collect();

                                ProdukState::get(&ui).set_products(slint::ModelRc::new(slint::VecModel::from(rows)));
                                ProdukState::get(&ui).set_selected_product_ids(slint::ModelRc::new(slint::VecModel::from(ids)));
                            }
                        });
                    }
                    Err(e) => tracing::error!("Failed to load products: {}", e),
                }
            });
        });
    }

    // --- Search products ---
    {
        let ui_handle = ui.as_weak();
        let service = product_service.clone();
        let rth = rt_handle.clone();
        ProdukState::get(ui).on_search_products(move |query, provider, kategori| {
            let ui_weak = ui_handle.clone();
            let svc = service.clone();
            let q = query.to_string();
            let p = if provider.is_empty() { None } else { Some(provider.to_string()) };
            let k = if kategori.is_empty() { None } else { Some(kategori.to_string()) };
            rth.spawn(async move {
                let provider_filter = p.as_deref();
                let kategori_filter = k.as_deref();
                match svc.search_products(&q, provider_filter, kategori_filter).await {
                    Ok(products) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let rows: Vec<slint::ModelRc<slint::StandardListViewItem>> = products.iter().map(|p| {
                                    let items: Vec<slint::StandardListViewItem> = vec![
                                        slint::StandardListViewItem::from(p.provider.as_str()),
                                        slint::StandardListViewItem::from(p.nama_produk.as_str()),
                                        slint::StandardListViewItem::from(p.kode_produk.as_str()),
                                        slint::StandardListViewItem::from(p.kategori.as_str()),
                                        slint::StandardListViewItem::from(p.harga.as_str()),
                                        slint::StandardListViewItem::from(p.kode_addon.as_str()),
                                    ];
                                    slint::ModelRc::new(slint::VecModel::from(items))
                                }).collect();
                                let ids: Vec<i32> = products.iter().map(|p| p.id as i32).collect();

                                ProdukState::get(&ui).set_products(slint::ModelRc::new(slint::VecModel::from(rows)));
                                ProdukState::get(&ui).set_selected_product_ids(slint::ModelRc::new(slint::VecModel::from(ids)));
                            }
                        });
                    }
                    Err(e) => tracing::error!("Failed to search products: {}", e),
                }
            });
        });
    }

    // --- Save product ---
    {
        let ui_handle = ui.as_weak();
        let tx = db_cmd_tx_clone.clone();
        let rth = rt_handle.clone();
        ProdukState::get(ui).on_save_product(move |id, provider, nama, kode_produk, kategori, harga, addon| {
            let ui_weak = ui_handle.clone();
            let cmd_tx = tx.clone();
            let provider = provider.to_string();
            let nama = nama.to_string();
            let kode_produk = kode_produk.to_string();
            let kategori = kategori.to_string();
            let harga_val: f64 = harga.parse().unwrap_or(0.0);
            let addon = if addon.is_empty() { None } else { Some(addon.to_string()) };

            rth.spawn(async move {
                let cmd = if id < 0 {
                    vouchflow::domain::DbCommand::CreateProduct {
                        provider, nama_produk: nama, kode_produk, kategori, harga: harga_val, kode_addon: addon
                    }
                } else {
                    vouchflow::domain::DbCommand::UpdateProduct {
                        id: id as i64, provider, nama_produk: nama, kode_produk, kategori, harga: harga_val, kode_addon: addon
                    }
                };

                if let Err(e) = cmd_tx.send(cmd).await {
                    tracing::error!("Failed to save product: {}", e);
                } else {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ProdukState::get(&ui).invoke_load_products();
                        }
                    });
                }
            });
        });
    }

    // --- Delete products ---
    {
        let ui_handle = ui.as_weak();
        let tx = db_cmd_tx_clone.clone();
        let rth = rt_handle.clone();
        ProdukState::get(ui).on_delete_products(move |ids| {
            let ui_weak = ui_handle.clone();
            let cmd_tx = tx.clone();
            let id_vec: Vec<i64> = ids.iter().map(|id| id as i64).collect();

            rth.spawn(async move {
                let cmd = vouchflow::domain::DbCommand::DeleteProducts { ids: id_vec };
                if let Err(e) = cmd_tx.send(cmd).await {
                    tracing::error!("Failed to delete products: {}", e);
                } else {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ProdukState::get(&ui).invoke_load_products();
                        }
                    });
                }
            });
        });
    }

    // --- Load product for edit ---
    {
        let ui_handle = ui.as_weak();
        let service = product_service.clone();
        let rth = rt_handle.clone();
        ProdukState::get(ui).on_load_product_for_edit(move |id| {
            let ui_weak = ui_handle.clone();
            let svc = service.clone();
            rth.spawn(async move {
                match svc.get_product(id as i64).await {
                    Ok(Some(product)) => {
                        let provider_idx = match product.provider.as_str() {
                            "Byu" => 0,
                            "Smartfren" => 1,
                            "Telkomsel" => 2,
                            _ => 0,
                        };
                        let kategori_idx = match product.kategori.as_str() {
                            "CEK" => 0,
                            "RDM" => 1,
                            "FIS" => 2,
                            _ => 1,
                        };

                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let state = ProdukState::get(&ui);
                                state.set_edit_product_id(product.id as i32);
                                state.set_form_provider_index(provider_idx);
                                state.set_form_nama_produk(product.nama_produk.into());
                                state.set_form_kode_produk(product.kode_produk.into());
                                state.set_form_kategori_index(kategori_idx);
                                state.set_form_harga(format!("{:.0}", product.harga).into());
                                let addon_val = product.kode_addon.unwrap_or_default();
                                let addon_options = state.get_addon_options();
                                let addon_idx = addon_options.iter().position(|a| a.as_str() == addon_val.as_str()).map(|i| i as i32).unwrap_or(-1);
                                state.set_form_addon_index(addon_idx);
                            }
                        });
                    }
                    Ok(None) => tracing::warn!("Product not found: {}", id),
                    Err(e) => tracing::error!("Failed to load product: {}", e),
                }
            });
        });
    }

    // --- Duplicate product ---
    {
        let ui_handle = ui.as_weak();
        let service = product_service.clone();
        let rth = rt_handle.clone();
        ProdukState::get(ui).on_duplicate_product(move |id| {
            let ui_weak = ui_handle.clone();
            let svc = service.clone();
            rth.spawn(async move {
                match svc.get_product(id as i64).await {
                    Ok(Some(product)) => {
                        let provider_idx = match product.provider.as_str() {
                            "Byu" => 0,
                            "Smartfren" => 1,
                            "Telkomsel" => 2,
                            _ => 0,
                        };
                        let kategori_idx = match product.kategori.as_str() {
                            "CEK" => 0,
                            "RDM" => 1,
                            "FIS" => 2,
                            _ => 1,
                        };

                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let state = ProdukState::get(&ui);
                                state.set_edit_product_id(-1);
                                state.set_form_provider_index(provider_idx);
                                state.set_form_nama_produk(format!("{} (Copy)", product.nama_produk).into());
                                state.set_form_kode_produk(format!("{}_COPY", product.kode_produk).into());
                                state.set_form_kategori_index(kategori_idx);
                                state.set_form_harga(format!("{:.0}", product.harga).into());
                                let addon_val = product.kode_addon.unwrap_or_default();
                                let addon_options = state.get_addon_options();
                                let addon_idx = addon_options.iter().position(|a| a.as_str() == addon_val.as_str()).map(|i| i as i32).unwrap_or(-1);
                                state.set_form_addon_index(addon_idx);
                                state.set_form_panel_open(true);
                            }
                        });
                    }
                    Ok(None) => tracing::warn!("Product not found for duplicate: {}", id),
                    Err(e) => tracing::error!("Failed to load product for duplicate: {}", e),
                }
            });
        });
    }

    // --- Load addon options ---
    {
        let ui_handle = ui.as_weak();
        let service = product_service.clone();
        let rth = rt_handle.clone();
        ProdukState::get(ui).on_load_addon_options(move || {
            let ui_weak = ui_handle.clone();
            let svc = service.clone();
            rth.spawn(async move {
                match svc.get_addon_options().await {
                    Ok(addons) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                let addon_model: Vec<slint::SharedString> = addons.iter()
                                    .map(|a| slint::SharedString::from(a.as_str()))
                                    .collect();
                                ProdukState::get(&ui).set_addon_options(slint::ModelRc::new(slint::VecModel::from(addon_model)));
                            }
                        });
                    }
                    Err(e) => tracing::error!("Failed to load addon options: {}", e),
                }
            });
        });
    }
}

