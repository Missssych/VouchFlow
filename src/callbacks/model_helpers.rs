//! Persistent Table Model helpers
//!
//! Provides `TableModel` wrapper around Slint `VecModel` that supports
//! granular `push_front` / `update_row` / `remove_by_id` operations
//! instead of full model rebuilds.
//!
//! Thread safety: All table models are stored in `thread_local!` and
//! accessed only from the Slint UI thread. The `with_table` function
//! provides safe access to each page's model.

use slint::{Model, ModelRc, StandardListViewItem, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;

type RowModel = ModelRc<StandardListViewItem>;

thread_local! {
    static DASHBOARD_MODEL: RefCell<TableModelInner> = RefCell::new(TableModelInner::new());
    static PRODUK_MODEL: RefCell<TableModelInner> = RefCell::new(TableModelInner::new());
    static STOK_ACTIVE_MODEL: RefCell<TableModelInner> = RefCell::new(TableModelInner::new());
    static STOK_USED_MODEL: RefCell<TableModelInner> = RefCell::new(TableModelInner::new());
    static STOK_SUMMARY_MODEL: RefCell<TableModelInner> = RefCell::new(TableModelInner::new());
}

/// Which page/table this model belongs to.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum TableId {
    Dashboard,
    Produk,
    StokActive,
    StokUsed,
    StokSummary,
}

/// Access a table model by ID. Must be called from the Slint UI thread.
pub fn with_table<F, R>(id: TableId, f: F) -> R
where
    F: FnOnce(&mut TableModelInner) -> R,
{
    match id {
        TableId::Dashboard => DASHBOARD_MODEL.with(|m| f(&mut m.borrow_mut())),
        TableId::Produk => PRODUK_MODEL.with(|m| f(&mut m.borrow_mut())),
        TableId::StokActive => STOK_ACTIVE_MODEL.with(|m| f(&mut m.borrow_mut())),
        TableId::StokUsed => STOK_USED_MODEL.with(|m| f(&mut m.borrow_mut())),
        TableId::StokSummary => STOK_SUMMARY_MODEL.with(|m| f(&mut m.borrow_mut())),
    }
}

/// Inner state for a single table model.
///
/// Holds a persistent `VecModel` and a parallel ID list.
/// All operations reuse the same `VecModel` instance so Slint can avoid
/// tearing down and recreating model objects on each refresh.
pub struct TableModelInner {
    rows: Option<ModelRc<RowModel>>,
    ids: Option<ModelRc<i32>>,
}

#[allow(dead_code)]
impl TableModelInner {
    fn new() -> Self {
        Self {
            rows: None,
            ids: None,
        }
    }

    /// Replace entire dataset. Returns model references to bind to UI state.
    ///
    /// If models already exist, this keeps the same `VecModel` instance and
    /// replaces content through `set_vec`, avoiding repeated O(n^2) front-removal.
    pub fn set_all(
        &mut self,
        rows_data: Vec<RowModel>,
        ids_data: Vec<i32>,
    ) -> (ModelRc<RowModel>, ModelRc<i32>) {
        let rows_model = match self.rows.as_ref() {
            Some(existing) => {
                if let Some(vm) = existing.as_any().downcast_ref::<VecModel<RowModel>>() {
                    vm.set_vec(rows_data);
                }
                existing.clone()
            }
            None => {
                let vm = ModelRc::new(VecModel::from(rows_data));
                self.rows = Some(vm.clone());
                vm
            }
        };

        let ids_model = match self.ids.as_ref() {
            Some(existing) => {
                if let Some(vm) = existing.as_any().downcast_ref::<VecModel<i32>>() {
                    vm.set_vec(ids_data);
                }
                existing.clone()
            }
            None => {
                let vm = ModelRc::new(VecModel::from(ids_data));
                self.ids = Some(vm.clone());
                vm
            }
        };

        (rows_model, ids_model)
    }

