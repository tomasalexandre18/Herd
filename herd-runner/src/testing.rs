//! Test support, shared between this crate's unit tests and its end-to-end
//! tests. Not part of the runner's interface; it exists so that neither kind
//! of test grows its own half-right temporary-directory handling, and so that
//! no test can leave a data root behind on the machine it ran on.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A directory under the system temporary directory, removed on drop.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// A fresh directory named after `label`, unique per process and call.
    pub fn new(label: &str) -> Self {
        let unique = NEXT.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("herd-test-{label}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temporary directory is creatable");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Every Rust source file of this crate, as (path, text).
///
/// Used by the tests that are about the code itself rather than about its
/// behaviour — the one that forbids reaching for an escalation tool, and the
/// one that forbids binding a socket. `CARGO_MANIFEST_DIR` is set by cargo
/// for both unit and end-to-end tests, so the walk starts from the crate
/// rather than from whatever directory the suite was launched in.
pub fn source_files() -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR"))];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    found.push((path, text));
                }
            }
        }
    }
    found
}
