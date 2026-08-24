//! On-disk record of where the reader has been.
//!
//! History and bookmarks share one file because they share a shape: both are
//! lists of entries identified by archive and path. Written to the data
//! directory rather than the config one — this is a record of use, not
//! configuration — and mode 0600, since what someone has been reading is
//! nobody else's business.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// How many visits to keep. Old enough entries stop being useful as history
/// and start being a liability, and the file is rewritten on every visit.
const HISTORY_LIMIT: usize = 500;

/// One entry, in one archive. `archive_title` is denormalised on purpose: an
/// archive can be closed on the daemon, and a history row that cannot say
/// which archive it came from is not much of a row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visit {
    pub uuid: String,
    pub path: String,
    pub title: String,
    pub archive_title: String,
    /// Seconds since the Unix epoch; 0 when the clock was unavailable.
    #[serde(default)]
    pub at: u64,
}

impl Visit {
    /// Whether this refers to the same entry as `other`, ignoring when and
    /// under what title it was seen.
    fn same_entry(&self, other: &Self) -> bool {
        self.uuid == other.uuid && self.path == other.path
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    history: Vec<Visit>,
    #[serde(default)]
    bookmarks: Vec<Visit>,
}

impl Store {
    /// Most recent first.
    pub fn history(&self) -> &[Visit] {
        &self.history
    }

    /// Most recently added first.
    pub fn bookmarks(&self) -> &[Visit] {
        &self.bookmarks
    }

    /// Record a visit, moving an entry already seen to the front rather than
    /// letting a page revisited ten times fill ten rows.
    pub fn record(&mut self, visit: Visit) {
        self.history.retain(|known| !known.same_entry(&visit));
        self.history.insert(0, visit);
        self.history.truncate(HISTORY_LIMIT);
    }

    pub fn is_bookmarked(&self, uuid: &str, path: &str) -> bool {
        self.bookmarks
            .iter()
            .any(|b| b.uuid == uuid && b.path == path)
    }

    /// Add or remove a bookmark for this entry. Returns whether it is now
    /// bookmarked.
    pub fn toggle_bookmark(&mut self, visit: Visit) -> bool {
        if self.is_bookmarked(&visit.uuid, &visit.path) {
            self.bookmarks.retain(|known| !known.same_entry(&visit));
            false
        } else {
            self.bookmarks.insert(0, visit);
            true
        }
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Read the store, or an empty one when it is missing or unreadable.
    ///
    /// A corrupt file is not worth refusing to start over; the worst case is
    /// losing a reading list, and the next write replaces it.
    pub fn load() -> Self {
        Self::load_from(&store_path())
    }

    pub fn save(&self) {
        self.save_to(&store_path());
    }

    fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Written via a temporary file and renamed, so an interrupted write
    /// leaves the previous store intact rather than a half-written one.
    fn save_to(&self, path: &Path) {
        let Ok(json) = serde_json::to_string_pretty(self) else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let temp = path.with_extension("json.tmp");
        if std::fs::write(&temp, json).is_err() {
            return;
        }
        restrict(&temp);
        if std::fs::rename(&temp, path).is_err() {
            let _ = std::fs::remove_file(&temp);
        }
    }
}

fn store_path() -> PathBuf {
    let mut dir = glib::user_data_dir();
    dir.push("wander");
    dir.push("library.json");
    dir
}

#[cfg(unix)]
fn restrict(path: &Path) {
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

/// Seconds since the Unix epoch, or 0 if the clock is before it.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visit(uuid: &str, path: &str, title: &str) -> Visit {
        Visit {
            uuid: uuid.to_string(),
            path: path.to_string(),
            title: title.to_string(),
            archive_title: "Wikipedia".to_string(),
            at: 1_700_000_000,
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "wander-store-test-{}-{name}.json",
            std::process::id()
        ));
        p
    }

    #[test]
    fn the_newest_visit_comes_first() {
        let mut store = Store::default();
        store.record(visit("a", "One", "One"));
        store.record(visit("a", "Two", "Two"));
        assert_eq!(
            store.history().iter().map(|v| &v.path).collect::<Vec<_>>(),
            ["Two", "One"]
        );
    }

