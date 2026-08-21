use crate::settings::{self, ServerConfig};
use adw::prelude::*;
use cairn_client::{ArchiveSummary, CairnClient};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub struct Window {
    win: adw::ApplicationWindow,
    state: Rc<State>,
}

struct State {
    client: RefCell<Option<Arc<CairnClient>>>,
    nav: adw::NavigationView,
    list: gtk::ListBox,
    stack: gtk::Stack,
    toasts: adw::ToastOverlay,
}

impl std::ops::Deref for Window {
    type Target = adw::ApplicationWindow;

    fn deref(&self) -> &Self::Target {
        &self.win
    }
}

impl Window {
    pub fn new(app: &adw::Application) -> Self {
        let win = adw::ApplicationWindow::builder()
            .application(app)
            .title("Wander")
            .default_width(1000)
            .default_height(700)
            .icon_name("io.github.muhnschein.Wander")
            .build();

        let header = adw::HeaderBar::new();

        let refresh_button = gtk::Button::from_icon_name("view-refresh-symbolic");
        refresh_button.set_tooltip_text(Some("Refresh library"));
        header.pack_start(&refresh_button);

        let prefs_button = gtk::Button::from_icon_name("emblem-system-symbolic");
        prefs_button.set_tooltip_text(Some("Server settings"));
        header.pack_end(&prefs_button);

        let list = gtk::ListBox::new();
        list.set_css_classes(&["boxed-list"]);
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_margin_top(12);
        list.set_margin_bottom(12);
        list.set_margin_start(12);
        list.set_margin_end(12);

        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_child(Some(&list));
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);

        let status_page = adw::StatusPage::builder()
            .icon_name("system-search-symbolic")
            .title("Nothing to show yet")
            .description("Point Wander at a cairn instance to browse its archives.")
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&scrolled, Some("library"));
        stack.add_named(&status_page, Some("status"));
        stack.set_visible_child_name("status");

        let nav = adw::NavigationView::new();
        let library_page = adw::NavigationPage::builder()
            .title("Wander")
            .child(&stack)
            .build();
        nav.add(&library_page);

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&nav));

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&toasts));

        win.set_content(Some(&toolbar));

        let window = Self {
            win,
            state: Rc::new(State {
                client: RefCell::new(None),
                nav,
                list,
                stack,
                toasts,
            }),
        };

        let refresh_state = window.state.clone();
        refresh_button.connect_clicked(move |_| {
            refresh_library(&refresh_state);
        });

        let prefs_state = window.state.clone();
        prefs_button.connect_clicked(move |_| {
            open_settings(&prefs_state);
        });

        if let Some(config) = settings::load() {
            apply_config(&window.state, &config);
        }

        window
    }

    pub fn open_settings(&self) {
        open_settings(&self.state);
    }
}

fn apply_config(state: &State, config: &ServerConfig) {
    match CairnClient::new(&config.host, config.port, config.token.as_deref()) {
        Ok(client) => {
            *state.client.borrow_mut() = Some(Arc::new(client));
        }
        Err(err) => toast(state, &err.to_string()),
    }
}

fn toast(state: &State, message: &str) {
    state
        .toasts
        .add_toast(adw::Toast::builder().title(message).timeout(4).build());
}

fn refresh_library(state: &Rc<State>) {
    while let Some(child) = state.list.first_child() {
        state.list.remove(&child);
    }
    let Some(client) = state.client.borrow().clone() else {
        state.stack.set_visible_child_name("status");
        return;
    };
    state.stack.set_visible_child_name("library");

    let weak_list = state.list.downgrade();
    let weak_stack = state.stack.downgrade();
    let state_for_error = Rc::downgrade(state);

    glib::spawn_future_local(async move {
        let fetched = gio::spawn_blocking(move || client.archives()).await;
        let (Some(list), Some(stack)) = (weak_list.upgrade(), weak_stack.upgrade()) else {
            return;
        };
        match fetched {
            Ok(Ok(archives)) => {
                for archive in archives {
                    list.append(&archive_row(&state_for_error, archive));
                }
                if list.first_child().is_none() {
                    stack.set_visible_child_name("status");
                    if let Some(state) = state_for_error.upgrade() {
                        toast(&state, "The server is reachable but has no open archives.");
                    }
                }
            }
            Ok(Err(err)) => {
                stack.set_visible_child_name("status");
                if let Some(state) = state_for_error.upgrade() {
                    toast(&state, &err.to_string());
                }
            }
            Err(_) => {
                stack.set_visible_child_name("status");
                if let Some(state) = state_for_error.upgrade() {
                    toast(&state, "Background task failed.");
                }
            }
        }
    });
}

fn archive_row(state: &std::rc::Weak<State>, archive: ArchiveSummary) -> gtk::Widget {
    let row = adw::ActionRow::builder()
        .title(&archive.title)
        .subtitle(format!(
            "{} · {} entries",
            archive.uuid, archive.entry_count
        ))
        .activatable(true)
        .build();

    let suffix = gtk::Label::new(None);
    suffix.set_text(if archive.suggest { "searchable" } else { "" });
    suffix.add_css_class("dim-label");
    row.add_suffix(&suffix);

    let state = state.clone();
    row.connect_activated(move |_| {
        let Some(state) = state.upgrade() else {
            return;
        };
        let Some(client) = state.client.borrow().clone() else {
            return;
        };
        let page = crate::reader::reader_page(client, archive.clone());
        state.nav.push(&page);
    });

    row.upcast()
}

fn open_settings(state: &Rc<State>) {
    let existing = settings::load().unwrap_or_default();

    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Cairn server");

    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    group.set_title("Connection");
    group.set_description(Some("Address of the cairn daemon to browse."));

    let host_row = adw::EntryRow::builder()
        .title("Host or IP address")
        .text(&existing.host)
        .build();
    let port_row = adw::EntryRow::builder()
        .title("Port")
        .text(existing.port.to_string())
        .build();
    let token_row = adw::PasswordEntryRow::builder()
        .title("Bearer token (optional)")
        .text(existing.token.as_deref().unwrap_or(""))
        .build();

    group.add(&host_row);
    group.add(&port_row);
    group.add(&token_row);
    page.add(&group);
    dialog.add(&page);

    let host_row = host_row.clone();
    let port_row = port_row.clone();
    let token_row = token_row.clone();
    let state = state.clone();
    dialog.connect_closed(move |_| {
        let host = host_row.text().trim().to_string();
        let port = match port_row.text().trim().parse::<u16>() {
            Ok(port) => port,
            Err(_) => {
                toast(&state, "Port must be a number between 0 and 65535.");
                return;
            }
        };
        let token = token_row.text().trim().to_string();
        let token = (!token.is_empty()).then_some(token);

        let config = ServerConfig {
            host: host.clone(),
            port,
            token,
        };
        settings::save(&config);
        apply_config(&state, &config);
        toast(&state, &format!("Saved. Connecting to {host}:{port}…"));
        refresh_library(&state);
    });
}
