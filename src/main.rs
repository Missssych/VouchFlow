use slint::platform::PlatformError;
use i_slint_backend_winit::WinitWindowAccessor;

slint::include_modules!();

fn main() -> Result<(), PlatformError> {
    let ui = AppWindow::new()?;

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
            ui.set_is_maximized(new_state);
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

    ui.run()
}