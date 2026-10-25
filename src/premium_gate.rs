//! Sonora-style fail-closed premium gate (P1).
//!
//! ## Signals (strict badge + behavioral fallback + player ad markers)
//!
//! - **Strict badge** (`premium_badge` via [`parse_premium_badge`]): ONLY a
//!   `musicInlineBadgeRenderer` accessibility label naming Premium counts
//!   (`Some(true)`). Any other label (Explicit/...) is not a signal and
//!   scanning continues; titles/subtitles/descriptions are NEVER matched
//!   (free-tier "Get Premium" upsell must not read as premium — fail-open
//!   risk). Absent/unparseable → `None` (ambiguous, fail-closed).
//! - **Behavioral** (`explore_top_songs` via [`parse_explore_top_songs`]):
//!   `browse FEmusic_explore` Top Songs presence. Per vendored ytmusic-rs
//!   `mixins/explore.rs:145-149` the Top Songs chart is only returned for a
//!   premium account (a `VLPL*` carousel header `browseId`); `mixins/explore.rs:337`
//!   notes premium sessions get daily/weekly carousels in place of "videos".
//!   `Some(true)` = `VLPL*` header seen (premium evidence);
//!   `Some(false)` = explore probe succeeded but no Top Songs (free-shape);
//!   `None` = explore probe failed/unparseable (ambiguous, fail-closed).
//!   ONLY the machine-readable `VLPL*` header counts — title text ("Top
//!   Songs") alone never counts (localization/upsell risk).
//! - **Topbar logo** (`topbar_premium` via [`parse_topbar_premium`]): the
//!   NEW primary (yt-dlp pattern, vendored offline as
//!   `_video.py:3899-3908`): authed web front-page initial data at
//!   `topbar.desktopTopbarRenderer.logo.topbarLogoRenderer` —
//!   `iconImage.iconType == "YOUTUBE_PREMIUM_LOGO"` ([`TOPBAR_PREMIUM_ICON`];
//!   CORRECTED path — live ground truth: the fetched youtube.com front page
//!   nests the icon type under `iconImage`, so the old direct-`iconType`
//!   check read every Premium topbar as free-shape; the direct `iconType`
//!   check is kept as a back-compat OR) or `"premium"` in `tooltipText`
//!   (case-insensitive) counts `Some(true)`. Checked on the browse-home
//!   topbar AND both fetched front pages (music + youtube, OR-folded).
//!   Each front page is fetched with its OWN origin + SAPISIDHASH
//!   (`https://music.youtube.com` vs `https://www.youtube.com` — a shared
//!   music hash on the youtube leg reads as logged-out) and folds GATED
//!   ([`parse_topbar_premium_if_authed`]: [`parse_front_page_authed`] must
//!   see avatar/account markers with NO `Sign in` text, else `None` — a
//!   logged-out fetch NEVER yields `Some(false)`). Per-URL auth
//!   (`front_auth: music=… youtube=…`) and per-page icons (`front_icons:`)
//!   print on the dump path via [`render_front_auth_line`] /
//!   [`render_front_icons_line`] + [`topbar_icon_type`].
//!   Gated on `is_authenticated` (the fetch in
//!   [`crate::auth::http::fetch_front_page_initial_data`] returns nothing
//!   when logged out). `Some(false)` = topbar logo parsed and not premium
//!   (free-shape); `None` = topbar missing/unparseable (ambiguous,
//!   fail-closed). Strict match on `iconType`/`tooltipText` ONLY — other
//!   keys (`iconImageId`, titles, ...) never count. Any failure → `None`
//!   (fail-closed). Never panics on unknown shapes.
//! - **Alternate-client player experiment** (evidence ONLY, never gates):
//!   the same probe track is ALSO requested under the vendored mobile client
//!   context ([`crate::auth::http::ALT_CLIENT_NAME`]/
//!   [`crate::auth::http::ALT_CLIENT_VERSION`] — the `ANDROID_MUSIC`
//!   switch ytmusic-rs itself performs for timed lyrics) via
//!   [`crate::auth::http::alt_client_player_response`], and the dump path
//!   prints the comparison as `altclient: NAME/VERSION ads=[...]
//!   audio=[...]` ([`render_altclient_line`]: alt ad-marker key names +
//!   alt `itag@bitrate` ladder beside the default `ads:`/`audio:` lines, or
//!   `... unknown` when the alt leg fails). Default transport unchanged;
//!   error/match/mismatch are all recorded honestly — experiment, not
//!   commitment.
//! - **High-audio capability** (`hires_audio` via [`parse_hires_audio`]):
//!   the NEW capability leg (identity detection is exhausted — this gates on
//!   what the account can actually stream). The probe `player` request asks
//!   for HIGH audio quality (see [`crate::auth::http::AUDIO_QUALITY_HIGH`])
//!   and the parser reads `streamingData.adaptiveFormats` ONLY:
//!   `Some(true)` when a premium-only marker is present (audio `itag` 141
//!   and/or max audio bitrate ≥ [`HIRES_MIN_BITRATE_BPS`], i.e. the HIGH
//!   ladder was honored); `Some(false)` when an audio ladder parses but is
//!   capped below the threshold (free-shape); `None` when there is no audio
//!   evidence (missing/unparseable — ambiguous, fail-closed). `itag` /
//!   `bitrate` NUMBERS only — media URLs and ad payloads stay unread (same
//!   discipline as the ad leg). Never panics on unknown shapes.
//! - **Player ad markers** (`ads_absent` via [`parse_player_ads_absent`]):
//!   EVIDENCE ONLY — DEMOTED, never gates (see below). Presence/absence of
//!   ad-marker keys ([`PLAYER_AD_KEYS`]: `adPlacements` / `adBreaks` /
//!   `playerAds`) in the probe-track RAW `player` response (see
//!   [`crate::auth::http::raw_player_response`]). Research verdict
//!   (verified, yt-dlp pattern): our `ads=fail` is a tier-independent
//!   bot-guard fallback (no poToken) — ad-marker presence tracks bot-guard
//!   filtering, NOT the account tier, so absence must NOT read as premium
//!   evidence. Until a poToken exists the leg stays evidence-only: still
//!   collected, still shown (`ads=...` in the report, `ads:` dump line), but
//!   EXCLUDED from the Premium decision in [`evaluate`] and never named by
//!   [`deciding_leg`]. (History: round 4 hypothesized free accounts get ad
//!   markers in `player` while Premium accounts get none — live probe #3
//!   shapes plus the yt-dlp review refute the tier link, hence the
//!   demotion. The vacuity guard below still applies to the parser.)
//!
//!   VACUITY (manager-verified in vendored ytmusic-rs-0.1.1
//!   `mixins/browsing.rs:559-567`): `get_song` strips the player response to
//!   5 keys (`videoDetails`, `playabilityStatus`, `streamingData`,
//!   `microformat`, `playbackTracking`), so top-level `adPlacements` /
//!   `adBreaks` can NEVER appear in its output and a naive "no ad keys"
//!   check is vacuous — it would pass a free account identically (fail-OPEN,
//!   violating the core legal requirement). Hence `ads_absent` counts ONLY
//!   from a RAW (unfiltered) response: any object whose keys are all within
//!   [`GET_SONG_KEPT_KEYS`] is provably `get_song`-shaped (or degenerate)
//!   and folds to `None` — the gate blocks everyone, Premium included,
//!   until the raw probe succeeds. That fail-closed block is correct and
//!   legal; only the raw probe (which carries `responseContext` et al.
//!   beyond the 5 kept keys) can yield `Some(true)`.
//!
//! [`evaluate`] ORs the four account-level signals: `logged_in && home_ok &&
//! (badge || top_songs || topbar || hires_audio) && playability==Playable` →
//! [`GateStatus::Premium`];
//! everything else blocks fail-closed ([`GateStatus::UpgradeRequired`], or
//! [`GateStatus::LoggedOut`] when not logged in). `ads_absent` is
//! deliberately NOT in the OR (demoted evidence-only leg — see above).
//! Residual live risk: confirm on a Premium AND a free account that the
//! topbar logo tracks the tier AND that the HIGH audio ask splits the ladder
//! (Premium returns itag 141 / ≥200k audio, free stays capped) — probe #2
//! showed NO Premium-named inline badge exists in `FEmusic_home`, so the
//! badge leg alone stays `unknown` on Premium; the topbar + hires legs are
//! the yt-dlp-verified replacement for the demoted ads leg.
//!
//! ## Shape dump (evidence path)
//!
//! Env-gated flag [`SHAPE_DUMP_ENV`] (`SPINWAVE_DUMP_SHAPE=1`) makes the
//! live probe print skeletons of the `browse` home + explore responses via
//! [`dump_shape`] (renderer type names + key names, PLUS short human-label
//! values for `title` / `text` / `label` / `subtitle` / `simpleText` AND short
//! UI-chrome values for `iconType` / `iconImage` / `tooltipText` — each
//! truncated to 80 chars and single-lined — so a live re-probe reveals the
//! ACTUAL topbar icon type on both front pages). Secret denylist
//! (case-insensitive substring on key names, subtree dropped entirely):
//! `cookie` / `authorization` / `token` / `sapisid` / `url` /
//! `streamingData` / `continuation` / `trackingParams` NEVER appear in dump
//! output — not as keys, not as values. The chrome allowlist is key-EXACT
//! (never a substring: `iconImageId` and friends stay tags) and still
//! secret-VALUE-filtered, so chrome constants are non-secret evidence while
//! everything else keeps the secret denylist for keys AND values. Label
//! values containing a denylisted substring are replaced by a type tag, and
//! all other scalars stay tags, so secret bytes, media URLs, and pagination
//! tokens cannot leak even when the input carries them. No network in tests;
//! dump is offline + unit-tested with secret-bearing fixtures (labels and
//! chrome values survive, secrets do not).
//!
//! Live probe #3 shapes (CONFIRMED Premium): home tab = 2
//! `musicCarouselShelfRenderer` + `musicTastebuilderShelfRenderer`; explore
//! tab = `gridRenderer` + carousels. The old keys-only dump collapsed exactly
//! where human labels live, so round 3 keeps short label values alongside
//! keys plus a separate `label_hits` line (see [`collect_label_hits`]).
//!
//! ## Human paste-back needed
//!
//! 1. Sign in with the in-app browser window (or the cookie fallback), then
//!    run the binary with `SPINWAVE_DUMP_SHAPE=1` and paste back the FULL
//!    stderr skeleton lines (`shape home: ...` + `shape explore: ...` +
//!    `audio: ...` + `topbar_shape: ...` + `altclient: ...`).
//! 2. Which account tier the run used (Premium vs free) + the one-line probe
//!    report (`gate=... via=badge|explore|topbar|audio|none logged_in=... home_ok=...
//!    badge=... explore=... topbar=... audio=... ads=... playability=...`).
//!
//! With the skeleton we pin exact badge/Top-Songs JSON pointers; no secret
//! bytes are ever needed (cookie/authorization/token/sapisid/url/streamingData
//! are already stripped — do NOT paste raw responses).
//!
//! Handover notes:
//! - Model: after login we probe `browse` `FEmusic_home` ([`HOME_BROWSE_ID`]) +
//!   the premium badge + `player` `playabilityStatus`, then fail CLOSED: any
//!   ambiguous signal yields [`GateStatus::UpgradeRequired`], never playback.
//! - This gate runs BEFORE any playback-URL fetch: P2 playback code must call
//!   [`PremiumGate::can_fetch_playback_url`] first (no playback code exists in
//!   P1 — this module only exposes the boolean API plus unit-testable states).
//! - Legal: the live probe reads `playabilityStatus.status`/`reason` plus the
//!   PRESENCE of ad-marker key names ([`PLAYER_AD_KEYS`] — names only, values
//!   never read) ONLY. The `player` response also carries `streamingData`
//!   (media URLs) and ad payloads; we never extract, log, persist, or export
//!   them — the values are dropped with the response. No download/persist/
//!   export bytes paths, no ad playback/insertion logic, anywhere.
//! - Recheck: the live probe runs on every launch and sign-in.
//!
//! Open items: pin the exact badge JSON paths against live responses; pick a
//! stable premium-only probe track; split track-level vs account-level blocks;
//! fill account info from `get_account_info`.
// Badge structure verified offline against ytmusic-rs-0.1.1 (see
// `BADGE_CANDIDATE_POINTERS`); residual live step: confirm a Premium home
// response carries a Premium-labeled inline badge AND a free-account home does
// not (plus pick a premium-only probe track — `PROBE_VIDEO_ID`).

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// `browse` id of the YouTube Music home tab (login probe target).
pub const HOME_BROWSE_ID: &str = "FEmusic_home";

/// `browse` id of the YouTube Music explore tab (behavioral Top Songs probe).
pub const EXPLORE_BROWSE_ID: &str = "FEmusic_explore";

/// Env flag enabling the redacted shape-dump evidence path: when set to `1`,
/// the live probe prints KEYS-ONLY skeletons of the home + explore `browse`
/// responses via [`dump_shape`] (see module docs for the paste-back).
pub const SHAPE_DUMP_ENV: &str = "SPINWAVE_DUMP_SHAPE";

/// Upper bound for the whole live probe (fail-closed on timeout).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Probe track (placeholder). Any valid video id exercises
/// `player.playabilityStatus`; the gate only reads `status`/`reason`.
/// FINDING (probe #1 + vendored `get_song`, browsing.rs:542-573): `get_song`
/// keeps `playabilityStatus` in its filtered response, but status `OK` on an
/// arbitrary track is NON-DISCRIMINATING — free tracks return `OK` for every
/// account tier, so `Playable` here carries zero premium information (probe #1:
/// `Playable` on a confirmed Premium account told us nothing new). A
/// discriminating probe track must be a KNOWN premium-only videoId: `OK` iff
/// the account has Premium, `LOGIN_REQUIRED` + premium reason otherwise. No
/// verified premium-only id is obtainable offline, so the id stays unchanged
/// until a live run verifies one against Premium AND free accounts.
// A verified premium-only track (OK-on-Premium, LOGIN_REQUIRED+premium-reason
// on free) would let this leg gate; until then it stays evidence-only.
pub const PROBE_VIDEO_ID: &str = "4Tr0otuiQuU";

/// Candidate premium-badge JSON pointers, checked first by
/// [`parse_premium_badge`] (fast path).
///
/// VERIFIED (offline) against ytmusic-rs-0.1.1 `navigation.rs::BADGE_LABEL` /
/// `SUBTITLE_BADGE_LABEL` (src/navigation.rs:202-216): InnerTube badges live in
/// `badges[]` / `subtitleBadges[]` as `musicInlineBadgeRenderer` nodes whose
/// accessibility label names the badge. The crate reads these paths for
/// Explicit/live detection (parsers/search.rs:418,434;
/// parsers/playlists.rs:402; parsers/browsing.rs:200,306;
/// parsers/albums.rs:34). A hit counts as premium ONLY when the label names
/// Premium — any other label (e.g. `Explicit`) is not a signal.
///
/// There is NO `premiumBadge` boolean key and NO `musicPremiumBadgeRenderer`
/// node anywhere in the vendored shapes (full-crate grep: zero hits), and live
/// probe #1's (old, exhaustive) walk found neither at ANY depth of a real
/// Premium home response — so the previous pointers matched a schema that does
/// not exist. Bare presence matching is also a fail-OPEN risk, hence the
/// strict label check. Unknown shapes yield `None` (fail-closed), never panic.
///
/// Residual live risk (needs human probe): whether a Premium home response
/// actually carries a Premium-labeled inline badge, and whether a free-account
/// home could carry one (upsell) — P2 must check both, plus the behavioral
/// premium signals the crate documents (`FEmusic_explore` Top Songs only for
/// premium, mixins/explore.rs:146-149 + tests/smoke.rs:168; `FEmusic_charts`
/// daily/weekly carousels replacing "videos", mixins/explore.rs:337).
pub const BADGE_CANDIDATE_POINTERS: &[&str] = &[
    "/badges/0/musicInlineBadgeRenderer/accessibilityData/accessibilityData/label",
    "/subtitleBadges/0/musicInlineBadgeRenderer/accessibilityData/accessibilityData/label",
];

/// Gate outcome. Fail-closed: only [`GateStatus::Premium`] permits fetching a
/// playback URL; every other state blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GateStatus {
    /// Logged out (or restored session invalid) — blocked.
    #[default]
    LoggedOut,
    /// Authenticated premium account — the only state that allows playback.
    Premium,
    /// Authenticated but non-premium, OR any ambiguous probe signal
    /// (probe failure, missing badge, unknown playability) — blocked with an
    /// upgrade prompt (see `upgrade_message`).
    UpgradeRequired,
}

/// `player.playabilityStatus` reading for the probe track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Playability {
    /// `status == "OK"` — streamable.
    Playable,
    /// Non-OK status whose reason names Premium (account lacks Premium).
    PremiumOnly,
    /// Non-OK status for other reasons (region, removed, ...).
    Unplayable,
    /// Missing/unparseable status — ambiguous, blocks fail-closed.
    #[default]
    Unknown,
}

/// Raw probe inputs. Constructed by the live probe ([`PremiumGate::probe`]) or
/// by hand in tests — `evaluate` itself never touches the network.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PremiumSignals {
    /// Session logged in (`YTMusic::is_authenticated`).
    pub logged_in: bool,
    /// `browse FEmusic_home` round-trip succeeded.
    pub home_ok: bool,
    /// Premium badge seen on the home response. `None` = badge absent or
    /// unparseable = ambiguous (fail-closed).
    pub premium_badge: Option<bool>,
    /// `browse FEmusic_explore` Top Songs presence (behavioral premium
    /// evidence — see [`parse_explore_top_songs`]). `Some(true)` = `VLPL*`
    /// carousel header seen; `Some(false)` = explore ok but Top Songs absent;
    /// `None` = explore probe failed/unparseable = ambiguous (fail-closed).
    pub explore_top_songs: Option<bool>,
    /// `player` ad-marker absence (see [`parse_player_ads_absent`]).
    /// DEMOTED evidence-only leg: still collected + reported (`ads=...`,
    /// `ads:` dump line) but EXCLUDED from the Premium decision until a
    /// poToken exists — ad-marker presence is a tier-independent bot-guard
    /// fallback (no poToken), not tier evidence (see module docs).
    /// [`evaluate`] and [`deciding_leg`] ignore this field entirely.
    #[serde(default)]
    pub ads_absent: Option<bool>,
    /// Authed front-page topbar logo (see [`parse_topbar_premium`]).
    /// `Some(true)` = `iconImage.iconType` (or legacy direct `iconType`)
    /// `== "YOUTUBE_PREMIUM_LOGO"` or `"premium"` in `tooltipText`
    /// `Some(false)` = topbar logo parsed and not premium (free-shape);
    /// `None` = topbar missing/unparseable = ambiguous (fail-closed).
    #[serde(default)]
    pub topbar_premium: Option<bool>,
    /// High-audio capability (see [`parse_hires_audio`]): the probe `player`
    /// request asks for HIGH quality (see
    /// [`crate::auth::http::AUDIO_QUALITY_HIGH`]) and this reads the
    /// returned ladder. `Some(true)` = premium-only audio present (audio
    /// itag 141 and/or max audio bitrate ≥ [`HIRES_MIN_BITRATE_BPS`]);
    /// `Some(false)` = audio ladder parsed but capped below the threshold
    /// (free-shape); `None` = no audio evidence = ambiguous (fail-closed).
    #[serde(default)]
    pub hires_audio: Option<bool>,
    /// `player.playabilityStatus` reading for the probe track.
    pub playability: Playability,
}

