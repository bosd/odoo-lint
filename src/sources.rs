//! File contents for the linter: the files on disk, overlaid with the
//! in-memory results of fixes, so several fix passes and `--diff` work
//! without writing anything until the end.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Debug, Default)]
pub struct Sources {
    overrides: RwLock<HashMap<PathBuf, String>>,
}

impl Sources {
    /// The current contents of `path` as text.
    pub fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if let Some(text) = self.overrides.read().expect("not poisoned").get(path) {
            return Ok(text.clone());
        }
        std::fs::read_to_string(path)
    }

    /// The current contents of `path` as bytes.
    pub fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        if let Some(text) = self.overrides.read().expect("not poisoned").get(path) {
            return Ok(text.clone().into_bytes());
        }
        std::fs::read(path)
    }

    pub fn set(&self, path: PathBuf, contents: String) {
        self.overrides.write().expect("not poisoned").insert(path, contents);
    }

    /// Back to the contents on disk.
    pub fn remove(&self, path: &Path) {
        self.overrides.write().expect("not poisoned").remove(path);
    }

    /// An independent copy, to fix without touching the original.
    pub fn snapshot(&self) -> Sources {
        Sources {
            overrides: RwLock::new(self.overrides.read().expect("not poisoned").clone()),
        }
    }
}

/// Contents of a `.po`/`.pot` file as the PO rules see them: universal
/// newlines, as Python reads text files.
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
