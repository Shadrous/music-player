//! Home/browse over `ytmusic-rs`: live `get_home` wrappers + offline mocks.
//!
//! Every entry point calls `super::check_gate` before touching data. The
//! `live_*` entry points additionally restore the keyring session into an
//! ephemeral client (see [`super::live_client`]) and return Rust-decided
//! status lines for the QML status labels — failures are explicit, never
//! silent mocks. `*_mock` entry points are offline-test-only.

use crate::models::{Album, Artist, Track};
#[cfg(test)]
use crate::models::mock_tracks;
use crate::models::{albums_from_shelves, artists_from_shelves, tracks_from_shelves};

use super::{FetchError, fetch_error_message, live_client};

/// Rows per live home fetch (shelf count passed to `get_home`).
pub const HOME_LIMIT: usize = 10;

/// How long one `get_home` response serves tracks/albums/artists and repeated
/// QML loads. The three model slots share this cache, so a gate change costs
/// ONE network call instead of three; app startup/login repeats hit it too.
const HOME_TTL: std::time::Duration = std::time::Duration::from_secs(600);

struct HomeCache {
    fetched: std::time::Instant,
    // Loaded from disk: shown at once, but always refreshed.
    stale: bool,
    shelves: Vec<serde_json::Value>,
}

static HOME_CACHE: std::sync::OnceLock<std::sync::Mutex<Option<HomeCache>>> =
    std::sync::OnceLock::new();

fn home_cache() -> &'static std::sync::Mutex<Option<HomeCache>> {
    HOME_CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

/// Background home-load state, polled by QML so the network fetch never
/// blocks the UI thread.
#[derive(Clone)]
enum HomeLoadState {
    Idle,
    Loading(u64),
    // Old rows are on hand (memory/disk) while a refresh runs.
    Cached(u64),
    Ready(u64),
    Failed(u64, String),
}

static HOME_LOAD_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static HOME_LOAD_STATE: std::sync::OnceLock<std::sync::Mutex<HomeLoadState>> =
    std::sync::OnceLock::new();

fn home_load_state() -> &'static std::sync::Mutex<HomeLoadState> {
    HOME_LOAD_STATE.get_or_init(|| std::sync::Mutex::new(HomeLoadState::Idle))
}

/// Drop the cached home shelves (account switch / logout) and supersede any
/// in-flight background load, whose result must not land in the new session.
pub fn clear_home_cache() {
    HOME_LOAD_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut cache) = home_cache().lock() {
        *cache = None;
    }
    crate::disk_cache::remove("home");
    if let Ok(mut state) = home_load_state().lock() {
        *state = HomeLoadState::Idle;
    }
}

fn store_home_cache(shelves: Vec<serde_json::Value>) {
    crate::disk_cache::write("home", &serde_json::Value::Array(shelves.clone()));
    if let Ok(mut cache) = home_cache().lock() {
        *cache = Some(HomeCache {
            fetched: std::time::Instant::now(),
            stale: false,
            shelves,
        });
    }
}

/// Rows on hand: `Some(true)` fresh, `Some(false)` stale (show + refresh),
/// `None` nothing yet. Seeds memory from the disk copy on first use.
fn home_rows_on_hand() -> Option<bool> {
    let mut cache = home_cache().lock().ok()?;
    if cache.is_none() {
        if let Some(serde_json::Value::Array(shelves)) = crate::disk_cache::read("home") {
            *cache = Some(HomeCache {
                fetched: std::time::Instant::now(),
                stale: true,
                shelves,
            });
        }
    }
    cache
        .as_ref()
        .map(|entry| !entry.stale && entry.fetched.elapsed() < HOME_TTL)
}

/// Fetch the live shelves WITHOUT touching the cache (background thread).
fn fetch_home_shelves(gate_status: &str) -> Result<Vec<serde_json::Value>, String> {
    let gate = super::gate_from_status_str(gate_status);
    super::check_gate(&gate).map_err(fetch_error_message)?;
    let (yt, _gate) = live_client(gate_status)?;
    let rt = live_runtime()?;
    rt.block_on(yt.get_home(HOME_LIMIT))
        .map_err(|_| fetch_error_message(FetchError::Transport))
}

