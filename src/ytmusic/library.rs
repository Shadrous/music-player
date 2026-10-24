//! Library surfaces over `ytmusic-rs`: the account's playlists, playlist
//! detail, and play history. Every fetch is gate-checked, runs on a worker
//! thread, and publishes metadata-only rows through a polled token. No media
//! URLs, no secrets.

use crate::models::{thumbnail_url_from_json, Track};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::{FetchError, fetch_error_message, live_client};

/// How long one library snapshot / playlist detail serves the UI.
const TTL: Duration = Duration::from_secs(600);

struct LibraryCache {
    fetched: Instant,
    // Loaded from disk: shown at once, but always refreshed.
    stale: bool,
    playlists: Vec<serde_json::Value>,
    history: Vec<serde_json::Value>,
}

static LIBRARY_CACHE: OnceLock<Mutex<Option<LibraryCache>>> = OnceLock::new();
static LIBRARY_GEN: AtomicU64 = AtomicU64::new(0);
static LIBRARY_STATE: OnceLock<Mutex<LoadState>> = OnceLock::new();

struct DetailCache {
    playlist_id: String,
    fetched: Instant,
    stale: bool,
    // Continuation token for the next page (`None` = fully loaded).
    next: Option<String>,
    rows: Vec<serde_json::Value>,
}

static DETAIL_CACHE: OnceLock<Mutex<Option<DetailCache>>> = OnceLock::new();
static DETAIL_GEN: AtomicU64 = AtomicU64::new(0);
static DETAIL_STATE: OnceLock<Mutex<LoadState>> = OnceLock::new();

#[derive(Clone)]
enum LoadState {
    Idle,
    Loading(u64),
    // Old rows are on hand (memory/disk) while a refresh runs.
    Cached(u64),
    Ready(u64),
    Failed(u64, String),
}

fn library_cache() -> &'static Mutex<Option<LibraryCache>> {
    LIBRARY_CACHE.get_or_init(|| Mutex::new(None))
}

fn library_state() -> &'static Mutex<LoadState> {
    LIBRARY_STATE.get_or_init(|| Mutex::new(LoadState::Idle))
}

fn detail_cache() -> &'static Mutex<Option<DetailCache>> {
    DETAIL_CACHE.get_or_init(|| Mutex::new(None))
}

fn detail_state() -> &'static Mutex<LoadState> {
    DETAIL_STATE.get_or_init(|| Mutex::new(LoadState::Idle))
}

fn state_status(state: &LoadState, token: u64) -> String {
    match state {
        LoadState::Loading(t) if *t == token => "loading".to_string(),
        LoadState::Cached(t) if *t == token => "cached".to_string(),
        LoadState::Ready(t) if *t == token => "ready".to_string(),
        LoadState::Failed(t, msg) if *t == token => format!("error:{msg}"),
        _ => "stale".to_string(),
    }
}

fn live_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| fetch_error_message(FetchError::Transport))
}

/// Rows on hand: `Some(true)` fresh, `Some(false)` stale (show + refresh),
/// `None` nothing yet. Seeds memory from the disk copy on first use.
fn library_rows_on_hand() -> Option<bool> {
    let mut cache = library_cache().lock().ok()?;
    if cache.is_none() {
        if let Some(saved) = crate::disk_cache::read("library") {
            let rows = |key: &str| saved.get(key).and_then(|v| v.as_array()).cloned().unwrap_or_default();
            *cache = Some(LibraryCache {
                fetched: Instant::now(),
                stale: true,
                playlists: rows("playlists"),
                history: rows("history"),
            });
        }
    }
    cache.as_ref().map(|entry| !entry.stale && entry.fetched.elapsed() < TTL)
}

/// Same as [`library_rows_on_hand`] for one playlist's detail rows (only the
/// last opened playlist stays in memory; each has its own disk copy).
fn detail_rows_on_hand(playlist_id: &str) -> Option<bool> {
    let mut cache = detail_cache().lock().ok()?;
    if cache.as_ref().is_none_or(|entry| entry.playlist_id != playlist_id) {
        let serde_json::Value::Array(rows) =
            crate::disk_cache::read(&format!("playlist-{playlist_id}"))?
        else {
            return None;
        };
        *cache = Some(DetailCache {
            playlist_id: playlist_id.to_string(),
            fetched: Instant::now(),
            stale: true,
            next: None,
            rows,
        });
    }
    cache.as_ref().map(|entry| !entry.stale && entry.fetched.elapsed() < TTL)
}

