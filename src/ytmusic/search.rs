//! Search/suggest over `ytmusic-rs`: live `search` + `get_search_suggestions`
//! wrappers plus offline mocks.
//!
//! Every entry point calls `super::check_gate` before touching data. The
//! `live_*` entry points additionally restore the keyring session into an
//! ephemeral client (see [`super::live_client`]) and return Rust-decided
//! status lines — failures are explicit, never silent mocks. `*_mock`
//! entry points are offline-test-only.

use crate::models::{Album, Artist, Track};
#[cfg(test)]
use crate::models::mock_tracks;
use crate::models::{albums_from_search, artists_from_search, tracks_from_search};
use crate::premium_gate::PremiumGate;

use super::{FetchError, fetch_error_message, live_client};
use ytmusicapi::SearchFilter;

/// Results per live search fetch.
pub const SEARCH_LIMIT: usize = 20;

/// One query's raw search rows, shared by the track/album/artist slots so a
/// search costs ONE network call (and repeats within the TTL cost none).
const SEARCH_TTL: std::time::Duration = std::time::Duration::from_secs(300);

struct SearchCache {
    query: String,
    fetched: std::time::Instant,
    /// `Filter::Songs` results — the track lane.
    songs: Vec<serde_json::Value>,
    /// Unfiltered results — albums/artists (and a fallback track lane).
    mixed: Vec<serde_json::Value>,
}

#[derive(Clone, Copy)]
enum SearchLane {
    Songs,
    Mixed,
}

static SEARCH_CACHE: std::sync::OnceLock<std::sync::Mutex<Option<SearchCache>>> =
    std::sync::OnceLock::new();

fn search_cache() -> &'static std::sync::Mutex<Option<SearchCache>> {
    SEARCH_CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

/// Background search-load state, polled by QML.
#[derive(Clone)]
enum SearchLoadState {
    Idle,
    Loading(u64),
    Ready(u64),
    Failed(u64, String),
}

static SEARCH_LOAD_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SEARCH_LOAD_STATE: std::sync::OnceLock<std::sync::Mutex<SearchLoadState>> =
    std::sync::OnceLock::new();

fn search_load_state() -> &'static std::sync::Mutex<SearchLoadState> {
    SEARCH_LOAD_STATE.get_or_init(|| std::sync::Mutex::new(SearchLoadState::Idle))
}

/// Drop cached search rows and supersede any in-flight search.
pub fn clear_search_cache() {
    SEARCH_LOAD_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut cache) = search_cache().lock() {
        *cache = None;
    }
    if let Ok(mut state) = search_load_state().lock() {
        *state = SearchLoadState::Idle;
    }
}

fn store_search_cache(
    query: String,
    songs: Vec<serde_json::Value>,
    mixed: Vec<serde_json::Value>,
) {
    if let Ok(mut cache) = search_cache().lock() {
        *cache = Some(SearchCache {
            query,
            fetched: std::time::Instant::now(),
            songs,
            mixed,
        });
    }
}

