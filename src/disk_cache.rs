//! Small metadata-only JSON cache in `~/.cache/spinwave/` (or
//! `$XDG_CACHE_HOME`): home/library/playlist rows (titles, video ids,
//! thumbnail links) and the last gate result. Never media, stream URLs,
//! cookies or tokens. Best-effort: any I/O or parse error reads as a miss.
//! Disabled under `cfg(test)` so tests never see the user's files.

use serde_json::Value;

#[cfg(not(test))]
fn dir() -> Option<std::path::PathBuf> {
    Some(crate::paths::cache_home()?.join("spinwave"))
}

/// File-safe name: ids come from YouTube, keep only `[A-Za-z0-9_-]`.
fn file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    format!("{safe}.json")
}

#[cfg(not(test))]
pub fn read(name: &str) -> Option<Value> {
    let text = std::fs::read_to_string(dir()?.join(file_name(name))).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
pub fn read(_name: &str) -> Option<Value> {
    None
}

/// Write atomically (temp file + rename), owner-only permissions.
#[cfg(not(test))]
pub fn write(name: &str, value: &Value) {
    use std::io::Write;
    let Some(dir) = dir() else { return };
    let mut dirs = std::fs::DirBuilder::new();
    let mut file = std::fs::OpenOptions::new();
    file.write(true).create(true).truncate(true);
    // Owner-only on Unix; Windows profile folders are per-user already.
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        dirs.mode(0o700);
        file.mode(0o600);
    }
    let _ = dirs.recursive(true).create(&dir);
    let path = dir.join(file_name(name));
    let tmp = path.with_extension("json.tmp");
    let written = file
        .open(&tmp)
        .and_then(|mut f| f.write_all(value.to_string().as_bytes()));
    if written.is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

#[cfg(test)]
pub fn write(_name: &str, _value: &Value) {}

/// Forget one entry.
#[cfg(not(test))]
pub fn remove(name: &str) {
    if let Some(dir) = dir() {
        let _ = std::fs::remove_file(dir.join(file_name(name)));
    }
}

#[cfg(test)]
pub fn remove(_name: &str) {}

/// Forget every entry with this prefix (e.g. all `playlist-*` rows).
#[cfg(not(test))]
pub fn remove_prefix(prefix: &str) {
    let Some(dir) = dir() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
pub fn remove_prefix(_prefix: &str) {}

#[cfg(test)]
mod tests {
    #[test]
    fn names_are_sanitized() {
        assert_eq!(super::file_name("playlist-PL_a-1"), "playlist-PL_a-1.json");
        assert_eq!(super::file_name("../../etc/passwd"), "etcpasswd.json");
    }
}