impl PremiumSignals {
    /// All-absent signals for a logged-out session (blocked).
    pub fn logged_out() -> Self {
        Self::default()
    }
}

/// Pure, unit-testable gate decision: fail-closed over [`PremiumSignals`].
pub fn evaluate(signals: &PremiumSignals) -> GateStatus {
    if !signals.logged_in {
        return GateStatus::LoggedOut;
    }
    // Probe transport failure tells us nothing — ambiguous, block.
    if !signals.home_ok {
        return GateStatus::UpgradeRequired;
    }
    // Account-level evidence is an OR: strict Premium badge OR behavioral Top
    // Songs presence OR authed front-page topbar Premium logo OR high-audio
    // capability. Missing (`None`) or explicit absent (`Some(false)`) on ALL
    // FOUR legs blocks fail-closed (probe #2: Premium home carries no
    // Premium-named inline badge, so the badge leg alone stays unknown; the
    // topbar + hires legs are the yt-dlp-verified primaries). `ads_absent`
    // is DEMOTED evidence-only and never participates here
    // (tier-independent bot-guard fallback without poToken — see module
    // docs).
    let badge_hit = signals.premium_badge == Some(true);
    let behavioral_hit = signals.explore_top_songs == Some(true);
    let topbar_hit = signals.topbar_premium == Some(true);
    let hires_hit = signals.hires_audio == Some(true);
    if !badge_hit && !behavioral_hit && !topbar_hit && !hires_hit {
        return GateStatus::UpgradeRequired;
    }
    // Only a clean `OK` playability passes; PremiumOnly / Unplayable / Unknown
    // all block (ambiguous or not premium).
    if signals.playability != Playability::Playable {
        return GateStatus::UpgradeRequired;
    }
    GateStatus::Premium
}

/// Extract the premium-badge signal from a raw `browse` (`FEmusic_home`)
/// response. Returns `None` when the badge is absent or unparseable
/// (ambiguous → [`GateStatus::UpgradeRequired`] via [`evaluate`]).
///
/// Defensive: never panics on unknown shapes — non-object/array nodes,
/// nulls, and unexpected scalars all fold to `None` (fail-closed).
/// Checks [`BADGE_CANDIDATE_POINTERS`] first, then falls back to a recursive
/// walk over the verified badge structure (`badges[]` / `subtitleBadges[]`
/// holding `musicInlineBadgeRenderer` nodes — ytmusic-rs-0.1.1
/// `navigation.rs::BADGE_LABEL`, src/navigation.rs:202-216). ONLY a label
/// naming Premium counts (`Some(true)`): Explicit/live/other labels are not
/// premium signals and scanning continues past them. Titles, subtitles, and
/// descriptions are NEVER matched (a free-tier "Get Premium" upsell shelf must
/// not read as a premium badge — fail-open risk).
pub fn parse_premium_badge(home_json: &serde_json::Value) -> Option<bool> {
    // Fast path: verified renderer-label pointers (P2-verified live shape TBD).
    for ptr in BADGE_CANDIDATE_POINTERS {
        if let Some(label) = home_json.pointer(ptr).and_then(serde_json::Value::as_str) {
            if is_premium_label(label) {
                return Some(true);
            }
        }
    }
    fn walk(node: &serde_json::Value) -> Option<bool> {
        match node {
            serde_json::Value::Object(map) => {
                if let Some(renderer) = map.get("musicInlineBadgeRenderer") {
                    // Verified nesting per BADGE_LABEL
                    // (`.../accessibilityData/accessibilityData/label`); fall
                    // back to a direct `label` member on the renderer only.
                    let label = renderer
                        .pointer("/accessibilityData/accessibilityData/label")
                        .or_else(|| renderer.get("label"));
                    if label.is_some_and(is_premium_label_value) {
                        return Some(true);
                    }
                    // Non-premium badge (e.g. Explicit): not a signal — keep
                    // scanning siblings and children, do NOT return.
                }
                for v in map.values() {
                    if let Some(hit) = walk(v) {
                        return Some(hit);
                    }
                }
                None
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    if let Some(hit) = walk(v) {
                        return Some(hit);
                    }
                }
                None
            }
            _ => None,
        }
    }
    walk(home_json)
}

/// True when an accessibility label names a Premium badge
/// (case-insensitive substring; e.g. `Premium`, `Music Premium`).
fn is_premium_label(label: &str) -> bool {
    label.to_ascii_lowercase().contains("premium")
}

fn is_premium_label_value(v: &serde_json::Value) -> bool {
    v.as_str().is_some_and(is_premium_label)
}

/// `iconType` value the authed front-page topbar logo carries on a Premium
/// account (yt-dlp web initial-data pattern, `_video.py:3899-3908`: strict
/// equality against this constant). CORRECTED path (live ground truth):
/// `topbar.desktopTopbarRenderer.logo.topbarLogoRenderer.iconImage.iconType`
/// — the fetched youtube.com front page nests the icon type under
/// `iconImage`; the direct `iconType` member is kept as a back-compat OR.
pub const TOPBAR_PREMIUM_ICON: &str = "YOUTUBE_PREMIUM_LOGO";

/// Extract the topbar-logo signal from authed web front-page initial data
/// (`ytInitialData` — see [`crate::auth::http::fetch_front_page_initial_data`];
/// the authed InnerTube `browse FEmusic_home` response carries the same
/// `topbar` object, so the probe checks both).
///
/// Strict match on `iconType`/`tooltipText` ONLY at
/// `topbar.desktopTopbarRenderer.logo.topbarLogoRenderer`:
/// `Some(true)` when `iconImage.iconType == `[`TOPBAR_PREMIUM_ICON`]
/// (CORRECTED live path — the fetched front page nests the icon type under
/// `iconImage`) OR the legacy direct `iconType == `[`TOPBAR_PREMIUM_ICON`]
/// OR `"premium"` appears in `tooltipText` (case-insensitive; `tooltipText`
/// may be a plain string or a `{simpleText}` / `{runs[].text}` object).
/// `Some(false)` when the logo renderer parses and NONE of the signals fire
/// (free-shape: a concrete non-premium logo was seen). `None` when the
/// topbar/logo path is missing or unparseable (ambiguous → fail-closed).
///
/// Other keys (`iconImageId`, titles, ...) NEVER count, and the walk never
/// panics on unknown shapes (non-objects, nulls, wrong scalar types all
/// fold fail-closed).
pub fn parse_topbar_premium(initial_data: &serde_json::Value) -> Option<bool> {
    let logo = initial_data.pointer("/topbar/desktopTopbarRenderer/logo/topbarLogoRenderer")?;
    let renderer = logo.as_object()?;
    let icon = renderer.get("iconType").and_then(serde_json::Value::as_str);
    if icon == Some(TOPBAR_PREMIUM_ICON) {
        return Some(true);
    }
    // CORRECTED live path: `iconImage.iconType` (the fetched youtube.com
    // front page nests the icon type here — a direct-`iconType`-only check
    // misreads every such Premium topbar as free-shape).
    let nested_icon = renderer
        .get("iconImage")
        .and_then(|v| v.as_object())
        .and_then(|m| m.get("iconType"))
        .and_then(serde_json::Value::as_str);
    if nested_icon == Some(TOPBAR_PREMIUM_ICON) {
        return Some(true);
    }
    let tooltip = topbar_tooltip_text(renderer.get("tooltipText"));
    if tooltip.as_deref().is_some_and(is_premium_label) {
        return Some(true);
    }
    // A concrete logo was seen (an icon type and/or tooltip present on either
    // path) but nothing names Premium → free-shape. An empty/unreadable
    // renderer stays ambiguous (fail-closed).
    if icon.is_some() || nested_icon.is_some() || tooltip.is_some() {
        return Some(false);
    }
    None
}

/// Read logo `tooltipText` in its tolerated forms: plain string (the live
/// shape), `{simpleText}`, or `{runs[{text}]}` joined. `None` when absent
/// or the wrong type (ambiguous, never a hit). Values are short UI labels,
/// never secrets.
fn topbar_tooltip_text(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        serde_json::Value::Object(map) => {
            if let Some(simple) = map.get("simpleText").and_then(serde_json::Value::as_str) {
                let trimmed = simple.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            let runs = map.get("runs")?.as_array()?;
            let joined: String = runs
                .iter()
                .filter_map(|run| run.get("text").and_then(serde_json::Value::as_str))
                .collect();
            let trimmed = joined.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        _ => None,
    }
}

/// Prove one fetched front page is really logged in (per-URL auth evidence).
///
/// `true` ONLY when logged-in indicators are present AND the logged-out
/// marker is absent:
/// - account evidence: a topbar `avatar` node (live: the signed-in account
///   photo) or an `accountName` / `accountPhoto` / `email` label carrying a
///   non-empty string (live: the account-menu header name);
/// - logged-out veto: NO string value anywhere equal to `Sign in`
///   (case-insensitive — the logged-out topbar's sign-in button). A page
///   carrying both (crafted fixture) reads logged-out.
///
/// Anything else — missing topbar, no avatar/account name, mangled shapes —
/// reads `false` (fail-closed: the caller maps it to "unknown", never to a
/// topbar fail). Pure + offline, never panics. Secret-safe: returns a bool
/// only, no values leave this path.
pub fn parse_front_page_authed(initial_data: &serde_json::Value) -> bool {
    fn has_nonempty_string(value: &serde_json::Value, budget: &mut usize) -> bool {
        if *budget == 0 {
            return false;
        }
        *budget = budget.saturating_sub(1);
        match value {
            serde_json::Value::String(s) => !s.trim().is_empty(),
            serde_json::Value::Object(map) => {
                map.values().any(|v| has_nonempty_string(v, budget))
            }
            serde_json::Value::Array(items) => {
                items.iter().any(|v| has_nonempty_string(v, budget))
            }
            _ => false,
        }
    }
    fn walk(node: &serde_json::Value, account_hit: &mut bool, signin_hit: &mut bool, budget: &mut usize) {
        if *budget == 0 || (*account_hit && *signin_hit) {
            return;
        }
        *budget = budget.saturating_sub(1);
        match node {
            serde_json::Value::String(s) => {
                if s.trim().eq_ignore_ascii_case("sign in") {
                    *signin_hit = true;
                }
            }
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    let lower = k.to_ascii_lowercase();
                    if lower == "avatar" && !v.is_null() {
                        let mut inner = 64usize;
                        if has_nonempty_string(v, &mut inner) {
                            *account_hit = true;
                        } else if v.is_object() {
                            // Avatar node present but empty (unknown shape):
                            // NOT evidence — keep scanning, do not count.
                        }
                    } else if (lower == "accountname"
                        || lower == "accountphoto"
                        || lower == "email")
                        && !v.is_null()
                    {
                        let mut inner = 64usize;
                        if has_nonempty_string(v, &mut inner) {
                            *account_hit = true;
                        }
                    }
                    walk(v, account_hit, signin_hit, budget);
                    if *account_hit && *signin_hit {
                        return;
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    walk(item, account_hit, signin_hit, budget);
                    if *account_hit && *signin_hit {
                        return;
                    }
                }
            }
            _ => {}
        }
    }
    let mut account_hit = false;
    let mut signin_hit = false;
    let mut budget: usize = 20_000;
    walk(initial_data, &mut account_hit, &mut signin_hit, &mut budget);
    account_hit && !signin_hit
}

/// Read the per-page topbar icon value for the evidence path: the CORRECTED
/// `iconImage.iconType` first (the fetched youtube.com front page nests the
/// icon type here), falling back to the legacy direct `iconType`. `None`
/// when neither parses (missing/mangled — reported as `missing`).
/// Pure + offline, never panics. Short UI-chrome constant only, never a
/// secret.
pub fn topbar_icon_type(initial_data: &serde_json::Value) -> Option<String> {
    let renderer = initial_data
        .pointer("/topbar/desktopTopbarRenderer/logo/topbarLogoRenderer")?;
    let renderer = renderer.as_object()?;
    if let Some(nested) = renderer
        .get("iconImage")
        .and_then(|v| v.as_object())
        .and_then(|m| m.get("iconType"))
        .and_then(serde_json::Value::as_str)
    {
        if !nested.is_empty() {
            return Some(nested.to_string());
        }
    }
    renderer
        .get("iconType")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Gated topbar verdict for ONE fetched front page: `None` (unknown, never
/// fail) when the page is not proven logged in (see
/// [`parse_front_page_authed`]); otherwise [`parse_topbar_premium`]. A
/// logged-out fetch therefore NEVER yields `Some(false)` — the old shape
/// read every logged-out `YOUTUBE_LOGO` as free-shape fail. Pure + offline.
pub fn parse_topbar_premium_if_authed(initial_data: &serde_json::Value) -> Option<bool> {
    if !parse_front_page_authed(initial_data) {
        return None;
    }
    parse_topbar_premium(initial_data)
}

/// Render the per-URL auth evidence line for the dump path:
/// `front_auth: music=<true|false> youtube=<true|false>` (`false` covers
/// both logged-out AND unfetched legs — fail-closed either way). Bools only.
pub fn render_front_auth_line(music_authed: bool, youtube_authed: bool) -> String {
    format!("front_auth: music={music_authed} youtube={youtube_authed}")
}

/// Render the per-page icon evidence line for the dump path:
/// `front_icons: music=<icon|missing> youtube=<icon|missing>` (see
/// [`topbar_icon_type`] — the CORRECTED `iconImage.iconType` value per
/// page). Chrome constants only, never secrets.
pub fn render_front_icons_line(music_icon: Option<&str>, youtube_icon: Option<&str>) -> String {
    format!(
        "front_icons: music={} youtube={}",
        music_icon.unwrap_or("missing"),
        youtube_icon.unwrap_or("missing"),
    )
}

/// OR-fold two premium legs: `Some(true)` wins; otherwise the first known
/// value wins (`Some(false)` over `None`). Used to combine the browse-topbar
/// read with each front-page read — any leg seeing the Premium logo carries
/// the decision, while a parsed free-shape still beats "unknown".
fn or_premium(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), _) => Some(false),
        (None, b) => b,
    }
}

/// Audio `itag` whose presence reads as the HIGH ladder honored (premium-only
/// audio marker per the task's yt-dlp ref: itag 141 ≈ 256k AAC, the top of
/// the music audio ladder; claimed `verified:false` until the live probe
/// confirms the split on Premium AND free accounts).
pub const HIRES_AUDIO_ITAG: i64 = 141;

/// Minimum max-audio-bitrate (bits/s) that reads as HIGH honored. Sits
/// between the capped free ladder (itag 140 ≈ 128k AAC; Opus 251/250/249 at
/// ≤160k) and hires (itag 141 ≈ 256k AAC), so a capped ladder can never
/// reach it while a honored HIGH ask clears it. Task refs (claimed
/// `verified:false`): yt-dlp itag-141 256k note, mopidy-ytmusic
/// AUDIO_QUALITY_HIGH preference, ytmusicapi bitrate docstring. The live
/// Premium-vs-free probe is the confirming measurement.
pub const HIRES_MIN_BITRATE_BPS: u64 = 200_000;

/// Parse an `itag`-/`bitrate`-shaped JSON scalar (InnerTube serves both
/// numbers and numeric strings). `None` when absent or unparseable — the
/// caller treats that as "no evidence", never as zero.
fn as_i64_number(value: Option<&serde_json::Value>) -> Option<i64> {
    let v = value?;
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    if let Some(n) = v.as_u64() {
        return i64::try_from(n).ok();
    }
    v.as_str()?.trim().parse::<i64>().ok()
}

/// Same as [`as_i64_number`], for unsigned bitrates.
fn as_u64_number(value: Option<&serde_json::Value>) -> Option<u64> {
    let v = value?;
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    if let Some(n) = v.as_i64() {
        return u64::try_from(n).ok();
    }
    v.as_str()?.trim().parse::<u64>().ok()
}

/// Extract the high-audio capability signal from a RAW `player` response
/// (same response [`parse_player_ads_absent`] reads — the request asks for
/// HIGH quality, see [`crate::auth::http::AUDIO_QUALITY_HIGH`]).
///
/// Reads `streamingData.adaptiveFormats` ONLY, and only `mimeType` /
/// `itag` / `bitrate` NUMBERS (media URLs, ad payloads, tokens stay unread):
/// an entry counts as audio evidence only when its `mimeType` names audio
/// (case-insensitive — video-only bitrates must not move the needle).
/// `Some(true)` when an audio entry carries [`HIRES_AUDIO_ITAG`] or the max
/// audio bitrate reaches [`HIRES_MIN_BITRATE_BPS`] (HIGH honored);
/// `Some(false)` when an audio ladder parses but stays below the threshold
/// (capped free-shape, incl. entries with missing/unparseable bitrates —
/// absence of high-bitrate evidence is not high-bitrate evidence);
/// `None` when there is no audio evidence at all (missing/unparseable
/// `streamingData`/`adaptiveFormats`, non-audio-only ladder, or mangled
/// shapes — ambiguous → fail-closed).
///
/// Never panics on unknown shapes.
pub fn parse_hires_audio(player_json: &serde_json::Value) -> Option<bool> {
    let formats = player_json
        .pointer("/streamingData/adaptiveFormats")?
        .as_array()?;
    let mut saw_audio = false;
    let mut max_bitrate: u64 = 0;
    for entry in formats.iter().take(128) {
        let mime = entry
            .get("mimeType")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !mime.to_ascii_lowercase().contains("audio") {
            continue;
        }
        saw_audio = true;
        if as_i64_number(entry.get("itag")) == Some(HIRES_AUDIO_ITAG) {
            return Some(true);
        }
        max_bitrate = max_bitrate.max(as_u64_number(entry.get("bitrate")).unwrap_or(0));
    }
    if !saw_audio {
        return None;
    }
    if max_bitrate >= HIRES_MIN_BITRATE_BPS {
        Some(true)
    } else {
        Some(false)
    }
}