/// Start a background home fetch; returns a generation token for polling.
/// Only the newest generation may publish its shelves/state, so a login or
/// logout during a fetch cannot show the previous account's rows.
pub fn start_home_load(gate_status: &str) -> u64 {
    let token = HOME_LOAD_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let gate = super::gate_from_status_str(gate_status);
    if let Err(error) = super::check_gate(&gate) {
        if let Ok(mut state) = home_load_state().lock() {
            *state = HomeLoadState::Failed(token, fetch_error_message(error));
        }
        return token;
    }
    // Fresh rows answer immediately; stale rows (old memory or the disk
    // copy) show at once while a background refresh runs.
    let on_hand = home_rows_on_hand();
    if on_hand == Some(true) {
        if let Ok(mut state) = home_load_state().lock() {
            *state = HomeLoadState::Ready(token);
        }
        return token;
    }
    let have_rows = on_hand.is_some();
    if let Ok(mut state) = home_load_state().lock() {
        *state = if have_rows {
            HomeLoadState::Cached(token)
        } else {
            HomeLoadState::Loading(token)
        };
    }
    let gate = gate_status.to_string();
    let spawned = std::thread::Builder::new()
        .name("home-load".to_string())
        .spawn(move || {
            let fetched = fetch_home_shelves(&gate);
            if HOME_LOAD_GEN.load(std::sync::atomic::Ordering::SeqCst) != token {
                return;
            }
            let next = match fetched {
                Ok(shelves) => {
                    store_home_cache(shelves);
                    HomeLoadState::Ready(token)
                }
                // Offline with saved rows: keep showing them.
                Err(_) if have_rows => HomeLoadState::Ready(token),
                Err(msg) => HomeLoadState::Failed(token, msg),
            };
            if let Ok(mut state) = home_load_state().lock() {
                *state = next;
            }
        });
    if spawned.is_err() {
        if let Ok(mut state) = home_load_state().lock() {
            *state = HomeLoadState::Failed(
                token,
                "Couldn't load music. Check your connection and try again.".to_string(),
            );
        }
    }
    token
}

/// Poll a background load: `"loading" | "cached" | "ready" | "error:<msg>" |
/// "stale"`. `cached` = old rows are fillable now, a refresh follows.
pub fn home_load_status(token: u64) -> String {
    let state = home_load_state()
        .lock()
        .map(|state| state.clone())
        .unwrap_or(HomeLoadState::Idle);
    match state {
        HomeLoadState::Loading(t) if t == token => "loading".to_string(),
        HomeLoadState::Cached(t) if t == token => "cached".to_string(),
        HomeLoadState::Ready(t) if t == token => "ready".to_string(),
        HomeLoadState::Failed(t, msg) if t == token => format!("error:{msg}"),
        _ => "stale".to_string(),
    }
}

/// Home shelves for the three model lanes: whatever the loader left in
/// the cache (fresh or stale; [`start_home_load`] owns freshness), else one
/// fetch. Gate-checked before the cache is read, so a blocked session never
/// sees saved rows. `Err` carries the display-ready failure line.
pub fn live_home_shelves(gate_status: &str) -> Result<Vec<serde_json::Value>, String> {
    let gate = super::gate_from_status_str(gate_status);
    super::check_gate(&gate).map_err(fetch_error_message)?;
    if let Ok(cache) = home_cache().lock() {
        if let Some(entry) = cache.as_ref() {
            return Ok(entry.shelves.clone());
        }
    }
    let shelves = fetch_home_shelves(gate_status)?;
    store_home_cache(shelves.clone());
    Ok(shelves)
}

/// Offline home rows: gate-checked mock (no network). Offline-test-only —
/// the live path uses [`live_home_tracks`] and never falls back here.
#[cfg(test)]
pub fn home_mock(gate: &crate::premium_gate::PremiumGate) -> Result<Vec<Track>, FetchError> {
    super::check_gate(gate)?;
    Ok(mock_tracks())
}

/// Status line for a filled home lane.
fn home_status_line(count: usize, noun: &str) -> String {
    if count == 0 {
        "Nothing on home right now.".to_string()
    } else {
        format!("Live home • {count} {noun}")
    }
}