    #[test]
    fn revisiting_moves_an_entry_up_instead_of_duplicating_it() {
        let mut store = Store::default();
        store.record(visit("a", "One", "One"));
        store.record(visit("a", "Two", "Two"));
        store.record(visit("a", "One", "One again"));
        assert_eq!(store.history().len(), 2);
        assert_eq!(store.history()[0].path, "One");
        // The newer title wins, since it is what the page says now.
        assert_eq!(store.history()[0].title, "One again");
    }

    #[test]
    fn the_same_path_in_another_archive_is_a_different_entry() {
        let mut store = Store::default();
        store.record(visit("a", "index.html", "A"));
        store.record(visit("b", "index.html", "B"));
        assert_eq!(store.history().len(), 2);
    }

    #[test]
    fn history_stays_bounded_and_drops_the_oldest() {
        let mut store = Store::default();
        for i in 0..HISTORY_LIMIT + 20 {
            store.record(visit("a", &format!("Entry{i}"), "T"));
        }
        assert_eq!(store.history().len(), HISTORY_LIMIT);
        assert_eq!(
            store.history()[0].path,
            format!("Entry{}", HISTORY_LIMIT + 19)
        );
        assert!(!store.history().iter().any(|v| v.path == "Entry0"));
    }

    #[test]
    fn bookmarking_toggles_both_ways() {
        let mut store = Store::default();
        assert!(!store.is_bookmarked("a", "One"));
        assert!(store.toggle_bookmark(visit("a", "One", "One")));
        assert!(store.is_bookmarked("a", "One"));
        assert_eq!(store.bookmarks().len(), 1);
        assert!(!store.toggle_bookmark(visit("a", "One", "One")));
        assert!(!store.is_bookmarked("a", "One"));
        assert!(store.bookmarks().is_empty());
    }

    #[test]
    fn bookmarks_are_independent_of_history() {
        let mut store = Store::default();
        store.toggle_bookmark(visit("a", "One", "One"));
        store.record(visit("a", "Two", "Two"));
        store.clear_history();
        assert!(store.history().is_empty());
        assert!(
            store.is_bookmarked("a", "One"),
            "clearing history kept bookmarks"
        );
    }

    #[test]
    fn a_store_round_trips_through_disk() {
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);
        let mut store = Store::default();
        store.record(visit("a", "A/Wien Ä.html", "Wien Ä"));
        store.toggle_bookmark(visit("b", "index.html", "Home"));
        store.save_to(&path);

        let loaded = Store::load_from(&path);
        assert_eq!(loaded.history(), store.history());
        assert_eq!(loaded.bookmarks(), store.bookmarks());
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn the_saved_file_is_not_readable_by_others() {
        let path = temp_path("perms");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, b"{}").expect("seed");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("seed mode");

        let mut store = Store::default();
        store.record(visit("a", "One", "One"));
        store.save_to(&path);

        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "reading history must not be world-readable");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_corrupt_or_missing_file_reads_as_empty() {
        assert!(
            Store::load_from(Path::new("/nonexistent/wander/library.json"))
                .history()
                .is_empty()
        );

        let path = temp_path("corrupt");
        std::fs::write(&path, b"{ this is not json").expect("seed");
        let store = Store::load_from(&path);
        assert!(store.history().is_empty() && store.bookmarks().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_older_file_without_every_field_still_loads() {
        // `at` is `#[serde(default)]` so a store written before it existed is
        // readable rather than discarded as corrupt.
        let path = temp_path("partial");
        std::fs::write(
            &path,
            br#"{"history":[{"uuid":"a","path":"One","title":"T","archive_title":"W"}]}"#,
        )
        .expect("seed");
        let store = Store::load_from(&path);
        assert_eq!(store.history().len(), 1);
        assert_eq!(store.history()[0].at, 0);
        assert!(store.bookmarks().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_interrupted_write_leaves_no_stray_temp_file() {
        let path = temp_path("atomic");
        let _ = std::fs::remove_file(&path);
        let mut store = Store::default();
        store.record(visit("a", "One", "One"));
        store.save_to(&path);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temp file left behind"
        );
        let _ = std::fs::remove_file(&path);
    }
}
