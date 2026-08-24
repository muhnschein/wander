use crate::scheme;
use crate::toc::Heading;
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
    let headings = scheme::HeadingStore::default();
    scheme::install(
        &context,
        client.clone(),
        uuid.clone(),
        main_page.clone(),
        headings.clone(),
    );

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

    let toc_button = gtk::ToggleButton::new();
    toc_button.set_icon_name("view-list-symbolic");
    toc_button.set_tooltip_text(Some("Table of contents"));
    toc_button.set_sensitive(false);
    header.pack_start(&toc_button);

    let random_button = gtk::Button::from_icon_name("media-playlist-shuffle-symbolic");
    random_button.set_tooltip_text(Some("Random article"));
    header.pack_end(&random_button);

    let find_button = gtk::ToggleButton::new();
    find_button.set_icon_name("edit-find-symbolic");
    find_button.set_tooltip_text(Some("Find on page (Ctrl+F)"));
    header.pack_end(&find_button);

    let spinner = gtk::Spinner::new();
    header.pack_end(&spinner);

    let toc_list = gtk::ListBox::new();
    toc_list.set_selection_mode(gtk::SelectionMode::None);
    toc_list.add_css_class("navigation-sidebar");
    let toc_scroll = gtk::ScrolledWindow::new();
    toc_scroll.set_child(Some(&toc_list));
    toc_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    toc_scroll.set_vexpand(true);

    let sidebar = adw::ToolbarView::new();
    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.set_show_end_title_buttons(false);
    sidebar_header.set_title_widget(Some(&gtk::Label::new(Some("Contents"))));
    sidebar.add_top_bar(&sidebar_header);
    sidebar.set_content(Some(&toc_scroll));

    let split = adw::OverlaySplitView::new();
    split.set_sidebar(Some(&sidebar));
    split.set_content(Some(&view));
    split.set_show_sidebar(false);
    split.set_max_sidebar_width(320.0);
    split
        .bind_property("show-sidebar", &toc_button, "active")
        .bidirectional()
        .sync_create()
        .build();

    let find_entry = gtk::SearchEntry::new();
    find_entry.set_placeholder_text(Some("Find on page…"));
    find_entry.set_hexpand(true);
    let matches_label = gtk::Label::new(None);
    matches_label.add_css_class("dim-label");
    let find_prev = gtk::Button::from_icon_name("go-up-symbolic");
    find_prev.set_tooltip_text(Some("Previous match"));
    let find_next = gtk::Button::from_icon_name("go-down-symbolic");
    find_next.set_tooltip_text(Some("Next match"));
    let find_nav = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    find_nav.add_css_class("linked");
    find_nav.append(&find_prev);
    find_nav.append(&find_next);
    let find_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    find_box.append(&find_entry);
    find_box.append(&matches_label);
    find_box.append(&find_nav);
    let find_bar = gtk::SearchBar::new();
    find_bar.set_child(Some(&find_box));
    find_bar.connect_entry(&find_entry);
    find_bar
        .bind_property("search-mode-enabled", &find_button, "active")
        .bidirectional()
        .sync_create()
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&find_bar);
    toolbar.set_content(Some(&split));

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
        headings,
        toc_list: toc_list.clone(),
        anchors: RefCell::new(Vec::new()),
        current_path: RefCell::new(String::new()),
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
    let toc_ui = ui.clone();
    let toc_toggle = toc_button.clone();
    view.connect_load_changed(move |view, event| {
        back.set_sensitive(view.can_go_back());
        forward.set_sensitive(view.can_go_forward());
        let loading = view.is_loading();
        spin.set_visible(loading);
        spin.set_spinning(loading);
        if event == webkit::LoadEvent::Finished {
            refresh_outline(&toc_ui, &toc_toggle);
        }
    });

    let jump_ui = ui.clone();
    toc_list.connect_row_activated(move |_, row| {
        let index = row.index();
        if index < 0 {
            return;
        }
        let anchor = jump_ui
            .anchors
            .borrow()
            .get(index as usize)
            .cloned()
            .flatten();
        let Some(anchor) = anchor else {
            return;
        };
        let path = jump_ui.current_path.borrow().clone();
        jump_ui.view.load_uri(&scheme::entry_uri_with_fragment(
            &jump_ui.uuid,
            &path,
            &anchor,
        ));
    });

    install_find(
        &view,
        &find_bar,
        &find_entry,
        &matches_label,
        &find_prev,
        &find_next,
    );

    // Ctrl+F reveals the find bar; SearchBar handles Escape to dismiss it.
    let shortcuts = gtk::ShortcutController::new();
    shortcuts.set_scope(gtk::ShortcutScope::Managed);
    let reveal = find_bar.clone();
    shortcuts.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("<Control>f"),
        Some(gtk::CallbackAction::new(move |_, _| {
            reveal.set_search_mode(true);
            glib::Propagation::Stop
        })),
    ));
    toolbar.add_controller(shortcuts);

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
    headings: scheme::HeadingStore,
    toc_list: gtk::ListBox,
    /// Anchor for each row in `toc_list`, parallel by index; `None` for a
    /// heading the archive gave no id, which can be listed but not jumped to.
    anchors: RefCell<Vec<Option<String>>>,
    /// Entry path currently displayed, so an outline jump knows what to reload.
    current_path: RefCell<String>,
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

