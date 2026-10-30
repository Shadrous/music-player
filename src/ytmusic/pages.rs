//! Artist and album pages over `ytmusic-rs`: one page at a time, fetched on
//! a worker thread and published through a polled token, like the library.
//! Metadata only: no media URLs, no secrets.

use crate::models::{thumbnail_url_from_json, Track};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use super::{FetchError, fetch_error_message, live_client};

/// (token, result once the worker lands).
type PageSlot = (u64, Option<Result<Value, String>>);

static PAGE_GEN: AtomicU64 = AtomicU64::new(0);
static PAGE: OnceLock<Mutex<PageSlot>> = OnceLock::new();

fn page_slot() -> &'static Mutex<PageSlot> {
    PAGE.get_or_init(|| Mutex::new((0, None)))
}

/// Start loading the `kind` ("artist" | "album") page for `id`. A newer
/// load supersedes this one.
pub fn start_page_load(kind: &str, id: &str, gate_status: &str) -> u64 {
    let token = PAGE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    if let Ok(mut slot) = page_slot().lock() {
        *slot = (token, None);
    }
    let (kind, id, gate_status) = (kind.to_string(), id.to_string(), gate_status.to_string());
    let spawned = std::thread::Builder::new()
        .name("page-load".to_string())
        .spawn(move || {
            let result = fetch_page(&kind, &id, &gate_status);
            if PAGE_GEN.load(Ordering::SeqCst) != token {
                return;
            }
            if let Ok(mut slot) = page_slot().lock() {
                *slot = (token, Some(result));
            }
        });
    if spawned.is_err() {
        if let Ok(mut slot) = page_slot().lock() {
            *slot = (token, Some(Err(fetch_error_message(FetchError::Transport))));
        }
    }
    token
}

/// `loading` | `ready` | `error:<message>` | `stale` (superseded).
pub fn page_load_status(token: u64) -> String {
    let Ok(slot) = page_slot().lock() else {
        return "stale".to_string();
    };
    match &*slot {
        (t, _) if *t != token => "stale".to_string(),
        (_, None) => "loading".to_string(),
        (_, Some(Ok(_))) => "ready".to_string(),
        (_, Some(Err(msg))) => format!("error:{msg}"),
    }
}

/// The loaded page (`{}` before one lands): `kind`, `id`, `title`,
/// `subtitle`, `thumbnail_url`, `artist`/`artist_id` (albums) and `tracks`
/// (flat track rows).
pub fn page_json() -> String {
    page_slot()
        .lock()
        .ok()
        .and_then(|slot| match &slot.1 {
            Some(Ok(page)) => Some(page.to_string()),
            _ => None,
        })
        .unwrap_or_else(|| "{}".to_string())
}