/// Fetch BOTH lanes without touching the cache (background thread): the
/// track lane is the account's Songs shelf merged with the signed-out Songs
/// shelf, re-ranked by [`rank_songs`] (the signed-in shelf alone buried
/// e.g. Adele's "Hello" entirely); the unfiltered shelf still feeds
/// album/artist cards. All three run concurrently.
fn fetch_search_payload(
    query: &str,
    gate_status: &str,
) -> Result<(Vec<serde_json::Value>, Vec<serde_json::Value>), String> {
    let gate = super::gate_from_status_str(gate_status);
    super::check_gate(&gate).map_err(fetch_error_message)?;
    let key = query.trim().to_string();
    if key.is_empty() {
        return Err(fetch_error_message(FetchError::Transport));
    }
    let (yt, gate) = live_client(gate_status)?;
    let rt = live_runtime()?;
    // Metadata-only guest client (no cookies) for the global ranking.
    let guest = ytmusicapi::YTMusic::new().ok();
    rt.block_on(async {
        let guest_songs = async {
            match &guest {
                Some(g) => fetch_search_query(g, &gate, &key, Some(SearchFilter::Songs), SEARCH_LIMIT)
                    .await
                    .unwrap_or_default(),
                None => Vec::new(),
            }
        };
        let (songs, guest_songs, mixed) = tokio::join!(
            fetch_search_query(&yt, &gate, &key, Some(SearchFilter::Songs), SEARCH_LIMIT),
            guest_songs,
            fetch_search_query(&yt, &gate, &key, None, SEARCH_LIMIT),
        );
        let songs = songs.map(|mut rows| {
            rows.extend(guest_songs);
            rank_songs(&key, rows)
        });
        match (songs, mixed) {
            (Ok(songs), Ok(mixed)) => Ok((songs, mixed)),
            (Ok(songs), Err(_)) => Ok((songs.clone(), songs)),
            (Err(_), Ok(mixed)) => Ok((mixed.clone(), mixed)),
            (Err(error), Err(_)) => Err(fetch_error_message(error)),
        }
    })
}

/// Start a background search; returns a generation token for polling.
pub fn start_search_load(query: &str, gate_status: &str) -> u64 {
    let token = SEARCH_LOAD_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let gate = super::gate_from_status_str(gate_status);
    if let Err(error) = super::check_gate(&gate) {
        if let Ok(mut state) = search_load_state().lock() {
            *state = SearchLoadState::Failed(token, fetch_error_message(error));
        }
        return token;
    }
    let key = query.trim().to_string();
    if let Ok(cache) = search_cache().lock() {
        if let Some(entry) = cache.as_ref() {
            if entry.query == key && entry.fetched.elapsed() < SEARCH_TTL {
                drop(cache);
                if let Ok(mut state) = search_load_state().lock() {
                    *state = SearchLoadState::Ready(token);
                }
                return token;
            }
        }
    }
    if let Ok(mut state) = search_load_state().lock() {
        *state = SearchLoadState::Loading(token);
    }
    let query = query.to_string();
    let gate_status = gate_status.to_string();
    let spawned = std::thread::Builder::new()
        .name("search-load".to_string())
        .spawn(move || {
            let fetched = fetch_search_payload(&query, &gate_status);
            if SEARCH_LOAD_GEN.load(std::sync::atomic::Ordering::SeqCst) != token {
                return;
            }
            let next = match fetched {
                Ok((songs, mixed)) => {
                    store_search_cache(query.trim().to_string(), songs, mixed);
                    SearchLoadState::Ready(token)
                }
                Err(msg) => SearchLoadState::Failed(token, msg),
            };
            if let Ok(mut state) = search_load_state().lock() {
                *state = next;
            }
        });
    if spawned.is_err() {
        if let Ok(mut state) = search_load_state().lock() {
            *state = SearchLoadState::Failed(
                token,
                "Couldn't search. Check your connection and try again.".to_string(),
            );
        }
    }
    token
}

/// Poll a background search: `"loading" | "ready" | "error:<msg>" | "stale"`.
pub fn search_load_status(token: u64) -> String {
    let state = search_load_state()
        .lock()
        .map(|state| state.clone())
        .unwrap_or(SearchLoadState::Idle);
    match state {
        SearchLoadState::Loading(t) if t == token => "loading".to_string(),
        SearchLoadState::Ready(t) if t == token => "ready".to_string(),
        SearchLoadState::Failed(t, msg) if t == token => format!("error:{msg}"),
        _ => "stale".to_string(),
    }
}