/// Drop cached library data and supersede in-flight workers (account switch).
pub fn clear_library_cache() {
    LIBRARY_GEN.fetch_add(1, Ordering::SeqCst);
    DETAIL_GEN.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut cache) = library_cache().lock() {
        *cache = None;
    }
    if let Ok(mut cache) = detail_cache().lock() {
        *cache = None;
    }
    crate::disk_cache::remove("library");
    crate::disk_cache::remove_prefix("playlist-");
    if let Ok(mut state) = library_state().lock() {
        *state = LoadState::Idle;
    }
    if let Ok(mut state) = detail_state().lock() {
        *state = LoadState::Idle;
    }
}

fn track_row(track: &Track) -> serde_json::Value {
    serde_json::json!({
        "video_id": track.video_id,
        "title": track.title,
        "artist": track.artist,
        "album": track.album,
        "duration": track.duration,
        "thumbnail_url": track.thumbnail_url,
    })
}

fn playlist_rows(items: &[serde_json::Value]) -> Vec<serde_json::Value> {
    items
        .iter()
        .filter_map(|v| {
            let id = v
                .get("playlistId")
                .or_else(|| v.get("playlist_id"))
                .and_then(|x| x.as_str())?;
            Some(serde_json::json!({
                "playlist_id": id,
                "title": v.get("title").and_then(|x| x.as_str()).unwrap_or_default(),
                "count": v.get("count").and_then(|x| x.as_str()).unwrap_or_default(),
                "thumbnail_url": thumbnail_url_from_json(v),
            }))
        })
        .collect()
}

fn history_rows(items: &[serde_json::Value]) -> Vec<serde_json::Value> {
    items
        .iter()
        .map(Track::from_json)
        .filter(|t| !t.video_id.is_empty())
        .take(100)
        .map(|t| track_row(&t))
        .collect()
}

fn detail_rows(playlist: &serde_json::Map<String, serde_json::Value>) -> Vec<serde_json::Value> {
    playlist
        .get("tracks")
        .and_then(|t| t.as_array())
        .map(|tracks| {
            tracks
                .iter()
                .map(Track::from_json)
                .filter(|t| !t.video_id.is_empty())
                .map(|t| track_row(&t))
                .collect()
        })
        .unwrap_or_default()
}

fn fetch_library(gate_status: &str) -> Result<(Vec<serde_json::Value>, Vec<serde_json::Value>), String> {
    let gate = super::gate_from_status_str(gate_status);
    super::check_gate(&gate).map_err(fetch_error_message)?;
    let (yt, _gate) = live_client(gate_status)?;
    let rt = live_runtime()?;
    rt.block_on(async {
        let (playlists, history) = tokio::join!(
            yt.get_library_playlists(Some(100)),
            yt.get_history(),
        );
        let playlists = playlists.map_err(|_| fetch_error_message(FetchError::Transport));
        let history = history.map_err(|_| fetch_error_message(FetchError::Transport));
        match (playlists, history) {
            (Ok(playlists), Ok(history)) => {
                Ok((playlist_rows(&playlists), history_rows(&history)))
            }
            (Err(error), Err(_)) => Err(error),
            (Ok(playlists), Err(_)) => Ok((playlist_rows(&playlists), Vec::new())),
            (Err(_), Ok(history)) => Ok((Vec::new(), history_rows(&history))),
        }
    })
}

