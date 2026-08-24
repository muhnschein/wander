use crate::settings::{self, ServerConfig};
use crate::store::{Store, Visit};
use adw::prelude::*;
use cairn_client::{ArchiveSummary, CairnClient};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
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
    status: adw::StatusPage,
    spinner: gtk::Spinner,
    toasts: adw::ToastOverlay,
    /// Guards against a second listing racing the first when refresh is
    /// clicked repeatedly; the later response would otherwise win arbitrarily.
    busy: Cell<bool>,
    store: Rc<RefCell<Store>>,
    /// The last listing, so a history or bookmark row can find the archive it
    /// names. Reopening needs the summary, not just the uuid.
    archives: RefCell<Vec<ArchiveSummary>>,
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

        let spinner = gtk::Spinner::new();
        spinner.set_visible(false);
        header.pack_start(&spinner);

        let prefs_button = gtk::Button::from_icon_name("emblem-system-symbolic");
        prefs_button.set_tooltip_text(Some("Server settings"));
        header.pack_end(&prefs_button);

        let saved_button = gtk::Button::from_icon_name("user-bookmarks-symbolic");
        saved_button.set_tooltip_text(Some("History and bookmarks"));
        header.pack_end(&saved_button);

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

        let status = adw::StatusPage::builder()
            .icon_name("system-search-symbolic")
            .title("Nothing to show yet")
            .description("Point Wander at a cairn instance to browse its archives.")
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&scrolled, Some("library"));
        stack.add_named(&status, Some("status"));
        stack.set_visible_child_name("status");

        // The header belongs to the library page, not to the window. Hung off
        // the window it stays put while a reader page is pushed over it,
        // leaving two stacked header bars, two sets of window controls, and a
        // refresh button for a library nobody is looking at.
        let library_toolbar = adw::ToolbarView::new();
        library_toolbar.add_top_bar(&header);
        library_toolbar.set_content(Some(&stack));

        let nav = adw::NavigationView::new();
        let library_page = adw::NavigationPage::builder()
            .title("Wander")
            .child(&library_toolbar)
            .build();
        nav.add(&library_page);

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&nav));

        win.set_content(Some(&toasts));

        let window = Self {
            win,
            state: Rc::new(State {
                client: RefCell::new(None),
                nav,
                list,
                stack,
                status,
                spinner,
                toasts,
                busy: Cell::new(false),
                store: Rc::new(RefCell::new(Store::load())),
                archives: RefCell::new(Vec::new()),
            }),
        };

        // These closures live on widgets that the state itself owns, so they
        // hold a weak reference; a strong one would make the window immortal.
        let refresh_state = Rc::downgrade(&window.state);
        refresh_button.connect_clicked(move |_| {
            if let Some(state) = refresh_state.upgrade() {
                refresh_library(&state);
            }
        });

        let prefs_state = Rc::downgrade(&window.state);
        prefs_button.connect_clicked(move |_| {
            if let Some(state) = prefs_state.upgrade() {
                open_settings(&state);
            }
        });

        let saved_state = Rc::downgrade(&window.state);
        saved_button.connect_clicked(move |_| {
            if let Some(state) = saved_state.upgrade() {
                let page = saved_page(&state);
                state.nav.push(&page);
            }
        });

        if let Some(config) = settings::load() {
            // Without this the library stays empty on every launch until the
            // user thinks to press refresh, even with a working saved server.
            if apply_config(&window.state, &config) {
                refresh_library(&window.state);
            }
        }

        window
    }

    pub fn open_settings(&self) {
        open_settings(&self.state);
    }

    /// Whether the saved settings produced a usable client. False when nothing
    /// is stored yet and also when what is stored no longer parses.
    pub fn is_configured(&self) -> bool {
        self.state.client.borrow().is_some()
    }
}

/// Point the window at `config`. Returns whether a usable client came of it.
fn apply_config(state: &State, config: &ServerConfig) -> bool {
    match CairnClient::new(&config.host, config.port, config.token.as_deref()) {
        Ok(client) => {
            *state.client.borrow_mut() = Some(Arc::new(client));
            true
        }
        Err(err) => {
            // Dropping the old client matters: keeping it would leave the
            // window browsing the previous server while showing the new one.
            *state.client.borrow_mut() = None;
            toast(state, &err.to_string());
            false
        }
    }
}

fn toast(state: &State, message: &str) {
    state
        .toasts
        .add_toast(adw::Toast::builder().title(message).timeout(4).build());
}

fn show_status(state: &State, icon: &str, title: &str, description: &str) {
    state.status.set_icon_name(Some(icon));
    state.status.set_title(title);
    state.status.set_description(Some(description));
    state.stack.set_visible_child_name("status");
}

