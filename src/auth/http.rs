//! InnerTube HTTP wiring (P1).
//!
//! Handover notes:
//! - `ytmusic-rs` owns its own `reqwest` client internally (see
//!   `ytmusicapi::YTMusic`); this module is for *supplementary* InnerTube HTTP
//!   the app needs outside that client in P2 (e.g. visitor-id refresh).
//! - The builder mirrors the `YTMusic` transport settings (web-client
//!   user-agent, 30s timeout) and enables an in-memory cookie jar. Auth
//!   cookies themselves are NEVER written to disk: the jar is memory-only and
//!   the long-lived secret lives in the OS keyring (see `super`).
//! - Front-page fetch ([`fetch_front_page_initial_data`]) reuses the keyring
//!   session cookies + a FRESH per-request SAPISIDHASH PER URL (same
//!   `sapisid_from_cookie` + `get_authorization` formula ytmusic-rs
//!   `request_headers` uses, but bound to [`origin_for_front_page_url`]:
//!   music legs hash `https://music.youtube.com`, youtube legs hash
//!   `https://www.youtube.com` — the stale stored `authorization` value is
//!   never replayed, and one shared hash is never sent to both origins.
//!   Cookie-jar domain scoping is a non-issue: the session cookie travels
//!   as an explicit per-request `Cookie` header (same as ytmusic-rs), NOT
//!   via the client's memory jar, so cross-domain sends are exact.
//!   Secrets stay in memory only, never logged.
//!
//! P1 skeleton: consumed by P2 callers; no `qml/` changes.
// FIXME(P2): use this for visitor-id refresh + any non-`YTMusic` InnerTube GETs.

use std::time::Duration;

use ytmusicapi::YTMusic;

/// Connect + read timeout, mirroring `YTMusic`'s 30s transport timeout.
pub const INNERTUBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Per front-page GET bound: the probe runs two of these (music + youtube)
/// inside the gate's [`crate::premium_gate::PROBE_TIMEOUT`], so each leg is
/// capped well below it — a hung front page fails closed to "no pages"
/// instead of eating the whole probe budget.
pub const FRONT_PAGE_TIMEOUT: Duration = Duration::from_secs(10);

/// Per-URL SAPISIDHASH origin (AUDIT FIX): the old fetch sent ONE stored
/// (music) origin + hash to BOTH front pages, so the youtube.com leg carried
/// music.youtube.com's hash. SAPISIDHASH binds an origin — a mismatched hash
/// cannot authenticate — so each URL now uses its own origin for BOTH the
/// `Origin` header AND the hash input. NOTE: correct binding is necessary
/// but not sufficient — a stored cookie subset can still come back logged-out
/// (live: `AVATAR_LOGGED_OUT` on the youtube leg), which is why the gate
/// folds front pages GATED (logged-out → unknown, never fail).
pub const MUSIC_ORIGIN: &str = "https://music.youtube.com";
/// Origin + hash input for the `https://www.youtube.com/` front-page leg.
pub const YOUTUBE_ORIGIN: &str = "https://www.youtube.com";

/// Authed web front pages whose `ytInitialData` carries the topbar Premium
/// logo (see [`crate::premium_gate::parse_topbar_premium`]): the music front
/// page first (matches the session origin), then the youtube.com front page
/// carrying the yt-dlp-verified pattern. Both are fetched with the same
/// session cookies + fresh SAPISIDHASH; results are OR-folded by the caller.
pub const FRONT_PAGE_URLS: &[&str] = &[
    "https://music.youtube.com/",
    "https://www.youtube.com/",
];

/// Origin used when a URL matches neither front page (defensive fallback,
/// never a secret). Both [`FRONT_PAGE_URLS`] entries map via
/// [`origin_for_front_page_url`] and never hit this.
const FALLBACK_ORIGIN: &str = "https://music.youtube.com";

/// Select the SAPISIDHASH origin (and `Origin` header) for a front-page URL:
/// `music.youtube.com` → [`MUSIC_ORIGIN`], `www.youtube.com` →
/// [`YOUTUBE_ORIGIN`], anything else → [`FALLBACK_ORIGIN`] (fail-closed —
/// never panics, never empty). Pure (offline unit-tested).
pub fn origin_for_front_page_url(url: &str) -> &'static str {
    if url.contains("://www.youtube.com") {
        YOUTUBE_ORIGIN
    } else if url.contains("music.youtube.com") {
        MUSIC_ORIGIN
    } else {
        FALLBACK_ORIGIN
    }
}