/// Start a background library load (playlists + history).
pub fn start_library_load(gate_status: &str) -> u64 {
    let token = LIBRARY_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let gate = super::gate_from_status_str(gate_status);
    if let Err(error) = super::check_gate(&gate) {
        if let Ok(mut state) = library_state().lock() {
            *state = LoadState::Failed(token, fetch_error_message(error));
        }
        return token;
    }
    let on_hand = library_rows_on_hand();
    if on_hand == Some(true) {
        if let Ok(mut state) = library_state().lock() {
            *state = LoadState::Ready(token);
        }
        return token;
    }
    let have_rows = on_hand.is_some();
    if let Ok(mut state) = library_state().lock() {
        *state = if have_rows { LoadState::Cached(token) } else { LoadState::Loading(token) };
    }
    let gate_status = gate_status.to_string();
    let spawned = std::thread::Builder::new()
        .name("library-load".to_string())
        .spawn(move || {
            let fetched = fetch_library(&gate_status);
            if LIBRARY_GEN.load(Ordering::SeqCst) != token {
                return;
            }
            let next = match fetched {
                Ok((playlists, history)) => {
                    crate::disk_cache::write(
                        "library",
                        &serde_json::json!({ "playlists": playlists, "history": history }),
                    );
                    if let Ok(mut cache) = library_cache().lock() {
                        *cache = Some(LibraryCache {
                            fetched: Instant::now(),
                            stale: false,
                            playlists,
                            history,
                        });
                    }
                    LoadState::Ready(token)
                }
                // Offline with saved rows: keep showing them.
                Err(_) if have_rows => LoadState::Ready(token),
                Err(msg) => LoadState::Failed(token, msg),
            };
            if let Ok(mut state) = library_state().lock() {
                *state = next;
            }
        });
    if spawned.is_err() {
        if let Ok(mut state) = library_state().lock() {
            *state = LoadState::Failed(
                token,
                "Couldn't load your library. Check your connection and try again.".to_string(),
            );
        }
    }
    token
}

/// Poll a background library load.
pub fn library_load_status(token: u64) -> String {
    let state = library_state()
        .lock()
        .map(|state| state.clone())
        .unwrap_or(LoadState::Idle);
    state_status(&state, token)
}

/// Playlist cards JSON (empty before a successful load).
pub fn playlists_json() -> String {
    let cached = library_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref().map(|entry| entry.playlists.clone()))
        .unwrap_or_default();
    serde_json::to_string(&cached).unwrap_or_default()
}

/// Remote play-history JSON (empty before a load).
pub fn history_json() -> String {
    let cached = library_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref().map(|entry| entry.history.clone()))
        .unwrap_or_default();
    serde_json::to_string(&cached).unwrap_or_default()
}

/// One page of rows + the next page's continuation token.
type Page = (Vec<serde_json::Value>, Option<String>);

/// The first page's item list inside a playlist `browse` response.
fn first_page_items(response: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    response
        .pointer(
            "/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer\
             /contents/0/musicPlaylistShelfRenderer/contents",
        )
        .and_then(|v| v.as_array())
}

/// Rows + continuation token from one page's raw items (same parser
/// `get_playlist` uses internally).
fn page_from_items(items: &[serde_json::Value]) -> Page {
    let items = items.to_vec();
    let tracks = ytmusicapi::parsers::playlists::parse_playlist_items(&items, false, false);
    let rows = tracks
        .iter()
        .map(Track::from_json)
        .filter(|t| !t.video_id.is_empty())
        .map(|t| track_row(&t))
        .collect();
    (rows, ytmusicapi::continuations::get_continuation_token(&items))
}