fn refresh_library(state: &Rc<State>) {
    let Some(client) = state.client.borrow().clone() else {
        show_status(
            state,
            "system-search-symbolic",
            "No server configured",
            "Open the settings and point Wander at a cairn instance.",
        );
        return;
    };
    if state.busy.replace(true) {
        return;
    }
    state.spinner.set_visible(true);
    state.spinner.set_spinning(true);

    let weak = Rc::downgrade(state);
    glib::spawn_future_local(async move {
        let fetched = gio::spawn_blocking(move || client.archives()).await;
        let Some(state) = weak.upgrade() else {
            return;
        };
        state.busy.set(false);
        state.spinner.set_spinning(false);
        state.spinner.set_visible(false);

        match fetched {
            Ok(Ok(archives)) => {
                clear_list(&state);
                if archives.is_empty() {
                    show_status(
                        &state,
                        "folder-symbolic",
                        "No open archives",
                        "The server is reachable but has no archives open.",
                    );
                    return;
                }
                *state.archives.borrow_mut() = archives.clone();
                let weak = Rc::downgrade(&state);
                for archive in archives {
                    state.list.append(&archive_row(&weak, archive));
                }
                state.stack.set_visible_child_name("library");
            }
            Ok(Err(err)) => report_refresh_failure(&state, &err.to_string()),
            Err(_) => report_refresh_failure(&state, "Background task failed."),
        }
    });
}

/// A failed refresh keeps whatever the library already showed — a stale listing
/// beats an empty window — and only takes over the view when there is nothing
/// left to preserve.
fn report_refresh_failure(state: &Rc<State>, message: &str) {
    if state.list.first_child().is_some() {
        toast(state, message);
    } else {
        show_status(
            state,
            "network-error-symbolic",
            "Cannot reach cairn",
            message,
        );
    }
}

fn clear_list(state: &State) {
    while let Some(child) = state.list.first_child() {
        state.list.remove(&child);
    }
}

fn archive_row(state: &Weak<State>, archive: ArchiveSummary) -> gtk::Widget {
    let mut subtitle = format!("{} entries", thousands(archive.entry_count));
    if archive.suggest {
        subtitle.push_str(" · searchable");
    }

    // Titles come out of the ZIM file, and an ActionRow renders them as Pango
    // markup by default: an archive called "Arts & Crafts" fails to parse and
    // renders empty. Markup has to be turned off *before* the title is set —
    // supplying it to the builder means it is parsed on the way in, warning and
    // mangling the text before any later call can take effect.
    let row = adw::ActionRow::builder()
        .subtitle(subtitle)
        .activatable(true)
        .build();
    row.set_use_markup(false);
    row.set_title(&archive.title);
    row.set_tooltip_text(Some(&archive.uuid));

    let state = state.clone();
    row.connect_activated(move |_| {
        let Some(state) = state.upgrade() else {
            return;
        };
        open_entry(&state, &archive, None);
    });

    row.upcast()
}

/// Push a reader for `archive`, optionally opening a particular entry.
fn open_entry(state: &Rc<State>, archive: &ArchiveSummary, path: Option<String>) {
    let Some(client) = state.client.borrow().clone() else {
        return;
    };
    let page = crate::reader::reader_page(client, archive.clone(), state.store.clone(), path);
    state.nav.push(&page);
}

/// A saved row names its archive by uuid, which is only reopenable while that
/// archive is still one the daemon has open.
fn open_saved(state: &Rc<State>, visit: &Visit) {
    let archive = state
        .archives
        .borrow()
        .iter()
        .find(|a| a.uuid == visit.uuid)
        .cloned();
    match archive {
        Some(archive) => open_entry(state, &archive, Some(visit.path.clone())),
        None => toast(
            state,
            &format!(
                "{} is not open on this server any more.",
                visit.archive_title
            ),
        ),
    }
}

