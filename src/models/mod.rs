//! Browse/search models: live-filled `QListModel` rows.
//!
//! Handover notes:
//! - `Track` / `Album` / `Artist` are [`QModelItem`] roles; `TrackModel` /
//!   `AlbumModel` / `ArtistModel` are `Base = QListModel` QObjects registered
//!   explicitly in `main()` (singletons stay `LinkMe`).
//! - QML instantiates the models directly (`TrackModel {}`) and reads roles;
//!   models default to EMPTY (explicit empty state — never demo rows), and
//!   the live slots (`load_home` / `search`) REPLACE rows from the restored
//!   keyring session after the gate passes Premium (see `crate::ytmusic`).
//!   Live failures CLEAR + report (explicit error state, never silent mocks).
//!   Mock fixtures (`mock_*`) stay offline-test-only — no hard-coded demo
//!   tracks in any live path.
//! - `*_unnotified` storage ops are unit-testable offline. The notified
//!   `QListModelBase` wrappers (`push`/`pop`/`reset`) and `#[qslot]`s require
//!   an attached QML proxy and are covered by the offscreen smoke run, not by
//!   `cargo test`.
//! - Mock + live fixtures are shaped like `ytmusic-rs` JSON
//!   (`ytmusicapi::Json`) and parsed by the same `from_json` constructors the
//!   live layer uses — no live network in tests.

use std::collections::HashMap;

use qtbridge::{QListModel, QListModelBase, QModelItem, qobject};

/// A track row: roles are `title`, `artist`, `album`, `video_id`, `duration`,
/// `thumbnail_url` (live art; empty → wash fallback), plus `artist_id` /
/// `album_id` (open the artist / album page; empty when unknown) and
/// `set_video_id` (the row's id inside an owned playlist; empty elsewhere).
#[derive(QModelItem, Clone, Default, Debug, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub video_id: String,
    pub duration: String,
    pub thumbnail_url: String,
    pub artist_id: String,
    pub album_id: String,
    pub set_video_id: String,
}

/// An album card: roles are `title`, `artist`, `browse_id`, `year`,
/// `thumbnail_url` (live art; empty → wash fallback).
#[derive(QModelItem, Clone, Default, Debug, PartialEq)]
pub struct Album {
    pub title: String,
    pub artist: String,
    pub browse_id: String,
    pub year: String,
    pub thumbnail_url: String,
}

/// An artist row: roles are `name`, `channel_id`, `subscribers`,
/// `thumbnail_url` (live art; empty → wash fallback).
#[derive(QModelItem, Clone, Default, Debug, PartialEq)]
pub struct Artist {
    pub name: String,
    pub channel_id: String,
    pub subscribers: String,
    pub thumbnail_url: String,
}

