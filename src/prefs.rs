//! User preferences in `~/.config/spinwave/prefs.json` (or
//! `$XDG_CONFIG_HOME`): small booleans the Settings page toggles. Missing or
//! malformed files fall back to the defaults. Disabled under `cfg(test)`.

/// Settings-page toggles. Defaults: resume on, at the exact position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    /// Reopen the last queue on launch.
    pub resume: bool,
    /// Resume mid-song at the saved position (else from the song's start).
    pub resume_position: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { resume: true, resume_position: true }
    }
}

impl Prefs {
    fn from_json(v: &serde_json::Value) -> Self {
        let d = Self::default();
        let flag = |key: &str, default: bool| v.get(key).and_then(|x| x.as_bool()).unwrap_or(default);
        Self {
            resume: flag("resume", d.resume),
            resume_position: flag("resumePosition", d.resume_position),
        }
    }
}

#[cfg(not(test))]
fn path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("spinwave").join("prefs.json"))
}

#[cfg(not(test))]
pub fn load() -> Prefs {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .map(|v| Prefs::from_json(&v))
        .unwrap_or_default()
}

#[cfg(test)]
pub fn load() -> Prefs {
    Prefs::default()
}

/// Best-effort save (a preference isn't worth an error dialog).
#[cfg(not(test))]
pub fn save(prefs: Prefs) {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let json = serde_json::json!({ "resume": prefs.resume, "resumePosition": prefs.resume_position });
    let _ = std::fs::write(path, json.to_string());
}

#[cfg(test)]
pub fn save(_prefs: Prefs) {}

#[cfg(test)]
mod tests {
    use super::Prefs;

    #[test]
    fn missing_keys_keep_defaults() {
        let p = Prefs::from_json(&serde_json::json!({ "resumePosition": false }));
        assert_eq!(p, Prefs { resume: true, resume_position: false });
        assert_eq!(Prefs::from_json(&serde_json::json!(null)), Prefs::default());
    }
}
