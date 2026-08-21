use crate::scheme;
use adw::prelude::*;
use cairn_client::{ArchiveSummary, CairnClient};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use webkit::prelude::*;

const SUGGEST_LIMIT: u32 = 12;

const PATH_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

pub fn reader_page(client: Arc<CairnClient>, archive: ArchiveSummary) -> adw::NavigationPage {
    let uuid = archive.uuid.clone();
    let title = archive.title.clone();
    let main_page = archive.main_page.clone();

    let context = webkit::WebContext::new();
    let session = webkit::NetworkSession::new_ephemeral();
    scheme::install(&context, client.clone(), uuid.clone(), main_page);

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

    let search_entry = gtk::SearchEntry::new();
    search_entry.set_width_request(320);
    search_entry.set_placeholder_text(Some("Find article…"));
    header.set_title_widget(Some(&search_entry));

    let random_button = gtk::Button::from_icon_name("media-playlist-shuffle-symbolic");
    random_button.set_tooltip_text(Some("Random article"));
    header.pack_end(&random_button);

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
        generation: Cell::new(0),
    });

    let search_ui = ui.clone();
    let results_for_search = results.clone();
    search_entry.connect_search_changed(move |entry| {
        let ui = search_ui.clone();
        let results = results_for_search.clone();
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
            if ui.generation.get() != request_gen {
                return;
            }
            match fetched {
                Ok(Ok(suggestions)) => show_suggestions(&ui, &results, suggestions),
                _ => ui.popover.popdown(),
            }
        });
    });

    let activate_ui = ui.clone();
    results.connect_row_activated(move |_, row| {
        let name = row.widget_name().to_string();
        if let Some(path) = name.strip_prefix("path:") {
            load_entry(&activate_ui, path);
            activate_ui.popover.popdown();
        }
    });

    let random_ui = ui.clone();
    random_button.connect_clicked(move |_| {
        let ui = random_ui.clone();
        glib::spawn_future_local(async move {
            let client = ui.client.clone();
            let uuid = ui.uuid.clone();
            let fetched = gio::spawn_blocking(move || client.random(&uuid)).await;
            if let Ok(Ok(path)) = fetched {
                load_entry(&ui, &path);
            }
        });
    });

    view.load_uri(&format!("{}://{}/", scheme::SCHEME, uuid));

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
    generation: Cell<u64>,
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
    let encoded = utf8_percent_encode(path, PATH_SET).to_string();
    let uri = format!("{}://{}/{}", scheme::SCHEME, ui.uuid, encoded);
    ui.view.load_uri(&uri);
}

fn show_suggestions(
    ui: &ReaderUi,
    results: &gtk::ListBox,
    suggestions: Vec<cairn_client::Suggestion>,
) {
    while let Some(child) = results.first_child() {
        results.remove(&child);
    }
    if suggestions.is_empty() {
        ui.popover.popdown();
        return;
    }
    for suggestion in suggestions {
        let row = adw::ActionRow::builder()
            .title(&suggestion.title)
            .subtitle(&suggestion.path)
            .activatable(true)
            .build();
        row.set_widget_name(&format!("path:{}", suggestion.path));
        results.append(&row);
    }
    ui.popover.popup();
}