/// Cached rows for one lane, fetching once on a miss. Gate checked before
/// the cache is read; `Err` is the display-ready line.
fn live_search_lane(
    query: &str,
    gate_status: &str,
    lane: SearchLane,
) -> Result<Vec<serde_json::Value>, String> {
    let gate = super::gate_from_status_str(gate_status);
    super::check_gate(&gate).map_err(fetch_error_message)?;
    let key = query.trim().to_string();
    let cached = |entry: &SearchCache| match lane {
        SearchLane::Songs if !entry.songs.is_empty() => entry.songs.clone(),
        SearchLane::Songs => entry.mixed.clone(),
        SearchLane::Mixed => entry.mixed.clone(),
    };
    if let Ok(cache) = search_cache().lock() {
        if let Some(entry) = cache.as_ref() {
            if entry.query == key && entry.fetched.elapsed() < SEARCH_TTL {
                return Ok(cached(entry));
            }
        }
    }
    let (songs, mixed) = fetch_search_payload(&key, gate_status)?;
    store_search_cache(key, songs.clone(), mixed.clone());
    Ok(match lane {
        SearchLane::Songs if !songs.is_empty() => songs,
        SearchLane::Songs => mixed,
        SearchLane::Mixed => mixed,
    })
}

/// Offline suggestion pool (shaped like suggestion text rows). Test-only.
#[cfg(test)]
fn suggestion_pool() -> Vec<String> {
    [
        "Glass Coast",
        "Glass Coast Neon Tide",
        "Aiko Tanabe Drift",
        "The Meridians Westbound",
        "Vela Perigee",
        "June Calloway Ember Days",
        "midnight static",
        "copper sky live",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Offline search: gate-checked mock filtered by query substring
/// (case-insensitive, empty query returns everything). Offline-test-only —
/// the live path uses [`fetch_search`] / [`live_search_tracks`] and never
/// falls back here.
#[cfg(test)]
pub fn search_mock(gate: &PremiumGate, query: &str) -> Result<Vec<Track>, FetchError> {
    super::check_gate(gate)?;
    let q = query.trim().to_ascii_lowercase();
    Ok(mock_tracks()
        .into_iter()
        .filter(|t| {
            q.is_empty()
                || t.title.to_ascii_lowercase().contains(&q)
                || t.artist.to_ascii_lowercase().contains(&q)
                || t.album.to_ascii_lowercase().contains(&q)
        })
        .collect())
}

/// Offline suggestions: gate-checked prefix/substring matches.
/// Offline-test-only.
#[cfg(test)]
pub fn suggest_mock(gate: &PremiumGate, query: &str) -> Result<Vec<String>, FetchError> {
    super::check_gate(gate)?;
    let q = query.trim().to_ascii_lowercase();
    Ok(suggestion_pool()
        .into_iter()
        .filter(|s| q.is_empty() || s.to_ascii_lowercase().contains(&q))
        .collect())
}

/// "3.5B" / "428M" / "10K" / "950" play counts → number (0 when absent).
fn parse_plays(label: &str) -> f64 {
    let t = label.trim().replace(',', "");
    let (num, mult) = match t.chars().last() {
        Some('K' | 'k') => (&t[..t.len() - 1], 1e3),
        Some('M' | 'm') => (&t[..t.len() - 1], 1e6),
        Some('B' | 'b') => (&t[..t.len() - 1], 1e9),
        _ => (t.as_str(), 1.0),
    };
    num.trim().parse::<f64>().map(|n| n * mult).unwrap_or(0.0)
}

/// Lowercase alphanumeric words, with "(…)"/"[…]" suffixes dropped.
fn norm_words(s: &str) -> Vec<String> {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth > 0 => {}
            c if c.is_alphanumeric() => out.extend(c.to_lowercase()),
            _ => out.push(' '),
        }
    }
    out.split_whitespace().map(str::to_string).collect()
}