/// Fetch one page: the first (`token == None`, one `browse` request instead
/// of `get_playlist`'s walk) or the one after `token`. Gate-checked.
fn fetch_playlist_page(playlist_id: &str, token: Option<&str>, gate_status: &str) -> Result<Page, String> {
    let gate = super::gate_from_status_str(gate_status);
    super::check_gate(&gate).map_err(fetch_error_message)?;
    let (yt, _gate) = live_client(gate_status)?;
    let rt = live_runtime()?;
    let transport = |_| fetch_error_message(FetchError::Transport);
    let Some(token) = token else {
        let browse_id = if playlist_id.starts_with("VL") { playlist_id.to_string() } else { format!("VL{playlist_id}") };
        let body = serde_json::json!({ "browseId": browse_id });
        let response = rt
            .block_on(yt.debug_send_request("browse", body.as_object().cloned().unwrap_or_default()))
            .map_err(transport)?;
        if let Some(items) = first_page_items(&response) {
            return Ok(page_from_items(items));
        }
        // Unexpected shape (e.g. album-style lists): the library's own walk,
        // capped at one screenful-ish, no paging.
        let playlist = rt
            .block_on(yt.get_playlist(playlist_id, Some(100), false, 0))
            .map_err(transport)?;
        return Ok((detail_rows(&playlist), None));
    };
    let body = serde_json::json!({ "continuation": token });
    let response = rt
        .block_on(yt.debug_send_request("browse", body.as_object().cloned().unwrap_or_default()))
        .map_err(transport)?;
    let items = response
        .pointer("/onResponseReceivedActions/0/appendContinuationItemsAction/continuationItems")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(page_from_items(&items))
}

/// Start a background playlist-detail fetch.
pub fn start_playlist_load(playlist_id: &str, gate_status: &str) -> u64 {
    let token = DETAIL_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let gate = super::gate_from_status_str(gate_status);
    if let Err(error) = super::check_gate(&gate) {
        if let Ok(mut state) = detail_state().lock() {
            *state = LoadState::Failed(token, fetch_error_message(error));
        }
        return token;
    }
    let id = playlist_id.to_string();
    let on_hand = detail_rows_on_hand(&id);
    if on_hand == Some(true) {
        if let Ok(mut state) = detail_state().lock() {
            *state = LoadState::Ready(token);
        }
        return token;
    }
    let have_rows = on_hand.is_some();
    if let Ok(mut state) = detail_state().lock() {
        *state = if have_rows { LoadState::Cached(token) } else { LoadState::Loading(token) };
    }
    let gate_status = gate_status.to_string();
    let spawned = std::thread::Builder::new()
        .name("playlist-load".to_string())
        .spawn(move || {
            let fetched = fetch_playlist_page(&id, None, &gate_status);
            if DETAIL_GEN.load(Ordering::SeqCst) != token {
                return;
            }
            let next = match fetched {
                Ok((rows, next)) => {
                    // Only the first page is saved; later pages load on scroll.
                    crate::disk_cache::write(
                        &format!("playlist-{id}"),
                        &serde_json::Value::Array(rows.clone()),
                    );
                    if let Ok(mut cache) = detail_cache().lock() {
                        *cache = Some(DetailCache {
                            playlist_id: id,
                            fetched: Instant::now(),
                            stale: false,
                            next,
                            rows,
                        });
                    }
                    LoadState::Ready(token)
                }
                Err(_) if have_rows => LoadState::Ready(token),
                Err(msg) => LoadState::Failed(token, msg),
            };
            if let Ok(mut state) = detail_state().lock() {
                *state = next;
            }
        });
    if spawned.is_err() {
        if let Ok(mut state) = detail_state().lock() {
            *state = LoadState::Failed(
                token,
                "Couldn't load this playlist. Try again.".to_string(),
            );
        }
    }
    token
}

/// Poll a background playlist-detail load.
pub fn playlist_load_status(token: u64) -> String {
    let state = detail_state()
        .lock()
        .map(|state| state.clone())
        .unwrap_or(LoadState::Idle);
    state_status(&state, token)
}

/// Playlist rows JSON for `playlist_id` (empty before a successful load).
/// Whether the open playlist has another page to fetch.
pub fn playlist_has_more(playlist_id: &str) -> bool {
    detail_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref().map(|e| e.playlist_id == playlist_id && e.next.is_some()))
        .unwrap_or(false)
}