/// Collect the audio ladder for the evidence path: `(itag, bitrate)` per
/// audio-`mimeType` entry of `streamingData.adaptiveFormats`, sorted by
/// itag and deduped. NUMBERS ONLY — `mimeType` strings, media URLs, and ad
/// payloads never enter the output, so the `audio:` dump line cannot leak
/// secrets. Entries without a parseable `itag` are skipped; missing /
/// unparseable bitrates read as `0`. Never panics on unknown shapes
/// (missing/null/scalar roots yield an empty vec).
pub fn collect_audio_formats(player_json: &serde_json::Value) -> Vec<(i64, u64)> {
    let mut out = Vec::new();
    let Some(formats) = player_json
        .pointer("/streamingData/adaptiveFormats")
        .and_then(serde_json::Value::as_array)
    else {
        return out;
    };
    for entry in formats.iter().take(128) {
        let mime = entry
            .get("mimeType")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !mime.to_ascii_lowercase().contains("audio") {
            continue;
        }
        if let Some(itag) = as_i64_number(entry.get("itag")) {
            let bitrate = as_u64_number(entry.get("bitrate")).unwrap_or(0);
            if !out.contains(&(itag, bitrate)) {
                out.push((itag, bitrate));
            }
        }
    }
    out.sort();
    out
}

/// Render the `audio:` evidence line for the dump path: the audio ladder as
/// `itag@bitrate` pairs (`audio: unknown` when the ladder is absent).
/// Numbers only — the pairs come from [`collect_audio_formats`], so no
/// URLs, tokens, or payload bytes can appear.
pub fn render_audio_line(player_json: Option<&serde_json::Value>) -> String {
    match player_json {
        Some(json) => {
            let formats = collect_audio_formats(json);
            if formats.is_empty() {
                "audio: unknown".to_string()
            } else {
                let pairs: Vec<String> =
                    formats.iter().map(|(itag, bitrate)| format!("{itag}@{bitrate}")).collect();
                format!("audio: [{}]", pairs.join(","))
            }
        }
        None => "audio: unknown".to_string(),
    }
}

/// Render the `altclient:` evidence line for the dump path: the
/// alternate-client experiment comparison (see
/// [`crate::auth::http::alt_client_player_response`]) — client name/version
/// plus the alt response's ad-marker key NAMES ([`collect_player_ad_keys`])
/// and alt audio ladder (`itag@bitrate` via [`collect_audio_formats`]) —
/// printed beside the default `ads:`/`audio:` lines so a live re-probe shows
/// whether the mobile client context changes ad presence or the ladder cap.
/// `altclient: NAME/VERSION unknown` when the alt leg failed (honest
/// experiment record — error, bot-guard, and match are NOT distinguished
/// from "no evidence" here, and the line NEVER gates).
/// Names + numbers only — no URLs, tokens, or payload bytes can appear.
pub fn render_altclient_line(
    client_name: &str,
    client_version: &str,
    alt_player_json: Option<&serde_json::Value>,
) -> String {
    match alt_player_json {
        Some(json) => {
            let ads = collect_player_ad_keys(json);
            let formats = collect_audio_formats(json);
            let audio = if formats.is_empty() {
                "unknown".to_string()
            } else {
                let pairs: Vec<String> =
                    formats.iter().map(|(itag, bitrate)| format!("{itag}@{bitrate}")).collect();
                format!("[{}]", pairs.join(","))
            };
            format!("altclient: {client_name}/{client_version} ads={ads:?} audio={audio}")
        }
        None => format!("altclient: {client_name}/{client_version} unknown"),
    }
}

/// Skeleton of the fetched topbar subtree for the evidence path: the first
/// available `/topbar` object (front-page reads first, then the browse-home
/// read — both carry the same `desktopTopbarRenderer.logo` object) rendered
/// via [`dump_shape`], i.e. keys plus short human labels AND short chrome
/// values (`iconType` / `tooltipText` values print — ground truth for the
/// ACTUAL icon type on each front page) under the SAME secret denylist
/// [`collect_label_hits`] uses. `topbar_shape: unknown` when no topbar
/// object is present (fail-closed upstream).
pub fn dump_topbar_shape(home: &serde_json::Value, pages: &[serde_json::Value]) -> String {
    for candidate in pages.iter().chain(std::iter::once(home)) {
        if let Some(topbar) = candidate.pointer("/topbar") {
            if topbar.is_object() {
                return format!("topbar_shape: {}", dump_shape(topbar));
            }
        }
    }
    "topbar_shape: unknown".to_string()
}

/// Extract [`Playability`] from a `player` response (raw probe or `get_song`
/// — both preserve `playabilityStatus`), reading
/// `playabilityStatus.status`/`reason` ONLY — `streamingData` (media URLs) is
/// never touched here.
pub fn parse_playability(player_json: &serde_json::Value) -> Playability {
    let status = player_json
        .pointer("/playabilityStatus/status")
        .and_then(serde_json::Value::as_str);
    match status {
        Some("OK") => Playability::Playable,
        Some(_) => {
            let reason = player_json
                .pointer("/playabilityStatus/reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if reason.contains("premium") {
                Playability::PremiumOnly
            } else {
                Playability::Unplayable
            }
        }
        // Missing status: ambiguous → Unknown (fail-closed in `evaluate`).
        None => Playability::Unknown,
    }
}

/// InnerTube `player` ad-marker key names checked by
/// [`parse_player_ads_absent`] / [`collect_player_ad_keys`].
///
/// Hypothesis (round 4; browse-level markers exhausted by live probe #3 on a
/// CONFIRMED Premium account): free accounts get ad markers in the `player`
/// response while Premium accounts get none, so ABSENCE of all three keys is
/// premium evidence. Exact key-name match only — values are never read (same
/// discipline as [`parse_playability`'s streamingData rule](parse_playability):
/// ad payload values, URLs, and tokens stay unread).
pub const PLAYER_AD_KEYS: &[&str] = &["adPlacements", "adBreaks", "playerAds"];

/// Collect the ad-marker key NAMES present anywhere in a raw `player`
/// response (subset of [`PLAYER_AD_KEYS`], sorted, deduped). Names only —
/// values are never read, logged, or returned, so ad payloads, media URLs,
/// and tokens cannot leak through this path (the output can only ever be a
/// subset of [`PLAYER_AD_KEYS`]). Feeds the `ads:` shape-dump line. Never
/// panics on unknown shapes (missing/null/scalar roots yield an empty vec).
pub fn collect_player_ad_keys(player_json: &serde_json::Value) -> Vec<String> {
    fn walk(node: &serde_json::Value, hits: &mut Vec<String>, budget: &mut usize) {
        if *budget == 0 {
            return;
        }
        *budget = budget.saturating_sub(1);
        match node {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if *budget == 0 {
                        return;
                    }
                    if PLAYER_AD_KEYS.contains(&k.as_str()) && !hits.contains(k) {
                        hits.push(k.clone());
                    }
                    walk(v, hits, budget);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    if *budget == 0 {
                        return;
                    }
                    walk(item, hits, budget);
                }
            }
            _ => {}
        }
    }

    let mut hits = Vec::new();
    let mut budget: usize = 20_000;
    walk(player_json, &mut hits, &mut budget);
    hits.sort();
    hits.dedup();
    hits
}

/// Keys `get_song` KEEPS when filtering the `player` response (vendored
/// ytmusic-rs-0.1.1 `mixins/browsing.rs:559-567`). Any object whose keys are
/// all within this set is provably `get_song`-shaped (or degenerate) — NOT a
/// raw response — so [`parse_player_ads_absent`] must fold it to `None`
/// (fail-closed). Only a response carrying at least one key OUTSIDE this set
/// (e.g. `responseContext`, or an ad-marker key itself) proves it is raw.
pub const GET_SONG_KEPT_KEYS: &[&str] = &[
    "videoDetails",
    "playabilityStatus",
    "streamingData",
    "microformat",
    "playbackTracking",
];

/// True when a `player` JSON object is `get_song`-shaped (every key within
/// [`GET_SONG_KEPT_KEYS`]) rather than raw. Never panics (non-objects read
/// as filtered — they carry no raw evidence either way).
pub fn is_filtered_player_shape(player_json: &serde_json::Value) -> bool {
    match player_json.as_object() {
        Some(map) => map.keys().all(|k| GET_SONG_KEPT_KEYS.contains(&k.as_str())),
        None => true,
    }
}

/// Extract the player ad-marker signal from a RAW `player` response (see
/// [`crate::auth::http::raw_player_response`]): `Some(true)` when the RAW
/// probe succeeded and NONE of [`PLAYER_AD_KEYS`] appears anywhere (ads
/// absent — premium evidence); `Some(false)` when any ad-marker key is
/// present (free-shape — key presence counts regardless of value:
/// `null`/empty still reads as present, fail-closed); `None` when the shape
/// carries no RAW player evidence at all — non-object roots, empty objects,
/// or `get_song`-filtered dicts per [`is_filtered_player_shape`]
/// (ambiguous → fail-closed, so a filtered response can never read as "ads
/// absent": without this, the check is vacuous — see the module docs).
///
/// Never panics on unknown shapes.
pub fn parse_player_ads_absent(player_json: &serde_json::Value) -> Option<bool> {
    let map = player_json.as_object()?;
    if map.is_empty() {
        return None;
    }
    // Fail-closed on filtered shapes: a `get_song` dict can never carry ad
    // markers (stripped at browsing.rs:559-567), so absence there proves
    // nothing — only raw responses (extra keys beyond the kept five) count.
    if is_filtered_player_shape(player_json) {
        return None;
    }
    Some(collect_player_ad_keys(player_json).is_empty())
}

/// Extract the behavioral Top Songs signal from a raw `browse`
/// (`FEmusic_explore`) response.
///
/// Per vendored ytmusic-rs `mixins/explore.rs:145-149`, the Top Songs chart is
/// only returned for a premium account: `get_explore` inserts `top_songs`
/// exactly when a carousel header `browseId` starts with `VLPL` (a playlist
/// link), and `mixins/explore.rs:337` treats extra daily/weekly carousels as
/// the premium shape. This parser mirrors that check on the RAW browse JSON
/// (the live probe keeps the raw value so the shape-dump stays available):
/// `contents.singleColumnBrowseResultsRenderer.tabs[0].tabRenderer.content.
/// sectionListRenderer.contents[]` → each
/// `musicCarouselShelfRenderer.header.musicCarouselShelfBasicHeaderRenderer.
/// title.runs[0].navigationEndpoint.browseEndpoint.browseId` starting with
/// `VLPL` counts as `Some(true)`.
///
/// Returns `Some(false)` when the section list parses but no `VLPL*` header
/// is present (free-shape: explore ok, Top Songs absent) and `None` when the
/// shape is missing/unparseable (ambiguous → fail-closed). Title text alone
/// ("Top Songs") NEVER counts — only the machine-readable `VLPL*` header
/// does. Never panics on unknown shapes.
///
/// A filtered `get_explore` dict (with a top-level `top_songs` key) is also
/// accepted as `Some(true)` so the same predicate covers both raw + parsed
/// forms; a filtered dict with other explore keys but no `top_songs` reads as
/// `Some(false)`.
pub fn parse_explore_top_songs(explore_json: &serde_json::Value) -> Option<bool> {
    // Fast path: filtered `get_explore` dict form.
    if let Some(map) = explore_json.as_object() {
        if map.contains_key("top_songs") {
            return Some(true);
        }
        // Only treat as free-shape when it is clearly a filtered explore dict;
        // otherwise fall through to the raw browse check (which yields None on
        // truly unknown shapes).
        let filtered_keys = [
            "new_releases",
            "moods_and_genres",
            "top_episodes",
            "new_videos",
            "trending",
        ];
        if filtered_keys.iter().any(|k| map.contains_key(*k)) {
            return Some(false);
        }
    }
    let sections = explore_json
        .pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents")?
        .as_array()?;
    for section in sections {
        let browse_id = section.pointer(
            "/musicCarouselShelfRenderer/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/navigationEndpoint/browseEndpoint/browseId",
        );
        if browse_id
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| id.starts_with("VLPL"))
        {
            return Some(true);
        }
    }
    // Section list parsed, no Top Songs carousel → free-shape.
    Some(false)
}

/// Key substrings (lowercased) whose subtrees are NEVER included in
/// [`dump_shape`] output or [`collect_label_hits`] evidence — neither key
/// names nor values. Case-insensitive substring match, so
/// `__Secure-3PAPISID` / `SAPISIDHASH` / `thumbnail.url` / `streamingData` /
/// `continuation` / `clickTrackingParams` variants are all covered.
const SHAPE_SECRET_SUBSTRINGS: &[&str] = &[
    "cookie",
    "authorization",
    "token",
    "sapisid",
    "url",
    "streamingdata",
    "continuation",
    "trackingparams",
];

/// Raw `account/accounts_list` menu endpoint (account-item labels collected
/// for evidence plus the strict Premium check — see [`collect_label_hits`]).
pub const ACCOUNT_MENU_ENDPOINT: &str = "account/accounts_list";

/// Human-label keys whose SHORT string values are kept as evidence (dump +
/// [`collect_label_hits`]). Lowercase-compared; covers header titles,
/// run texts, accessibility labels, and account-menu labels.
const LABEL_VALUE_KEYS: &[&str] = &["title", "text", "label", "subtitle", "simpletext"];
/// UI-chrome keys whose SHORT string values are kept as evidence in
/// [`dump_shape`] ONLY (never in [`collect_label_hits`], never gated).
/// Lowercase-compared, EXACT key match (never a substring: `iconImageId`
/// stays a tag). Covers the topbar logo marker so a live re-probe reveals
/// the ACTUAL icon type on both front pages (`iconType` values print;
/// `iconImage` objects recurse and their inner `iconType` prints; plain
/// `tooltipText` strings print while `{simpleText}`/`runs` forms already
/// print via [`LABEL_VALUE_KEYS`]). These are UI-chrome constants,
/// non-secret; the secret denylist still applies to keys AND values first.
const CHROME_VALUE_KEYS: &[&str] = &["icontype", "tooltiptext", "iconimage"];
/// Max chars kept per label value (both dump and `label_hits`).
const MAX_LABEL_CHARS: usize = 80;
/// Max `label_hits` entries kept per probe (bounded walker).
const MAX_LABEL_HITS: usize = 50;
/// Max walker depth for label collection (live responses nest headers ~15
/// deep: contents/tabs/tabRenderer/content/sectionList/header/title/runs).
const LABEL_WALK_MAX_DEPTH: usize = 18;

/// True when a JSON key holds (or names) secret/URL material that must never
/// appear in shape-dump output.
fn is_secret_shape_key(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SHAPE_SECRET_SUBSTRINGS
        .iter()
        .any(|deny| lower.contains(deny))
}

/// True when a key carries a short human label worth keeping as evidence.
fn is_label_key(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    LABEL_VALUE_KEYS.iter().any(|k| lower == *k)
}

/// True when a key carries a short UI-chrome constant worth keeping in
/// [`dump_shape`] output (exact lowercase match — `iconImageId` does NOT
/// match `iconimage`). Secret keys/values are still filtered first.
fn is_chrome_key(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    CHROME_VALUE_KEYS.iter().any(|k| lower == *k)
}

/// True when a candidate label VALUE itself looks secret (denylisted
/// substring or URL marker) and must be dropped rather than emitted.
fn is_secret_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if SHAPE_SECRET_SUBSTRINGS
        .iter()
        .any(|deny| lower.contains(deny))
    {
        return true;
    }
    // URL markers: label evidence must never carry media/page URLs even when
    // they sit under a label key in a crafted fixture.
    ["http", "://", ".com", "www."].iter().any(|m| lower.contains(m))
}

/// Clean one candidate label: trim, single-line, drop empties and
/// secret-looking values, truncate to [`MAX_LABEL_CHARS`] chars.
fn clean_label_value(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || is_secret_value(trimmed) {
        return None;
    }
    let short: String = trimmed.chars().take(MAX_LABEL_CHARS).collect();
    let flat = short.replace(['\n', '\r', '\t'], " ");
    let flat = flat.trim().to_string();
    if flat.is_empty() {
        None
    } else {
        Some(flat)
    }
}

/// Escape one kept dump label so the skeleton stays single-line and
/// quote-safe (input already truncated to [`MAX_LABEL_CHARS`] chars).
fn escape_dump_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r', '\t'], " ")
}

/// True when the redacted shape-dump evidence path is enabled
/// (`SPINWAVE_DUMP_SHAPE=1`).
pub fn shape_dump_enabled() -> bool {
    std::env::var(SHAPE_DUMP_ENV).as_deref() == Ok("1")
}

/// Render a skeleton of an InnerTube response for the evidence path:
/// renderer type names + key names are kept (sorted for determinism), short
/// human-label values (`title` / `text` / `label` / `subtitle` /
/// `simpleText`, each truncated to 80 chars and shown as `key="value"`) AND
/// short UI-chrome values (`iconType` / `iconImage` / `tooltipText` — same
/// truncation, see [`CHROME_VALUE_KEYS`]) are kept alongside keys, every
/// other scalar is replaced by a type tag
/// (`<str>` / `<num>` / `<bool>` / `<null>` / `{...}` / `[...]`), arrays are
/// capped at 3 items with a `(+N more)` marker, and depth is capped (deeper
/// subtrees render as `{...}` / `[...]`).
///
/// Secret-safety: any key matching [`SHAPE_SECRET_SUBSTRINGS`]
/// (case-insensitive: `cookie` / `authorization` / `token` / `sapisid` /
/// `url` / `streamingData` / `continuation` / `trackingParams`) has its whole
/// subtree dropped — the key name itself never appears in the output — label
/// AND chrome values containing a denylisted substring are replaced by
/// `<str>`, and all other scalars stay tags, so secret bytes, media URLs,
/// and pagination tokens cannot leak even when the input carries them.
pub fn dump_shape(value: &serde_json::Value) -> String {
    fn tag_scalar(v: &serde_json::Value) -> &'static str {
        match v {
            serde_json::Value::Null => "<null>",
            serde_json::Value::Bool(_) => "<bool>",
            serde_json::Value::Number(_) => "<num>",
            serde_json::Value::String(_) => "<str>",
            serde_json::Value::Array(_) => "[...]",
            serde_json::Value::Object(_) => "{...}",
        }
    }

    fn walk(v: &serde_json::Value, depth: usize, out: &mut String) {
        const MAX_DEPTH: usize = 18;
        const MAX_ITEMS: usize = 3;
        if depth > MAX_DEPTH {
            out.push_str(tag_scalar(v));
            return;
        }
        match v {
            serde_json::Value::Object(map) => {
                // Sorted, secret-filtered keys for deterministic, safe output.
                let mut keys: Vec<&String> =
                    map.keys().filter(|k| !is_secret_shape_key(k)).collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(k);
                    out.push(':');
                    let child = &map[*k];
                    match child {
                        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                            walk(child, depth + 1, out);
                        }
                        serde_json::Value::String(s)
                            if (is_label_key(k) || is_chrome_key(k)) && !is_secret_value(s) =>
                        {
                            let short: String = s.chars().take(MAX_LABEL_CHARS).collect();
                            out.push_str(&format!("\"{}\"", escape_dump_label(&short)));
                        }
                        _ => out.push_str(tag_scalar(child)),
                    }
                }
                out.push('}');
            }
            serde_json::Value::Array(items) => {
                out.push('[');
                let shown = items.len().min(MAX_ITEMS);
                for (i, item) in items.iter().take(shown).enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    match item {
                        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                            walk(item, depth + 1, out);
                        }
                        _ => out.push_str(tag_scalar(item)),
                    }
                }
                if items.len() > shown {
                    if shown > 0 {
                        out.push(',');
                    }
                    out.push_str(&format!("(+{} more)", items.len() - shown));
                }
                out.push(']');
            }
            _ => out.push_str(tag_scalar(v)),
        }
    }

    let mut out = String::new();
    walk(value, 0, &mut out);
    out
}