/// Rebuild the sidebar from whatever outline the scheme handler recorded for
/// the entry now on screen, and enable the toggle only when there is one.
fn refresh_outline(ui: &Rc<ReaderUi>, toggle: &gtk::ToggleButton) {
    while let Some(child) = ui.toc_list.first_child() {
        ui.toc_list.remove(&child);
    }
    ui.anchors.borrow_mut().clear();

    let path = ui
        .view
        .uri()
        .and_then(|uri| scheme::parse_target(&uri))
        .unwrap_or_default();
    *ui.current_path.borrow_mut() = path.clone();

    let outline = ui.headings.get(&path).unwrap_or_default();
    // A lone title is not an outline worth a sidebar for.
    if outline.len() < 2 {
        toggle.set_sensitive(false);
        toggle.set_active(false);
        return;
    }

    let mut anchors = ui.anchors.borrow_mut();
    // Indent relative to the shallowest heading present, so a page whose
    // sections start at h2 is not uniformly indented.
    let top = outline.iter().map(|h| h.level).min().unwrap_or(1);
    for heading in outline.iter() {
        ui.toc_list.append(&outline_row(heading, top));
        anchors.push(heading.anchor.clone());
    }
    drop(anchors);
    toggle.set_sensitive(true);
}

fn outline_row(heading: &Heading, top: u8) -> gtk::ListBoxRow {
    let label = gtk::Label::new(Some(&heading.text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    // Headings are archive text, never markup.
    label.set_use_markup(false);
    label.set_margin_top(6);
    label.set_margin_bottom(6);
    label.set_margin_end(12);
    label.set_margin_start(12 + 12 * i32::from(heading.level.saturating_sub(top)));
    if heading.level == top {
        label.add_css_class("heading");
    }

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&label));
    // A heading with no id cannot be scrolled to, so it reads as a label.
    row.set_activatable(heading.anchor.is_some());
    row.set_selectable(false);
    row
}

fn install_find(
    view: &webkit::WebView,
    bar: &gtk::SearchBar,
    entry: &gtk::SearchEntry,
    matches: &gtk::Label,
    previous: &gtk::Button,
    next: &gtk::Button,
) {
    let Some(controller) = view.find_controller() else {
        bar.set_sensitive(false);
        return;
    };
    let options = (webkit::FindOptions::CASE_INSENSITIVE | webkit::FindOptions::WRAP_AROUND).bits();
    const MAX_MATCHES: u32 = 1000;

    let find = controller.clone();
    let label = matches.clone();
    entry.connect_search_changed(move |entry| {
        let text = entry.text();
        if text.is_empty() {
            find.search_finish();
            label.set_text("");
            return;
        }
        find.search(&text, options, MAX_MATCHES);
        find.count_matches(&text, options, MAX_MATCHES);
    });

    let find = controller.clone();
    previous.connect_clicked(move |_| find.search_previous());
    let find = controller.clone();
    next.connect_clicked(move |_| find.search_next());
    let find = controller.clone();
    entry.connect_activate(move |_| find.search_next());

    let label = matches.clone();
    controller.connect_counted_matches(move |_, count| {
        label.set_text(&match count {
            0 => String::new(),
            n if n >= MAX_MATCHES => format!("{MAX_MATCHES}+ matches"),
            1 => "1 match".to_string(),
            n => format!("{n} matches"),
        });
    });

    let label = matches.clone();
    controller.connect_failed_to_find_text(move |_| label.set_text("No matches"));

    // Leaving the bar must clear the highlight, or it persists over the page.
    let find = controller.clone();
    let entry = entry.clone();
    let label = matches.clone();
    bar.connect_search_mode_enabled_notify(move |bar| {
        if bar.is_search_mode() {
            entry.grab_focus();
        } else {
            find.search_finish();
            label.set_text("");
        }
    });
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
    let row = adw::ActionRow::builder().build();
    row.set_use_markup(false);
    row.set_title(message);
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
        // Suggestion titles and paths come from the archive, so markup has to
        // be off before either is set; the builder would parse them on the way
        // in and drop any title containing `&`.
        let row = adw::ActionRow::builder().activatable(true).build();
        row.set_use_markup(false);
        row.set_title(&suggestion.title);
        row.set_subtitle(&suggestion.path);
        ui.results.append(&row);
        paths.push(suggestion.path);
    }
    drop(paths);
    ui.popover.popup();
}
