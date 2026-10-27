//! Helpers the integration tests share.

use std::fs;
use std::path::{Path, PathBuf};

/// Writes `contents` to `path` under `root`, creating its folders.
pub fn write(root: &Path, path: &str, contents: impl AsRef<[u8]>) -> PathBuf {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}