/// Build the current-thread runtime for the live home lane (fail-closed).
fn live_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| fetch_error_message(FetchError::Transport))
}

/// LIVE home tracks: session → gate → `get_home` → rows. `Ok` carries the
/// rows plus the QML status line; `Err` carries the display-ready failure
/// line (callers clear the model). Blocked gates return before any
/// keyring/network touch.
pub fn live_home_tracks(gate_status: &str) -> Result<(Vec<Track>, String), String> {
    let shelves = live_home_shelves(gate_status)?;
    let tracks = tracks_from_shelves(&shelves);
    let msg = home_status_line(tracks.len(), "tracks");
    Ok((tracks, msg))
}

/// LIVE home albums (see [`live_home_tracks`]).
pub fn live_home_albums(gate_status: &str) -> Result<(Vec<Album>, String), String> {
    let shelves = live_home_shelves(gate_status)?;
    let albums = albums_from_shelves(&shelves);
    Ok((albums, String::new()))
}

/// LIVE home artists (see [`live_home_tracks`]).
pub fn live_home_artists(gate_status: &str) -> Result<(Vec<Artist>, String), String> {
    let shelves = live_home_shelves(gate_status)?;
    let artists = artists_from_shelves(&shelves);
    Ok((artists, String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::premium_gate::PremiumGate;
    use crate::models::{albums_from_search, artists_from_search, tracks_from_search};
    use crate::premium_gate::{Playability, PremiumSignals};
    use std::time::SystemTime;

    fn premium_gate() -> PremiumGate {
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(
            &PremiumSignals {
                logged_in: true,
                home_ok: true,
                premium_badge: Some(true),
                explore_top_songs: None,
                ads_absent: None,
                topbar_premium: None,
                hires_audio: None,
                playability: Playability::Playable,
            },
            SystemTime::now(),
        );
        gate
    }

    /// Live-shaped `get_home` shelves (song + album + artist contents, with
    /// `artists[]`, `thumbnails[]`, numeric durations).
    fn live_shelves_fixture() -> ytmusicapi::JsonList {
        serde_json::json!([
            {"title": "Quick picks", "contents": [
                {"title": "Külüstür Hafıza", "videoId": "vid-live-1",
                 "artists": [{"name": "Deniz", "id": "UC1"}],
                 "album": {"name": "Nostalji", "id": "MPRE1"},
                 "duration": "3:24", "duration_seconds": 204,
                 "thumbnails": [
                     {"url": "https://i.ytimg.com/vi/vid-live-1/small.jpg", "width": 60, "height": 60},
                     {"url": "https://i.ytimg.com/vi/vid-live-1/big.jpg", "width": 540, "height": 540}]},
                {"title": "Unplayable row", "artists": [{"name": "Nobody"}]}
            ]},
            {"title": "New releases", "contents": [
                {"title": "Neon Tide", "browseId": "MPREalb1",
                 "artists": [{"name": "Glass Coast", "id": "UC9"}],
                 "year": "2024",
                 "thumbnails": [{"url": "https://i.ytimg.com/alb1.jpg", "width": 200, "height": 200}]},
                {"title": "Vela", "browseId": "UCartist1", "subscribers": "310K",
                 "thumbnails": [{"url": "https://i.ytimg.com/art1.jpg", "width": 100, "height": 100}]}
            ]}
        ])
        .as_array()
        .cloned()
        .unwrap_or_default()
    }

    #[test]
    fn home_mock_needs_premium() {
        assert!(home_mock(&PremiumGate::new_logged_out()).is_err());
        let tracks = home_mock(&premium_gate()).expect("premium passes");
        assert_eq!(tracks.len(), 6);
    }

    #[test]
    fn home_fixture_parses() {
        let tracks = crate::models::tracks_from_json(&crate::models::mock_browse_json());
        assert_eq!(tracks.len(), 6);
        assert!(tracks.iter().all(|t| t.is_renderable()));
    }

    #[test]
    fn live_shelves_fill_tracks_albums_artists() {
        let shelves = live_shelves_fixture();
        let tracks = tracks_from_shelves(&shelves);
        // Only the row WITH a videoId enters the track lane (the title-only
        // row, album card, and artist row are unplayable as tracks).
        assert_eq!(tracks.len(), 1, "only playable rows become tracks");
        let t = &tracks[0];
        assert_eq!(t.title, "Külüstür Hafıza");
        assert_eq!(t.artist, "Deniz");
        assert_eq!(t.album, "Nostalji");
        assert_eq!(t.video_id, "vid-live-1");
        assert_eq!(t.duration, "3:24");
        assert_eq!(t.thumbnail_url, "https://i.ytimg.com/vi/vid-live-1/big.jpg");

        let albums = albums_from_shelves(&shelves);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].title, "Neon Tide");
        assert_eq!(albums[0].artist, "Glass Coast");
        assert_eq!(albums[0].browse_id, "MPREalb1");
        assert_eq!(albums[0].year, "2024");
        assert_eq!(albums[0].thumbnail_url, "https://i.ytimg.com/alb1.jpg");

        let artists = artists_from_shelves(&shelves);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].name, "Vela");
        assert_eq!(artists[0].channel_id, "UCartist1");
        assert_eq!(artists[0].subscribers, "310K");
    }

    #[test]
    fn numeric_duration_and_cover_fallbacks() {
        // Numeric-only duration formats; missing thumbnails → wash fallback ("").
        let v = serde_json::json!({
            "title": "T", "videoId": "x", "duration_seconds": 204u64,
        });
        let t = Track::from_json(&v);
        assert_eq!(t.duration, "3:24");
        assert_eq!(t.thumbnail_url, "", "absent art stays empty (wash fallback)");
        let bare = Track::from_json(&serde_json::json!({"title": "T", "videoId": "x"}));
        assert_eq!(bare.duration, "");
    }

    #[test]
    fn search_partition_maps_result_types() {
        let results = serde_json::json!([
            {"resultType": "song", "title": "S1", "videoId": "v1",
             "artists": [{"name": "A1"}], "duration": "2:00",
             "thumbnails": [{"url": "https://i.ytimg.com/v1.jpg", "width": 10, "height": 10}]},
            {"resultType": "video", "title": "V1", "videoId": "v2", "artists": [{"name": "A2"}]},
            {"resultType": "album", "title": "AL1", "browseId": "MPRE9", "year": "2023"},
            {"resultType": "artist", "artist": "AR1", "browseId": "UC7", "subscribers": "1M"},
            {"resultType": "playlist", "title": "PL1", "browseId": "VLPL1"}
        ])
        .as_array()
        .cloned()
        .unwrap_or_default();
        let tracks = tracks_from_search(&results);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].video_id, "v1");
        assert_eq!(tracks[0].thumbnail_url, "https://i.ytimg.com/v1.jpg");
        let albums = albums_from_search(&results);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].browse_id, "MPRE9");
        let artists = artists_from_search(&results);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].name, "AR1");
        assert_eq!(artists[0].channel_id, "UC7");
    }

    #[test]
    fn cover_url_selects_largest_thumbnail() {
        let v = serde_json::json!({"thumbnails": [
            {"url": "https://img/small.jpg", "width": 60, "height": 60},
            {"url": "https://img/big.jpg", "width": 540, "height": 540},
            {"url": "https://img/mid.jpg", "width": 120, "height": 120}
        ]});
        assert_eq!(
            crate::models::thumbnail_url_from_json(&v),
            "https://img/big.jpg"
        );
        assert_eq!(
            crate::models::thumbnail_url_from_json(&serde_json::json!({})),
            ""
        );
        // Non-image values never surface as art.
        let evil = serde_json::json!({"thumbnails": [
            {"url": "SAPISIDHASH fake-secret", "width": 999, "height": 999}
        ]});
        assert_eq!(crate::models::thumbnail_url_from_json(&evil), "");
    }

    #[test]
    fn live_entry_points_need_premium_offline() {
        // `live_*` entry points enforce the forwarded gate before touching the
        // keyring or network (no stored session needed to prove the block).
        for blocked in ["loggedOut", "upgradeRequired", "", "bogus"] {
            assert!(live_home_tracks(blocked).is_err());
            assert!(live_home_albums(blocked).is_err());
            assert!(live_home_artists(blocked).is_err());
        }
    }
}