/// Collect short human-label strings for the evidence path: badge-ish and
/// label-ish text across the home + explore + account-menu responses —
/// `badges[]` / `subtitleBadges[]` accessibility labels, header
/// titles/subtitles (`musicCarouselShelfBasicHeaderRenderer`,
/// `gridRenderer` headers), run `text`, and account-menu item labels.
///
/// Bounded (depth [`LABEL_WALK_MAX_DEPTH`], [`MAX_LABEL_HITS`] entries),
/// secret-subtree-skipping (any denylisted key drops its whole subtree),
/// denylist-filtered (values containing a denylisted substring are dropped),
/// single-lined and truncated to [`MAX_LABEL_CHARS`] chars each, deduped and
/// sorted for determinism. Never panics on unknown shapes (missing/null/
/// scalar roots yield an empty vec).
///
/// Fail-open guard: the collected strings are EVIDENCE ONLY. Titles, run
/// texts, and subtitles are never gated — only accessibility `label`
/// strings naming Premium count toward the gate (see
/// [`parse_premium_label`]), so a free-tier "Get Premium" upsell shelf shows
/// up in `label_hits` but never reads as premium.
pub fn collect_label_hits(
    home: &serde_json::Value,
    explore: &serde_json::Value,
    account: Option<&serde_json::Value>,
) -> Vec<String> {
    fn walk(node: &serde_json::Value, depth: usize, hits: &mut Vec<String>, budget: &mut usize) {
        if *budget == 0 || depth > LABEL_WALK_MAX_DEPTH || hits.len() >= MAX_LABEL_HITS {
            return;
        }
        *budget = budget.saturating_sub(1);
        match node {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if hits.len() >= MAX_LABEL_HITS || *budget == 0 {
                        return;
                    }
                    // Secret subtree: drop whole, never collect nor descend.
                    if is_secret_shape_key(k) {
                        continue;
                    }
                    if let serde_json::Value::String(s) = v {
                        if is_label_key(k) {
                            if let Some(clean) = clean_label_value(s) {
                                if !hits.contains(&clean) {
                                    hits.push(clean);
                                    if hits.len() >= MAX_LABEL_HITS {
                                        return;
                                    }
                                }
                            }
                            continue;
                        }
                    }
                    walk(v, depth + 1, hits, budget);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    if hits.len() >= MAX_LABEL_HITS || *budget == 0 {
                        return;
                    }
                    walk(item, depth + 1, hits, budget);
                }
            }
            _ => {}
        }
    }

    let mut hits = Vec::new();
    let mut budget: usize = 20_000;
    walk(home, 0, &mut hits, &mut budget);
    walk(explore, 0, &mut hits, &mut budget);
    if let Some(menu) = account {
        walk(menu, 0, &mut hits, &mut budget);
    }
    hits.sort();
    hits.dedup();
    hits.truncate(MAX_LABEL_HITS);
    hits
}

/// Strict Premium-label check over the deep label evidence: `Some(true)`
/// when any accessibility `label` string (home, explore, or account menu —
/// badge labels, header accessibility labels, account-item labels) names
/// Premium (same case-insensitive substring strictness as
/// [`parse_premium_badge`)). Anything else — run texts, header titles,
/// subtitles, descriptions (free-tier "Get Premium" upsell), absent or
/// mangled shapes — yields `None` (ambiguous, fail-closed). Never panics.
///
/// The live probe folds a `Some(true)` here into `premium_badge`, so
/// [`evaluate`] counts it exactly like a badge hit without changing the
/// [`PremiumSignals`] shape (other crates construct it literally).
pub fn parse_premium_label(
    home: &serde_json::Value,
    explore: &serde_json::Value,
    account: Option<&serde_json::Value>,
) -> Option<bool> {
    fn walk(node: &serde_json::Value, depth: usize, budget: &mut usize) -> Option<bool> {
        if *budget == 0 || depth > LABEL_WALK_MAX_DEPTH {
            return None;
        }
        *budget = budget.saturating_sub(1);
        match node {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if *budget == 0 {
                        return None;
                    }
                    if is_secret_shape_key(k) {
                        continue;
                    }
                    // Strict source: accessibility `label` strings ONLY.
                    // Header titles / run texts / subtitles are evidence
                    // (collected above) but never gate — upsell guard.
                    if k.eq_ignore_ascii_case("label") {
                        if let serde_json::Value::String(s) = v {
                            if !is_secret_value(s) && is_premium_label(s) {
                                return Some(true);
                            }
                            continue;
                        }
                    }
                    if let Some(hit) = walk(v, depth + 1, budget) {
                        return Some(hit);
                    }
                }
                None
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    if let Some(hit) = walk(item, depth + 1, budget) {
                        return Some(hit);
                    }
                }
                None
            }
            _ => None,
        }
    }

    let mut budget: usize = 20_000;
    for root in [home, explore].into_iter().chain(account) {
        if walk(root, 0, &mut budget) == Some(true) {
            return Some(true);
        }
    }
    None
}

/// Which account-level leg carries a Premium decision
/// (`badge` | `explore` | `topbar` | `audio`). `none` when the outcome is not Premium —
/// so `badge=unknown` no longer reads as failure when another leg carries
/// Premium. Follows [`evaluate`] exactly (fail-closed: blocked outcomes
/// never name a leg; the demoted `ads` leg never names one either). Pure,
/// unit-tested.
pub fn deciding_leg(signals: &PremiumSignals) -> &'static str {
    if evaluate(signals) != GateStatus::Premium {
        return "none";
    }
    if signals.premium_badge == Some(true) {
        "badge"
    } else if signals.explore_top_songs == Some(true) {
        "explore"
    } else if signals.topbar_premium == Some(true) {
        "topbar"
    } else if signals.hires_audio == Some(true) {
        "audio"
    } else {
        "none"
    }
}

/// Cached gate decision with recheck bookkeeping.
#[derive(Debug, Clone)]
pub struct PremiumGate {
    status: GateStatus,
    last_check: Option<SystemTime>,
}

impl Default for PremiumGate {
    fn default() -> Self {
        Self::new_logged_out()
    }
}

impl PremiumGate {
    /// Fresh gate: blocked until login + probe (fail-closed default).
    pub fn new_logged_out() -> Self {
        Self {
            status: GateStatus::LoggedOut,
            last_check: None,
        }
    }

    /// Current gate state (unit-testable without network).
    pub fn status(&self) -> GateStatus {
        self.status
    }

    /// THE gate: call before ANY playback-URL fetch. Only `Premium` proceeds.
    pub fn can_fetch_playback_url(&self) -> bool {
        self.status == GateStatus::Premium
    }

    /// Upgrade prompt when blocked for Premium reasons (`None` otherwise).
    pub fn upgrade_message(&self) -> Option<String> {
        match self.status {
            GateStatus::UpgradeRequired => Some(crate::legal::upgrade_message()),
            GateStatus::LoggedOut | GateStatus::Premium => None,
        }
    }

    /// Fold fresh probe signals into the cached decision.
    pub fn apply(&mut self, signals: &PremiumSignals, now: SystemTime) {
        self.status = evaluate(signals);
        self.last_check = Some(now);
    }

    /// Live probe that also returns the log-safe [`ProbeReport`]: gate
    /// status + which signals passed/failed + upgrade message when
    /// blocked. Single network round-trip (no double probe). The report
    /// carries no secret material.
    pub async fn probe_report(
        &mut self,
        yt: &ytmusicapi::YTMusic,
        probe_video_id: &str,
    ) -> ProbeReport {
        let now = SystemTime::now();
        let (signals, label_hits) = probe_signals(yt, probe_video_id).await;
        self.apply(&signals, now);
        let mut report = ProbeReport::from_signals(&signals, self);
        report.label_hits = label_hits;
        report
    }
}

/// Run the InnerTube probes and fold them into [`PremiumSignals`] plus the
/// secret-safe evidence [`collect_label_hits`].
/// Bounded by [`PROBE_TIMEOUT`]; every failure maps to an ambiguous signal.
async fn probe_signals(
    yt: &ytmusicapi::YTMusic,
    probe_video_id: &str,
) -> (PremiumSignals, Vec<String>) {
    if !yt.is_authenticated() {
        return (PremiumSignals::logged_out(), Vec::new());
    }
    let probe = async {
        // 1. browse FEmusic_home (raw JSON so the badge stays visible).
        let home = yt
            .debug_send_request(
                "browse",
                serde_json::json!({ "browseId": HOME_BROWSE_ID })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            )
            .await;
        // 2. browse FEmusic_explore (raw JSON so the Top Songs VLPL header
        // stays visible for the behavioral signal + shape-dump).
        let explore = yt
            .debug_send_request(
                "browse",
                serde_json::json!({ "browseId": EXPLORE_BROWSE_ID })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
            )
            .await;
        // 3. account menu (raw JSON so Premium item labels stay visible for
        // the deep label search; failure stays fail-closed and never blocks
        // the other legs).
        let account = yt
            .debug_send_request(ACCOUNT_MENU_ENDPOINT, serde_json::Map::new())
            .await;
        // 4. RAW player response for the probe track (UNFILTERED — see
        // `crate::auth::http::raw_player_response`): the SAME transport as
        // `get_song` without its 5-key strip, so ad-marker absence is sound.
        // `get_song` output must NEVER feed `ads_absent` (vacuous — its
        // strip removes the very keys the check looks for). Playability is
        // read from the same raw response (`playabilityStatus` survives in
        // both forms). `streamingData` is dropped unread with the value.
        let player = crate::auth::http::raw_player_response(yt, probe_video_id).await;
        // 5. Authed web front-page initial data for the topbar-logo leg
        // (same session cookies + fresh PER-URL SAPISIDHASH transport family
        // as the raw probe — see
        // `crate::auth::http::fetch_front_page_initial_data_by_url`; each
        // URL hashes its OWN origin, gated on `is_authenticated` inside,
        // `[]` when logged out or on ANY fetch/parse failure — the
        // browse-topbar read below still stands on its own).
        let topbar_pages = crate::auth::http::fetch_front_page_initial_data_by_url(yt).await;
        // 6. Alternate-client player experiment (evidence ONLY, never gates —
        // see `crate::auth::http::alt_client_player_response`): the same
        // probe track under the vendored mobile client context. `None` on
        // ANY failure (recorded as `altclient: ... unknown`).
        let alt_player =
            crate::auth::http::alt_client_player_response(yt, probe_video_id).await;
        (home, explore, account, player, topbar_pages, alt_player)
    };
    let (home, explore, account, player, topbar_pages, alt_player) =
        match tokio::time::timeout(PROBE_TIMEOUT, probe).await
    {
        Ok(sex) => sex,
        Err(_) => {
            return (
                PremiumSignals {
                    logged_in: true,
                    home_ok: false,
                    premium_badge: None,
                    explore_top_songs: None,
                    ads_absent: None,
                    topbar_premium: None,
                    hires_audio: None,
                    playability: Playability::Unknown,
                },
                Vec::new(),
            );
        }
    };
    let badge_from_home = match &home {
        Ok(json) => parse_premium_badge(json),
        Err(_) => None,
    };
    let null = serde_json::Value::Null;
    let (home_ok, premium_badge) = match &home {
        Ok(json) => {
            // Deep label leg (home + explore + account menu, strict
            // `label`-only): a Premium-naming hit folds into the badge leg
            // so `evaluate` counts it exactly like a badge hit.
            let explore_ref: &serde_json::Value = explore.as_ref().unwrap_or(&null);
            let account_ref: Option<&serde_json::Value> = account.as_ref().ok();
            let label_hit = parse_premium_label(json, explore_ref, account_ref);
            let folded = match (badge_from_home, label_hit) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (b, _) => b,
            };
            (true, folded)
        }
        Err(_) => (false, None),
    };
    let explore_top_songs = match &explore {
        Ok(json) => parse_explore_top_songs(json),
        Err(_) => None,
    };
    // Topbar-logo leg (primary): the authed InnerTube `browse FEmusic_home`
    // response carries the same `topbar` object the web front page embeds as
    // `ytInitialData`, so it is checked first (zero extra round-trips, same
    // transport as the raw probe); each fetched front page is OR-folded in
    // (`Some(true)` wins — any leg seeing the Premium logo carries Premium).
    // Front pages fold GATED (see `parse_topbar_premium_if_authed`): a page
    // not proven logged in contributes NOTHING (never `Some(false)`), so a
    // logged-out `YOUTUBE_LOGO` fetch reads unknown, never fail.
    let mut topbar_premium = match &home {
        Ok(json) => parse_topbar_premium(json),
        Err(_) => None,
    };
    let mut music_authed = false;
    let mut youtube_authed = false;
    let mut music_icon: Option<String> = None;
    let mut youtube_icon: Option<String> = None;
    for page in &topbar_pages {
        let authed = parse_front_page_authed(&page.data);
        let icon = topbar_icon_type(&page.data);
        if page.url.contains("music.youtube.com") {
            music_authed = authed;
            music_icon = icon;
        } else {
            youtube_authed = authed;
            youtube_icon = icon;
        }
        topbar_premium = or_premium(topbar_premium, parse_topbar_premium_if_authed(&page.data));
    }
    let topbar_page_values: Vec<serde_json::Value> =
        topbar_pages.iter().map(|page| page.data.clone()).collect();
    // Evidence: bounded, denylist-filtered label hits across the available
    // raw responses (failed legs contribute nothing).
    let label_hits = collect_label_hits(
        home.as_ref().unwrap_or(&null),
        explore.as_ref().unwrap_or(&null),
        account.as_ref().ok(),
    );
    // Evidence path: env-gated skeletons (secret-safe by construction in
    // `dump_shape`; never raw values) plus the filtered label list.
    if shape_dump_enabled() {
        if let Ok(json) = &home {
            eprintln!("shape home: {}", dump_shape(json));
        }
        if let Ok(json) = &explore {
            eprintln!("shape explore: {}", dump_shape(json));
        }
        if let Ok(json) = &account {
            eprintln!("shape account: {}", dump_shape(json));
        }
        // Player ad-marker evidence: key NAMES only, never values (see
        // `collect_player_ad_keys` — the output is a subset of PLAYER_AD_KEYS).
        // Demoted evidence-only: shown here, never gated (until poToken).
        match &player {
            Some(json) => eprintln!("ads: {:?}", collect_player_ad_keys(json)),
            None => eprintln!("ads: unknown"),
        }
        // Topbar-logo leg state (primary): which shape the probe saw.
        eprintln!(
            "topbar: {}",
            match topbar_premium {
                Some(true) => "pass",
                Some(false) => "fail",
                None => "unknown",
            }
        );
        // Per-fetch auth proof + per-page corrected icon values (topbar lane:
        // a verdict from a logged-out fetch must read unknown, never fail).
        eprintln!("{}", render_front_auth_line(music_authed, youtube_authed));
        eprintln!(
            "{}",
            render_front_icons_line(music_icon.as_deref(), youtube_icon.as_deref())
        );
        // Capability evidence: audio itags/bitrates (numbers only — see
        // `collect_audio_formats`) plus the topbar-subtree skeleton (same
        // denylist as `label_hits` — see `dump_topbar_shape`) plus the
        // alternate-client comparison (see `render_altclient_line`).
        eprintln!("{}", render_audio_line(player.as_ref()));
        eprintln!(
            "{}",
            render_altclient_line(
                crate::auth::http::ALT_CLIENT_NAME,
                crate::auth::http::ALT_CLIENT_VERSION,
                alt_player.as_ref(),
            )
        );
        eprintln!(
            "{}",
            dump_topbar_shape(home.as_ref().unwrap_or(&null), &topbar_page_values)
        );
        eprintln!("label_hits: {label_hits:?}");
    }
    // The alt response is dropped here unread (evidence printed above only).
    let playability = match &player {
        // `player` (incl. streamingData) is dropped at end of scope unread.
        Some(json) => parse_playability(json),
        None => Playability::Unknown,
    };
    let ads_absent = match &player {
        Some(json) => parse_player_ads_absent(json),
        None => None,
    };
    // Capability leg: same raw HIGH-quality response, audio ladder only
    // (`itag`/`bitrate` numbers — `streamingData` URLs stay unread).
    let hires_audio = match &player {
        Some(json) => parse_hires_audio(json),
        None => None,
    };
    (
        PremiumSignals {
            logged_in: true,
            home_ok,
            premium_badge,
            explore_top_songs,
            ads_absent,
            topbar_premium,
            hires_audio,
            playability,
        },
        label_hits,
    )
}

/// Plain dev-probe report: gate outcome + which signals passed/failed.
/// Carries NO secret material — only booleans/enums plus the public
/// upgrade message plus denylist-filtered short labels — so it is safe to
/// log, display in the dev dialog, and snapshot in tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeReport {
    /// Final gate decision.
    pub status: GateStatus,
    /// Session logged in (`YTMusic::is_authenticated`).
    pub logged_in: bool,
    /// `browse FEmusic_home` round-trip succeeded.
    pub home_ok: bool,
    /// Premium badge seen (`None` = absent/unparseable = ambiguous).
    /// A deep-label Premium hit folds in here as `Some(true)` (see
    /// [`parse_premium_label`]).
    pub premium_badge: Option<bool>,
    /// `browse FEmusic_explore` Top Songs presence (`None` = ambiguous).
    pub explore_top_songs: Option<bool>,
    /// Authed front-page topbar Premium logo (`None` = ambiguous). A
    /// `Some(true)` here counts exactly like a badge hit in [`evaluate`]
    /// (primary leg); `via=topbar` names it in [`ProbeReport::render`].
    #[serde(default)]
    pub topbar_premium: Option<bool>,
    /// High-audio capability (`None` = ambiguous). A `Some(true)` here
    /// counts exactly like a badge hit in [`evaluate`] (capability leg);
    /// `via=audio` names it in [`ProbeReport::render`].
    #[serde(default)]
    pub hires_audio: Option<bool>,
    /// `player` ad-marker absence (`None` = ambiguous). DEMOTED
    /// evidence-only: shown (`ads=...`) but NEVER gates — [`evaluate`]
    /// ignores it until a poToken exists (tier-independent bot-guard
    /// fallback, see module docs).
    #[serde(default)]
    pub ads_absent: Option<bool>,
    /// `player.playabilityStatus` reading for the probe track.
    pub playability: Playability,
    /// Upgrade prompt when blocked for Premium reasons (`None` otherwise).
    pub upgrade_message: Option<String>,
    /// Deep-label evidence: short human labels from home + explore +
    /// account menu (denylist-filtered, ≤80 chars each, sorted, capped).
    /// Printed on the `SPINWAVE_DUMP_SHAPE=1` path as `label_hits`.
    #[serde(default)]
    pub label_hits: Vec<String>,
}

