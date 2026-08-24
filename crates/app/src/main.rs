mod reader;
mod scheme;
mod settings;
mod store;
mod toc;
mod window;

use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

const APP_ID: &str = "io.github.muhnschein.Wander";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();

    // GTK owns the widgets, but nothing else owns the state behind them, and
    // the callbacks reach it weakly. Dropping the `Window` at the end of
    // `activate` would therefore free that state while the window is still on
    // screen: the first async refresh would come back to nothing, leaving an
    // empty library under a spinner that never stops.
    let windows: Rc<RefCell<Vec<window::Window>>> = Rc::default();
    app.connect_activate(move |app| {
        let window = window::Window::new(app);
        window.present();
        // Also covers a stored config that no longer yields a client, which the
        // old file-presence check treated as configured and left unusable.
        if !window.is_configured() {
            window.open_settings();
        }
        windows.borrow_mut().push(window);
    });

    app.run()
}