fn str_at(v: &ytmusicapi::Json, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

/// First non-empty string across alias keys (live shapes vary by endpoint:
/// `browseId` vs `browse_id`, `title` vs `name`, ...). Pure, unit-tested.
fn str_at_any(v: &ytmusicapi::Json, keys: &[&str]) -> String {
    keys.iter()
        .filter_map(|k| v.get(*k).and_then(|x| x.as_str()))
        .find(|s| !s.trim().is_empty())
        .unwrap_or_default()
        .to_string()
}

/// First artist name: `artist` string, else `artists[0].name` (live song
/// shape), else `author` (live playlist-shaped search rows). Pure, tested.
pub(crate) fn artist_name_from_json(v: &ytmusicapi::Json) -> String {
    if let Some(s) = v.get("artist").and_then(|x| x.as_str()) {
        if !s.trim().is_empty() {
            return s.to_string();
        }
    }
    v.get("artists")
        .and_then(|x| x.as_array())
        .and_then(|xs| xs.first())
        .and_then(|a| a.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .or_else(|| {
            v.get("author")
                .and_then(|x| x.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// First artist's channel id: flat `artist_id`, else `artists[0].id` (live
/// song shape). Empty when unknown (plain-text artist). Pure, tested.
pub(crate) fn artist_id_from_json(v: &ytmusicapi::Json) -> String {
    let flat = str_at(v, "artist_id");
    if !flat.is_empty() {
        return flat;
    }
    v.get("artists")
        .and_then(|x| x.as_array())
        .and_then(|xs| xs.first())
        .and_then(|a| a.get("id").and_then(|n| n.as_str()))
        .unwrap_or_default()
        .to_string()
}

/// Album browse id: flat `album_id`, else `album.id` (live song shape).
/// Empty when unknown. Pure, tested.
pub(crate) fn album_id_from_json(v: &ytmusicapi::Json) -> String {
    let flat = str_at(v, "album_id");
    if !flat.is_empty() {
        return flat;
    }
    v.get("album")
        .and_then(|x| x.get("id"))
        .and_then(|n| n.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Album name: `album` string, else `album.name` (live song shape). Pure.
fn album_name_from_json(v: &ytmusicapi::Json) -> String {
    v.get("album")
        .and_then(|x| x.as_str().map(str::to_string))
        .or_else(|| {
            v.get("album")
                .and_then(|x| x.get("name"))
                .and_then(|n| n.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// Format seconds as `m:ss` (total minutes, no hour rollover — mirrors the
/// QML `fmt` lane and `crate::format_time`). Pure, unit-tested.
pub fn format_duration_secs(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Duration label for a track row: `duration` / `length` string first, else
/// the numeric `duration_seconds` / `durationSeconds` (live song shape),
/// else empty (unknown length never blocks). Pure, unit-tested.
pub fn duration_label_from_json(v: &ytmusicapi::Json) -> String {
    if let Some(s) = v
        .get("duration")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("length").and_then(|x| x.as_str()))
    {
        if !s.trim().is_empty() {
            return s.to_string();
        }
    }
    v.get("duration_seconds")
        .or_else(|| v.get("durationSeconds"))
        .and_then(serde_json::Value::as_u64)
        .map(|s| format_duration_secs(s.min(u64::from(u32::MAX)) as u32))
        .unwrap_or_default()
}

/// Pick the live thumbnail URL: largest-area entry of `thumbnails[]`
/// (`{url, width, height}`). Empty when absent — callers show the 2-color
/// wash fallback instead (never a placeholder URL). Values are public CDN
/// image URLs (never secrets, never media bytes). Pure, unit-tested.
/// Upgrade a thumbnail URL to a size suitable for the 420px cover panel.
/// YouTube Music's `lh3` URLs carry `=w60-h60-l90-rj`; YouTube's static
/// images use `hqdefault`/`maxresdefault` filenames. Unknown hosts pass
/// through untouched. Pure and unit-tested.
pub fn hires_thumbnail_url(url: &str) -> String {
    // `hqdefault` is 480×360 and always available; maxres can 404 on some
    // uploads, so prefer the guaranteed size.
    resize_thumbnail_url(url, 544, &["/mqdefault.jpg", "/default.jpg"], "/hqdefault.jpg")
}

/// Shrink a thumbnail URL for a ~40px row icon, so a list costs a few KB
/// per row instead of a full cover each. `mqdefault` (320×180) is the
/// smallest YouTube still without letterbox bars. Pure and unit-tested.
pub fn small_thumbnail_url(url: &str) -> String {
    resize_thumbnail_url(
        url,
        96,
        &["/maxresdefault.jpg", "/sddefault.jpg", "/hqdefault.jpg", "/default.jpg"],
        "/mqdefault.jpg",
    )
}

/// Rewrite a Google content URL's `w`/`h`/`s` size tokens to `px`, or swap
/// the first of `yt_from` for `yt_to` in a YouTube static image URL.
fn resize_thumbnail_url(url: &str, px: u32, yt_from: &[&str], yt_to: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return String::new();
    }
    // Google content URLs: rewrite the `w`/`h`/`s` size tokens (keep any
    // crop/quality flags such as `l90-rj`).
    if let Some(eq) = url.rfind('=') {
        let params = &url[eq + 1..];
        let rewrite = params.contains("-w")
            || params.contains("-h")
            || params.starts_with('w')
            || params.starts_with('h')
            || (params.starts_with('s')
                && params.get(1..).is_some_and(|rest| {
                    rest.chars().next().is_some_and(|c| c.is_ascii_digit())
                }));
        if rewrite {
            let upgraded: Vec<String> = params
                .split('-')
                .map(|part| {
                    let numeric = |p: &str| {
                        p.get(1..).is_some_and(|rest| {
                            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
                        })
                    };
                    if part.starts_with('w') && numeric(part) {
                        format!("w{px}")
                    } else if part.starts_with('h') && numeric(part) {
                        format!("h{px}")
                    } else if part.starts_with('s') && numeric(part) {
                        format!("s{px}")
                    } else {
                        part.to_string()
                    }
                })
                .collect();
            return format!("{}{}", &url[..eq + 1], upgraded.join("-"));
        }
    }
    if url.contains("ytimg.com") || url.contains("img.youtube.com") {
        for size in yt_from {
            if url.contains(size) {
                return url.replace(size, yt_to);
            }
        }
    }
    url.to_string()
}

pub fn thumbnail_url_from_json(v: &ytmusicapi::Json) -> String {
    // Flat rows (TrackModel rows_json / QueueItem to_json) carry one URL.
    if let Some(url) = v
        .get("thumbnail_url")
        .or_else(|| v.get("thumbnail"))
        .and_then(|u| u.as_str())
    {
        let lower = url.trim().to_ascii_lowercase();
        if lower.starts_with("https://") || lower.starts_with("http://") {
            return hires_thumbnail_url(url.trim());
        }
    }
    let items = match v.get("thumbnails").and_then(|x| x.as_array()) {
        Some(items) => items,
        None => return String::new(),
    };
    let mut best: Option<(&str, u64)> = None;
    for item in items {
        let url = match item.get("url").and_then(|u| u.as_str()) {
            Some(u) if !u.trim().is_empty() => u,
            _ => continue,
        };
        // Skip secret-looking values defensively (thumbnails must be https
        // image URLs; anything else is dropped, never surfaced).
        let lower = url.to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            continue;
        }
        let w = item.get("width").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let h = item.get("height").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let area = w.saturating_mul(h);
        let replace = match best {
            None => true,
            Some((_, a)) => area >= a,
        };
        if replace {
            best = Some((url, area));
        }
    }
    best
        .map(|(u, _)| hires_thumbnail_url(u))
        .unwrap_or_default()
}

impl Track {
    /// Build from a `ytmusic-rs`-shaped JSON dict (defensive: missing keys
    /// become empty strings). Covers BOTH the offline mock shape AND the
    /// live shapes (`parse_song_mtrir` / `parse_song_flat` / search results:
    /// `artists[]`, `thumbnails[]`, numeric `duration_seconds`,
    /// `album{name}`).
    pub fn from_json(v: &ytmusicapi::Json) -> Self {
        Self {
            title: str_at_any(v, &["title", "name"]),
            artist: artist_name_from_json(v),
            album: album_name_from_json(v),
            video_id: str_at_any(v, &["videoId", "video_id"]),
            duration: duration_label_from_json(v),
            thumbnail_url: thumbnail_url_from_json(v),
            artist_id: artist_id_from_json(v),
            album_id: album_id_from_json(v),
            set_video_id: str_at_any(v, &["setVideoId", "set_video_id"]),
        }
    }

    /// Flat metadata-only row (the shape `from_json` reads back): what QML
    /// models, the queue and the disk cache pass around.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "video_id": self.video_id,
            "title": self.title,
            "artist": self.artist,
            "album": self.album,
            "duration": self.duration,
            "thumbnail_url": self.thumbnail_url,
            "artist_id": self.artist_id,
            "album_id": self.album_id,
            "set_video_id": self.set_video_id,
        })
    }

    /// True when the row carries anything renderable.
    pub fn is_renderable(&self) -> bool {
        !self.title.is_empty() || !self.video_id.is_empty()
    }
}

impl Album {
    /// Build from a `ytmusic-rs`-shaped JSON dict (defensive). Covers the
    /// offline mock shape AND the live shapes (`parse_album_mtrir`:
    /// `artists[]`, `browseId`, `year`, `thumbnails[]`; search `album`
    /// results: `browseId`, `year`, `thumbnails[]`).
    pub fn from_json(v: &ytmusicapi::Json) -> Self {
        Self {
            title: str_at_any(v, &["title", "name"]),
            artist: {
                let a = artist_name_from_json(v);
                if a.is_empty() {
                    str_at(v, "artist")
                } else {
                    a
                }
            },
            browse_id: str_at_any(v, &["browseId", "browse_id", "browseID"]),
            year: str_at(v, "year"),
            thumbnail_url: thumbnail_url_from_json(v),
        }
    }
}

impl Artist {
    /// Build from a `ytmusic-rs`-shaped JSON dict (defensive). Covers the
    /// offline mock shape AND the live shapes (`parse_related_artist`:
    /// `title` name, `browseId`, `subscribers`, `thumbnails[]`; search
    /// `artist` results: `artist` name, `browseId`, `thumbnails[]`).
    pub fn from_json(v: &ytmusicapi::Json) -> Self {
        Self {
            name: str_at_any(v, &["name", "artist", "title"]),
            channel_id: str_at_any(v, &["channelId", "browseId", "browse_id", "channel_id"]),
            subscribers: str_at(v, "subscribers"),
            thumbnail_url: thumbnail_url_from_json(v),
        }
    }
}

/// Mock `browse`-shaped JSON fixture (`ytmusicapi::Json`, offline).
pub fn mock_browse_json() -> ytmusicapi::Json {
    serde_json::json!({
        "tracks": [
            {"title": "Midnight Static", "artist": "Glass Coast", "album": "Neon Tide", "videoId": "mock-vid-001", "duration": "3:24"},
            {"title": "Paper Lanterns", "artist": "Aiko Tanabe", "album": "Drift", "videoId": "mock-vid-002", "duration": "4:01"},
            {"title": "Copper Sky", "artist": "The Meridians", "album": "Westbound", "videoId": "mock-vid-003", "duration": "2:58"},
            {"title": "Low Orbit", "artist": "Vela", "album": "Perigee", "videoId": "mock-vid-004", "duration": "3:47"},
            {"title": "Honey & Smoke", "artist": "June Calloway", "album": "Ember Days", "videoId": "mock-vid-005", "duration": "3:12"},
            {"title": "Ultraviolet", "artist": "Glass Coast", "album": "Neon Tide", "videoId": "mock-vid-006", "duration": "5:03"}
        ],
        "albums": [
            {"title": "Neon Tide", "artist": "Glass Coast", "browseId": "mock-alb-001", "year": "2024"},
            {"title": "Drift", "artist": "Aiko Tanabe", "browseId": "mock-alb-002", "year": "2023"},
            {"title": "Westbound", "artist": "The Meridians", "browseId": "mock-alb-003", "year": "2025"},
            {"title": "Perigee", "artist": "Vela", "browseId": "mock-alb-004", "year": "2022"}
        ],
        "artists": [
            {"name": "Glass Coast", "channelId": "mock-art-001", "subscribers": "1.2M"},
            {"name": "Aiko Tanabe", "channelId": "mock-art-002", "subscribers": "840K"},
            {"name": "The Meridians", "channelId": "mock-art-003", "subscribers": "2.1M"},
            {"name": "Vela", "channelId": "mock-art-004", "subscribers": "310K"}
        ]
    })
}

/// Parse tracks out of [`mock_browse_json`]-shaped JSON (shared with live).
pub fn tracks_from_json(json: &ytmusicapi::Json) -> Vec<Track> {
    json.get("tracks")
        .and_then(|x| x.as_array())
        .map(|xs| {
            xs.iter()
                .map(Track::from_json)
                .filter(|t| t.is_renderable())
                .collect()
        })
        .unwrap_or_default()
}

/// Parse albums out of [`mock_browse_json`]-shaped JSON (shared with live).
pub fn albums_from_json(json: &ytmusicapi::Json) -> Vec<Album> {
    json.get("albums")
        .and_then(|x| x.as_array())
        .map(|xs| xs.iter().map(Album::from_json).collect())
        .unwrap_or_default()
}

/// Parse artists out of [`mock_browse_json`]-shaped JSON (shared with live).
pub fn artists_from_json(json: &ytmusicapi::Json) -> Vec<Artist> {
    json.get("artists")
        .and_then(|x| x.as_array())
        .map(|xs| xs.iter().map(Artist::from_json).collect())
        .unwrap_or_default()
}

/// Flatten live `get_home` shelves (`[{title, contents:[...]}]` from
/// `parse_mixed_content`) into track rows. Shelf wrappers carry no `videoId`
/// themselves — only `contents` items do. Rows without a `videoId` are
/// unplayable and never enter the track lane (they surface as album/artist
/// cards instead). Pure, unit-tested with fixtures.
pub fn tracks_from_shelves(shelves: &ytmusicapi::JsonList) -> Vec<Track> {
    shelves
        .iter()
        .flat_map(|shelf| {
            shelf
                .get("contents")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default()
        })
        .map(|item| Track::from_json(&item))
        .filter(|t| !t.video_id.is_empty())
        .collect()
}

/// True when a shelf/search item is an album card: carries a `browseId` but
/// no playable `videoId` and no artist `subscribers` marker.
fn is_album_item(v: &ytmusicapi::Json) -> bool {
    let has_browse = v
        .get("browseId")
        .and_then(|x| x.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    let has_video = v
        .get("videoId")
        .and_then(|x| x.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    has_browse && !has_video && v.get("subscribers").is_none()
}

/// True when a shelf/search item is an artist row: carries a `browseId` but
/// no playable `videoId`, plus the artist `subscribers` marker (home
/// shelves) or an explicit `resultType == "artist"` (search).
fn is_artist_item(v: &ytmusicapi::Json) -> bool {
    let has_browse = v
        .get("browseId")
        .and_then(|x| x.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    let has_video = v
        .get("videoId")
        .and_then(|x| x.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    if has_video || !has_browse {
        return false;
    }
    if v.get("subscribers").is_some() {
        return true;
    }
    v.get("resultType").and_then(|x| x.as_str()) == Some("artist")
}

/// Flatten live `get_home` shelves into album cards (see [`is_album_item`]).
/// Pure, unit-tested with fixtures.
pub fn albums_from_shelves(shelves: &ytmusicapi::JsonList) -> Vec<Album> {
    shelves
        .iter()
        .flat_map(|shelf| {
            shelf
                .get("contents")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default()
        })
        .filter(is_album_item)
        .map(|item| Album::from_json(&item))
        .filter(|a| !a.title.is_empty() || !a.browse_id.is_empty())
        .collect()
}

/// Flatten live `get_home` shelves into artist rows (see [`is_artist_item`]).
/// Pure, unit-tested with fixtures.
pub fn artists_from_shelves(shelves: &ytmusicapi::JsonList) -> Vec<Artist> {
    shelves
        .iter()
        .flat_map(|shelf| {
            shelf
                .get("contents")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default()
        })
        .filter(is_artist_item)
        .map(|item| Artist::from_json(&item))
        .filter(|a| !a.name.is_empty() || !a.channel_id.is_empty())
        .collect()
}

/// Partition live `search` results (`resultType`-tagged dicts) into track
/// rows: `song` / `video` results (or untagged rows carrying a `videoId`).
/// Results without a `videoId` are unplayable and never enter the track
/// lane. Pure, unit-tested with fixtures.
pub fn tracks_from_search(results: &ytmusicapi::JsonList) -> Vec<Track> {
    results
        .iter()
        .filter(|v| {
            match v.get("resultType").and_then(|x| x.as_str()) {
                Some("song") | Some("video") => true,
                Some(_) => false,
                None => v
                    .get("videoId")
                    .and_then(|x| x.as_str())
                    .is_some_and(|s| !s.trim().is_empty()),
            }
        })
        .map(Track::from_json)
        .filter(|t| !t.video_id.is_empty())
        .collect()
}

/// Partition live `search` results into album cards (`resultType == "album"`
/// or untagged `browseId` rows that are not artists). Pure, tested.
pub fn albums_from_search(results: &ytmusicapi::JsonList) -> Vec<Album> {
    results
        .iter()
        .filter(|v| {
            if v.get("resultType").and_then(|x| x.as_str()) == Some("album") {
                return true;
            }
            v.get("resultType").is_none() && is_album_item(v)
        })
        .map(Album::from_json)
        .filter(|a| !a.title.is_empty() || !a.browse_id.is_empty())
        .collect()
}

/// Partition live `search` results into artist rows (`resultType == "artist"`
/// or untagged artist-shaped rows). Pure, tested.
pub fn artists_from_search(results: &ytmusicapi::JsonList) -> Vec<Artist> {
    results
        .iter()
        .filter(|v| {
            if v.get("resultType").and_then(|x| x.as_str()) == Some("artist") {
                return true;
            }
            v.get("resultType").is_none() && is_artist_item(v)
        })
        .map(Artist::from_json)
        .filter(|a| !a.name.is_empty() || !a.channel_id.is_empty())
        .collect()
}

/// Mock track rows (offline).
pub fn mock_tracks() -> Vec<Track> {
    tracks_from_json(&mock_browse_json())
}

/// Mock album cards (offline).
pub fn mock_albums() -> Vec<Album> {
    albums_from_json(&mock_browse_json())
}

/// Mock artist rows (offline).
pub fn mock_artists() -> Vec<Artist> {
    artists_from_json(&mock_browse_json())
}

/// List model of [`Track`] rows. Defaults to EMPTY (explicit empty state;
/// offline mocks via [`mock_tracks`] are test-only — see [`TrackModel::load_home`]).
#[derive(Default)]
pub struct TrackModel {
    items: Vec<Track>,
    // Rows `reset_unnotified` installs: a replace is ONE model reset instead
    // of a remove + insert notification per row.
    staged: Vec<Track>,
}

#[cfg(test)]
impl TrackModel {
    /// Build from explicit rows (data layer / tests).
    pub fn from_vec(items: Vec<Track>) -> Self {
        Self { items, staged: Vec::new() }
    }

    /// Current rows (read-only).
    pub fn items(&self) -> &[Track] {
        &self.items
    }

    /// Replace storage without Qt notification (tests / unnotified paths).
    pub fn replace_unnotified(&mut self, items: Vec<Track>) {
        self.items = items;
    }
}

#[qobject(Base = QListModel, ConvertToCamelCase)]
impl TrackModel {
    /// Reload mock rows (notified; requires attached QML proxy).
    #[qslot]
    fn load_mocks(&mut self) {
        while self.len() > 0 {
            self.pop();
        }
        for t in mock_tracks() {
            self.push(t);
        }
    }

    /// Clear all rows (notified; requires attached QML proxy).
    #[qslot]
    fn clear(&mut self) {
        self.clear_notified();
    }

    /// Append rows from a metadata-only JSON array as ONE insert, so a view
    /// keeps its scroll position (next playlist page). Notified.
    #[qslot]
    fn append_json(&mut self, rows: String) {
        let values: Vec<serde_json::Value> = serde_json::from_str(&rows).unwrap_or_default();
        let tracks: Vec<Track> = values
            .iter()
            .map(Track::from_json)
            .filter(Track::is_renderable)
            .collect();
        if tracks.is_empty() {
            return;
        }
        use qtbridge::QObjectHolder;
        let Some(proxy) = self.try_get_rust_proxy_ptr() else {
            return;
        };
        let first = self.items.len() as i32;
        let last = first + tracks.len() as i32 - 1;
        let parent = qtbridge::qtbridge_type_lib::QModelIndex::default();
        // Same begin/end pairing qtbridge's own `push` uses, for a range.
        unsafe { &mut *proxy }.base_begin_insert_rows(&mut *self, &parent, first, last);
        self.items.extend(tracks);
        unsafe { &mut *proxy }.base_end_insert_rows(&mut *self);
    }

    /// All rows as a JSON array (metadata only) for queue seeding from the
    /// visible list. No URLs or secrets.
    #[qslot]
    fn rows_json(&self) -> String {
        let rows: Vec<serde_json::Value> = self.items.iter().map(Track::to_json).collect();
        serde_json::to_string(&rows).unwrap_or_default()
    }

    /// Replace rows from a metadata-only JSON array (queue popup + playlist
    /// detail). Notified; requires attached QML proxy.
    #[qslot]
    fn load_json(&mut self, rows: String) {
        let values: Vec<serde_json::Value> = serde_json::from_str(&rows).unwrap_or_default();
        let tracks: Vec<Track> = values
            .iter()
            .map(Track::from_json)
            .filter(Track::is_renderable)
            .collect();
        self.replace_notified(tracks);
    }

    /// LIVE home fill: restores the keyring session and enforces the
    /// forwarded gate (`Backend.gateStatus` — Premium only, blocked returns
    /// before any keyring/network touch), then replaces rows with the live
    /// `get_home` shelves. Returns a Rust-decided status line for the QML
    /// status label. Failures clear (explicit error state, never silent
    /// mocks). QML calls this on launch + whenever `gateStatus` changes.
    #[qslot]
    fn load_home(&mut self, gate_status: String) -> String {
        match crate::ytmusic::home::live_home_tracks(&gate_status) {
            Ok((tracks, msg)) => {
                self.replace_notified(tracks);
                msg
            }
            Err(msg) => {
                self.clear_notified();
                msg
            }
        }
    }

    /// LIVE search fill: same session/gate lane as [`load_home`](Self::load_home),
    /// then replaces rows with live `search` results. Empty query reloads
    /// home (reset). Returns a Rust-decided status line. Failures clear.
    #[qslot]
    fn search(&mut self, query: String, gate_status: String) -> String {
        match crate::ytmusic::search::live_search_tracks(&query, &gate_status) {
            Ok((tracks, msg)) => {
                self.replace_notified(tracks);
                msg
            }
            Err(msg) => {
                self.clear_notified();
                msg
            }
        }
    }
}

impl TrackModel {
    /// Notified replace as one model reset (requires attached QML proxy).
    fn replace_notified(&mut self, items: Vec<Track>) {
        self.staged = items;
        self.reset();
    }

    /// Notified clear for the live-failure path (see [`load_home`](Self::load_home)).
    fn clear_notified(&mut self) {
        self.replace_notified(Vec::new());
    }
}

impl QListModel for TrackModel {
    type Item = Track;

    fn len(&self) -> usize {
        self.items.len()
    }

    fn get(&self, index: usize) -> Option<&Self::Item> {
        self.items.get(index)
    }

    fn set_unnotified(&mut self, index: usize, value: Self::Item) -> bool {
        if index < self.items.len() {
            self.items[index] = value;
            true
        } else {
            false
        }
    }

    fn push_unnotified(&mut self, value: Self::Item) {
        self.items.push(value);
    }

    fn insert_unnotified(&mut self, index: usize, value: Self::Item) {
        self.items.insert(index, value);
    }

    fn remove_unnotified(&mut self, index: usize) -> Self::Item {
        self.items.remove(index)
    }

    fn reset_unnotified(&mut self) {
        self.items = std::mem::take(&mut self.staged);
    }
}

/// List model of [`Album`] cards. Defaults to EMPTY (see [`TrackModel`]).
#[derive(Default)]
pub struct AlbumModel {
    items: Vec<Album>,
}

#[cfg(test)]
impl AlbumModel {
    /// Build from explicit rows (data layer / tests).
    pub fn from_vec(items: Vec<Album>) -> Self {
        Self { items }
    }

    /// Current rows (read-only).
    pub fn items(&self) -> &[Album] {
        &self.items
    }
}

#[qobject(Base = QListModel, ConvertToCamelCase)]
impl AlbumModel {
    /// Replace rows from a JSON array of cards (artist/album page shelves).
    #[qslot]
    fn load_json(&mut self, rows: String) {
        let values: Vec<serde_json::Value> = serde_json::from_str(&rows).unwrap_or_default();
        while self.len() > 0 {
            self.pop();
        }
        for item in values.iter().map(Album::from_json).filter(|x| !x.browse_id.is_empty()) {
            self.push(item);
        }
    }

    /// Reload mock rows (notified; requires attached QML proxy).
    #[qslot]
    fn load_mocks(&mut self) {
        while self.len() > 0 {
            self.pop();
        }
        for a in mock_albums() {
            self.push(a);
        }
    }

    /// Clear all rows (notified; requires attached QML proxy).
    #[qslot]
    fn clear(&mut self) {
        while self.len() > 0 {
            self.pop();
        }
    }

    /// LIVE home fill (see [`TrackModel::load_home`]): album cards from the
    /// live `get_home` shelves. Returns a Rust-decided status line (`""` on
    /// success — the tracks lane owns the banner); failures clear.
    #[qslot]
    fn load_home(&mut self, gate_status: String) -> String {
        match crate::ytmusic::home::live_home_albums(&gate_status) {
            Ok((albums, msg)) => {
                while self.len() > 0 {
                    self.pop();
                }
                for a in albums {
                    self.push(a);
                }
                msg
            }
            Err(msg) => {
                while self.len() > 0 {
                    self.pop();
                }
                msg
            }
        }
    }

    /// LIVE search fill (see [`TrackModel::search`]): album cards from live
    /// `search` results. Empty query reloads home. Failures clear.
    #[qslot]
    fn search(&mut self, query: String, gate_status: String) -> String {
        match crate::ytmusic::search::live_search_albums(&query, &gate_status) {
            Ok((albums, msg)) => {
                while self.len() > 0 {
                    self.pop();
                }
                for a in albums {
                    self.push(a);
                }
                msg
            }
            Err(msg) => {
                while self.len() > 0 {
                    self.pop();
                }
                msg
            }
        }
    }
}

impl QListModel for AlbumModel {
    type Item = Album;

    fn len(&self) -> usize {
        self.items.len()
    }

    fn get(&self, index: usize) -> Option<&Self::Item> {
        self.items.get(index)
    }

    fn set_unnotified(&mut self, index: usize, value: Self::Item) -> bool {
        if index < self.items.len() {
            self.items[index] = value;
            true
        } else {
            false
        }
    }

    fn push_unnotified(&mut self, value: Self::Item) {
        self.items.push(value);
    }

    fn insert_unnotified(&mut self, index: usize, value: Self::Item) {
        self.items.insert(index, value);
    }

    fn remove_unnotified(&mut self, index: usize) -> Self::Item {
        self.items.remove(index)
    }

    fn reset_unnotified(&mut self) {
        self.items.clear();
    }
}

/// List model of [`Artist`] rows. Defaults to EMPTY (see [`TrackModel`]).
#[derive(Default)]
pub struct ArtistModel {
    items: Vec<Artist>,
}

#[cfg(test)]
impl ArtistModel {
    /// Build from explicit rows (data layer / tests).
    pub fn from_vec(items: Vec<Artist>) -> Self {
        Self { items }
    }

    /// Current rows (read-only).
    pub fn items(&self) -> &[Artist] {
        &self.items
    }
}

#[qobject(Base = QListModel, ConvertToCamelCase)]
impl ArtistModel {
    /// Replace rows from a JSON array of cards (artist/album page shelves).
    #[qslot]
    fn load_json(&mut self, rows: String) {
        let values: Vec<serde_json::Value> = serde_json::from_str(&rows).unwrap_or_default();
        while self.len() > 0 {
            self.pop();
        }
        for item in values.iter().map(Artist::from_json).filter(|x| !x.channel_id.is_empty()) {
            self.push(item);
        }
    }

    /// Reload mock rows (notified; requires attached QML proxy).
    #[qslot]
    fn load_mocks(&mut self) {
        while self.len() > 0 {
            self.pop();
        }
        for a in mock_artists() {
            self.push(a);
        }
    }

    /// Clear all rows (notified; requires attached QML proxy).
    #[qslot]
    fn clear(&mut self) {
        while self.len() > 0 {
            self.pop();
        }
    }

    /// LIVE home fill (see [`TrackModel::load_home`]): artist rows from the
    /// live `get_home` shelves. Returns a Rust-decided status line (`""` on
    /// success); failures clear.
    #[qslot]
    fn load_home(&mut self, gate_status: String) -> String {
        match crate::ytmusic::home::live_home_artists(&gate_status) {
            Ok((artists, msg)) => {
                while self.len() > 0 {
                    self.pop();
                }
                for a in artists {
                    self.push(a);
                }
                msg
            }
            Err(msg) => {
                while self.len() > 0 {
                    self.pop();
                }
                msg
            }
        }
    }

    /// LIVE search fill (see [`TrackModel::search`]): artist rows from live
    /// `search` results. Empty query reloads home. Failures clear.
    #[qslot]
    fn search(&mut self, query: String, gate_status: String) -> String {
        match crate::ytmusic::search::live_search_artists(&query, &gate_status) {
            Ok((artists, msg)) => {
                while self.len() > 0 {
                    self.pop();
                }
                for a in artists {
                    self.push(a);
                }
                msg
            }
            Err(msg) => {
                while self.len() > 0 {
                    self.pop();
                }
                msg
            }
        }
    }
}

impl QListModel for ArtistModel {
    type Item = Artist;

    fn len(&self) -> usize {
        self.items.len()
    }

    fn get(&self, index: usize) -> Option<&Self::Item> {
        self.items.get(index)
    }

    fn set_unnotified(&mut self, index: usize, value: Self::Item) -> bool {
        if index < self.items.len() {
            self.items[index] = value;
            true
        } else {
            false
        }
    }

    fn push_unnotified(&mut self, value: Self::Item) {
        self.items.push(value);
    }

    fn insert_unnotified(&mut self, index: usize, value: Self::Item) {
        self.items.insert(index, value);
    }

    fn remove_unnotified(&mut self, index: usize) -> Self::Item {
        self.items.remove(index)
    }

    fn reset_unnotified(&mut self) {
        self.items.clear();
    }
}

/// One library playlist card: roles are `title`, `playlist_id`, `count`,
/// `thumbnail_url` (live art; empty → initial fallback).
#[derive(QModelItem, Clone, Default, Debug, PartialEq)]
pub struct Playlist {
    pub title: String,
    pub playlist_id: String,
    pub count: String,
    pub thumbnail_url: String,
}

impl Playlist {
    /// Build from the flat JSON the library loader emits.
    pub fn from_json(v: &ytmusicapi::Json) -> Self {
        Self {
            title: str_at_any(v, &["title", "name"]),
            playlist_id: str_at_any(v, &["playlist_id", "playlistId"]),
            count: str_at_any(v, &["count"]),
            thumbnail_url: thumbnail_url_from_json(v),
        }
    }
}

/// List model of [`Playlist`] cards. Defaults to EMPTY.
#[derive(Default)]
pub struct PlaylistModel {
    items: Vec<Playlist>,
}

#[qobject(Base = QListModel, ConvertToCamelCase)]
impl PlaylistModel {
    /// Replace rows from a metadata-only JSON array (notified).
    #[qslot]
    fn load_json(&mut self, rows: String) {
        let values: Vec<serde_json::Value> = serde_json::from_str(&rows).unwrap_or_default();
        let playlists: Vec<Playlist> = values
            .iter()
            .map(Playlist::from_json)
            .filter(|p| !p.playlist_id.is_empty())
            .collect();
        while self.len() > 0 {
            self.pop();
        }
        for p in playlists {
            self.push(p);
        }
    }

    #[qslot]
    fn clear(&mut self) {
        while self.len() > 0 {
            self.pop();
        }
    }
}

impl QListModel for PlaylistModel {
    type Item = Playlist;

    fn len(&self) -> usize {
        self.items.len()
    }

    fn get(&self, index: usize) -> Option<&Self::Item> {
        self.items.get(index)
    }

    fn set_unnotified(&mut self, index: usize, value: Self::Item) -> bool {
        if index < self.items.len() {
            self.items[index] = value;
            true
        } else {
            false
        }
    }

    fn push_unnotified(&mut self, value: Self::Item) {
        self.items.push(value);
    }

    fn insert_unnotified(&mut self, index: usize, value: Self::Item) {
        self.items.insert(index, value);
    }

    fn remove_unnotified(&mut self, index: usize) -> Self::Item {
        self.items.remove(index)
    }

    fn reset_unnotified(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mocks_parse_over_ytmusic_json_types() {
        let json: ytmusicapi::Json = mock_browse_json();
        let tracks = tracks_from_json(&json);
        let albums = albums_from_json(&json);
        let artists = artists_from_json(&json);
        assert_eq!(tracks.len(), 6);
        assert_eq!(albums.len(), 4);
        assert_eq!(artists.len(), 4);
        assert_eq!(tracks[0].title, "Midnight Static");
        assert_eq!(tracks[0].artist, "Glass Coast");
        assert_eq!(tracks[0].video_id, "mock-vid-001");
        assert_eq!(albums[0].browse_id, "mock-alb-001");
        assert_eq!(artists[0].channel_id, "mock-art-001");
    }

    #[test]
    fn from_json_is_defensive() {
        let empty = serde_json::json!({});
        let t = Track::from_json(&empty);
        assert!(!t.is_renderable());
        assert!(tracks_from_json(&empty).is_empty());
        assert!(albums_from_json(&empty).is_empty());
        assert!(artists_from_json(&empty).is_empty());

        // Alternate artist/album shapes.
        let alt = serde_json::json!({
            "title": "Alt",
            "artists": [{"name": "Alt Artist"}],
            "album": {"name": "Alt Album"},
            "videoId": "x",
            "length": "1:00"
        });
        let t = Track::from_json(&alt);
        assert_eq!(t.artist, "Alt Artist");
        assert_eq!(t.album, "Alt Album");
        assert_eq!(t.duration, "1:00");
        assert!(t.is_renderable());
    }

    #[test]
    fn models_default_to_explicit_empty() {
        // No demo rows in any live path: QML views render an explicit empty
        // state until the live fill arrives. Mock fixtures stay test-only.
        assert_eq!(TrackModel::default().len(), 0);
        assert_eq!(AlbumModel::default().len(), 0);
        assert_eq!(ArtistModel::default().len(), 0);
        assert_eq!(mock_tracks().len(), 6, "mocks stay available for tests");
        assert_eq!(mock_albums().len(), 4);
        assert_eq!(mock_artists().len(), 4);
    }

    #[test]
    fn unnotified_storage_ops_work_offline() {
        let mut m = TrackModel::from_vec(Vec::new());
        assert_eq!(m.len(), 0);
        assert!(m.get(0).is_none());

        m.push_unnotified(Track {
            title: "A".to_string(),
            ..Track::default()
        });
        assert_eq!(m.len(), 1);
        assert!(!m.set_unnotified(5, Track::default()));
        assert!(m.set_unnotified(0, Track {
            title: "B".to_string(),
            ..Track::default()
        }));
        assert_eq!(m.get(0).map(|t| t.title.as_str()), Some("B"));

        m.insert_unnotified(0, Track {
            title: "C".to_string(),
            ..Track::default()
        });
        assert_eq!(m.get(0).map(|t| t.title.as_str()), Some("C"));

        let removed = m.remove_unnotified(0);
        assert_eq!(removed.title, "C");
        m.reset_unnotified();
        assert_eq!(m.len(), 0);

        m.replace_unnotified(mock_tracks());
        assert_eq!(m.len(), 6);
        assert_eq!(m.items().len(), 6);
    }

    #[test]
    fn album_artist_models_store_rows() {
        let mut albums = AlbumModel::from_vec(Vec::new());
        albums.push_unnotified(mock_albums().remove(0));
        assert_eq!(albums.len(), 1);
        assert_eq!(albums.items()[0].title, "Neon Tide");

        let mut artists = ArtistModel::from_vec(Vec::new());
        artists.push_unnotified(mock_artists().remove(0));
        assert_eq!(artists.len(), 1);
        assert_eq!(artists.items()[0].name, "Glass Coast");
    }

    #[test]
    fn live_track_shapes_parse() {
        // `parse_song_mtrir` shape: artists[], album{name}, numeric duration.
        let v = serde_json::json!({
            "title": "Live Song",
            "videoId": "vid-9",
            "artists": [{"name": "Live Artist", "id": "UC9"}],
            "album": {"name": "Live Album", "id": "MPRE9"},
            "duration_seconds": 3723u64,
            "thumbnails": [{"url": "https://img/a.jpg", "width": 100, "height": 100}]
        });
        let t = Track::from_json(&v);
        assert_eq!(t.artist, "Live Artist");
        assert_eq!(t.album, "Live Album");
        assert_eq!(t.duration, "62:03");
        assert_eq!(t.thumbnail_url, "https://img/a.jpg");
        assert!(t.is_renderable());
    }

    #[test]
    fn live_album_artist_aliases_parse() {
        let album = Album::from_json(&serde_json::json!({
            "title": "AL", "artists": [{"name": "AA"}],
            "browseId": "MPRE1", "year": "2024",
            "thumbnails": [{"url": "https://img/al.jpg", "width": 50, "height": 50}]
        }));
        assert_eq!(album.artist, "AA");
        assert_eq!(album.browse_id, "MPRE1");
        assert_eq!(album.thumbnail_url, "https://img/al.jpg");

        // Search `artist` results carry the name under `artist`.
        let artist = Artist::from_json(&serde_json::json!({
            "artist": "AR", "browseId": "UC1", "subscribers": "2M",
            "thumbnails": [{"url": "https://img/ar.jpg", "width": 50, "height": 50}]
        }));
        assert_eq!(artist.name, "AR");
        assert_eq!(artist.channel_id, "UC1");
        assert_eq!(artist.subscribers, "2M");
        assert_eq!(artist.thumbnail_url, "https://img/ar.jpg");

        // Home `parse_related_artist` shape carries the name under `title`.
        let home_artist = Artist::from_json(&serde_json::json!({
            "title": "HA", "browseId": "UC2", "subscribers": "5K"
        }));
        assert_eq!(home_artist.name, "HA");
        assert_eq!(home_artist.thumbnail_url, "");
    }

    #[test]
    fn duration_labels_format_and_fallback() {
        assert_eq!(format_duration_secs(0), "0:00");
        assert_eq!(format_duration_secs(93), "1:33");
        assert_eq!(format_duration_secs(3723), "62:03");
        // String labels win over numeric ones.
        let both = serde_json::json!({"duration": "1:00", "duration_seconds": 999u64});
        assert_eq!(duration_label_from_json(&both), "1:00");
    }

    #[test]
    fn hires_thumbnails_upgrade_google_and_ytimg_sizes() {
        assert_eq!(
            hires_thumbnail_url("https://lh3.googleusercontent.com/abc=w60-h60-l90-rj"),
            "https://lh3.googleusercontent.com/abc=w544-h544-l90-rj"
        );
        assert_eq!(
            hires_thumbnail_url("https://i.ytimg.com/vi/x/mqdefault.jpg"),
            "https://i.ytimg.com/vi/x/hqdefault.jpg"
        );
        assert_eq!(
            hires_thumbnail_url("https://i.ytimg.com/vi/x/sddefault.jpg"),
            "https://i.ytimg.com/vi/x/sddefault.jpg"
        );
        assert_eq!(hires_thumbnail_url(""), "");
    }

    #[test]
    fn small_thumbnails_shrink_google_and_ytimg_sizes() {
        assert_eq!(
            small_thumbnail_url("https://lh3.googleusercontent.com/abc=w544-h544-l90-rj"),
            "https://lh3.googleusercontent.com/abc=w96-h96-l90-rj"
        );
        assert_eq!(
            small_thumbnail_url("https://i.ytimg.com/vi/x/hqdefault.jpg"),
            "https://i.ytimg.com/vi/x/mqdefault.jpg"
        );
        assert_eq!(small_thumbnail_url(""), "");
    }

    #[test]
    fn track_ids_read_live_and_flat_shapes() {
        let live = serde_json::json!({
            "videoId": "v", "title": "T", "setVideoId": "s1",
            "artists": [{"name": "A", "id": "UCa"}], "album": {"name": "Al", "id": "MPREb1"}
        });
        let t = Track::from_json(&live);
        assert_eq!((t.artist_id.as_str(), t.album_id.as_str(), t.set_video_id.as_str()), ("UCa", "MPREb1", "s1"));
        // The flat row round-trips.
        assert_eq!(Track::from_json(&t.to_json()), t);
        // Plain-text artist/album: no ids.
        let plain = Track::from_json(&serde_json::json!({"videoId": "v", "artist": "A", "album": "Al"}));
        assert!(plain.artist_id.is_empty() && plain.album_id.is_empty());
    }
}