impl ProbeReport {
    /// Rebuild the [`PremiumSignals`] this report was folded from (the live
    /// [`PremiumGate::probe_report`] path fills `label_hits` separately, so
    /// the legs round-trip exactly). Pure, offline.
    fn signals(&self) -> PremiumSignals {
        PremiumSignals {
            logged_in: self.logged_in,
            home_ok: self.home_ok,
            premium_badge: self.premium_badge,
            explore_top_songs: self.explore_top_songs,
            ads_absent: self.ads_absent,
            topbar_premium: self.topbar_premium,
            hires_audio: self.hires_audio,
            playability: self.playability,
        }
    }

    /// Fold probe signals + the applied gate into a log-safe report.
    /// `label_hits` starts empty offline; the live [`PremiumGate::probe_report`]
    /// fills it from [`collect_label_hits`].
    pub fn from_signals(signals: &PremiumSignals, gate: &PremiumGate) -> Self {
        Self {
            status: gate.status(),
            logged_in: signals.logged_in,
            home_ok: signals.home_ok,
            premium_badge: signals.premium_badge,
            explore_top_songs: signals.explore_top_songs,
            ads_absent: signals.ads_absent,
            topbar_premium: signals.topbar_premium,
            hires_audio: signals.hires_audio,
            playability: signals.playability,
            upgrade_message: gate.upgrade_message(),
            label_hits: Vec::new(),
        }
    }

    /// One-line-per-signal plain report (no secrets by construction).
    /// Leads with the decision + deciding leg (`via=badge|explore|topbar|audio|none`)
    /// so `badge=unknown` no longer reads as failure when another leg
    /// carries Premium. The demoted `ads` leg is shown as evidence only
    /// (never `via=ads`). Label hits are account content (names, playlist
    /// titles), so they print only on the `SPINWAVE_DUMP_SHAPE=1` path.
    pub fn render(&self) -> String {
        let mut out = format!(
            "gate={:?} via={} logged_in={} home_ok={} badge={} explore={} topbar={} audio={} ads={} playability={:?}",
            self.status,
            deciding_leg(&self.signals()),
            ok_str(self.logged_in),
            ok_str(self.home_ok),
            match self.premium_badge {
                Some(true) => "pass",
                Some(false) => "fail",
                None => "unknown",
            },
            match self.explore_top_songs {
                Some(true) => "pass",
                Some(false) => "fail",
                None => "unknown",
            },
            match self.topbar_premium {
                Some(true) => "pass",
                Some(false) => "fail",
                None => "unknown",
            },
            match self.hires_audio {
                Some(true) => "pass",
                Some(false) => "fail",
                None => "unknown",
            },
            match self.ads_absent {
                Some(true) => "pass",
                Some(false) => "fail",
                None => "unknown",
            },
            self.playability,
        );
        if let Some(msg) = &self.upgrade_message {
            out.push_str(&format!("\n{msg}"));
        }
        out
    }
}

fn ok_str(ok: bool) -> &'static str {
    if ok { "pass" } else { "fail" }
}

