//! Per-user base folders: XDG on Linux (`~/.config`, `~/.cache`), `%APPDATA%`
//! (settings) and `%LOCALAPPDATA%` (cache) on Windows. Callers add `spinwave`.

use std::path::PathBuf;

fn base(xdg_var: &str, home_sub: &str, windows_var: &str) -> Option<PathBuf> {
    std::env::var_os(xdg_var)
        .or_else(|| if cfg!(windows) { std::env::var_os(windows_var) } else { None })
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(home_sub)))
}

pub fn config_home() -> Option<PathBuf> {
    base("XDG_CONFIG_HOME", ".config", "APPDATA")
}

pub fn cache_home() -> Option<PathBuf> {
    base("XDG_CACHE_HOME", ".cache", "LOCALAPPDATA")
}