/// Fetch the next page of the open playlist on a worker and append it to
/// the cache. Polled via [`playlist_load_status`]; a new playlist load
/// supersedes it. Returns 0 when there is nothing more to load.
pub fn start_playlist_more(playlist_id: &str, gate_status: &str) -> u64 {
    let Some(next) = detail_cache().lock().ok().and_then(|cache| {
        cache
            .as_ref()
            .filter(|e| e.playlist_id == playlist_id)
            .and_then(|e| e.next.clone())
    }) else {
        return 0;
    };
    let token = DETAIL_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    if let Ok(mut state) = detail_state().lock() {
        *state = LoadState::Loading(token);
    }
    let id = playlist_id.to_string();
    let gate_status = gate_status.to_string();
    let spawned = std::thread::Builder::new()
        .name("playlist-more".to_string())
        .spawn(move || {
            let fetched = fetch_playlist_page(&id, Some(&next), &gate_status);
            if DETAIL_GEN.load(Ordering::SeqCst) != token {
                return;
            }
            let state = match fetched {
                Ok((rows, next)) => {
                    if let Ok(mut cache) = detail_cache().lock() {
                        if let Some(entry) = cache.as_mut().filter(|e| e.playlist_id == id) {
                            entry.rows.extend(rows);
                            entry.next = next;
                        }
                    }
                    LoadState::Ready(token)
                }
                Err(msg) => LoadState::Failed(token, msg),
            };
            if let Ok(mut slot) = detail_state().lock() {
                *slot = state;
            }
        });
    if spawned.is_err() {
        if let Ok(mut state) = detail_state().lock() {
            *state = LoadState::Failed(token, "Couldn't load more of this playlist.".to_string());
        }
    }
    token
}

/// Rows from `offset` on (what a page append adds to the view).
pub fn playlist_rows_after(playlist_id: &str, offset: usize) -> String {
    let rows = detail_cache()
        .lock()
        .ok()
        .and_then(|cache| {
            cache
                .as_ref()
                .filter(|e| e.playlist_id == playlist_id)
                .map(|e| e.rows.iter().skip(offset).cloned().collect::<Vec<_>>())
        })
        .unwrap_or_default();
    serde_json::to_string(&rows).unwrap_or_default()
}

pub fn playlist_rows_json(playlist_id: &str) -> String {
    let cached = detail_cache()
        .lock()
        .ok()
        .and_then(|cache| {
            cache.as_ref().and_then(|entry| {
                (entry.playlist_id == playlist_id).then(|| entry.rows.clone())
            })
        })
        .unwrap_or_default();
    serde_json::to_string(&cached).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_page_items_follow_the_shelf_path() {
        let response = serde_json::json!({"contents": {"twoColumnBrowseResultsRenderer": {
            "secondaryContents": {"sectionListRenderer": {"contents": [
                {"musicPlaylistShelfRenderer": {"contents": [{"x": 1}, {"x": 2}]}}
            ]}}}}});
        assert_eq!(first_page_items(&response).map(Vec::len), Some(2));
        assert!(first_page_items(&serde_json::json!({})).is_none());
    }

    #[test]
    fn playlist_rows_flatten_metadata() {
        let items = vec![serde_json::json!({
            "playlistId": "PL1",
            "title": "Mix",
            "count": "16",
            "thumbnails": [{"url": "https://i.example/x.jpg", "width": 60, "height": 60}]
        })];
        let rows = playlist_rows(&items);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["playlist_id"], "PL1");
        assert_eq!(rows[0]["title"], "Mix");
        assert_eq!(rows[0]["count"], "16");
        assert_eq!(rows[0]["thumbnail_url"], "https://i.example/x.jpg");
    }

    #[test]
    fn history_rows_keep_playable_tracks_only() {
        let items = vec![
            serde_json::json!({
                "videoId": "v1",
                "title": "Song",
                "duration": "3:00",
                "artists": [{"name": "Artist"}]
            }),
            serde_json::json!({"title": "no id"}),
        ];
        let rows = history_rows(&items);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["video_id"], "v1");
        assert_eq!(rows[0]["artist"], "Artist");
        assert_eq!(rows[0]["duration"], "3:00");
    }

    #[test]
    fn detail_rows_read_the_tracks_array() {
        let playlist = serde_json::json!({
            "tracks": [
                {"videoId": "v2", "title": "Detail", "duration": "4:20"},
                {"title": "unplayable"}
            ]
        });
        let rows = detail_rows(playlist.as_object().unwrap());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["video_id"], "v2");
        assert_eq!(rows[0]["title"], "Detail");
    }
}
