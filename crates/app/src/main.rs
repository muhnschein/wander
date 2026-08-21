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
        if settings::load().is_none() {
            window.open_settings();
        }
    });

    app.run()
}