/// Build a `reqwest` async client for supplementary InnerTube HTTP.
///
/// Mirrors the `ytmusic-rs` transport per its docs (web-client user-agent,
/// JSON, in-memory cookie jar). Returns the client; callers attach
/// auth headers per request — never bake secrets into a shared default.
pub fn build_innertube_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .user_agent(ytmusicapi::constants::USER_AGENT)
        .timeout(INNERTUBE_TIMEOUT)
        .cookie_store(true)
        .build()
}

/// Audio quality requested on the probe `player` call: `AUDIO_QUALITY_HIGH`.
///
/// Vendored-source note (verified offline, `cargo`-registry
/// ytmusic-rs-0.1.1): `get_song` (`src/mixins/browsing.rs:542-557`) sends NO
/// audio-quality parameter — a crate-wide grep for `AUDIO_QUALITY` /
/// `audioQuality` / `audio_quality` returns zero hits — so this is an
/// additive extension, not a mirror of a vendored field. It follows the
/// mopidy-ytmusic HIGH-preference pattern (task ref, claimed
/// `verified:false`): the same `playbackContext.contentPlaybackContext`
/// object `get_song` already sends, plus `audioQuality`, so a Premium
/// session returns its premium-only audio ladder while a free session stays
/// capped. The gate only reads `itag`/`bitrate` NUMBERS downstream (see
/// [`crate::premium_gate::parse_hires_audio`]) — media URLs stay unread.
pub const AUDIO_QUALITY_HIGH: &str = "AUDIO_QUALITY_HIGH";

/// Build the `player` request body, `get_song`-compatible
/// (`src/mixins/browsing.rs:550-556`): `playbackContext` signature stamp +
/// `video_id`, PLUS [`AUDIO_QUALITY_HIGH`] (see above — the vendored body
/// has no such key). Pure (offline unit-tested); [`raw_player_response`]
/// sends it via the unfiltered path.
pub fn raw_player_body(video_id: &str, signature_timestamp: i64) -> serde_json::Map<String, serde_json::Value> {
    serde_json::json!({
        "playbackContext": {"contentPlaybackContext": {
            "signatureTimestamp": signature_timestamp,
            "audioQuality": AUDIO_QUALITY_HIGH,
        }},
        "video_id": video_id,
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}
/// Alternate InnerTube client for the probe's player experiment (NOT the
/// gate path — evidence only, default transport unchanged).
///
/// Mirrors the vendored mobile switch ytmusic-rs already performs in
/// `get_lyrics` (`src/mixins/browsing.rs:649-659`: `ANDROID_MUSIC` /
/// `7.21.50` for timed lyrics): the same client name + version the vendored
/// stack itself supports, trivially constructed here as a direct `player`
/// POST body (our crate cannot mutate `YTMusic`'s private context, and
/// `send_request` overwrites any caller-supplied `context` — so the
/// experiment bypasses `debug_send_request` with the same cookie +
/// fresh-SAPISIDHASH credential family [`front_page_auth`] builds).
/// The gate compares the default (`WEB_REMIX`) ladder/ad markers against
/// this response and prints both on the dump path (`altclient:` line via
/// [`crate::premium_gate::render_altclient_line`]); the result NEVER gates.
/// `None`/error/match are all recorded honestly — experiment, not commitment.
pub const ALT_CLIENT_NAME: &str = "ANDROID_MUSIC";

/// Client version paired with [`ALT_CLIENT_NAME`] (vendored `get_lyrics`
/// mobile switch value).
pub const ALT_CLIENT_VERSION: &str = "7.21.50";

/// Per alternate-client POST bound: runs inside the gate's
/// [`crate::premium_gate::PROBE_TIMEOUT`], so a hung alt leg fails closed
/// to "unknown" instead of eating the probe budget.
pub const ALT_CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// VisionOS guest client user agent. The guest player is the path whose
/// stream URLs need no `n` deciphering (see the Sonora `ytmusic` client's
/// `guest_audio`); the UA must also be used for the media range requests.
pub const VISIONOS_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

/// Cached guest visitor id (`sw.js_data` issues one per run).
static VISIONOS_VISITOR: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Extract a `Cg…` visitor id from the `sw.js_data` body without a regex.
/// Pure; returns `None` for short/non-visitor strings.
pub fn extract_visitor(body: &str) -> Option<String> {
    let mut rest = body;
    while let Some(pos) = rest.find("\"Cg") {
        let candidate = &rest[pos + 1..];
        if let Some(end) = candidate.find('"') {
            let id = &candidate[..end];
            let allowed = id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '%' | '_' | '+' | '=' | '-'));
            if id.len() >= 22 && allowed {
                return Some(id.to_string());
            }
        }
        rest = &rest[pos + 3..];
    }
    None
}