/// YouTube-like ordering for song rows: dedupe by video id, then score =
/// 2 × text tier (3 exact title or "artist title", 2 title starts with the
/// query, else the share of query words in title/artists) + log10(plays). Stable for
/// ties, so YouTube Music's own order breaks them.
fn rank_songs(query: &str, rows: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let q = norm_words(query);
    let mut seen = std::collections::HashSet::new();
    let mut scored: Vec<(f64, serde_json::Value)> = rows
        .into_iter()
        .filter(|r| {
            let id = r.get("videoId").and_then(|v| v.as_str()).unwrap_or("");
            !id.is_empty() && seen.insert(id.to_string())
        })
        .map(|r| {
            let title = norm_words(r.get("title").and_then(|v| v.as_str()).unwrap_or(""));
            let artists: Vec<String> = r
                .get("artists")
                .and_then(|a| a.as_array())
                .into_iter()
                .flatten()
                .filter_map(|a| a.get("name")?.as_str())
                .flat_map(norm_words)
                .collect();
            let artist_title: Vec<String> = artists.iter().chain(title.iter()).cloned().collect();
            let tier = if q.is_empty() {
                0.0
            } else if title == q || artist_title == q {
                3.0
            } else if title.starts_with(&q) {
                2.0
            } else {
                // Share of query words found in title/artists.
                q.iter().filter(|w| artist_title.contains(w)).count() as f64 / q.len() as f64
            };
            let plays = parse_plays(r.get("views").and_then(|v| v.as_str()).unwrap_or(""));
            (2.0 * tier + (plays + 1.0).log10(), r)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, r)| r).collect()
}

/// Raw live `search` results (resultType-tagged dicts): gate first, then
/// InnerTube. Shared core for the partitioned wrappers below.
async fn fetch_search_query(
    yt: &ytmusicapi::YTMusic,
    gate: &PremiumGate,
    query: &str,
    filter: Option<SearchFilter>,
    limit: usize,
) -> Result<ytmusicapi::JsonList, FetchError> {
    super::check_gate(gate)?;
    if query.trim().is_empty() {
        return Err(FetchError::Transport);
    }
    yt.search(query, filter, None, limit, false)
        .await
        .map_err(|_| FetchError::Transport)
}

/// Build the current-thread runtime for the live search lane (fail-closed).
fn live_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| fetch_error_message(FetchError::Transport))
}

/// Short query echo for status lines (user input, never secrets; capped).
fn short_query(query: &str) -> String {
    let q: String = query.trim().chars().take(60).collect();
    q.replace(['\n', '\r', '\t'], " ")
}

/// Status line for a filled search lane.
fn search_status_line(count: usize, noun: &str, query: &str) -> String {
    let q = short_query(query);
    if count == 0 {
        format!("No results for \"{q}\".")
    } else {
        format!("{count} {noun} for \"{q}\" • live")
    }
}

/// Empty query means "reset to home" (decided here in Rust, not in QML).
fn is_home_reset(query: &str) -> bool {
    query.trim().is_empty()
}

/// LIVE search tracks: session → gate → `search` → rows. Empty query reloads
/// home. `Ok` carries rows + the QML status line; `Err` the failure line.
/// Blocked gates return before any keyring/network touch.
pub fn live_search_tracks(query: &str, gate_status: &str) -> Result<(Vec<Track>, String), String> {
    if is_home_reset(query) {
        return super::home::live_home_tracks(gate_status);
    }
    let items = live_search_lane(query, gate_status, SearchLane::Songs)?;
    let tracks = tracks_from_search(&items);
    let msg = search_status_line(tracks.len(), "tracks", query);
    Ok((tracks, msg))
}

/// LIVE search albums (see [`live_search_tracks`]; empty query reloads home).
pub fn live_search_albums(query: &str, gate_status: &str) -> Result<(Vec<Album>, String), String> {
    if is_home_reset(query) {
        return super::home::live_home_albums(gate_status);
    }
    let items = live_search_lane(query, gate_status, SearchLane::Mixed)?;
    Ok((albums_from_search(&items), String::new()))
}

/// LIVE search artists (see [`live_search_tracks`]; empty query reloads home).
pub fn live_search_artists(
    query: &str,
    gate_status: &str,
) -> Result<(Vec<Artist>, String), String> {
    if is_home_reset(query) {
        return super::home::live_home_artists(gate_status);
    }
    let items = live_search_lane(query, gate_status, SearchLane::Mixed)?;
    Ok((artists_from_search(&items), String::new()))
}

