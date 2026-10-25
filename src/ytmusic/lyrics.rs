//! Lyrics for the current song, when YouTube Music has them (plain text).
//! One background fetch per song; only the latest result is kept, in memory.
//!
//! shortcut: no synced lyrics. Timed lines come only from the Android
//! client, which rejects the web session (HTTP 400) and strips the timings
//! for guests (checked 2026-10-25); revisit with an Android OAuth session.

use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

/// (video id, result JSON; `None` while the worker runs).
type Slot = Option<(String, Option<String>)>;

fn slot() -> &'static Mutex<Slot> {
    static SLOT: OnceLock<Mutex<Slot>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// The song's lyrics JSON (`{"lines": [text], "source"}`; empty `lines` =
/// none), or `""` while loading. The first call for a song
/// starts the fetch; callers poll.
pub fn lyrics_json(video_id: &str, gate_status: &str) -> String {
    if video_id.is_empty() {
        return String::new();
    }
    let Ok(mut guard) = slot().lock() else {
        return String::new();
    };
    if let Some((id, result)) = guard.as_ref() {
        if id == video_id {
            return result.clone().unwrap_or_default();
        }
    }
    *guard = Some((video_id.to_string(), None));
    drop(guard);
    let (id, gate) = (video_id.to_string(), gate_status.to_string());
    let _ = std::thread::Builder::new().name("lyrics".to_string()).spawn(move || {
        // Any failure reads as "no lyrics" (never an error dialog).
        let found = fetch(&id, &gate).unwrap_or(None);
        let result = parse_lyrics(found.as_ref()).to_string();
        if let Ok(mut guard) = slot().lock() {
            if let Some((current, pending)) = guard.as_mut() {
                if *current == id {
                    *pending = Some(result);
                }
            }
        }
    });
    String::new()
}

fn fetch(video_id: &str, gate_status: &str) -> Result<Option<Value>, String> {
    let (mut yt, _gate) = super::live_client(gate_status)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let watch = rt
        .block_on(yt.get_watch_playlist(Some(video_id), None, 10, false, false))
        .map_err(|e| e.to_string())?;
    let Some(browse_id) = watch.get("lyrics").and_then(Value::as_str).map(str::to_string) else {
        return Ok(None);
    };
    let found = rt.block_on(yt.get_lyrics(&browse_id, false)).map_err(|e| e.to_string())?;
    Ok(found.map(Value::Object))
}

/// `get_lyrics` output -> the QML shape.
fn parse_lyrics(found: Option<&Value>) -> Value {
    let text = found.and_then(|f| f.get("lyrics")).and_then(Value::as_str).unwrap_or_default();
    let source = found.and_then(|f| f.get("source")).and_then(Value::as_str).unwrap_or_default();
    json!({ "lines": text.lines().collect::<Vec<_>>(), "source": source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lyrics_become_lines() {
        let found = json!({"hasTimestamps": false, "lyrics": "a\n\nb", "source": null});
        assert_eq!(parse_lyrics(Some(&found)), json!({"lines": ["a", "", "b"], "source": ""}));
        assert_eq!(parse_lyrics(None)["lines"], json!([]));
    }
}