fn fetch_page(kind: &str, id: &str, gate_status: &str) -> Result<Value, String> {
    let (yt, _gate) = live_client(gate_status)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| fetch_error_message(FetchError::Transport))?;
    let transport = |_| fetch_error_message(FetchError::Transport);
    if kind == "album" {
        // A song without an album id (the player's title): its watch entry
        // names the album, single releases included.
        let browse_id = if id.starts_with("MPRE") {
            id.to_string()
        } else {
            let watch = rt
                .block_on(yt.get_watch_playlist(Some(id), None, 10, false, false))
                .map_err(transport)?;
            let first = watch.get("tracks").and_then(|t| t.get(0)).cloned().unwrap_or(Value::Null);
            let album_id = crate::models::album_id_from_json(&first);
            if album_id.is_empty() {
                return Err("This song isn't part of an album.".to_string());
            }
            album_id
        };
        let album = rt.block_on(yt.get_album(&browse_id)).map_err(transport)?;
        Ok(album_page(&browse_id, &Value::Object(album)))
    } else {
        // `get_artist` keeps only the header and top songs; `get_channel`
        // parses the same page's shelves (albums, singles, related...).
        // Both run at once; the shelves are a bonus if that one fails.
        let (artist, channel) = rt.block_on(async { tokio::join!(yt.get_artist(id), yt.get_channel(id)) });
        let artist = artist.map_err(transport)?;
        let channel = channel.map(Value::Object).unwrap_or(Value::Null);
        Ok(artist_page(id, &Value::Object(artist), &channel))
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// `get_album` → page. Album tracks carry no art of their own, so each row
/// takes the album cover (the player shows it) and the album's id.
fn album_page(browse_id: &str, album: &Value) -> Value {
    let head = Track::from_json(album);
    let cover = thumbnail_url_from_json(album);
    let tracks: Vec<Value> = album
        .get("tracks")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(Track::from_json)
                .filter(|t| !t.video_id.is_empty())
                .map(|mut t| {
                    t.album_id = browse_id.to_string();
                    if t.thumbnail_url.is_empty() {
                        t.thumbnail_url = cover.clone();
                    }
                    t.to_json()
                })
                .collect()
        })
        .unwrap_or_default();
    let count = match album.get("trackCount").and_then(Value::as_u64) {
        Some(1) => "1 song".to_string(),
        Some(n) => format!("{n} songs"),
        None => String::new(),
    };
    let subtitle = [str_of(album, "type"), str_of(album, "year"), &count, str_of(album, "duration")]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let mut shelves = Vec::new();
    push_shelf(&mut shelves, "Other versions", "album", album.get("other_versions"));
    serde_json::json!({
        "kind": "album",
        "id": browse_id,
        // "Album", "Single", "EP": the page's chip.
        "release_type": str_of(album, "type"),
        "title": head.title,
        "subtitle": subtitle,
        "artist": head.artist,
        "artist_id": head.artist_id,
        "thumbnail_url": cover,
        "description": str_of(album, "description"),
        "tracks": tracks,
        "shelves": shelves,
    })
}

/// Card shelf for the page: `kind` says what a card opens ("album",
/// "artist" or "playlist"); items take the card models' flat shapes
/// (album: `title`/`artist`/`browse_id`/`year`; artist: `name`/
/// `channel_id`/`subscribers`). Empty shelves are left out.
fn push_shelf(shelves: &mut Vec<Value>, title: &str, kind: &str, results: Option<&Value>) {
    let items: Vec<Value> = results
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| shelf_item(kind, item))
        .collect();
    if !items.is_empty() {
        shelves.push(serde_json::json!({ "title": title, "kind": kind, "items": items }));
    }
}

fn shelf_item(kind: &str, item: &Value) -> Option<Value> {
    let thumbnail_url = thumbnail_url_from_json(item);
    let value = match kind {
        "artist" => {
            let a = crate::models::Artist::from_json(item);
            if a.channel_id.is_empty() {
                return None;
            }
            serde_json::json!({
                "name": a.name, "channel_id": a.channel_id,
                "subscribers": a.subscribers, "thumbnail_url": thumbnail_url,
            })
        }
        "playlist" => {
            let id = str_of(item, "playlistId");
            if id.is_empty() {
                return None;
            }
            let author = crate::models::artist_name_from_json(&serde_json::json!({"artists": item.get("author")}));
            serde_json::json!({
                "title": str_of(item, "title"), "artist": if author.is_empty() { "Playlist".to_string() } else { author },
                "browse_id": id, "year": "", "thumbnail_url": thumbnail_url,
            })
        }
        _ => {
            let a = crate::models::Album::from_json(item);
            if a.browse_id.is_empty() {
                return None;
            }
            // Singles carry no artist on an artist's own page: show the type.
            let artist = if a.artist.is_empty() { str_of(item, "type").to_string() } else { a.artist };
            serde_json::json!({
                "title": a.title, "artist": artist, "browse_id": a.browse_id,
                "year": a.year, "thumbnail_url": thumbnail_url,
            })
        }
    };
    Some(value)
}

