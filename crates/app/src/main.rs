mod reader;
mod scheme;
mod settings;
mod window;

use adw::prelude::*;

const APP_ID: &str = "io.github.muhnschein.Wander";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        let window = window::Window::new(app);
        window.present();
        // Also covers a stored config that no longer yields a client, which the
        // old file-presence check treated as configured and left unusable.
        if !window.is_configured() {
            window.open_settings();
        }
    });

    app.run()
}