impl std::fmt::Display for ProbeReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn premium_signals() -> PremiumSignals {
        PremiumSignals {
            logged_in: true,
            home_ok: true,
            premium_badge: Some(true),
            explore_top_songs: None,
            ads_absent: None,
            topbar_premium: None,
            hires_audio: None,
            playability: Playability::Playable,
        }
    }

    #[test]
    fn logged_out_is_blocked() {
        let gate = PremiumGate::new_logged_out();
        assert_eq!(gate.status(), GateStatus::LoggedOut);
        assert!(!gate.can_fetch_playback_url());
        assert_eq!(evaluate(&PremiumSignals::logged_out()), GateStatus::LoggedOut);
    }

    #[test]
    fn non_premium_is_blocked_with_upgrade_state() {
        let mut signals = premium_signals();
        signals.premium_badge = Some(false);
        signals.explore_top_songs = Some(false);
        assert_eq!(evaluate(&signals), GateStatus::UpgradeRequired);

        signals.premium_badge = Some(true);
        signals.explore_top_songs = None;
        signals.playability = Playability::PremiumOnly;
        assert_eq!(evaluate(&signals), GateStatus::UpgradeRequired);

        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&signals, SystemTime::now());
        assert!(!gate.can_fetch_playback_url());
        assert!(gate.upgrade_message().is_some());
    }

    #[test]
    fn behavioral_top_songs_rescues_unknown_badge() {
        // Probe #2 shape: CONFIRMED Premium home carries NO Premium-named
        // inline badge (badge leg unknown) but explore carries Top Songs.
        let mut signals = premium_signals();
        signals.premium_badge = None;
        signals.explore_top_songs = Some(true);
        assert_eq!(evaluate(&signals), GateStatus::Premium);

        // Badge leg alone still passes when behavioral is unknown.
        let mut badge_only = premium_signals();
        badge_only.explore_top_songs = None;
        assert_eq!(evaluate(&badge_only), GateStatus::Premium);

        // Both legs absent/unknown block fail-closed.
        for explore in [None, Some(false)] {
            let blocked = PremiumSignals {
                premium_badge: None,
                explore_top_songs: explore,
                ..premium_signals()
            };
            assert_eq!(evaluate(&blocked), GateStatus::UpgradeRequired);
            let blocked = PremiumSignals {
                premium_badge: Some(false),
                explore_top_songs: explore,
                ..premium_signals()
            };
            assert_eq!(evaluate(&blocked), GateStatus::UpgradeRequired);
        }
    }

    #[test]
    fn ambiguous_signals_are_blocked_with_upgrade_state() {
        // Missing badge, unknown playability, failed home probe: all ambiguous.
        // (Badge-unknown cases also carry explore-unknown so NEITHER
        // account-level leg passes.)
        let cases = [
            PremiumSignals { premium_badge: None, explore_top_songs: None, ..premium_signals() },
            PremiumSignals { playability: Playability::Unknown, ..premium_signals() },
            PremiumSignals { home_ok: false, ..premium_signals() },
            PremiumSignals { playability: Playability::Unplayable, ..premium_signals() },
            PremiumSignals {
                premium_badge: Some(false),
                explore_top_songs: Some(false),
                ..premium_signals()
            },
        ];
        for signals in cases {
            assert_eq!(
                evaluate(&signals),
                GateStatus::UpgradeRequired,
                "ambiguous input must block fail-closed: {signals:?}"
            );
        }
    }

    #[test]
    fn premium_passes_the_gate() {
        let status = evaluate(&premium_signals());
        assert_eq!(status, GateStatus::Premium);

        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&premium_signals(), SystemTime::now());
        assert!(gate.can_fetch_playback_url());
        assert_eq!(gate.upgrade_message(), None);
    }

    #[test]
    fn parse_playability_reads_status_only() {
        let ok = serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "streamingData": {"formats": ["MUST-NOT-BE-READ"]},
        });
        assert_eq!(parse_playability(&ok), Playability::Playable);

        let premium = serde_json::json!({
            "playabilityStatus": {"status": "LOGIN_REQUIRED", "reason": "Premium members only"},
        });
        assert_eq!(parse_playability(&premium), Playability::PremiumOnly);

        let blocked = serde_json::json!({
            "playabilityStatus": {"status": "UNPLAYABLE", "reason": "Video unavailable"},
        });
        assert_eq!(parse_playability(&blocked), Playability::Unplayable);

        assert_eq!(parse_playability(&serde_json::json!({})), Playability::Unknown);
    }

    #[test]
    fn parse_premium_badge_matches_verified_renderer_label_only() {
        // Real `browse` skeleton per ytmusic-rs-0.1.1 `navigation.rs::SINGLE_COLUMN_TAB`
        // (src/navigation.rs:100-103) + `SECTION_LIST`, with an item-level badge per
        // `navigation.rs::BADGE_LABEL` (src/navigation.rs:202-208). Synthetic content,
        // no secrets — only the SHAPE mirrors the vendored sources.
        fn home_with_badge_label(label: &str) -> serde_json::Value {
            serde_json::json!({
                "contents": {
                    "singleColumnBrowseResultsRenderer": {
                        "tabs": [{
                            "tabRenderer": {
                                "content": {
                                    "sectionListRenderer": {
                                        "contents": [{
                                            "musicCarouselShelfRenderer": {
                                                "contents": [{
                                                    "musicResponsiveListItemRenderer": {
                                                        "flexColumns": [],
                                                        "badges": [{
                                                            "musicInlineBadgeRenderer": {
                                                                "accessibilityData": {
                                                                    "accessibilityData": {
                                                                        "label": label
                                                                    }
                                                                }
                                                            }
                                                        }]
                                                    }
                                                }]
                                            }
                                        }]
                                    }
                                }
                            }
                        }]
                    }
                }
            })
        }
        // Premium-labeled inline badge → premium.
        assert_eq!(parse_premium_badge(&home_with_badge_label("Premium")), Some(true));
        // Explicit-labeled badge (the crate's own use-case: parsers/search.rs:418,
        // parsers/playlists.rs:402) is NOT a premium signal → fail-closed.
        assert_eq!(parse_premium_badge(&home_with_badge_label("Explicit")), None);
        // Same verified structure via `subtitleBadges` (SUBTITLE_BADGE_LABEL).
        let mut sub = home_with_badge_label("none");
        sub.pointer_mut("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicCarouselShelfRenderer/contents/0/musicResponsiveListItemRenderer")
            .expect("fixture shape")
            .as_object_mut()
            .expect("item object")
            .remove("badges");
        sub.pointer_mut("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicCarouselShelfRenderer/contents/0/musicResponsiveListItemRenderer")
            .expect("fixture shape")
            .as_object_mut()
            .expect("item object")
            .insert(
                "subtitleBadges".into(),
                serde_json::json!([{
                    "musicInlineBadgeRenderer": {
                        "accessibilityData": {
                            "accessibilityData": { "label": "Music Premium" }
                        }
                    }
                }]),
            );
        assert_eq!(parse_premium_badge(&sub), Some(true));
        // Real home skeleton with NO badge at all → ambiguous (fail-closed).
        assert_eq!(parse_premium_badge(&home_with_badge_label("none")), None);
        // Bare verified fast-path shape (top-level `badges`, per
        // BADGE_CANDIDATE_POINTERS): Premium label hits, Explicit does not.
        let bare = |label: &str| {
            serde_json::json!({"badges": [{"musicInlineBadgeRenderer": {
                "accessibilityData": {"accessibilityData": {"label": label}}
            }}]})
        };
        assert_eq!(parse_premium_badge(&bare("Premium")), Some(true));
        assert_eq!(parse_premium_badge(&bare("Explicit")), None);
    }

    #[test]
    fn parse_premium_badge_rejects_unverified_and_mangled_shapes() {
        // The old fictional schema has zero vendored evidence (full-crate grep:
        // no `premiumBadge` key, no `musicPremiumBadgeRenderer` node) and live
        // probe #1's exhaustive walk found neither at any depth of a real
        // Premium home response — all fail closed now (never a positive).
        let fictional = [
            serde_json::json!({"premiumBadge": true}),
            serde_json::json!({"premiumBadge": false}),
            serde_json::json!({"contents": {"sectionListRenderer": {"header": {"premiumBadge": true}}}}),
            serde_json::json!({"musicPremiumBadgeRenderer": null}),
            serde_json::json!({"header": {"musicPremiumBadgeRenderer": {}}}),
        ];
        for shape in &fictional {
            assert_eq!(
                parse_premium_badge(shape),
                None,
                "unverified shape must stay fail-closed: {shape}"
            );
        }
        // Free-tier upsell copy must never read as a premium badge (fail-open
        // risk): titles/descriptions/subtitles are not badge signals.
        let upsell = serde_json::json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicDescriptionShelfRenderer": {
                                            "title": {"runs": [{"text": "Get Music Premium"}]},
                                            "description": {"runs": [{"text": "Premium members enjoy ad-free music"}]},
                                            "subtitle": {"runs": [{"text": "Premium offer"}]}
                                        }
                                    }]
                                }
                            }}
                    }]
                }
            }
        });
        assert_eq!(parse_premium_badge(&upsell), None);
        // Mangled shapes: no panic, fail closed.
        let mangled = [
            serde_json::json!(null),
            serde_json::json!("musicPremiumBadgeRenderer"),
            serde_json::json!("Premium"),
            serde_json::json!(42),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!({"contents": {}}),
            serde_json::json!({"contents": {"tabs": [{"x": [null, {"y": 1}]}]}}),
            serde_json::json!({"badges": [null, 42, "x"]}),
            serde_json::json!({"badges": [{"musicInlineBadgeRenderer": null}]}),
            serde_json::json!({"badges": [{"musicInlineBadgeRenderer": {"accessibilityData": null}}]}),
        ];
        for shape in &mangled {
            let _ = parse_premium_badge(shape);
            assert_eq!(
                parse_premium_badge(shape),
                None,
                "mangled shape must stay fail-closed: {shape}"
            );
        }
    }

    #[test]
    fn probe_report_renders_signals_without_secrets() {
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&premium_signals(), SystemTime::now());
        let report = ProbeReport::from_signals(&premium_signals(), &gate);
        assert_eq!(report.status, GateStatus::Premium);
        assert_eq!(report.upgrade_message, None);
        let text = report.render();
        assert!(text.contains("gate=Premium"));
        assert!(text.contains("logged_in=pass") && text.contains("home_ok=pass"));
        assert!(text.contains("badge=pass") && text.contains("playability=Playable"));
        assert!(text.contains("explore=unknown"));

        // Behavioral-only pass renders explore=pass (badge unknown).
        let behavioral = PremiumSignals {
            premium_badge: None,
            explore_top_songs: Some(true),
            ..premium_signals()
        };
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&behavioral, SystemTime::now());
        let report = ProbeReport::from_signals(&behavioral, &gate);
        assert_eq!(report.status, GateStatus::Premium);
        assert!(report.render().contains("explore=pass"));

        // Blocked report carries the upgrade message, still secret-free.
        let blocked = PremiumSignals {
            premium_badge: Some(false),
            explore_top_songs: Some(false),
            ..premium_signals()
        };
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&blocked, SystemTime::now());
        let report = ProbeReport::from_signals(&blocked, &gate);
        assert_eq!(report.status, GateStatus::UpgradeRequired);
        let text = report.render();
        assert!(text.contains("gate=UpgradeRequired") && text.contains("badge=fail"));
        assert!(text.contains("Premium"));
        for secret in ["cookie", "SAPISID", "sapisid", "fake-secret"] {
            assert!(!text.contains(secret), "report leaked {secret}");
        }
        assert!(!format!("{report:?}").contains("fake-secret"));
    }

    #[test]
    fn gate_report_leads_with_deciding_leg() {
        // `badge=unknown` must not read as failure when another leg carries
        // Premium: the report leads `gate=... via=<leg>`.
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&premium_signals(), SystemTime::now());
        let text = ProbeReport::from_signals(&premium_signals(), &gate).render();
        assert!(text.starts_with("gate=Premium via=badge"), "{text}");

        let behavioral = PremiumSignals {
            premium_badge: None,
            explore_top_songs: Some(true),
            ..premium_signals()
        };
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&behavioral, SystemTime::now());
        let text = ProbeReport::from_signals(&behavioral, &gate).render();
        assert!(text.starts_with("gate=Premium via=explore"), "{text}");

        // DEMOTION REGRESSION: ads-absent alone NEVER gates (evidence-only
        // until poToken exists — tier-independent bot-guard fallback).
        let ads_only = PremiumSignals {
            premium_badge: None,
            explore_top_songs: None,
            ads_absent: Some(true),
            topbar_premium: None,
            ..premium_signals()
        };
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&ads_only, SystemTime::now());
        let report = ProbeReport::from_signals(&ads_only, &gate);
        assert_eq!(report.status, GateStatus::UpgradeRequired);
        let text = report.render();
        assert!(text.starts_with("gate=UpgradeRequired via=none"), "{text}");
        assert!(text.contains("ads=pass"), "demoted leg stays visible: {text}");

        let topbar_only = PremiumSignals {
            premium_badge: None,
            explore_top_songs: Some(false),
            ads_absent: Some(false),
            topbar_premium: Some(true),
            ..premium_signals()
        };
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&topbar_only, SystemTime::now());
        let text = ProbeReport::from_signals(&topbar_only, &gate).render();
        assert!(text.starts_with("gate=Premium via=topbar"), "{text}");
        assert!(text.contains("topbar=pass"), "{text}");

        // Blocked outcomes never name a leg (fail-closed).
        let blocked = PremiumSignals {
            premium_badge: None,
            explore_top_songs: Some(false),
            ..premium_signals()
        };
        assert_eq!(deciding_leg(&blocked), "none");
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&blocked, SystemTime::now());
        let text = ProbeReport::from_signals(&blocked, &gate).render();
        assert!(text.starts_with("gate=UpgradeRequired via=none"), "{text}");
        assert_eq!(deciding_leg(&PremiumSignals::logged_out()), "none");
    }

    fn explore_with_browse_id(browse_id: Option<&str>) -> serde_json::Value {
        let header_title = match browse_id {
            Some(id) => serde_json::json!({
                "runs": [{
                    "text": "Top Songs",
                    "navigationEndpoint": {"browseEndpoint": {"browseId": id}},
                }],
            }),
            None => serde_json::json!({"runs": [{"text": "New releases"}]}),
        };
        serde_json::json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicCarouselShelfRenderer": {
                                            "header": {
                                                "musicCarouselShelfBasicHeaderRenderer": {
                                                    "title": header_title,
                                                },
                                            },
                                            "contents": [],
                                        },
                                    }],
                                },
                            },
                        },
                    }],
                },
            },
        })
    }

    #[test]
    fn parse_explore_top_songs_detects_vlpl_only() {
        // Premium-shape: VLPL* carousel header → premium evidence.
        assert_eq!(
            parse_explore_top_songs(&explore_with_browse_id(Some("VLPLabc123"))),
            Some(true)
        );
        // Filtered `get_explore` dict form with top_songs also counts.
        assert_eq!(
            parse_explore_top_songs(&serde_json::json!({
                "top_songs": {"playlist": "VLPLabc123", "items": []},
                "trending": {"playlist": "VLOLAxyz", "items": []},
            })),
            Some(true)
        );
        // Free-shape: valid explore without Top Songs → not premium.
        assert_eq!(
            parse_explore_top_songs(&explore_with_browse_id(Some(
                "FEmusic_new_releases_albums"
            ))),
            Some(false)
        );
        assert_eq!(
            parse_explore_top_songs(&explore_with_browse_id(None)),
            Some(false)
        );
        // Trending-only (VLOLA) is NOT Top Songs.
        assert_eq!(
            parse_explore_top_songs(&explore_with_browse_id(Some("VLOLAxyz"))),
            Some(false)
        );
        // Title text alone never counts (no browseId).
        let title_only = serde_json::json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicCarouselShelfRenderer": {
                                            "header": {
                                                "musicCarouselShelfBasicHeaderRenderer": {
                                                    "title": {"runs": [{"text": "Top Songs"}]},
                                                },
                                            },
                                            "contents": [],
                                        },
                                    }],
                                },
                            },
                        },
                    }],
                },
            },
        });
        assert_eq!(parse_explore_top_songs(&title_only), Some(false));
        // Full gate: premium-shape explore rescues an unknown badge.
        let premium_shape = PremiumSignals {
            premium_badge: None,
            explore_top_songs: parse_explore_top_songs(&explore_with_browse_id(Some(
                "VLPLabc123",
            ))),
            ..premium_signals()
        };
        assert_eq!(evaluate(&premium_shape), GateStatus::Premium);
        // Full gate: free-shape explore stays blocked.
        let free_shape = PremiumSignals {
            premium_badge: None,
            explore_top_songs: parse_explore_top_songs(&explore_with_browse_id(None)),
            ..premium_signals()
        };
        assert_eq!(evaluate(&free_shape), GateStatus::UpgradeRequired);
    }

    #[test]
    fn parse_explore_top_songs_mangled_is_fail_closed() {
        let mangled = [
            serde_json::json!(null),
            serde_json::json!("FEmusic_explore"),
            serde_json::json!(42),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!({"contents": {}}),
            serde_json::json!({"contents": {"singleColumnBrowseResultsRenderer": {}}}),
            serde_json::json!({
                "contents": {
                    "singleColumnBrowseResultsRenderer": {"tabs": [{"x": [null, {"y": 1}]}]},
                },
            }),
        ];
        for shape in &mangled {
            let parsed = parse_explore_top_songs(shape);
            assert!(
                parsed != Some(true),
                "mangled shape must never read as premium: {shape}"
            );
            // No panic above; the gate stays fail-closed either way.
            let signals = PremiumSignals {
                premium_badge: None,
                explore_top_songs: parsed,
                ..premium_signals()
            };
            assert_eq!(evaluate(&signals), GateStatus::UpgradeRequired);
        }
    }

    #[test]
    fn parse_player_ads_absent_reads_key_presence_only() {
        // Premium-shape RAW response: probe succeeded, no ad-marker keys
        // anywhere → ads absent counts as premium evidence. `responseContext`
        // (outside the 5 `get_song`-kept keys) proves it is raw — without a
        // raw marker the same dict would be filtered-shaped and fail closed
        // (see the vacuity regression test below).
        let premium_player = serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "videoDetails": {"videoId": "4Tr0otuiQuU"},
            "responseContext": {"serviceTrackingParams": []},
        });
        assert_eq!(parse_player_ads_absent(&premium_player), Some(true));
        assert!(collect_player_ad_keys(&premium_player).is_empty());

        // Free-shape: EACH ad-marker key present on its own blocks.
        for key in PLAYER_AD_KEYS {
            let mut player = serde_json::json!({"playabilityStatus": {"status": "OK"}});
            player
                .as_object_mut()
                .expect("fixture object")
                .insert(key.to_string(), serde_json::json!([]));
            assert_eq!(
                parse_player_ads_absent(&player),
                Some(false),
                "ad key {key} must read as free-shape"
            );
            assert_eq!(collect_player_ad_keys(&player), vec![key.to_string()]);
        }
        // Nested ad markers still detected; the dump line carries NAMES ONLY
        // (ad payload URLs/values never leak through it).
        let nested = serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "responseContext": {
                "adPlacements": [{"adUrl": "https://example.com/secret-ad-bytes"}],
            },
            "playerAds": [{"id": 1}],
        });
        assert_eq!(parse_player_ads_absent(&nested), Some(false));
        let seen = collect_player_ad_keys(&nested);
        assert_eq!(seen, vec!["adPlacements".to_string(), "playerAds".to_string()]);
        for s in &seen {
            assert!(
                !s.contains("example.com"),
                "ads dump line must carry names only: {s}"
            );
        }
        // Key present with null/empty value still counts as present
        // (fail-closed: a mangled ad marker is not "ads absent").
        assert_eq!(
            parse_player_ads_absent(&serde_json::json!({"adBreaks": null})),
            Some(false)
        );
        assert_eq!(
            parse_player_ads_absent(&serde_json::json!({"playerAds": {}})),
            Some(false)
        );

        // Full gate (DEMOTED): ads-absent alone NEVER passes — evidence-only
        // until poToken exists (tier-independent bot-guard fallback).
        let ads_only = PremiumSignals {
            premium_badge: None,
            explore_top_songs: None,
            ads_absent: parse_player_ads_absent(&premium_player),
            playability: Playability::Playable,
            ..premium_signals()
        };
        assert_eq!(ads_only.ads_absent, Some(true));
        assert_eq!(
            evaluate(&ads_only),
            GateStatus::UpgradeRequired,
            "demoted leg must not gate on its own"
        );

        // Full gate: free-shape ads stay blocked even when playable.
        let free = PremiumSignals {
            premium_badge: None,
            explore_top_songs: None,
            ads_absent: parse_player_ads_absent(&nested),
            playability: Playability::Playable,
            ..premium_signals()
        };
        assert_eq!(evaluate(&free), GateStatus::UpgradeRequired);

        // Report still renders the demoted leg as evidence (secret-free)
        // while the gate blocks.
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&ads_only, SystemTime::now());
        let report = ProbeReport::from_signals(&ads_only, &gate);
        assert_eq!(report.status, GateStatus::UpgradeRequired);
        let text = report.render();
        assert!(text.contains("ads=pass"), "report must show ads leg: {text}");
        assert!(!text.to_ascii_lowercase().contains("example.com"));
    }

    #[test]
    fn parse_player_ads_absent_mangled_is_fail_closed() {
        // Non-object roots and empty objects carry no player evidence:
        // never premium, never panic.
        let mangled = [
            serde_json::json!(null),
            serde_json::json!("playerAds"),
            serde_json::json!("Premium"),
            serde_json::json!(42),
            serde_json::json!(true),
            serde_json::json!([]),
            serde_json::json!([{"adPlacements": []}]),
            serde_json::json!({}),
        ];
        for shape in &mangled {
            let parsed = parse_player_ads_absent(shape);
            assert!(
                parsed != Some(true),
                "mangled player shape must never read as premium: {shape}"
            );
            // No panic above; the gate stays fail-closed either way.
            let signals = PremiumSignals {
                premium_badge: None,
                explore_top_songs: None,
                ads_absent: parsed,
                ..premium_signals()
            };
            assert_eq!(evaluate(&signals), GateStatus::UpgradeRequired);
        }
        // Evidence collection on mangled shapes never panics and never
        // emits values (only fixed key names from PLAYER_AD_KEYS).
        for shape in &mangled {
            for hit in collect_player_ad_keys(shape) {
                assert!(
                    PLAYER_AD_KEYS.contains(&hit.as_str()),
                    "unexpected ads hit: {hit}"
                );
            }
        }
    }

    #[test]
    fn filtered_get_song_dict_never_counts_as_ads_absent_vacuity_regression() {
        // VACUITY REGRESSION: `get_song` strips the player response to the 5
        // `GET_SONG_KEPT_KEYS` (vendored browsing.rs:559-567), so top-level
        // ad markers can NEVER appear in its output — a naive "no ad keys"
        // check on that shape passes free accounts identically (fail-OPEN).
        // `parse_player_ads_absent` must therefore return `None`
        // (fail-closed) for ANY `get_song`-shaped dict, even one with no ad
        // keys that would otherwise read as premium evidence.
        let filtered_no_ads = serde_json::json!({
            "videoDetails": {"videoId": "4Tr0otuiQuU"},
            "playabilityStatus": {"status": "OK"},
            "streamingData": {"expiresInSeconds": "21540"},
            "microformat": {"playerMicroformatRenderer": {}},
            "playbackTracking": {"videostatsPlaybackUrl": {"baseUrl": "https://example.com/t"}},
        });
        assert!(is_filtered_player_shape(&filtered_no_ads));
        assert_eq!(
            parse_player_ads_absent(&filtered_no_ads),
            None,
            "filtered dict without ad keys must NOT read as ads-absent"
        );
        // Subsets of the kept keys are equally filtered (a 2-key dict is
        // exactly what the old vacuous path scored as `Some(true)`).
        let filtered_subset = serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "videoDetails": {"videoId": "4Tr0otuiQuU"},
        });
        assert!(is_filtered_player_shape(&filtered_subset));
        assert_eq!(parse_player_ads_absent(&filtered_subset), None);
        // Non-object roots read as filtered too (no raw evidence).
        assert!(is_filtered_player_shape(&serde_json::json!(null)));
        assert!(is_filtered_player_shape(&serde_json::json!([])));
        // The SAME content plus one raw marker key (`responseContext`, which
        // `get_song` always strips) proves raw provenance → counts.
        let mut raw = filtered_subset.clone();
        raw.as_object_mut()
            .expect("fixture object")
            .insert(
                "responseContext".into(),
                serde_json::json!({"serviceTrackingParams": []}),
            );
        assert!(!is_filtered_player_shape(&raw));
        assert_eq!(parse_player_ads_absent(&raw), Some(true));
        // Full gate (DEMOTED): raw-without-ads + logged_in + home_ok +
        // Playable NO LONGER passes on the ads leg alone — evidence-only
        // until poToken exists (tier-independent bot-guard fallback).
        let premium = PremiumSignals {
            logged_in: true,
            home_ok: true,
            premium_badge: None,
            explore_top_songs: None,
            ads_absent: parse_player_ads_absent(&raw),
            topbar_premium: None,
            hires_audio: None,
            playability: Playability::Playable,
        };
        assert_eq!(premium.ads_absent, Some(true));
        assert_eq!(
            evaluate(&premium),
            GateStatus::UpgradeRequired,
            "demoted ads leg must not pass the gate on its own"
        );
        // The same signals PLUS the topbar Premium logo pass via=topbar
        // (the demoted ads value neither helps nor vetoes).
        let via_topbar = PremiumSignals {
            topbar_premium: Some(true),
            ..premium.clone()
        };
        assert_eq!(evaluate(&via_topbar), GateStatus::Premium);
        assert_eq!(deciding_leg(&via_topbar), "topbar");
        // Full gate, raw-WITH-ads → blocked even when everything else passes.
        let mut with_ads = raw.clone();
        with_ads
            .as_object_mut()
            .expect("fixture object")
            .insert("adPlacements".into(), serde_json::json!([]));
        assert_eq!(parse_player_ads_absent(&with_ads), Some(false));
        let free = PremiumSignals {
            ads_absent: parse_player_ads_absent(&with_ads),
            ..premium.clone()
        };
        assert_eq!(evaluate(&free), GateStatus::UpgradeRequired);
        // Full gate, FILTERED input blocks EVERYONE including Premium-shaped
        // signals — correct and legal fail-closed until the raw probe works.
        let blocked = PremiumSignals {
            ads_absent: parse_player_ads_absent(&filtered_no_ads),
            ..premium.clone()
        };
        assert_eq!(blocked.ads_absent, None);
        assert_eq!(evaluate(&blocked), GateStatus::UpgradeRequired);
        // Mangled inputs through this path: no panic, never premium, and the
        // report stays secret-free (ad values like the tracking URL above
        // never surface — only key names exist in this module's outputs).
        for shape in [&filtered_no_ads, &raw, &with_ads] {
            for hit in collect_player_ad_keys(shape) {
                assert!(PLAYER_AD_KEYS.contains(&hit.as_str()));
            }
        }
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&premium, SystemTime::now());
        let text = ProbeReport::from_signals(&premium, &gate).render();
        assert!(text.contains("ads=pass"));
        assert!(text.starts_with("gate=UpgradeRequired via=none"), "{text}");
        assert!(!text.to_ascii_lowercase().contains("example.com"));
    }

    #[test]
    fn shape_dump_is_keys_only_and_never_leaks_secrets() {
        let long_title = "L".repeat(120);
        let secret_fixture = serde_json::json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicCarouselShelfRenderer": {
                                            "header": {
                                                "musicCarouselShelfBasicHeaderRenderer": {
                                                    "title": {"runs": [{
                                                        "text": "Top Songs",
                                                        "navigationEndpoint": {
                                                            "browseEndpoint": {
                                                                "browseId": "VLPL-secret-should-stay-hidden"
                                                            },
                                                        },
                                                    }]},
                                                    "subtitle": {"runs": [{"text": long_title}]},
                                                },
                                            },
                                            "contents": [],
                                        },
                                    }],
                                },
                            },
                        },
                    }],
                },
            },
            "cookie": "__Secure-3PAPISID=fake-test-sapisid; HSID=fake-hsid",
            "authorization": "SAPISIDHASH fake-hash-for-tests",
            "access_token": "fake-token-value",
            "SAPISID": "fake-sapisid-value",
            "streamingData": {"formats": [{"url": "https://example.com/secret-bytes"}]},
            "thumbnail": {"thumbnails": [{"url": "https://example.com/thumb.jpg"}]},
            "continuation": "fake-continuation-token-abc123",
            "trackingParams": "fake-tracking-xyz",
            "clickTrackingParams": "fake-click-123",
        });
        let dump = dump_shape(&secret_fixture);
        // Renderer type names + key names survive (evidence stays useful).
        assert!(dump.contains("musicCarouselShelfRenderer"));
        assert!(dump.contains("sectionListRenderer"));
        // Short human labels survive alongside keys (round-3 evidence).
        assert!(dump.contains("Top Songs"), "label must survive: {dump}");
        assert!(dump.contains("subtitle"), "subtitle key must survive: {dump}");
        // Long label values are capped at 80 chars (full 120-char run absent).
        assert!(!dump.contains(&long_title), "long label must be capped: {dump}");
        assert!(dump.contains(&"L".repeat(80)), "80-char prefix survives: {dump}");
        // Non-label scalars stay tags.
        assert!(dump.contains("<str>") || dump.contains("{"));
        // Denylist: NONE of these substrings may appear, even as key names.
        for denied in [
            "cookie",
            "authorization",
            "token",
            "sapisid",
            "SAPISID",
            "url",
            "URL",
            "streamingData",
            "continuation",
            "trackingparams",
            "trackingParams",
            "fake-test-sapisid",
            "fake-hash-for-tests",
            "fake-hsid",
            "fake-continuation-token-abc123",
            "fake-tracking-xyz",
            "fake-click-123",
            "example.com",
        ] {
            assert!(
                !dump.to_ascii_lowercase().contains(&denied.to_ascii_lowercase()),
                "dump leaked {denied}: {dump}"
            );
        }
        // Secret-looking label VALUES are replaced by tags, never echoed.
        let sneaky = serde_json::json!({
            "title": {"runs": [{"text": "https://example.com/sneaky"}]},
            "label": "fake-token-value",
        });
        let sneaky_dump = dump_shape(&sneaky);
        assert!(!sneaky_dump.to_ascii_lowercase().contains("example.com"));
        assert!(!sneaky_dump.to_ascii_lowercase().contains("fake-token-value"));
        // Mangled inputs never panic and stay secret-free.
        for shape in [
            serde_json::json!(null),
            serde_json::json!("secret-cookie-string"),
            serde_json::json!(42),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            let dump = dump_shape(&shape);
            assert!(!dump.to_ascii_lowercase().contains("cookie"));
        }
    }

    /// Live probe #3 home shape: 2 `musicCarouselShelfRenderer` +
    /// `musicTastebuilderShelfRenderer`, with a Premium accessibility label
    /// tucked in the first carousel header (NOT a badge renderer — the old
    /// badge walk misses it, the deep label search must find it).
    fn home_premium_label_in_header() -> serde_json::Value {
        serde_json::json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": {"runs": [{"text": "Your favorites"}]},
                                                        "accessibilityData": {
                                                            "accessibilityData": {"label": "Premium"}
                                                        },
                                                    },
                                                },
                                                "contents": [],
                                            },
                                        },
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": {"runs": [{"text": "Mixed for you"}]},
                                                    },
                                                },
                                                "contents": [],
                                            },
                                        },
                                        {
                                            "musicTastebuilderShelfRenderer": {
                                                "title": {"runs": [{"text": "Tell us which artists you like"}]},
                                            },
                                        },
                                    ],
                                },
                            },
                        },
                    }],
                },
            },
        })
    }

    /// Plausible `account/accounts_list` menu shape carrying a Premium item
    /// label (raw endpoint also covered by the shape dump).
    fn account_menu_with_premium_label() -> serde_json::Value {
        serde_json::json!({
            "actions": [{
                "getMultiPageMenuAction": {
                    "menu": {
                        "multiPageMenuRenderer": {
                            "header": {
                                "activeAccountHeaderRenderer": {
                                    "accountName": {"simpleText": "Test User"},
                                },
                            },
                            "sections": [{
                                "accountSectionListRenderer": {
                                    "contents": [{
                                        "accountItemSectionRenderer": {
                                            "contents": [{
                                                "menuItemRenderer": {
                                                    "text": {"runs": [{"text": "YouTube Premium"}]},
                                                    "accessibilityData": {
                                                        "accessibilityData": {"label": "YouTube Premium"}
                                                    },
                                                },
                                            }],
                                        },
                                    }],
                                },
                            }],
                        },
                    },
                },
            }],
        })
    }

    /// Free shape: upsell copy in title/description/subtitle runs (keys
    /// `text`, never `label`) plus a `gridRenderer` explore without Top Songs
    /// and an account menu without any Premium label.
    fn home_free_with_upsell() -> serde_json::Value {
        serde_json::json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": {"runs": [{"text": "Mixed for you"}]},
                                                    },
                                                },
                                                "contents": [],
                                            },
                                        },
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": {"runs": [{"text": "New releases"}]},
                                                    },
                                                },
                                                "contents": [],
                                            },
                                        },
                                        {
                                            "musicDescriptionShelfRenderer": {
                                                "title": {"runs": [{"text": "Get Music Premium"}]},
                                                "description": {"runs": [{"text": "Premium members enjoy ad-free music"}]},
                                                "subtitle": {"runs": [{"text": "Premium offer"}]},
                                            },
                                        },
                                    ],
                                },
                            },
                        },
                    }],
                },
            },
        })
    }

    fn explore_free_grid_shape() -> serde_json::Value {
        serde_json::json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "gridRenderer": {
                                                "header": {
                                                    "gridHeaderRenderer": {
                                                        "title": {"runs": [{"text": "Moods & genres"}]},
                                                    },
                                                },
                                                "items": [],
                                            },
                                        },
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": {"runs": [{"text": "New releases"}]},
                                                    },
                                                },
                                                "contents": [],
                                            },
                                        },
                                    ],
                                },
                            },
                        },
                    }],
                },
            },
        })
    }

    fn account_menu_free() -> serde_json::Value {
        serde_json::json!({
            "actions": [{
                "getMultiPageMenuAction": {
                    "menu": {
                        "multiPageMenuRenderer": {
                            "header": {
                                "activeAccountHeaderRenderer": {
                                    "accountName": {"simpleText": "Test User"},
                                },
                            },
                            "sections": [],
                        },
                    },
                },
            }],
        })
    }

    #[test]
    fn deep_label_search_finds_premium_in_header_and_account_menu() {
        let home = home_premium_label_in_header();
        let explore = explore_free_grid_shape();
        let account = account_menu_with_premium_label();
        let empty = serde_json::json!({});

        // Header accessibility label (not a badge renderer): old badge walk
        // misses it, deep search hits it with badge-level strictness.
        assert_eq!(parse_premium_badge(&home), None);
        assert_eq!(
            parse_premium_label(&home, &explore, None),
            Some(true),
            "premium label in header must hit"
        );
        // Account-menu Premium label hits even when home/explore are empty.
        assert_eq!(parse_premium_label(&empty, &empty, Some(&account)), Some(true));
        assert_eq!(
            parse_premium_label(&empty, &empty, Some(&account_menu_free())),
            None
        );
        // Evidence collects the human labels (badges, header titles,
        // accessibility labels, account-item labels).
        let hits = collect_label_hits(&home, &explore, Some(&account));
        for want in ["Premium", "Your favorites", "YouTube Premium", "Moods & genres"] {
            assert!(hits.iter().any(|h| h == want), "missing {want}: {hits:?}");
        }
        // Hits are denylist-filtered, capped, sorted/deduped.
        for h in &hits {
            assert!(h.chars().count() <= MAX_LABEL_CHARS, "hit too long: {h}");
            assert!(!is_secret_value(h), "hit must be filtered: {h}");
        }
        let mut sorted = hits.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(hits, sorted, "hits must be sorted/deduped");
        assert!(hits.len() <= MAX_LABEL_HITS);

        // Full gate: a Premium-naming label hit folds into the badge leg, so
        // `evaluate` passes exactly like a badge hit (playable + home ok).
        let folded_badge = match (parse_premium_badge(&home), parse_premium_label(&home, &explore, Some(&account))) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (b, _) => b,
        };
        assert_eq!(folded_badge, Some(true));
        let signals = PremiumSignals {
            logged_in: true,
            home_ok: true,
            premium_badge: folded_badge,
            explore_top_songs: parse_explore_top_songs(&explore),
            ads_absent: None,
            topbar_premium: None,
            hires_audio: None,
            playability: Playability::Playable,
        };
        assert_eq!(evaluate(&signals), GateStatus::Premium);

        // Report carries the evidence and renders it secret-free.
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&signals, SystemTime::now());
        let mut report = ProbeReport::from_signals(&signals, &gate);
        report.label_hits = hits.clone();
        let text = report.render();
        assert!(text.contains("badge=pass") && !text.contains("labels="));
        assert!(!text.contains("YouTube Premium"), "account labels stay out of the default log");
        for secret in ["cookie", "sapisid", "fake-secret", "example.com"] {
            assert!(!text.to_ascii_lowercase().contains(secret));
        }
    }

    #[test]
    fn deep_label_search_ignores_upsell_and_mangled() {
        let home = home_free_with_upsell();
        let explore = explore_free_grid_shape();
        let account = account_menu_free();

        // Upsell copy lives in title/description/subtitle runs (keys `text`),
        // never in an accessibility `label`: evidence may list it, but the
        // strict check must NOT fire (fail-open guard).
        assert_eq!(parse_premium_badge(&home), None);
        assert_eq!(parse_premium_label(&home, &explore, Some(&account)), None);
        let hits = collect_label_hits(&home, &explore, Some(&account));
        // Full gate stays blocked fail-closed.
        let signals = PremiumSignals {
            logged_in: true,
            home_ok: true,
            premium_badge: parse_premium_badge(&home),
            explore_top_songs: parse_explore_top_songs(&explore),
            ads_absent: None,
            topbar_premium: None,
            hires_audio: None,
            playability: Playability::Playable,
        };
        assert_eq!(signals.explore_top_songs, Some(false));
        assert_eq!(evaluate(&signals), GateStatus::UpgradeRequired);

        // Secret-bearing candidates never enter evidence, values capped.
        let sneaky_home = serde_json::json!({
            "contents": {"sectionListRenderer": {"contents": []}},
            "continuation": "fake-continuation-abc",
            "trackingParams": "fake-tracking-xyz",
            "thumbnail": {"thumbnails": [{"url": "https://example.com/x"}]},
        });
        let sneaky_hits = collect_label_hits(
            &sneaky_home,
            &serde_json::json!({"label": "fake-token-value"}),
            None,
        );
        assert!(sneaky_hits.is_empty(), "secrets must be filtered: {sneaky_hits:?}");
        assert!(hits.iter().all(|h| h.chars().count() <= MAX_LABEL_CHARS));

        // Mangled shapes: no panic, fail-closed, empty evidence.
        for shape in [
            serde_json::json!(null),
            serde_json::json!("Premium"),
            serde_json::json!("Get Music Premium"),
            serde_json::json!(42),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!({"contents": {}}),
            serde_json::json!({"label": 42}),
            serde_json::json!({"label": null}),
            serde_json::json!({"text": ["not", "a", "string"]}),
        ] {
            let hits = collect_label_hits(&shape, &shape, Some(&shape));
            assert!(hits.iter().all(|h| h.chars().count() <= MAX_LABEL_CHARS));
            assert_eq!(
                parse_premium_label(&shape, &shape, Some(&shape)),
                None,
                "mangled must stay fail-closed: {shape}"
            );
            let signals = PremiumSignals {
                premium_badge: parse_premium_label(&shape, &shape, Some(&shape)),
                explore_top_songs: None,
                ..premium_signals()
            };
            assert_eq!(evaluate(&signals), GateStatus::UpgradeRequired);
        }
        // Bare-string "Premium" root is NOT a label source (no `label` key).
        assert_eq!(
            parse_premium_label(
                &serde_json::json!("Premium"),
                &serde_json::json!({}),
                None
            ),
            None
        );
    }

    /// Minimal `ytInitialData`-shaped fixture: ONLY the topbar logo renderer
    /// varies — the yt-dlp-verified Premium/Free shapes plus the tooltip
    /// variant. Synthetic content, no secrets.
    fn initial_data_with_logo(logo: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "contents": {"sectionListRenderer": {"contents": []}},
            "topbar": {
                "desktopTopbarRenderer": {
                    "logo": {"topbarLogoRenderer": logo},
                },
            },
        })
    }

    #[test]
    fn parse_topbar_premium_matches_icon_and_tooltip_only() {
        // Premium-shape: exact `YOUTUBE_PREMIUM_LOGO` iconType → premium.
        let premium = initial_data_with_logo(serde_json::json!({
            "iconType": "YOUTUBE_PREMIUM_LOGO",
            "tooltipText": "YouTube Premium",
        }));
        assert_eq!(parse_topbar_premium(&premium), Some(true));
        // Icon alone suffices (no tooltip).
        let icon_only = initial_data_with_logo(serde_json::json!({
            "iconType": "YOUTUBE_PREMIUM_LOGO",
        }));
        assert_eq!(parse_topbar_premium(&icon_only), Some(true));
        // Tooltip variant: free icon but Premium-naming tooltip → premium
        // (plain string, `simpleText`, and `runs` forms).
        for tooltip in [
            serde_json::json!("YouTube Premium"),
            serde_json::json!({"simpleText": "Get YouTube Premium"}),
            serde_json::json!({"runs": [{"text": "YouTube"}, {"text": " Premium"}]}),
        ] {
            let shape = initial_data_with_logo(serde_json::json!({
                "iconType": "YOUTUBE_LOGO",
                "tooltipText": tooltip,
            }));
            assert_eq!(
                parse_topbar_premium(&shape),
                Some(true),
                "premium tooltip must hit: {tooltip}"
            );
        }
        // Free-shape: concrete non-premium logo → free (fail-closed `false`).
        let free = initial_data_with_logo(serde_json::json!({
            "iconType": "YOUTUBE_LOGO",
            "tooltipText": "YouTube",
        }));
        assert_eq!(parse_topbar_premium(&free), Some(false));
        // Free-shape via `simpleText` tooltip alone (no iconType).
        let free_tip = initial_data_with_logo(serde_json::json!({
            "tooltipText": {"simpleText": "YouTube Home"},
        }));
        assert_eq!(parse_topbar_premium(&free_tip), Some(false));
        // Lookalike keys NEVER count: `iconImageId`, titles, labels outside
        // the logo renderer are not signals.
        for decoy in [
            serde_json::json!({"iconImageId": "YOUTUBE_PREMIUM_LOGO"}),
            serde_json::json!({"iconType": "youtube_premium_logo"}),
            serde_json::json!({"title": "YouTube Premium"}),
        ] {
            let shape = initial_data_with_logo(decoy);
            assert!(
                parse_topbar_premium(&shape) != Some(true),
                "decoy shape must never read as premium: {shape}"
            );
        }
        // Full gate: topbar hit carries Premium with badge+explore unknown
        // (and the demoted ads leg present — it neither helps nor vetoes).
        for ads in [None, Some(true), Some(false)] {
            let signals = PremiumSignals {
                premium_badge: None,
                explore_top_songs: None,
                ads_absent: ads,
                topbar_premium: parse_topbar_premium(&premium),
                ..premium_signals()
            };
            assert_eq!(signals.topbar_premium, Some(true));
            assert_eq!(evaluate(&signals), GateStatus::Premium);
            assert_eq!(deciding_leg(&signals), "topbar");
        }
        // Full gate: free-shape topbar blocks even when playable.
        let free_signals = PremiumSignals {
            premium_badge: None,
            explore_top_songs: None,
            topbar_premium: parse_topbar_premium(&free),
            ..premium_signals()
        };
        assert_eq!(evaluate(&free_signals), GateStatus::UpgradeRequired);
        assert_eq!(deciding_leg(&free_signals), "none");
    }

    #[test]
    fn parse_topbar_premium_mangled_is_fail_closed() {
        // Missing topbar/logo/renderer, wrong types, empty renderer: all
        // ambiguous (`None`), never premium, never panic.
        let mangled = [
            serde_json::json!(null),
            serde_json::json!("YOUTUBE_PREMIUM_LOGO"),
            serde_json::json!(42),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!({"contents": {}}),
            serde_json::json!({"topbar": null}),
            serde_json::json!({"topbar": {"desktopTopbarRenderer": null}}),
            serde_json::json!({"topbar": {"desktopTopbarRenderer": {"logo": null}}}),
            serde_json::json!({"topbar": {"desktopTopbarRenderer": {"logo": []}}}),
            serde_json::json!({"topbar": {"desktopTopbarRenderer": {"logo": {"topbarLogoRenderer": null}}}}),
            serde_json::json!({"topbar": {"desktopTopbarRenderer": {"logo": {"topbarLogoRenderer": {}}}}}),
            serde_json::json!({"topbar": {"desktopTopbarRenderer": {"logo": {"topbarLogoRenderer": {"iconType": 42, "tooltipText": null}}}}}),
            serde_json::json!({"topbar": {"desktopTopbarRenderer": {"logo": {"topbarLogoRenderer": {"tooltipText": ""}}}}}),
        ];
        for shape in &mangled {
            let parsed = parse_topbar_premium(shape);
            assert!(
                parsed != Some(true),
                "mangled topbar must never read as premium: {shape}"
            );
            // No panic above; the gate stays fail-closed either way.
            let signals = PremiumSignals {
                premium_badge: None,
                explore_top_songs: None,
                topbar_premium: parsed,
                ..premium_signals()
            };
            assert_eq!(evaluate(&signals), GateStatus::UpgradeRequired);
            assert_eq!(deciding_leg(&signals), "none");
        }
        // OR-fold: `Some(true)` wins from either side; known beats unknown.
        assert_eq!(or_premium(Some(true), Some(false)), Some(true));
        assert_eq!(or_premium(Some(false), Some(true)), Some(true));
        assert_eq!(or_premium(Some(false), None), Some(false));
        assert_eq!(or_premium(None, Some(false)), Some(false));
        assert_eq!(or_premium(None, None), None);
    }

    /// Raw HIGH-quality `player` fixture: `playabilityStatus` (survives in
    /// both raw + `get_song` forms) + a raw marker (`responseContext`, which
    /// `get_song` strips) + the audio ladder under test. Synthetic content,
    /// no secrets — only the `streamingData.adaptiveFormats` SHAPE mirrors
    /// InnerTube (`itag` / `bitrate` / `mimeType` entries).
    fn player_with_audio(formats: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "responseContext": {"serviceTrackingParams": []},
            "streamingData": {"adaptiveFormats": formats},
        })
    }

    #[test]
    fn parse_hires_audio_detects_itag_141_and_high_bitrate() {
        // itag 141 (numeric) → hires even with low siblings around it.
        let hires = player_with_audio(serde_json::json!([
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 128000},
            {"itag": 141, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 256000},
        ]));
        assert_eq!(parse_hires_audio(&hires), Some(true));
        // String-encoded itag/bitrate (InnerTube serves both forms) → hires.
        let hires_str = player_with_audio(serde_json::json!([
            {"itag": "141", "mimeType": "audio/mp4", "bitrate": "256000"},
        ]));
        assert_eq!(parse_hires_audio(&hires_str), Some(true));
        // Missing bitrate on a 141 entry still hits via itag alone.
        let itag_only = player_with_audio(serde_json::json!([
            {"itag": 141, "mimeType": "audio/mp4"},
        ]));
        assert_eq!(parse_hires_audio(&itag_only), Some(true));
        // No 141 but max audio bitrate ≥ HIRES_MIN_BITRATE_BPS → HIGH honored.
        let hires_bitrate = player_with_audio(serde_json::json!([
            {"itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 160000},
            {"itag": 774, "mimeType": "audio/mp4", "bitrate": 256000},
        ]));
        assert_eq!(parse_hires_audio(&hires_bitrate), Some(true));
        // Threshold boundary: exactly 200_000 counts, one below does not.
        let at = player_with_audio(serde_json::json!([
            {"itag": 250, "mimeType": "audio/webm", "bitrate": 200000},
        ]));
        assert_eq!(parse_hires_audio(&at), Some(true));
        let below = player_with_audio(serde_json::json!([
            {"itag": 250, "mimeType": "audio/webm", "bitrate": 199999},
        ]));
        assert_eq!(parse_hires_audio(&below), Some(false));

        // Full gate: the hires leg ALONE carries Premium (badge / explore /
        // topbar all unknown — the CONFIRMED-Premium live shape).
        let audio_only = PremiumSignals {
            premium_badge: None,
            explore_top_songs: None,
            topbar_premium: None,
            hires_audio: parse_hires_audio(&hires),
            ..premium_signals()
        };
        assert_eq!(audio_only.hires_audio, Some(true));
        assert_eq!(evaluate(&audio_only), GateStatus::Premium);
        assert_eq!(deciding_leg(&audio_only), "audio");

        // Report renders the audio leg (secret-free: itags/bitrates only).
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&audio_only, SystemTime::now());
        let report = ProbeReport::from_signals(&audio_only, &gate);
        assert_eq!(report.status, GateStatus::Premium);
        let text = report.render();
        assert!(text.starts_with("gate=Premium via=audio"), "{text}");
        assert!(text.contains("audio=pass"), "{text}");
    }

    #[test]
    fn parse_hires_audio_capped_ladder_blocks() {
        // Capped free-shape ladder (128k AAC + ≤160k Opus, no 141,
        // nothing ≥ 200k) → free-shape. The high-bitrate VIDEO entry must
        // not count (mime gate: audio only).
        let capped = player_with_audio(serde_json::json!([
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 128000},
            {"itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 160000},
            {"itag": 22, "mimeType": "video/mp4", "bitrate": 2000000},
        ]));
        assert_eq!(parse_hires_audio(&capped), Some(false));
        // Video-only ladder at any bitrate is not audio evidence → unknown.
        let video_only = player_with_audio(serde_json::json!([
            {"itag": 22, "mimeType": "video/mp4", "bitrate": 5000000},
        ]));
        assert_eq!(parse_hires_audio(&video_only), None);
        // Full gate stays blocked fail-closed on the capped ladder.
        let blocked = PremiumSignals {
            premium_badge: None,
            explore_top_songs: None,
            topbar_premium: None,
            hires_audio: parse_hires_audio(&capped),
            ..premium_signals()
        };
        assert_eq!(evaluate(&blocked), GateStatus::UpgradeRequired);
        assert_eq!(deciding_leg(&blocked), "none");
    }

    #[test]
    fn parse_hires_audio_mangled_is_fail_closed() {
        let mangled = [
            serde_json::json!(null),
            serde_json::json!("audio"),
            serde_json::json!(141),
            serde_json::json!(42),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!({"streamingData": null}),
            serde_json::json!({"streamingData": {}}),
            serde_json::json!({"streamingData": {"adaptiveFormats": []}}),
            serde_json::json!({"streamingData": {"adaptiveFormats": null}}),
            serde_json::json!({"streamingData": {"adaptiveFormats": "141"}}),
            serde_json::json!({"streamingData": {"adaptiveFormats": [null, 42, "x"]}}),
            // itag 141 WITHOUT an audio mimeType is not audio evidence
            // (strict mime gate — a bare number never reads as premium).
            serde_json::json!({"streamingData": {"adaptiveFormats": [{"itag": 141}]}}),
            serde_json::json!({"streamingData": {"adaptiveFormats": [{"itag": 141, "mimeType": "video/mp4", "bitrate": 256000}]}}),
        ];
        for shape in &mangled {
            let parsed = parse_hires_audio(shape);
            assert!(
                parsed != Some(true),
                "mangled player shape must never read as hires: {shape}"
            );
            // No panic above; the gate stays fail-closed either way.
            let signals = PremiumSignals {
                premium_badge: None,
                explore_top_songs: None,
                topbar_premium: None,
                hires_audio: parsed,
                ..premium_signals()
            };
            assert_eq!(evaluate(&signals), GateStatus::UpgradeRequired);
        }
        // Evidence collection on mangled shapes never panics and stays
        // numeric (only itag/bitrate numbers ever leave this module).
        for shape in &mangled {
            for (itag, bitrate) in collect_audio_formats(shape) {
                assert!(itag > 0, "unexpected itag: {itag}");
                assert!(bitrate <= 10_000_000, "unexpected bitrate: {bitrate}");
            }
        }
    }

    #[test]
    fn audio_dump_line_lists_numbers_only_and_topbar_shape_shares_denylist() {
        // Ladder with secret-carrying URL values alongside the numbers: the
        // dump line must carry itag@bitrate pairs ONLY.
        let player = serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "responseContext": {"serviceTrackingParams": []},
            "streamingData": {"adaptiveFormats": [
                {"itag": 140, "mimeType": "audio/mp4", "bitrate": 128000, "url": "https://example.com/secret-bytes"},
                {"itag": 141, "mimeType": "audio/mp4", "bitrate": 256000, "url": "https://example.com/more-secret"},
            ]},
        });
        assert_eq!(
            collect_audio_formats(&player),
            vec![(140, 128000), (141, 256000)]
        );
        let line = render_audio_line(Some(&player));
        assert_eq!(line, "audio: [140@128000,141@256000]", "{line}");
        assert!(!line.contains("example.com"), "audio line leaked a URL: {line}");
        assert_eq!(render_audio_line(None), "audio: unknown");
        assert_eq!(
            render_audio_line(Some(&serde_json::json!({}))),
            "audio: unknown"
        );
        // Topbar skeleton: keys + short labels survive, secrets never appear
        // (same denylist `dump_shape` + `collect_label_hits` share).
        let page = serde_json::json!({
            "topbar": {"desktopTopbarRenderer": {
                "logo": {"topbarLogoRenderer": {
                    "iconType": "YOUTUBE_PREMIUM_LOGO",
                    "tooltipText": "YouTube Premium",
                }},
            }},
            "cookie": "fake-secret-cookie",
        });
        let shape = dump_topbar_shape(&serde_json::json!({}), std::slice::from_ref(&page));
        assert!(shape.starts_with("topbar_shape: "), "{shape}");
        assert!(shape.contains("topbarLogoRenderer"), "{shape}");
        // `iconType`/`tooltipText` names survive AND their short chrome values
        // print (see `CHROME_VALUE_KEYS`): ground truth for the ACTUAL icon
        // type on each front page; the pass/fail reading itself stays on the
        // `topbar:` dump line.
        assert!(shape.contains("iconType"), "{shape}");
        assert!(shape.contains("tooltipText"), "{shape}");
        assert!(shape.contains("YOUTUBE_PREMIUM_LOGO"), "{shape}");
        assert!(shape.contains("YouTube Premium"), "{shape}");
        for denied in ["cookie", "fake-secret-cookie", "sapisid", "continuation"] {
            assert!(
                !shape.to_ascii_lowercase().contains(denied),
                "topbar_shape leaked {denied}: {shape}"
            );
        }
        // Browse-home fallback when no front page was fetched.
        let home_shape = dump_topbar_shape(&page, &[]);
        assert!(home_shape.contains("topbarLogoRenderer"), "{home_shape}");
        // No topbar anywhere → unknown (fail-closed upstream).
        assert_eq!(
            dump_topbar_shape(&serde_json::json!({}), &[]),
            "topbar_shape: unknown"
        );
        assert_eq!(
            dump_topbar_shape(&serde_json::json!(null), &[serde_json::json!(null)]),
            "topbar_shape: unknown"
        );
    }

    #[test]
    fn parse_topbar_premium_matches_nested_icon_image_path() {
        // CORRECTED live path (youtube.com front-page ground truth): the icon
        // type nests under `iconImage` — a direct-`iconType`-only check
        // misreads every such Premium topbar as free-shape.
        let nested_premium = initial_data_with_logo(serde_json::json!({
            "iconImage": {"iconType": "YOUTUBE_PREMIUM_LOGO"},
            "tooltipText": "YouTube Ana Sayfası",
        }));
        assert_eq!(parse_topbar_premium(&nested_premium), Some(true));
        // Nested icon alone suffices (Turkish tooltip names no Premium).
        let nested_icon_only = initial_data_with_logo(serde_json::json!({
            "iconImage": {"iconType": "YOUTUBE_PREMIUM_LOGO"},
        }));
        assert_eq!(parse_topbar_premium(&nested_icon_only), Some(true));
        // Nested free-shape: concrete non-premium nested icon → fail-closed
        // `false` (NOT unknown — the logo parsed and is not premium).
        let nested_free = initial_data_with_logo(serde_json::json!({
            "iconImage": {"iconType": "YOUTUBE_LOGO"},
            "tooltipText": "YouTube Ana Sayfası",
        }));
        assert_eq!(parse_topbar_premium(&nested_free), Some(false));
        // Full gate: nested hit carries Premium with badge+explore unknown.
        let signals = PremiumSignals {
            premium_badge: None,
            explore_top_songs: None,
            topbar_premium: parse_topbar_premium(&nested_premium),
            ..premium_signals()
        };
        assert_eq!(evaluate(&signals), GateStatus::Premium);
        assert_eq!(deciding_leg(&signals), "topbar");
        // Full gate: nested free-shape blocks.
        let free_signals = PremiumSignals {
            premium_badge: None,
            explore_top_songs: None,
            topbar_premium: parse_topbar_premium(&nested_free),
            ..premium_signals()
        };
        assert_eq!(evaluate(&free_signals), GateStatus::UpgradeRequired);
        // Mangled nested shapes: never premium, never panic.
        let mangled = [
            serde_json::json!({"iconImage": null}),
            serde_json::json!({"iconImage": []}),
            serde_json::json!({"iconImage": "YOUTUBE_PREMIUM_LOGO"}),
            serde_json::json!({"iconImage": {}}),
            serde_json::json!({"iconImage": {"iconType": null}}),
            serde_json::json!({"iconImage": {"iconType": 42}}),
            serde_json::json!({"iconImage": {"iconType": ""}}),
            serde_json::json!({"iconImage": {"iconType": "youtube_premium_logo"}}),
            serde_json::json!({"iconImageId": "YOUTUBE_PREMIUM_LOGO"}),
        ];
        for logo in &mangled {
            let shape = initial_data_with_logo(logo.clone());
            let parsed = parse_topbar_premium(&shape);
            assert!(
                parsed != Some(true),
                "mangled nested topbar must never read as premium: {shape}"
            );
            let signals = PremiumSignals {
                premium_badge: None,
                explore_top_songs: None,
                topbar_premium: parsed,
                ..premium_signals()
            };
            assert_eq!(evaluate(&signals), GateStatus::UpgradeRequired);
        }
    }

    #[test]
    fn shape_dump_prints_chrome_values_but_not_secrets() {
        // Live youtube.com shape: nested icon type + Turkish tooltip must
        // surface as VALUES so a re-probe reveals the actual icon type.
        let page = serde_json::json!({
            "topbar": {"desktopTopbarRenderer": {
                "logo": {"topbarLogoRenderer": {
                    "iconImage": {"iconType": "YOUTUBE_LOGO"},
                    "tooltipText": "YouTube Ana Sayfası",
                }},
            }},
        });
        let shape = dump_topbar_shape(&serde_json::json!({}), std::slice::from_ref(&page));
        assert!(shape.contains("YOUTUBE_LOGO"), "nested icon value must print: {shape}");
        assert!(
            shape.contains("YouTube Ana Sayfası"),
            "plain tooltip value must print: {shape}"
        );
        // Direct-iconType premium value prints too.
        let direct = serde_json::json!({
            "topbar": {"desktopTopbarRenderer": {
                "logo": {"topbarLogoRenderer": {"iconType": "YOUTUBE_PREMIUM_LOGO"}},
            }},
        });
        let direct_shape =
            dump_topbar_shape(&serde_json::json!({}), std::slice::from_ref(&direct));
        assert!(direct_shape.contains("YOUTUBE_PREMIUM_LOGO"), "{direct_shape}");
        // Exact-key allowlist: `iconImageId` values stay tags (never evidence
        // for the gate — decoy guard at the dump layer too).
        let decoy = serde_json::json!({
            "topbar": {"desktopTopbarRenderer": {
                "logo": {"topbarLogoRenderer": {"iconImageId": "YOUTUBE_PREMIUM_LOGO"}},
            }},
        });
        let decoy_shape =
            dump_topbar_shape(&serde_json::json!({}), std::slice::from_ref(&decoy));
        assert!(
            !decoy_shape.contains("YOUTUBE_PREMIUM_LOGO"),
            "iconImageId value must stay a tag: {decoy_shape}"
        );
        // Secret denylist still wins over the chrome allowlist (keys AND
        // values): a secret-looking chrome value renders as a tag, and
        // secret subtrees never appear even beside chrome keys.
        let sneaky = serde_json::json!({
            "topbar": {"desktopTopbarRenderer": {
                "logo": {"topbarLogoRenderer": {
                    "iconType": "YOUTUBE_PREMIUM_LOGO",
                    "tooltipText": "https://example.com/sneaky",
                }},
                "trackingParams": "fake-tracking-xyz",
            }},
            "cookie": "fake-secret-cookie",
        });
        let sneaky_shape =
            dump_topbar_shape(&serde_json::json!({}), std::slice::from_ref(&sneaky));
        assert!(sneaky_shape.contains("YOUTUBE_PREMIUM_LOGO"), "{sneaky_shape}");
        for denied in [
            "example.com",
            "fake-tracking-xyz",
            "fake-secret-cookie",
            "trackingparams",
            "cookie",
        ] {
            assert!(
                !sneaky_shape.to_ascii_lowercase().contains(denied),
                "topbar_shape leaked {denied}: {sneaky_shape}"
            );
        }
        // Chrome values never enter `label_hits` (dump-only evidence).
        let hits = collect_label_hits(&page, &serde_json::json!({}), None);
        assert!(
            !hits.iter().any(|h| h == "YOUTUBE_LOGO"),
            "chrome constants must not pollute label_hits: {hits:?}"
        );
    }

    /// Logged-in `ytInitialData` fixture: topbar avatar (account photo) +
    /// account name, Premium nested icon, NO "Sign in" text.
    fn front_page_authed_premium() -> serde_json::Value {
        serde_json::json!({
            "contents": {"sectionListRenderer": {"contents": []}},
            "topbar": {"desktopTopbarRenderer": {
                "avatar": {"avatarViewModel": {"image": {"sources": [{"url": "https://example-photo.invalid/a.jpg"}]}}},
                "logo": {"topbarLogoRenderer": {
                    "iconImage": {"iconType": "YOUTUBE_PREMIUM_LOGO"},
                    "tooltipText": "YouTube Premium",
                }},
            }},
            "actions": [{"getMultiPageMenuAction": {"menu": {"multiPageMenuRenderer": {
                "header": {"activeAccountHeaderRenderer": {"accountName": {"simpleText": "Test User"}}},
            }}}}],
        })
    }

    /// Logged-out `ytInitialData` fixture: sign-in button, free logo, NO
    /// avatar / account name (the shape a mismatched-origin fetch returns).
    fn front_page_logged_out_free() -> serde_json::Value {
        serde_json::json!({
            "contents": {"sectionListRenderer": {"contents": []}},
            "topbar": {"desktopTopbarRenderer": {
                "logo": {"topbarLogoRenderer": {
                    "iconImage": {"iconType": "YOUTUBE_LOGO"},
                    "tooltipText": "YouTube",
                }},
                "button": {"buttonRenderer": {"text": {"simpleText": "Sign in"}}},
            }},
        })
    }

    #[test]
    fn front_page_authed_requires_avatar_and_no_signin() {
        // Logged-in page (avatar + account name, no sign-in) → authed.
        assert!(parse_front_page_authed(&front_page_authed_premium()));
        // Logged-out page (sign-in, no avatar) → not authed.
        assert!(!parse_front_page_authed(&front_page_logged_out_free()));
        // Sign-in veto wins even when avatar/account markers are present
        // (crafted contradictory shape fails closed to logged-out).
        let mut both = front_page_authed_premium();
        both.pointer_mut("/topbar/desktopTopbarRenderer")
            .expect("fixture topbar")
            .as_object_mut()
            .expect("topbar object")
            .insert(
                "button".into(),
                serde_json::json!({"buttonRenderer": {"text": {"simpleText": "Sign in"}}}),
            );
        assert!(!parse_front_page_authed(&both));
        // Avatar alone suffices (account name is a second independent path).
        let avatar_only = serde_json::json!({
            "topbar": {"desktopTopbarRenderer": {
                "avatar": {"avatarViewModel": {"image": "photo-bytes-evidence-only"}},
                "logo": {"topbarLogoRenderer": {"iconType": "YOUTUBE_LOGO"}},
            }},
        });
        assert!(parse_front_page_authed(&avatar_only));
        // Account name alone suffices.
        let name_only = serde_json::json!({
            "header": {"activeAccountHeaderRenderer": {"accountName": {"simpleText": "Test User"}}},
        });
        assert!(parse_front_page_authed(&name_only));
        // Mangled / empty / avatar-null shapes: never authed, never panic.
        for shape in [
            serde_json::json!(null),
            serde_json::json!("Sign in"),
            serde_json::json!(42),
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!({"topbar": {"desktopTopbarRenderer": {"avatar": null}}}),
            serde_json::json!({"topbar": {"desktopTopbarRenderer": {"logo": {"topbarLogoRenderer": {"iconType": "YOUTUBE_PREMIUM_LOGO"}}}}}),
        ] {
            assert!(
                !parse_front_page_authed(&shape),
                "mangled page must read logged-out: {shape}"
            );
        }
    }

    #[test]
    fn logged_out_fetch_yields_no_topbar_verdict() {
        // The lane's fail-closed core: a Premium LOGO on a logged-out page
        // is NOT a signal (raw `parse_topbar_premium` still sees the logo,
        // the gated read returns unknown).
        let logged_out_premium_logo = serde_json::json!({
            "topbar": {"desktopTopbarRenderer": {
                "logo": {"topbarLogoRenderer": {
                    "iconImage": {"iconType": "YOUTUBE_PREMIUM_LOGO"},
                }},
                "button": {"buttonRenderer": {"text": {"simpleText": "Sign in"}}},
            }},
        });
        assert_eq!(parse_topbar_premium(&logged_out_premium_logo), Some(true));
        assert_eq!(parse_topbar_premium_if_authed(&logged_out_premium_logo), None);
        // Same for the free shape: logged-out must be unknown, NEVER fail.
        assert_eq!(parse_topbar_premium(&front_page_logged_out_free()), Some(false));
        assert_eq!(parse_topbar_premium_if_authed(&front_page_logged_out_free()), None);
        // Authed pages pass through untouched (both verdicts preserved).
        let authed = front_page_authed_premium();
        assert_eq!(parse_topbar_premium_if_authed(&authed), Some(true));
        let mut authed_free = front_page_authed_premium();
        *authed_free
            .pointer_mut("/topbar/desktopTopbarRenderer/logo/topbarLogoRenderer/iconImage/iconType")
            .expect("fixture icon") = serde_json::json!("YOUTUBE_LOGO");
        *authed_free
            .pointer_mut("/topbar/desktopTopbarRenderer/logo/topbarLogoRenderer/tooltipText")
            .expect("fixture tooltip") = serde_json::json!("YouTube");
        assert_eq!(parse_topbar_premium(&authed_free), Some(false));
        assert_eq!(parse_topbar_premium_if_authed(&authed_free), Some(false));
        // Full gate: gated-unknown from both front pages blocks (browse home
        // without a topbar stays unknown — the old code read this as fail).
        let signals = PremiumSignals {
            premium_badge: None,
            explore_top_songs: None,
            topbar_premium: parse_topbar_premium_if_authed(&front_page_logged_out_free()),
            ..premium_signals()
        };
        assert_eq!(signals.topbar_premium, None);
        assert_eq!(evaluate(&signals), GateStatus::UpgradeRequired);
    }

    #[test]
    fn topbar_icon_reports_nested_path_per_page() {
        // Nested (CORRECTED) path wins per page.
        let nested = initial_data_with_logo(serde_json::json!({
            "iconImage": {"iconType": "YOUTUBE_PREMIUM_LOGO"},
            "iconType": "YOUTUBE_LOGO",
        }));
        assert_eq!(topbar_icon_type(&nested).as_deref(), Some("YOUTUBE_PREMIUM_LOGO"));
        // Direct-only legacy shape still reports.
        let direct = initial_data_with_logo(serde_json::json!({"iconType": "YOUTUBE_LOGO"}));
        assert_eq!(topbar_icon_type(&direct).as_deref(), Some("YOUTUBE_LOGO"));
        // Missing/mangled → None (dump renders `missing`).
        for shape in [
            serde_json::json!(null),
            serde_json::json!({}),
            initial_data_with_logo(serde_json::json!({})),
            initial_data_with_logo(serde_json::json!({"iconImage": null})),
            initial_data_with_logo(serde_json::json!({"iconImageId": "YOUTUBE_PREMIUM_LOGO"})),
        ] {
            assert!(topbar_icon_type(&shape).is_none(), "icon must be missing: {shape}");
        }
        // Dump lines: exact per-URL format the live probe prints.
        assert_eq!(render_front_auth_line(true, false), "front_auth: music=true youtube=false");
        assert_eq!(render_front_auth_line(false, false), "front_auth: music=false youtube=false");
        assert_eq!(
            render_front_icons_line(Some("YOUTUBE_PREMIUM_LOGO"), Some("YOUTUBE_LOGO")),
            "front_icons: music=YOUTUBE_PREMIUM_LOGO youtube=YOUTUBE_LOGO"
        );
        assert_eq!(
            render_front_icons_line(None, None),
            "front_icons: music=missing youtube=missing"
        );
    }

    #[test]
    fn altclient_line_compares_ladder_and_ads() {
        // Alt response with ad markers + capped ladder: both sides of the
        // comparison print (names + numbers only).
        let alt = serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "responseContext": {"serviceTrackingParams": []},
            "adPlacements": [{"adUrl": "https://example.com/secret-ad-bytes"}],
            "streamingData": {"adaptiveFormats": [
                {"itag": 140, "mimeType": "audio/mp4", "bitrate": 128000, "url": "https://example.com/secret-bytes"},
                {"itag": 251, "mimeType": "audio/webm", "bitrate": 160000},
            ]},
        });
        let line = render_altclient_line("ANDROID_MUSIC", "7.21.50", Some(&alt));
        assert_eq!(
            line,
            "altclient: ANDROID_MUSIC/7.21.50 ads=[\"adPlacements\"] audio=[140@128000,251@160000]",
            "{line}"
        );
        assert!(!line.contains("example.com"), "altclient line leaked a URL: {line}");
        // Alt response without ads but high ladder: empty ads list + ladder.
        let alt_clean = serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "responseContext": {"serviceTrackingParams": []},
            "streamingData": {"adaptiveFormats": [
                {"itag": 141, "mimeType": "audio/mp4", "bitrate": 256000},
            ]},
        });
        let clean = render_altclient_line("ANDROID_MUSIC", "7.21.50", Some(&alt_clean));
        assert_eq!(
            clean,
            "altclient: ANDROID_MUSIC/7.21.50 ads=[] audio=[141@256000]",
            "{clean}"
        );
        // Failed alt leg: honest unknown (error/match/mismatch unresolved).
        assert_eq!(
            render_altclient_line("ANDROID_MUSIC", "7.21.50", None),
            "altclient: ANDROID_MUSIC/7.21.50 unknown"
        );
        // Mangled alt shapes: no panic, secret-free, never gate (the line is
        // evidence-only — `evaluate` has no alt leg).
        for shape in [
            serde_json::json!(null),
            serde_json::json!({}),
            serde_json::json!([]),
            serde_json::json!({"streamingData": null}),
        ] {
            let line = render_altclient_line("ANDROID_MUSIC", "7.21.50", Some(&shape));
            assert!(line.starts_with("altclient: ANDROID_MUSIC/7.21.50 "), "{line}");
            assert!(!line.contains("example.com"));
        }
    }
}