#[cfg(test)]
mod rank_tests {
    use super::{parse_plays, rank_songs};

    fn row(id: &str, title: &str, artist: &str, views: &str) -> serde_json::Value {
        serde_json::json!({"videoId": id, "title": title, "artists": [{"name": artist}], "views": views})
    }

    #[test]
    fn plays_parse() {
        assert_eq!(parse_plays("3.5B"), 3.5e9);
        assert_eq!(parse_plays("10K"), 1e4);
        assert_eq!(parse_plays("950"), 950.0);
        assert_eq!(parse_plays(""), 0.0);
    }

    #[test]
    fn exact_popular_match_wins_and_duplicates_drop() {
        let rows = vec![
            row("cat", "Hello", "The Cat Empire", "3.1M"),
            row("tiny", "Hello", "1600luhe", "10K"),
            row("other", "Halo", "Beyoncé", "2.7B"),
            row("adele", "Hello", "Adele", "3.5B"),
            row("cat", "Hello", "The Cat Empire", "3.1M"),
        ];
        let ids: Vec<String> = rank_songs("Hello", rows)
            .iter()
            .map(|r| r["videoId"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, ["adele", "cat", "tiny", "other"]);
    }

    #[test]
    fn artist_query_ranks_by_popularity() {
        let rows = vec![row("a", "Shikidim", "Tarkan", "3.3M"), row("b", "Dudu", "Tarkan", "546M")];
        let ranked = rank_songs("tarkan", rows);
        assert_eq!(ranked[0]["videoId"], "b");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn search_mock_needs_premium() {
        assert!(search_mock(&PremiumGate::new_logged_out(), "glass").is_err());
        assert!(suggest_mock(&PremiumGate::new_logged_out(), "glass").is_err());
    }

    #[test]
    fn search_mock_filters_offline() {
        let gate = premium_gate();
        let all = search_mock(&gate, "").expect("empty query returns all");
        assert_eq!(all.len(), 6);

        let glass = search_mock(&gate, "glass").expect("premium passes");
        assert_eq!(glass.len(), 2);
        assert!(glass.iter().all(|t| t.artist == "Glass Coast"));

        let none = search_mock(&gate, "zzz-no-match").expect("premium passes");
        assert!(none.is_empty());
    }

    #[test]
    fn suggest_mock_matches_offline() {
        let gate = premium_gate();
        let s = suggest_mock(&gate, "glass").expect("premium passes");
        assert_eq!(s.len(), 2);
        let all = suggest_mock(&gate, "").expect("premium passes");
        assert_eq!(all.len(), 8);
    }

    #[test]
    fn search_status_lines_echo_short_queries() {
        let msg = search_status_line(3, "tracks", "  deniz  ");
        assert!(msg.contains("\"deniz\"") && msg.contains('3'));
        let msg = search_status_line(0, "tracks", "zzz");
        assert!(msg.contains("No results") && msg.contains("zzz"));
        // Newlines in pasted input never break the single-line status.
        let msg = search_status_line(0, "tracks", "a\nb");
        assert!(!msg.contains('\n'));
        assert!(is_home_reset("   "));
        assert!(!is_home_reset("x"));
    }

    #[test]
    fn live_search_entry_points_need_premium_offline() {
        // Blocked gates return before keyring/network (no stored session).
        for blocked in ["loggedOut", "upgradeRequired", "", "bogus"] {
            assert!(live_search_tracks("deniz", blocked).is_err());
            assert!(live_search_albums("deniz", blocked).is_err());
            assert!(live_search_artists("deniz", blocked).is_err());
        }
    }

    #[test]
    fn blocked_search_load_reports_error_without_network() {
        let token = start_search_load("deniz", "loggedOut");
        assert!(search_load_status(token).starts_with("error:"));
    }
}


