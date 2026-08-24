use crate::scheme;
use adw::prelude::*;
use cairn_client::{ArchiveSummary, CairnClient};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use webkit::prelude::*;

const SUGGEST_LIMIT: u32 = 12;

pub fn reader_page(client: Arc<CairnClient>, archive: ArchiveSummary) -> adw::NavigationPage {
    let uuid = archive.uuid.clone();
    let title = archive.title.clone();
    let main_page = archive.main_page.clone();

    let context = webkit::WebContext::new();
    let session = webkit::NetworkSession::new_ephemeral();
    block_outbound_network(&session);
    scheme::install(&context, client.clone(), uuid.clone(), main_page.clone());

    let view = webkit::WebView::builder()
        .web_context(&context)
        .network_session(&session)
        .build();
    view.set_vexpand(true);
    view.set_hexpand(true);
    if let Some(settings) = webkit::prelude::WebViewExt::settings(&view) {
        settings.set_enable_javascript(false);
    }

    install_policy_guard(&view);

    let header = adw::HeaderBar::new();

    let back_button = gtk::Button::from_icon_name("go-previous-symbolic");
    back_button.set_tooltip_text(Some("Back"));
    back_button.set_sensitive(false);
    let forward_button = gtk::Button::from_icon_name("go-next-symbolic");
    forward_button.set_tooltip_text(Some("Forward"));
    forward_button.set_sensitive(false);

    let history = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    history.add_css_class("linked");
    history.append(&back_button);
    history.append(&forward_button);
    header.pack_start(&history);

    let search_entry = gtk::SearchEntry::new();
    search_entry.set_width_request(320);
    search_entry.set_placeholder_text(Some("Find article…"));
    header.set_title_widget(Some(&search_entry));

    let random_button = gtk::Button::from_icon_name("media-playlist-shuffle-symbolic");
    random_button.set_tooltip_text(Some("Random article"));
    header.pack_end(&random_button);

    let spinner = gtk::Spinner::new();
    header.pack_end(&spinner);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&view));

    let popover = gtk::Popover::new();
    let results = gtk::ListBox::new();
    results.set_css_classes(&["boxed-list"]);
    popover.set_child(Some(&results));
    popover.set_autohide(true);
    popover.set_parent(&search_entry);

    let ui = Rc::new(ReaderUi {
        client,
        uuid: uuid.clone(),
        view: view.clone(),
        popover: popover.clone(),
        results: results.clone(),
        paths: RefCell::new(Vec::new()),
        generation: Cell::new(0),
    });

    // A popover parented to a widget is not owned by it: without this GTK warns
    // about a finalised widget that still has children when the page is popped.
    let orphan = popover.clone();
    search_entry.connect_destroy(move |_| orphan.unparent());

    let search_ui = ui.clone();
    search_entry.connect_search_changed(move |entry| {
        let ui = search_ui.clone();
        let query = entry.text().to_string();
        let request_gen = ui.generation.get() + 1;
        ui.generation.set(request_gen);
        glib::spawn_future_local(async move {
            if query.is_empty() {
                ui.popover.popdown();
                return;
            }
            let client = ui.client.clone();
            let uuid = ui.uuid.clone();
            let fetched =
                gio::spawn_blocking(move || client.suggest(&uuid, &query, SUGGEST_LIMIT)).await;
            // A slower earlier request must not overwrite the newest results.
            if ui.generation.get() != request_gen {
                return;
            }
            match fetched {
                Ok(Ok(suggestions)) => show_suggestions(&ui, suggestions),
                // Reporting a live-search failure as a toast would fire on every
                // keystroke while the daemon is down, so it goes in the popover
                // where the next keystroke replaces it.
                Ok(Err(err)) => show_notice(&ui, &format!("Search failed: {err}")),
                Err(_) => show_notice(&ui, "Search failed: background task failed."),
            }
        });
    });

    let activate_ui = ui.clone();
    results.connect_row_activated(move |_, row| {
        let index = row.index();
        if index < 0 {
            return;
        }
        let path = activate_ui.paths.borrow().get(index as usize).cloned();
        if let Some(path) = path {
            load_entry(&activate_ui, &path);
            activate_ui.popover.popdown();
        }
    });

    let random_ui = ui.clone();
    random_button.connect_clicked(move |button| {
        let ui = random_ui.clone();
        let button = button.clone();
        button.set_sensitive(false);
        glib::spawn_future_local(async move {
            let client = ui.client.clone();
            let uuid = ui.uuid.clone();
            let fetched = gio::spawn_blocking(move || client.random(&uuid)).await;
            button.set_sensitive(true);
            match fetched {
                Ok(Ok(path)) => load_entry(&ui, &path),
                Ok(Err(err)) => toast(&ui.view, &format!("No random article: {err}")),
                Err(_) => toast(&ui.view, "No random article: background task failed."),
            }
        });
    });

    let back_view = view.clone();
    back_button.connect_clicked(move |_| back_view.go_back());
    let forward_view = view.clone();
    forward_button.connect_clicked(move |_| forward_view.go_forward());

    let back = back_button.clone();
    let forward = forward_button.clone();
    let spin = spinner.clone();
    view.connect_load_changed(move |view, _| {
        back.set_sensitive(view.can_go_back());
        forward.set_sensitive(view.can_go_forward());
        let loading = view.is_loading();
        spin.set_visible(loading);
        spin.set_spinning(loading);
    });

    view.connect_load_failed(|view, _, uri, error| {
        // The scheme handler already turned a cairn error into this message;
        // surface it instead of leaving the reader on a blank page.
        toast(view, &format!("Could not open {uri}: {}", error.message()));
        false
    });

    // Loading the main page under its own URI rather than the archive root
    // gives WebKit the correct base for the relative links inside it.
    match main_page.as_deref() {
        Some(main) if !main.is_empty() => load_entry(&ui, main),
        _ => view.load_uri(&format!("{}://{}/", scheme::SCHEME, uuid)),
    }

    adw::NavigationPage::builder()
        .title(title)
        .child(&toolbar)
        .build()
}

