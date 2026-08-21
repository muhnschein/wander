//! Headless HTTP client for the [cairn](https://github.com/muhnschein/cairn)
//! archive server. Speaks the API documented in `cairn-api(7)`: metadata as
//! JSON, entries as raw stored bytes, errors in one stable shape.
//!
//! The crate has no GTK linkage so it can be unit-tested anywhere.

mod client;
mod error;
mod model;

pub use client::{CairnClient, Entry, EntryMeta};
pub use error::{Error, code};
pub use model::{ArchiveDetail, ArchiveSummary, SandboxLayer, Status, Suggestion};