    /// Find the index of a row by its ID. Returns `None` if not found.
    pub fn find_index_by_id(&self, id: i32) -> Option<usize> {
        let ids = self.ids.as_ref()?;
        if let Some(vm) = ids.as_any().downcast_ref::<VecModel<i32>>() {
            for i in 0..vm.row_count() {
                if vm.row_data(i) == Some(id) {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Update a single row by ID. Does nothing if ID is not found.
    pub fn update_row(&self, id: i32, new_row: RowModel) {
        let idx = match self.find_index_by_id(id) {
            Some(i) => i,
            None => return,
        };
        if let Some(rows) = self.rows.as_ref() {
            if let Some(vm) = rows.as_any().downcast_ref::<VecModel<RowModel>>() {
                vm.set_row_data(idx, new_row);
            }
        }
    }

    /// Insert a new row at position 0 (top of DESC-ordered table).
    pub fn push_front(&self, id: i32, row: RowModel) {
        if let (Some(rows), Some(ids)) = (self.rows.as_ref(), self.ids.as_ref()) {
            if let (Some(rvm), Some(ivm)) = (
                rows.as_any().downcast_ref::<VecModel<RowModel>>(),
                ids.as_any().downcast_ref::<VecModel<i32>>(),
            ) {
                rvm.insert(0, row);
                ivm.insert(0, id);
            }
        }
    }

    /// Remove a single row by ID. Returns `true` if found and removed.
    pub fn remove_by_id(&mut self, id: i32) -> bool {
        let idx = match self.find_index_by_id(id) {
            Some(i) => i,
            None => return false,
        };
        if let (Some(rows), Some(ids)) = (self.rows.as_ref(), self.ids.as_ref()) {
            if let (Some(rvm), Some(ivm)) = (
                rows.as_any().downcast_ref::<VecModel<RowModel>>(),
                ids.as_any().downcast_ref::<VecModel<i32>>(),
            ) {
                rvm.remove(idx);
                ivm.remove(idx);
                return true;
            }
        }
        false
    }

    /// Remove multiple rows by their IDs.
    pub fn remove_by_ids(&mut self, ids: &[i32]) {
        if ids.is_empty() {
            return;
        }

        if let (Some(rows), Some(id_model)) = (self.rows.as_ref(), self.ids.as_ref()) {
            if let (Some(rvm), Some(ivm)) = (
                rows.as_any().downcast_ref::<VecModel<RowModel>>(),
                id_model.as_any().downcast_ref::<VecModel<i32>>(),
            ) {
                // Build lookup once and scan current model once.
                let remove_set: HashSet<i32> = ids.iter().copied().collect();
                let mut indices: Vec<usize> = Vec::new();
                let count = ivm.row_count();

                for idx in 0..count {
                    if let Some(id) = ivm.row_data(idx) {
                        if remove_set.contains(&id) {
                            indices.push(idx);
                        }
                    }
                }

                // Remove in reverse order to avoid shifting issues.
                for idx in indices.into_iter().rev() {
                    rvm.remove(idx);
                    ivm.remove(idx);
                }
            }
        }
    }

    /// Sort by the specified column index, maintaining parallel IDs.
    pub fn sort_by_column(&mut self, column_index: usize, ascending: bool) {
        if let (Some(rows), Some(ids)) = (self.rows.as_ref(), self.ids.as_ref()) {
            if let (Some(rvm), Some(ivm)) = (
                rows.as_any().downcast_ref::<VecModel<RowModel>>(),
                ids.as_any().downcast_ref::<VecModel<i32>>(),
            ) {
                let count = rvm.row_count();
                if count <= 1 {
                    return;
                }

                let mut data: Vec<(RowModel, i32)> = Vec::with_capacity(count);
                for i in 0..count {
                    if let (Some(row), Some(id)) = (rvm.row_data(i), ivm.row_data(i)) {
                        data.push((row, id));
                    }
                }

                data.sort_by(|a, b| {
                    let text_a = a
                        .0
                        .row_data(column_index)
                        .map(|item| item.text.to_string())
                        .unwrap_or_default();
                    let text_b = b
                        .0
                        .row_data(column_index)
                        .map(|item| item.text.to_string())
                        .unwrap_or_default();

                    let cmp = match (text_a.parse::<f64>(), text_b.parse::<f64>()) {
                        (Ok(num_a), Ok(num_b)) => {
                            num_a.partial_cmp(&num_b).unwrap_or(std::cmp::Ordering::Equal)
                        }
                        _ => text_a.cmp(&text_b),
                    };

                    if ascending { cmp } else { cmp.reverse() }
                });

                let mut sorted_rows: Vec<RowModel> = Vec::with_capacity(data.len());
                let mut sorted_ids: Vec<i32> = Vec::with_capacity(data.len());
                for (row, id) in data {
                    sorted_rows.push(row);
                    sorted_ids.push(id);
                }

                rvm.set_vec(sorted_rows);
                ivm.set_vec(sorted_ids);
            }
        }
    }
}