struct ReaderUi {
    client: Arc<CairnClient>,
    uuid: String,
    view: webkit::WebView,
    popover: gtk::Popover,
    results: gtk::ListBox,
    /// Entry path for each row currently in `results`, parallel by index. Rows
    /// carry no payload of their own, so this is what an activated row maps to.
    paths: RefCell<Vec<String>>,
    generation: Cell<u64>,
}

/// Fail every real network load in this session closed.
///
/// The navigation policy handler only sees navigations. An archived page that
/// carries an absolute `https://` image, stylesheet or font would have those
/// subresources fetched for real, announcing the reader's address to whoever
/// the archive author linked to — precisely what an offline archive reader
/// should not do. Pointing every proxied scheme at a closed local port is the
/// bluntest reliable way to stop that.
///
/// `cairn://` is untouched: a registered URI scheme is served by its handler
/// inside the web process and never reaches the networking stack.
fn block_outbound_network(session: &webkit::NetworkSession) {
    let blackhole = webkit::NetworkProxySettings::new(Some("http://127.0.0.1:9"), &[]);
    session.set_proxy_settings(webkit::NetworkProxyMode::Custom, Some(&blackhole));
}

fn install_policy_guard(view: &webkit::WebView) {
    view.connect_decide_policy(|view, decision, _| {
        let Some(navigation) = decision.downcast_ref::<webkit::NavigationPolicyDecision>() else {
            return false;
        };
        let Some(action) = navigation.navigation_action() else {
            return false;
        };
        let Some(request) = action.request() else {
            return false;
        };
        let Some(uri) = request.uri() else {
            return false;
        };
        let uri = uri.to_string();
        if uri.starts_with(&format!("{}://", scheme::SCHEME)) {
            return false;
        }
        if let Some(parent) = view
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        {
            ask_open_external(&parent, &uri);
        }
        // Claiming the decision without resolving it would leave the navigation
        // pending forever; the default handler that would have called `use_` no
        // longer runs once this returns true.
        decision.ignore();
        true
    });
}

fn ask_open_external(parent: &gtk::Window, uri: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading("External link")
        .body(format!("Open this link outside Wander?\n\n{uri}"))
        .body_use_markup(false)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("open", "Open");
    dialog.set_response_appearance("open", adw::ResponseAppearance::Suggested);
    dialog.set_close_response("cancel");

    let uri = uri.to_string();
    let parent: gtk::Window = parent.clone();
    glib::spawn_future_local(async move {
        if dialog.choose_future(Some(&parent)).await.as_str() == "open" {
            let launcher = gtk::UriLauncher::builder().uri(&uri).build();
            let _ = launcher.launch_future(Some(&parent)).await;
        }
    });
}

fn load_entry(ui: &ReaderUi, path: &str) {
    ui.view.load_uri(&scheme::entry_uri(&ui.uuid, path));
}

/// Post a message on the window's toast overlay, if the widget is in one.
fn toast(widget: &impl IsA<gtk::Widget>, message: &str) {
    if let Some(overlay) = widget
        .as_ref()
        .ancestor(adw::ToastOverlay::static_type())
        .and_then(|w| w.downcast::<adw::ToastOverlay>().ok())
    {
        overlay.add_toast(adw::Toast::builder().title(message).timeout(4).build());
    }
}

fn clear_results(ui: &ReaderUi) {
    while let Some(child) = ui.results.first_child() {
        ui.results.remove(&child);
    }
    ui.paths.borrow_mut().clear();
}

/// Show a single non-activatable row in the suggestion popover.
fn show_notice(ui: &ReaderUi, message: &str) {
    clear_results(ui);
    let row = adw::ActionRow::builder().title(message).build();
    row.set_use_markup(false);
    row.add_css_class("dim-label");
    ui.results.append(&row);
    ui.popover.popup();
}

fn show_suggestions(ui: &ReaderUi, suggestions: Vec<cairn_client::Suggestion>) {
    clear_results(ui);
    if suggestions.is_empty() {
        show_notice(ui, "No matching articles.");
        return;
    }
    let mut paths = ui.paths.borrow_mut();
    for suggestion in suggestions {
        let row = adw::ActionRow::builder()
            .title(&suggestion.title)
            .subtitle(&suggestion.path)
            .activatable(true)
            .build();
        // Suggestion titles and paths come from the archive; an ActionRow would
        // otherwise render them as Pango markup and drop any containing `&`.
        row.set_use_markup(false);
        ui.results.append(&row);
        paths.push(suggestion.path);
    }
    drop(paths);
    ui.popover.popup();
}