/// Fresh `cpn` (client playback nonce): 16 hex chars; YouTube only needs a
/// per-request identifier.
fn cpn() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:016x}")
}

/// Guest `VISIONOS` player body. No cookies and no signature timestamp: the
/// guest format URLs are already addressable, which is why this path avoids
/// `n` ciphering entirely.
pub fn visionos_player_body(
    video_id: &str,
    visitor: &str,
    cpn: &str,
) -> serde_json::Value {
    serde_json::json!({
        "context": {
            "client": {
                "hl": "en",
                "gl": "US",
                "visitorData": visitor,
                "clientName": "VISIONOS",
                "clientVersion": "1.02",
                "userAgent": VISIONOS_UA,
                "utcOffsetMinutes": 0,
                "deviceMake": "Apple",
                "deviceModel": "RealityDevice17,1",
                "osName": "visionOS",
                "osVersion": "26.5.23O471",
            },
            "user": {"enableSafetyMode": false, "lockedSafetyMode": false},
            "request": {"useSsl": true, "internalExperimentFlags": []},
        },
        "videoId": video_id,
        "contentCheckOk": true,
        "racyCheckOk": true,
        "cpn": cpn,
        "playbackContext": {"contentPlaybackContext": {
            "html5Preference": "HTML5_PREF_WANTS",
        }},
    })
}

/// Fetch (once) the guest visitor id from YouTube's service-worker data.
async fn visionos_visitor() -> Option<String> {
    if let Some(visitor) = VISIONOS_VISITOR.get() {
        return Some(visitor.clone());
    }
    let client = reqwest::Client::builder()
        .timeout(INNERTUBE_TIMEOUT)
        .build()
        .ok()?;
    let body = client
        .get("https://www.youtube.com/sw.js_data")
        .header("User-Agent", VISIONOS_UA)
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let visitor = extract_visitor(&body)?;
    let _ = VISIONOS_VISITOR.set(visitor.clone());
    Some(visitor)
}

