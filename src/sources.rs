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

    /// Files whose contents differ from disk, with their new contents.
    pub fn changed(&self) -> Vec<(PathBuf, String)> {
        let mut changed: Vec<(PathBuf, String)> = self
            .overrides
            .read()
            .expect("not poisoned")
            .iter()
            .filter(|(path, text)| std::fs::read_to_string(path).ok().as_deref() != Some(text.as_str()))
            .map(|(path, text)| (path.clone(), text.clone()))
            .collect();
        changed.sort();
        changed
    }
}

/// Contents of a `.po`/`.pot` file as the PO rules see them: universal
/// newlines, as Python reads text files.
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