/// History and bookmarks, side by side.
fn saved_page(state: &Rc<State>) -> adw::NavigationPage {
    let stack = adw::ViewStack::new();

    let history = saved_list(
        state,
        state.store.borrow().history(),
        "No history yet",
        "Entries you open are listed here.",
    );
    let bookmarks = saved_list(
        state,
        state.store.borrow().bookmarks(),
        "No bookmarks yet",
        "Star an entry while reading to keep it here.",
    );
    stack.add_titled_with_icon(
        &history,
        Some("history"),
        "History",
        "document-open-recent-symbolic",
    );
    stack.add_titled_with_icon(
        &bookmarks,
        Some("bookmarks"),
        "Bookmarks",
        "user-bookmarks-symbolic",
    );

    let header = adw::HeaderBar::new();
    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    header.set_title_widget(Some(&switcher));

    let clear = gtk::Button::from_icon_name("user-trash-symbolic");
    clear.set_tooltip_text(Some("Clear history"));
    header.pack_end(&clear);

    let clear_state = Rc::downgrade(state);
    let clear_stack = stack.clone();
    clear.connect_clicked(move |_| {
        let Some(state) = clear_state.upgrade() else {
            return;
        };
        state.store.borrow_mut().clear_history();
        state.store.borrow().save();
        // Rebuild in place so the emptied list is visible immediately.
        let refreshed = saved_list(
            &state,
            &[],
            "No history yet",
            "Entries you open are listed here.",
        );
        if let Some(old) = clear_stack.child_by_name("history") {
            clear_stack.remove(&old);
        }
        clear_stack.add_titled_with_icon(
            &refreshed,
            Some("history"),
            "History",
            "document-open-recent-symbolic",
        );
        clear_stack.set_visible_child_name("history");
        toast(&state, "History cleared.");
    });

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&stack));

    adw::NavigationPage::builder()
        .title("Saved")
        .child(&toolbar)
        .build()
}

fn saved_list(
    state: &Rc<State>,
    visits: &[Visit],
    empty_title: &str,
    empty_body: &str,
) -> gtk::Widget {
    if visits.is_empty() {
        return adw::StatusPage::builder()
            .icon_name("document-open-recent-symbolic")
            .title(empty_title)
            .description(empty_body)
            .build()
            .upcast();
    }

    let list = gtk::ListBox::new();
    list.set_css_classes(&["boxed-list"]);
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_margin_top(12);
    list.set_margin_bottom(12);
    list.set_margin_start(12);
    list.set_margin_end(12);

    for visit in visits {
        // Markup off before any archive-supplied text, as everywhere else.
        let row = adw::ActionRow::builder().activatable(true).build();
        row.set_use_markup(false);
        row.set_title(&visit.title);
        row.set_subtitle(&format!("{} · {}", visit.archive_title, visit.path));

        let weak = Rc::downgrade(state);
        let visit = visit.clone();
        row.connect_activated(move |_| {
            if let Some(state) = weak.upgrade() {
                open_saved(&state, &visit);
            }
        });
        list.append(&row);
    }

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_child(Some(&list));
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.upcast()
}

fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

fn open_settings(state: &Rc<State>) {
    let existing = settings::load().unwrap_or_default();
    let previous_host = existing.host.clone();
    let previous_port = existing.port;

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

    let state = Rc::downgrade(state);
    dialog.connect_closed(move |_| {
        let Some(state) = state.upgrade() else {
            return;
        };

        // A rejected field falls back to the previous value rather than
        // abandoning the whole edit: returning early here used to discard a
        // corrected host because the port next to it had a typo.
        let host = host_row.text().trim().to_string();
        let host = if host.is_empty() {
            toast(&state, "Host must not be empty; keeping the previous one.");
            previous_host.clone()
        } else {
            host
        };

        let port = match port_row.text().trim().parse::<u16>() {
            Ok(port) if port > 0 => port,
            _ => {
                toast(
                    &state,
                    &format!("Port must be between 1 and 65535; keeping {previous_port}."),
                );
                previous_port
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
        if apply_config(&state, &config) {
            toast(&state, &format!("Connecting to {host}:{port}…"));
            refresh_library(&state);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::thousands;

    #[test]
    fn small_numbers_are_left_alone() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(999), "999");
    }

    #[test]
    fn groups_of_three_are_separated() {
        // A narrow no-break space, so the count never wraps mid-number.
        assert_eq!(thousands(1_000), "1\u{202f}000");
        assert_eq!(thousands(20_317), "20\u{202f}317");
        assert_eq!(thousands(1_000_000), "1\u{202f}000\u{202f}000");
    }

    #[test]
    fn every_boundary_gets_exactly_one_separator() {
        for (n, expected) in [
            (999u64, 0usize),
            (1_000, 1),
            (999_999, 1),
            (1_000_000, 2),
            (u64::MAX, 6),
        ] {
            let formatted = thousands(n);
            assert_eq!(
                formatted.matches('\u{202f}').count(),
                expected,
                "{n} formatted as {formatted}"
            );
            // Separators are inserted, never substituted.
            assert_eq!(
                formatted.chars().filter(char::is_ascii_digit).count(),
                n.to_string().len()
            );
        }
    }
}