/// Guest `VISIONOS` player response: no cookies, no `n` deciphering. This is
/// the primary playback URL source; the signed `WEB_REMIX` path remains a
/// fallback. Returns `None` on any transport/parse failure (fail-closed).
pub async fn visionos_guest_player(video_id: &str) -> Option<serde_json::Value> {
    if video_id.trim().is_empty() {
        return None;
    }
    let visitor = visionos_visitor().await?;
    let client = reqwest::Client::builder()
        .timeout(INNERTUBE_TIMEOUT)
        .build()
        .ok()?;
    let body = visionos_player_body(video_id, &visitor, &cpn());
    let response = client
        .post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false&alt=json")
        .header("Accept", "*/*")
        .header("Accept-Language", "*")
        .header("Content-Type", "application/json")
        .header("Origin", "https://www.youtube.com")
        .header("User-Agent", VISIONOS_UA)
        .header("X-Youtube-Client-Name", "101")
        .header("X-Youtube-Client-Version", "1.02")
        .header("X-Goog-Visitor-Id", &visitor)
        .json(&body)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

/// Build the alternate-client `player` request body: an explicit `context`
/// carrying [`ALT_CLIENT_NAME`]/[`ALT_CLIENT_VERSION`] (a direct POST does
/// not pass through `send_request`, which would otherwise overwrite
/// `context`), plus the `get_song`-compatible `playbackContext` signature
/// stamp + [`AUDIO_QUALITY_HIGH`] and `video_id` so the returned ladder is
/// directly comparable with the default probe's. Pure (offline unit-tested).
pub fn alt_player_body(
    video_id: &str,
    signature_timestamp: i64,
) -> serde_json::Map<String, serde_json::Value> {
    serde_json::json!({
        "context": {"client": {
            "clientName": ALT_CLIENT_NAME,
            "clientVersion": ALT_CLIENT_VERSION,
        }},
        "playbackContext": {"contentPlaybackContext": {
            "signatureTimestamp": signature_timestamp,
            "audioQuality": AUDIO_QUALITY_HIGH,
        }},
        "video_id": video_id,
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

/// UNFILTERED alternate-client `player` probe for the experiment described
/// on [`ALT_CLIENT_NAME`]: the same probe track under the mobile client
/// context, so ad-marker presence ([`crate::premium_gate::PLAYER_AD_KEYS`]
/// key names only) and the audio ladder (`itag`/`bitrate` numbers only) can
/// be compared against the default `WEB_REMIX` probe.
///
/// Transport: a fresh client from [`build_innertube_client`] POSTed at
/// `YTM_BASE_API + "player" + params + key` with the keyring session cookie
/// and a fresh per-request SAPISIDHASH (see [`front_page_auth`] — the same
/// credential family ytmusic-rs attaches per InnerTube request). Gated on
/// `is_authenticated` (logged-out yields `None` with zero I/O).
///
/// Returns the raw response on success; `None` on ANY failure (no auth,
/// transport error, non-2xx incl. bot-guard, unparseable — recorded as
/// `altclient: ... unknown` downstream, never a premium read). Values (ad
/// payloads, media URLs, tokens) are never read, logged, or returned by the
/// gate parsers — only key names / numbers leave this path.
pub async fn alt_client_player_response(
    yt: &YTMusic,
    video_id: &str,
) -> Option<serde_json::Value> {
    if !yt.is_authenticated() {
        return None;
    }
    let (cookie, authorization, origin) = front_page_auth()?;
    let Ok(client) = build_innertube_client() else {
        return None;
    };
    let url = format!(
        "{}player{}{}",
        ytmusicapi::constants::YTM_BASE_API,
        ytmusicapi::constants::YTM_PARAMS,
        ytmusicapi::constants::YTM_PARAMS_KEY,
    );
    let body = alt_player_body(video_id, ytmusicapi::helpers::get_datestamp() - 1);
    let fetch = async {
        use reqwest::header::HeaderValue;
        let response = client
            .post(&url)
            .header(reqwest::header::COOKIE, HeaderValue::from_str(&cookie).ok()?)
            .header(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_str(&authorization).ok()?,
            )
            .header(reqwest::header::ORIGIN, HeaderValue::from_str(&origin).ok()?)
            .json(&body)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.json::<serde_json::Value>().await.ok()
    };
    tokio::time::timeout(ALT_CLIENT_TIMEOUT, fetch).await.ok()?
}

/// UNFILTERED `player` probe for the premium gate's ad-marker leg AND its
/// high-audio capability leg (the request asks for [`AUDIO_QUALITY_HIGH`],
/// so [`crate::premium_gate::parse_hires_audio`] can tell a Premium ladder
/// from a capped free ladder on the same response).
///
/// Replication notes (vendored ytmusic-rs-0.1.1, offline inspection):
/// - Body mirrors `get_song` (`src/mixins/browsing.rs:542-557`): endpoint
///   `"player"`, `playbackContext.contentPlaybackContext.signatureTimestamp`
///   (`helpers::get_datestamp() - 1`, `src/helpers.rs:127-130`), `video_id`.
/// - Transport IS `YTMusic::send_request` (`src/ytmusic.rs:331-362`):
///   `context` insert (`:338`), URL `YTM_BASE_API + endpoint + params`
///   (`:340`; base `src/constants.rs:4`, key `:6`), per-request
///   `request_headers` (`:270-318`) incl. `SAPISIDHASH` (`src/helpers.rs:55-66`
///   `get_authorization`, SAPISID from the keyring session cookie
///   `src/helpers.rs:39-53`), web client `WEB_REMIX`
///   (`src/helpers.rs:22-37`), visitor id (`src/ytmusic.rs:254-261`).
///   `debug_send_request` (`src/ytmusic.rs:323-329`) exposes exactly this path
///   with `""` additional params and returns the FULL JSON — unlike
///   `get_song`, which strips the response to 5 keys
///   (`src/mixins/browsing.rs:559-567`) and can therefore NEVER carry
///   `adPlacements`/`adBreaks`/`playerAds` (see the vacuity note on
///   [`crate::premium_gate::parse_player_ads_absent`]).
///
/// Returns the raw response on success; `None` on ANY transport/parse failure
/// (fail-closed — the caller maps it to `ads_absent = None`). Key names only
/// downstream: values (ad payloads, media URLs, tokens) are never read,
/// logged, or returned by the gate parsers.
pub async fn raw_player_response(yt: &YTMusic, video_id: &str) -> Option<serde_json::Value> {
    let body = raw_player_body(video_id, ytmusicapi::helpers::get_datestamp() - 1);
    yt.debug_send_request("player", body).await.ok()
}

/// Extract the embedded `ytInitialData` JSON object from a YouTube(-Music)
/// front-page HTML document (`var ytInitialData = {...};` /
/// `window["ytInitialData"] = {...}` forms).
///
/// Pure + offline: scans occurrences of the marker (with an identifier
/// boundary so `ytInitialPlayerResponse` never matches), requires a bare
/// `=` then a JSON object, and brace-matches string-aware (double-quote +
/// backslash escapes) before `serde_json` parsing. First successful parse
/// wins; missing markers, truncated JSON, and unparseable shapes all yield
/// `None` (fail-closed upstream). Never panics on any input.
pub fn extract_yt_initial_data(html: &str) -> Option<serde_json::Value> {
    const MARKER: &str = "ytInitialData";
    const MAX_ATTEMPTS: usize = 8;
    let bytes = html.as_bytes();
    let mut search_from = 0usize;
    let mut attempts = 0;
    while attempts < MAX_ATTEMPTS {
        let rel = html.get(search_from..)?.find(MARKER)?;
        let mark = search_from + rel;
        // Identifier boundary: `ytInitialPlayerResponse` must not match.
        let boundary_ok = html[mark + MARKER.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        if !boundary_ok {
            search_from = mark + MARKER.len();
            continue;
        }
        let rest = &html[mark + MARKER.len()..];
        let Some(eq) = rest.find('=') else {
            search_from = mark + MARKER.len();
            continue;
        };
        // Between the marker and `=` only `window["..."]` / `var` / `.` /
        // whitespace plumbing may appear — anything else is not our
        // assignment (e.g. a bare `var ytInitialData;` declaration whose
        // `=` belongs to later, unrelated script).
        if !rest[..eq]
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '"' | '\'' | ']' | '[' | '.'))
        {
            search_from = mark + MARKER.len();
            continue;
        }
        let after_eq = &rest[eq + 1..];
        let Some(brace) = after_eq.find('{') else {
            search_from = mark + MARKER.len();
            continue;
        };
        if !after_eq[..brace].trim().is_empty() {
            search_from = mark + MARKER.len();
            continue;
        }
        let abs_brace = mark + MARKER.len() + eq + 1 + brace;
        if let Some(end) = find_json_object_end(&bytes[abs_brace..]) {
            if let Ok(value) =
                serde_json::from_str::<serde_json::Value>(&html[abs_brace..abs_brace + end])
            {
                return Some(value);
            }
        }
        attempts += 1;
        search_from = abs_brace + 1;
    }
    None
}

/// Exclusive end index (one past the matching `}`) of the JSON object at
/// the start of `bytes` (which must begin with `{`). String-aware: braces
/// inside double-quoted strings (with backslash escapes) do not count.
/// `None` when the object never closes. Byte-oriented — safe on any UTF-8
/// (only ASCII bytes are special in JSON).
fn find_json_object_end(bytes: &[u8]) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Load the keyring session cookie blob (pure credential read — no hashing).
/// `None` on ANY failure (no blob, unparsable, non-browser shape, missing
/// cookie) — fail-closed upstream. Secrets stay in memory only, never logged.
fn front_page_cookie() -> Option<String> {
    let store = super::KeyringStore;
    let blob = super::SecretStore::load_cookie(&store).ok()??;
    let headers = ytmusicapi::auth::parse_auth_str(&blob).ok()?;
    if !ytmusicapi::auth::browser::is_browser(&headers) {
        return None;
    }
    headers.get("cookie").cloned()
}

/// Build a fresh timestamped SAPISIDHASH for `cookie` bound to `origin`
/// (same `sapisid_from_cookie` + `get_authorization` formula ytmusic-rs
/// attaches per InnerTube request). Pure (offline unit-tested): `None`
/// when the cookie carries no SAPISID. The two front-page legs call this
/// with DIFFERENT origins ([`MUSIC_ORIGIN`] vs [`YOUTUBE_ORIGIN`]) so each
/// hash matches the origin it is sent with.
fn authorization_for_cookie_origin(cookie: &str, origin: &str) -> Option<String> {
    let sapisid = ytmusicapi::helpers::sapisid_from_cookie(cookie).ok()?;
    Some(ytmusicapi::helpers::get_authorization(&format!(
        "{sapisid} {origin}"
    )))
}

/// Build per-origin session auth for one front-page URL: the keyring cookie
/// plus a FRESH timestamped SAPISIDHASH bound to that URL's own origin (see
/// [`origin_for_front_page_url`]). `None` on ANY failure — fail-closed
/// upstream. Values are request headers only, never logged.
fn front_page_auth_for_url(url: &str) -> Option<(String, String, String)> {
    let cookie = front_page_cookie()?;
    let origin = origin_for_front_page_url(url);
    let authorization = authorization_for_cookie_origin(&cookie, origin)?;
    Some((cookie, authorization, origin.to_string()))
}

/// Session auth material for InnerTube POSTs (NOT front-page GETs): the
/// keyring cookie blob + stored origin, with a FRESH timestamped SAPISIDHASH
/// (same formula ytmusic-rs attaches per InnerTube request). `None` on ANY
/// failure — fail-closed upstream. InnerTube lives on the music origin so
/// the stored (music) origin is correct here; front-page GETs must use
/// [`front_page_auth_for_url`] instead (per-URL origin — see
/// [`MUSIC_ORIGIN`]). Values are request headers only, never logged.
fn front_page_auth() -> Option<(String, String, String)> {
    let store = super::KeyringStore;
    let blob = super::SecretStore::load_cookie(&store).ok()??;
    let headers = ytmusicapi::auth::parse_auth_str(&blob).ok()?;
    if !ytmusicapi::auth::browser::is_browser(&headers) {
        return None;
    }
    let cookie = headers.get("cookie")?.clone();
    let origin = headers
        .get("origin")
        .or_else(|| headers.get("x-origin"))
        .cloned()
        .unwrap_or_else(|| FALLBACK_ORIGIN.to_string());
    let authorization = authorization_for_cookie_origin(&cookie, &origin)?;
    Some((cookie, authorization, origin))
}

/// Authed GET of one front-page URL. Returns the raw HTML on HTTP success;
/// `None` on invalid headers / transport failure / non-2xx (fail-closed —
/// the caller maps it to "no topbar evidence"). Bounded by
/// [`FRONT_PAGE_TIMEOUT`].
async fn fetch_front_page_html(
    client: &reqwest::Client,
    url: &str,
    cookie: &str,
    authorization: &str,
    origin: &str,
) -> Option<String> {
    use reqwest::header::HeaderValue;
    let fetch = async {
        let response = client
            .get(url)
            .header(reqwest::header::COOKIE, HeaderValue::from_str(cookie).ok()?)
            .header(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_str(authorization).ok()?,
            )
            .header(reqwest::header::ORIGIN, HeaderValue::from_str(origin).ok()?)
            .header(
                reqwest::header::ACCEPT,
                HeaderValue::from_static("text/html,*/*"),
            )
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.text().await.ok()
    };
    tokio::time::timeout(FRONT_PAGE_TIMEOUT, fetch).await.ok()?
}

/// One fetched front page with its source URL (see [`FRONT_PAGE_URLS`]).
/// The gate needs the URL alongside the JSON to report per-page auth
/// (`front_auth: music=… youtube=…`) and per-page icon values — a bare
/// `Vec<Value>` loses which page is which.
#[derive(Debug, Clone)]
pub struct FrontPageFetch {
    /// Source front-page URL (one of [`FRONT_PAGE_URLS`]).
    pub url: &'static str,
    /// Extracted `ytInitialData` JSON for that URL.
    pub data: serde_json::Value,
}

/// Fetch the authed web front-page initial data per URL (see
/// [`fetch_front_page_initial_data`]): same transport, but each leg carries
/// its OWN per-URL SAPISIDHASH + `Origin` (see [`front_page_auth_for_url`]).
/// Gated on `is_authenticated` (`[]` when logged out, zero I/O). Entries
/// keep [`FRONT_PAGE_URLS`] order; a failed leg is simply absent
/// (fail-closed upstream). Page JSON values are dropped after the topbar
/// walk except the logo fields the parser reads.
pub async fn fetch_front_page_initial_data_by_url(yt: &YTMusic) -> Vec<FrontPageFetch> {
    if !yt.is_authenticated() {
        return Vec::new();
    }
    if front_page_cookie().is_none() {
        return Vec::new();
    }
    let Ok(client) = build_innertube_client() else {
        return Vec::new();
    };
    // Precompute per-URL credentials, then fetch both legs concurrently so
    // two slow front pages cannot stack past the gate's PROBE_TIMEOUT.
    let music_auth = front_page_auth_for_url(FRONT_PAGE_URLS[0]);
    let youtube_auth = front_page_auth_for_url(FRONT_PAGE_URLS[1]);
    let (music, youtube) = tokio::join!(
        async {
            match &music_auth {
                Some((cookie, authorization, origin)) => {
                    fetch_front_page_html(&client, FRONT_PAGE_URLS[0], cookie, authorization, origin).await
                }
                None => None,
            }
        },
        async {
            match &youtube_auth {
                Some((cookie, authorization, origin)) => {
                    fetch_front_page_html(&client, FRONT_PAGE_URLS[1], cookie, authorization, origin).await
                }
                None => None,
            }
        },
    );
    let mut out = Vec::new();
    for (url, html) in [FRONT_PAGE_URLS[0], FRONT_PAGE_URLS[1]].into_iter().zip([music, youtube]) {
        if let Some(html) = html {
            if let Some(data) = extract_yt_initial_data(&html) {
                out.push(FrontPageFetch { url, data });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_builds_offline() {
        // Build-only: must not touch the network (verified:false until manager reruns).
        assert!(build_innertube_client().is_ok());
    }

    #[test]
    fn raw_player_body_mirrors_get_song() {
        // Same keys/values `get_song` sends (browsing.rs:550-556): no network.
        // PLUS the HIGH audio-quality ask (see AUDIO_QUALITY_HIGH).
        let body = raw_player_body("4Tr0otuiQuU", 20123);
        assert_eq!(
            body.get("video_id").and_then(|v| v.as_str()),
            Some("4Tr0otuiQuU")
        );
        let value = serde_json::Value::Object(body);
        assert_eq!(
            value
                .pointer("/playbackContext/contentPlaybackContext/signatureTimestamp")
                .and_then(|v| v.as_i64()),
            Some(20123)
        );
        assert_eq!(
            value
                .pointer("/playbackContext/contentPlaybackContext/audioQuality")
                .and_then(|v| v.as_str()),
            Some(AUDIO_QUALITY_HIGH)
        );
    }

    /// Minimal front-page HTML carrying a topbar logo renderer (synthetic,
    /// no secrets — only the `ytInitialData` envelope + topbar shape).
    fn page_with_logo(logo_json: &str) -> String {
        format!(
            "<html><head></head><body><script>var ytInitialData = \
            {{\"topbar\": {{\"desktopTopbarRenderer\": \
            {{\"logo\": {{\"topbarLogoRenderer\": {logo_json}}}}}}}, \
            \"contents\": {{}}}};</script></body></html>"
        )
    }

    #[test]
    fn extract_initial_data_round_trips_topbar_shapes() {
        // Premium-logo page → the gate's topbar parser hits.
        let premium = page_with_logo(
            r#"{"iconType": "YOUTUBE_PREMIUM_LOGO", "tooltipText": "YouTube Premium"}"#,
        );
        let data = extract_yt_initial_data(&premium).expect("premium page parses");
        assert_eq!(
            crate::premium_gate::parse_topbar_premium(&data),
            Some(true)
        );
        // Free-logo page → free-shape (never premium).
        let free = page_with_logo(r#"{"iconType": "YOUTUBE_LOGO", "tooltipText": "YouTube"}"#);
        let data = extract_yt_initial_data(&free).expect("free page parses");
        assert_eq!(
            crate::premium_gate::parse_topbar_premium(&data),
            Some(false)
        );
        // `window["..."]` assignment form parses identically.
        let windowed = premium.replace("var ytInitialData =", r#"window["ytInitialData"] ="#);
        assert!(extract_yt_initial_data(&windowed).is_some());
        // Braces inside JSON strings do not break the brace matcher.
        let tricky = page_with_logo(r#"{"iconType": "YOUTUBE_LOGO", "tooltipText": "a } b { c"}"#);
        let data = extract_yt_initial_data(&tricky).expect("braces in strings parse");
        assert_eq!(
            data.pointer("/topbar/desktopTopbarRenderer/logo/topbarLogoRenderer/tooltipText")
                .and_then(|v| v.as_str()),
            Some("a } b { c")
        );
    }

    #[test]
    fn extract_initial_data_rejects_mangled_pages() {
        // No marker, player-response-only decoy, truncated JSON, and
        // non-object payloads all fail closed (never premium, never panic).
        let pages = [
            "<html><body>consent page</body></html>".to_string(),
            "<script>var ytInitialPlayerResponse = {\"a\":1};</script>".to_string(),
            "<script>var ytInitialData = {\"topbar\": ".to_string(),
            "<script>var ytInitialData = [1,2,3];</script>".to_string(),
            "<script>var ytInitialData;</script>".to_string(),
            String::new(),
        ];
        for html in &pages {
            let parsed = extract_yt_initial_data(html);
            assert!(parsed.is_none(), "mangled page must not parse: {html}");
            // No panic above; unknown shapes never read as premium either.
            let none = serde_json::Value::Null;
            assert_eq!(
                crate::premium_gate::parse_topbar_premium(parsed.as_ref().unwrap_or(&none)),
                None
            );
        }
    }

    #[test]
    fn origin_selection_is_per_url() {
        // The audit fix: each front-page URL binds its OWN origin for the
        // SAPISIDHASH input + Origin header (a shared music hash on the
        // youtube leg reads as logged-out).
        assert_eq!(origin_for_front_page_url("https://music.youtube.com/"), MUSIC_ORIGIN);
        assert_eq!(origin_for_front_page_url("https://www.youtube.com/"), YOUTUBE_ORIGIN);
        assert_eq!(origin_for_front_page_url("https://music.youtube.com/browse"), MUSIC_ORIGIN);
        // Unknown URLs fail closed to the music fallback (never empty/panic).
        assert_eq!(origin_for_front_page_url("https://example.com/"), FALLBACK_ORIGIN);
        assert_eq!(origin_for_front_page_url(""), FALLBACK_ORIGIN);
        // The two legs must differ (otherwise the bug is back).
        assert_ne!(MUSIC_ORIGIN, YOUTUBE_ORIGIN);
    }

    #[test]
    fn authorization_differs_per_origin() {
        // Same session cookie, different origins → different SAPISIDHASH
        // values (both well-formed). Proves the per-URL binding is real, not
        // a relabeled shared hash. No network, no keyring.
        let cookie = "__Secure-3PAPISID=fake-test-sapisid; HSID=fake-hsid";
        let music = authorization_for_cookie_origin(cookie, MUSIC_ORIGIN)
            .expect("music hash builds");
        let youtube = authorization_for_cookie_origin(cookie, YOUTUBE_ORIGIN)
            .expect("youtube hash builds");
        for hash in [&music, &youtube] {
            assert!(hash.starts_with("SAPISIDHASH "), "malformed hash: {hash}");
        }
        assert_ne!(music, youtube, "per-origin hashes must differ");
        // Missing SAPISID fails closed (no hash for either origin).
        assert!(authorization_for_cookie_origin("HSID=only", MUSIC_ORIGIN).is_none());
        assert!(authorization_for_cookie_origin("", YOUTUBE_ORIGIN).is_none());
    }

    #[test]
    fn alt_player_body_carries_mobile_client_context() {
        // Alternate client matches the vendored `get_lyrics` mobile switch
        // (browsing.rs:652-653): ANDROID_MUSIC / 7.21.50 — plus the same
        // HIGH audio-quality ask as the default probe so ladders compare.
        let body = alt_player_body("4Tr0otuiQuU", 20123);
        let value = serde_json::Value::Object(body);
        assert_eq!(
            value
                .pointer("/context/client/clientName")
                .and_then(|v| v.as_str()),
            Some(ALT_CLIENT_NAME)
        );
        assert_eq!(
            value
                .pointer("/context/client/clientVersion")
                .and_then(|v| v.as_str()),
            Some(ALT_CLIENT_VERSION)
        );
        assert_eq!(
            value.get("video_id").and_then(|v| v.as_str()),
            Some("4Tr0otuiQuU")
        );
        assert_eq!(
            value
                .pointer("/playbackContext/contentPlaybackContext/signatureTimestamp")
                .and_then(|v| v.as_i64()),
            Some(20123)
        );
        assert_eq!(
            value
                .pointer("/playbackContext/contentPlaybackContext/audioQuality")
                .and_then(|v| v.as_str()),
            Some(AUDIO_QUALITY_HIGH)
        );
    }

    #[tokio::test]
    async fn alt_client_probe_is_gated_offline_when_logged_out() {
        // Unauthenticated: short-circuits to `None` BEFORE any keyring or
        // network I/O (offline-safe); the dump path renders this as unknown.
        let yt = YTMusic::new().expect("offline client builds");
        assert!(!yt.is_authenticated());
        assert!(
            alt_client_player_response(&yt, "4Tr0otuiQuU")
                .await
                .is_none()
        );
    }

    #[test]
    fn visitor_extraction_requires_a_cg_token() {
        let body = r#"{"VISITOR_DATA":"CgABCDEFGHIJKLMNOPQRSTUVWX","x":1}"#;
        assert_eq!(
            extract_visitor(body).as_deref(),
            Some("CgABCDEFGHIJKLMNOPQRSTUVWX")
        );
        assert_eq!(extract_visitor(r#"{"v":"short"}"#), None);
        assert_eq!(extract_visitor(r#""Cgshort""#), None);
    }

    #[test]
    fn visionos_body_uses_the_guest_client() {
        let body = visionos_player_body("vid", "CgVISITOR123456789012345", "abc");
        assert_eq!(body["context"]["client"]["clientName"], "VISIONOS");
        assert_eq!(body["context"]["client"]["clientVersion"], "1.02");
        assert_eq!(
            body["context"]["client"]["visitorData"],
            "CgVISITOR123456789012345"
        );
        assert_eq!(body["videoId"], "vid");
        assert!(body["context"]["client"]["userAgent"]
            .as_str()
            .unwrap()
            .contains("Macintosh"));
    }
}

