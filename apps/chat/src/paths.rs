//! Well-known locations. Every path honours the XDG overrides so the whole app
//! can be pointed at a scratch copy of `~/.config` for testing.

pub use nyx_core::paths::*;

use std::path::PathBuf;

pub fn database() -> PathBuf {
    data_dir().join("chat.db")
}

pub fn attachments_dir() -> PathBuf {
    data_dir().join("attachments")
}