/// `get_artist` (header, top songs) + `get_channel` (the same page's
/// shelves) → page.
fn artist_page(channel_id: &str, artist: &Value, channel: &Value) -> Value {
    let tracks: Vec<Value> = artist
        .pointer("/songs/results")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(Track::from_json)
                .filter(|t| !t.video_id.is_empty())
                .map(|t| t.to_json())
                .collect()
        })
        .unwrap_or_default();
    let subscribers = str_of(artist, "subscribers");
    let monthly = str_of(artist, "monthlyListeners");
    let subtitle = if !monthly.is_empty() {
        format!("{monthly} monthly listeners")
    } else if !subscribers.is_empty() {
        format!("{subscribers} subscribers")
    } else {
        String::new()
    };
    let mut shelves = Vec::new();
    for (key, title, kind) in [
        ("albums", "Albums", "album"),
        ("singles", "Singles & EPs", "album"),
        ("playlists", "Playlists", "playlist"),
        ("related", "Fans also like", "artist"),
    ] {
        push_shelf(&mut shelves, title, kind, channel.pointer(&format!("/{key}/results")));
    }
    serde_json::json!({
        "kind": "artist",
        "id": channel_id,
        "title": str_of(artist, "name"),
        "subtitle": subtitle,
        "artist": "",
        "artist_id": "",
        "thumbnail_url": thumbnail_url_from_json(artist),
        "description": str_of(artist, "description"),
        "tracks": tracks,
        "shelves": shelves,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_rows_take_album_cover_and_id() {
        let album = serde_json::json!({
            "title": "Nostalji", "type": "Album", "year": "2019", "trackCount": 1, "duration": "3 minutes",
            "artists": [{"name": "Deniz", "id": "UCd"}],
            "thumbnails": [{"url": "https://lh3.googleusercontent.com/a=w60-h60", "width": 60, "height": 60}],
            "tracks": [
                {"videoId": "v1", "title": "One", "artists": [{"name": "Deniz", "id": "UCd"}], "album": "Nostalji", "duration": "3:00"},
                {"title": "unplayable"}
            ]
        });
        let page = album_page("MPREb1", &album);
        assert_eq!(page["title"], "Nostalji");
        assert_eq!(page["subtitle"], "Album · 2019 · 1 song · 3 minutes");
        assert_eq!(page["release_type"], "Album");
        assert_eq!(page["artist_id"], "UCd");
        let tracks = page["tracks"].as_array().unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0]["album_id"], "MPREb1");
        assert_eq!(tracks[0]["thumbnail_url"], page["thumbnail_url"]);
    }

    #[test]
    fn artist_page_lists_top_songs() {
        let artist = serde_json::json!({
            "name": "Deniz", "subscribers": "1.2K",
            "songs": {"results": [{"videoId": "v1", "title": "One", "artists": [{"name": "Deniz", "id": "UCd"}]}]}
        });
        let channel = serde_json::json!({
            "albums": {"results": [{"title": "Nostalji", "browseId": "MPREb1", "year": "2019", "artists": []}, {"title": "no id"}]},
            "singles": {"results": [{"title": "Tek", "browseId": "MPREb2", "type": "Single", "year": "2021"}]},
            "related": {"results": [{"title": "Other", "browseId": "UCo", "subscribers": "5K"}]}
        });
        let page = artist_page("UCd", &artist, &channel);
        let shelves = page["shelves"].as_array().unwrap();
        assert_eq!(shelves.len(), 3);
        assert_eq!(shelves[0]["items"].as_array().unwrap().len(), 1);
        assert_eq!(shelves[1]["items"][0]["artist"], "Single");
        assert_eq!(shelves[2]["kind"], "artist");
        assert_eq!(shelves[2]["items"][0]["channel_id"], "UCo");
        assert!(artist_page("UCd", &artist, &Value::Null)["shelves"].as_array().unwrap().is_empty());
        assert_eq!(page["title"], "Deniz");
        assert_eq!(page["subtitle"], "1.2K subscribers");
        assert_eq!(page["tracks"][0]["artist_id"], "UCd");
    }
}
