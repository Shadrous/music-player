//! `src/auth` — browser-cookie login, session restore, and logout over
//! `ytmusic-rs`.
//!
//! Handover notes:
//! - Transport: `ytmusic-rs` (`ytmusicapi::YTMusic`) emulates the YouTube Music
//!   web client against the private InnerTube API (browse/player calls). Our
//!   supplementary `reqwest` wiring lives in [`http`]; the async runtime is
//!   `tokio` (see `crate::premium_gate` live probe).
//! - Secrets: the raw `browser.json` cookie blob lives ONLY in the OS keyring
//!   via [`SecretStore`] ([`KeyringStore`] in prod). It is never written to a
//!   plaintext file, never printed, and every log helper here redacts
//!   cookie / SAPISID / authorization / token values ([`REDACTED`]).
//! - `YTMusic::with_auth` accepts a path *or* raw JSON; we always pass raw JSON
//!   from the keyring so no credential file ever exists on disk.
//! - Channel switch maps to InnerTube `onBehalfOfUser` via `YTMusic::set_user`
//!   (brand-account switching).
//!
//! ## Browser sign-in (Sonora-style)
//!
//! The primary login path opens an ephemeral WebKitGTK sign-in window through
//! [`BROWSER_LOGIN_HELPER`] (Python + PyGObject, the same engine Sonora uses
//! on Linux). The helper polls its own cookie manager and prints the
//! `Cookie` request-header value once the `__Secure-3PAPISID` proof cookie is
//! present, then exits. [`login_from_cookie_header`] normalizes the header,
//! synthesizes the `browser.json` blob (`ytmusic-rs` regenerates the real
//! `SAPISIDHASH` per request from that cookie), validates it, and persists it
//! to the OS keyring only.
//!
//! The fallback path accepts the same `Cookie` request-header value pasted
//! from the user's own browser (no DevTools `authorization` copy needed).
//!
//! Secrets policy: cookie headers go straight to the keyring; they are never
//! logged, never written elsewhere, and every error/log path here emits
//! [`REDACTED`] instead of values.
//!
//! Live identity comes from `get_account_info` via [`fetch_account_name`]
//! (called in `Backend::apply_probe` after restore/login); `Session`
//! itself stays identity-free offline.

pub mod http;

use ytmusicapi::{AuthHeaders, YTMusic};

/// OS-keyring identity for the stored browser cookie blob.
pub const KEYRING_SERVICE: &str = "music-player";
/// OS-keyring username for the stored browser cookie blob.
pub const KEYRING_USER_COOKIE: &str = "ytmusic-browser-cookie";

/// The cookie that proves a signed-in YouTube session and that `ytmusic-rs`
/// hashes into `SAPISIDHASH` for every request. Without it a cookie header is
/// not a session, so login rejects it before touching the keyring.
pub const PROOF_COOKIE: &str = "__Secure-3PAPISID";

/// Origin sent with InnerTube requests and used for the browser-auth hash.
pub const MUSIC_ORIGIN: &str = "https://music.youtube.com";

/// Ephemeral WebKitGTK sign-in helper (see the file header). It is passed to
/// `python3 -c`, so no helper file is written to disk and the cookie header
/// travels only through the child's stdout pipe.
pub const BROWSER_LOGIN_HELPER: &str = include_str!("../browser_login_helper.py");

/// Auth failures. Never carries secret material — only static text plus
/// non-sensitive keyring/transport summaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// OS-keyring operation failed (message is the keyring summary, no secrets).
    Keyring(String),
    /// `YTMusic` client construction failed (static text; raw input withheld).
    Client(String),
    /// Pasted headers lack browser-cookie credentials.
    MissingCookie,
    /// Operation needs a logged-in session.
    NotLoggedIn,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keyring(detail) => write!(f, "credential store error: {detail}"),
            Self::Client(detail) => write!(f, "auth client error: {detail}"),
            Self::MissingCookie => write!(
                f,
                "browser cookie missing (need a signed-in Cookie header with {PROOF_COOKIE})"
            ),
            Self::NotLoggedIn => write!(f, "not logged in"),
        }
    }
}

impl std::error::Error for AuthError {}

/// Secret storage abstraction. Prod uses [`KeyringStore`]; tests use
/// [`MemorySecretStore`] so `cargo test` never touches the OS keyring and
/// never hits the network.
pub trait SecretStore {
    /// Persist the raw `browser.json` cookie blob.
    fn save_cookie(&mut self, cookie_json: &str) -> Result<(), AuthError>;
    /// Load the cookie blob, if present.
    fn load_cookie(&self) -> Result<Option<String>, AuthError>;
    /// Delete the cookie blob; missing entry counts as success.
    fn delete_cookie(&mut self) -> Result<(), AuthError>;
}

/// OS-keyring-backed [`SecretStore`] (prod).
pub struct KeyringStore;

impl KeyringStore {
    fn entry() -> Result<keyring::Entry, AuthError> {
        // `keyring::Error` Display never echoes secret bytes (see keyring-core
        // error.rs), so its summary is safe to propagate.
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER_COOKIE)
            .map_err(|e| AuthError::Keyring(e.to_string()))
    }
}

impl SecretStore for KeyringStore {
    fn save_cookie(&mut self, cookie_json: &str) -> Result<(), AuthError> {
        Self::entry()?
            .set_password(cookie_json)
            .map_err(|e| AuthError::Keyring(e.to_string()))
    }

    fn load_cookie(&self) -> Result<Option<String>, AuthError> {
        match Self::entry()?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AuthError::Keyring(e.to_string())),
        }
    }

    fn delete_cookie(&mut self) -> Result<(), AuthError> {
        match Self::entry()?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AuthError::Keyring(e.to_string())),
        }
    }
}

/// In-memory [`SecretStore`] for unit tests (never touches OS keyring).
#[cfg(test)]
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    cookie: Option<String>,
}

#[cfg(test)]
impl SecretStore for MemorySecretStore {
    fn save_cookie(&mut self, cookie_json: &str) -> Result<(), AuthError> {
        self.cookie = Some(cookie_json.to_string());
        Ok(())
    }

    fn load_cookie(&self) -> Result<Option<String>, AuthError> {
        Ok(self.cookie.clone())
    }

    fn delete_cookie(&mut self) -> Result<(), AuthError> {
        self.cookie = None;
        Ok(())
    }
}

/// Normalize a browser `Cookie` request-header value into the canonical
/// `name=value; name=value` form.
///
/// Accepts the raw header value (with an optional leading `cookie:` label),
/// trims whitespace, drops malformed/empty pairs, and requires a non-empty
/// [`PROOF_COOKIE`] before returning. This is the same shape Sonora accepts:
/// one copy of the browser's `Cookie` header is the whole credential.
/// Rejects with [`AuthError::MissingCookie`] (callers persist only on `Ok`).
pub fn normalize_cookie_header(raw: &str) -> Result<String, AuthError> {
    let mut value = raw.trim();
    if value.len() >= 7 && value[..7].eq_ignore_ascii_case("cookie:") {
        value = value[7..].trim();
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    for pair in value.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let Some((name, raw_value)) = pair.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let raw_value = raw_value.trim();
        if name.is_empty()
            || raw_value.is_empty()
            || name
                .chars()
                .any(|c| c.is_whitespace() || c == '=' || c == ';')
            || raw_value.chars().any(char::is_control)
        {
            continue;
        }
        // Last occurrence wins (browsers send one value per cookie name).
        if let Some(slot) = pairs.iter_mut().find(|(k, _)| k == name) {
            slot.1 = raw_value.to_string();
        } else {
            pairs.push((name.to_string(), raw_value.to_string()));
        }
    }
    if !pairs
        .iter()
        .any(|(name, value)| name == PROOF_COOKIE && !value.is_empty())
    {
        return Err(AuthError::MissingCookie);
    }
    Ok(pairs
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; "))
}

/// Build the keyring `browser.json` blob from a normalized cookie header.
///
/// `ytmusic-rs` 0.1.x selects browser auth only when both `authorization`
/// and `cookie` keys are present, then regenerates a fresh `SAPISIDHASH` from
/// [`PROOF_COOKIE`] for every request. The stored `authorization` value is
/// therefore a placeholder that is never sent; keeping it lets a cookie-only
/// import use the browser-auth path without a second copied header.
fn browser_json_from_cookie_header(cookie: &str) -> String {
    let mut headers: AuthHeaders = AuthHeaders::new();
    // Placeholder only: `YTMusic::request_headers` recomputes this per request.
    headers.insert("authorization".to_string(), "SAPISIDHASH 0_0".to_string());
    headers.insert("cookie".to_string(), cookie.to_string());
    headers.insert("origin".to_string(), MUSIC_ORIGIN.to_string());
    serde_json::to_string(&headers).expect("string map serializes")
}

/// Log in from a browser `Cookie` request-header value.
///
/// Normalizes first, then [`Session::login_with_cookie`] validates and
/// persists to the OS keyring only on success. No credential file is created.
pub fn login_from_cookie_header(
    store: &mut impl SecretStore,
    raw_header: &str,
) -> Result<(Session, YTMusic), AuthError> {
    let cookie = normalize_cookie_header(raw_header)?;
    let blob = browser_json_from_cookie_header(&cookie);
    Session::login_with_cookie(store, &blob)
}

/// Parse the live `get_account_info` result into a display name.
///
/// `get_account_info` returns `{accountName: <simpleText string>, ...}` (the
/// crate already extracts `simpleText`; a nested `{simpleText}` object is
/// also accepted defensively). Returns the trimmed non-empty name, else
/// `None`. Identity only — never secrets. Pure, unit-tested with fixtures.
pub fn account_name_from_info(info: &ytmusicapi::JsonDict) -> Option<String> {
    let raw: Option<String> = match info.get("accountName") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Object(map)) => map
            .get("simpleText")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        _ => None,
    };
    let name = raw?;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Live account-identity fetch: `get_account_info` → display name.
/// Transport/auth failures yield `None` (callers keep the generic
/// "Signed in" label — never a fake name). Carries identity only.
pub async fn fetch_account_name(yt: &YTMusic) -> Option<String> {
    let info = yt.get_account_info().await.ok()?;
    account_name_from_info(&info)
}

/// Run the live fail-closed probe and fold it into a log-safe
/// [`ProbeReport`](crate::premium_gate::ProbeReport): gate status + which
/// signals passed/failed + upgrade message when blocked. The `player`
/// response's `streamingData` is dropped unread inside the probe; this
/// report never carries cookie/secret material.
pub async fn probe_report(
    yt: &ytmusicapi::YTMusic,
    probe_video_id: &str,
) -> crate::premium_gate::ProbeReport {
    let mut gate = crate::premium_gate::PremiumGate::new_logged_out();
    gate.probe_report(yt, probe_video_id).await
}

/// Logged-in state skeleton. Holds identity metadata only — never secrets
/// (account/channel names and ids are safe to log).
#[derive(Debug, Clone, Default)]
pub struct Session {
    logged_in: bool,
    account_name: Option<String>,
}

impl Session {
    /// Logged-out session (fail-closed default).
    pub fn logged_out() -> Self {
        Self::default()
    }

    /// Whether a login has established this session.
    pub fn is_logged_in(&self) -> bool {
        self.logged_in
    }

    /// Display name for the account chip. Constructors leave this `None`
    /// (identity-free offline); the live name is filled per probe via
    /// [`fetch_account_name`](crate::auth::fetch_account_name) and mirrored
    /// into `Backend` props by `apply_probe`.
    pub fn account_name(&self) -> Option<&str> {
        self.account_name.as_deref()
    }

    /// Log in with pasted `browser.json` contents: validate the headers,
    /// build the `YTMusic` client, then persist the blob to the secret store.
    /// The raw blob is never logged; on failure nothing is persisted.
    pub fn login_with_cookie(
        store: &mut impl SecretStore,
        browser_json: &str,
    ) -> Result<(Self, YTMusic), AuthError> {
        // `parse_auth_str` accepts raw JSON (falls back from file path); safe to
        // pass the pasted blob directly — no credential file is created.
        let headers =
            ytmusicapi::auth::parse_auth_str(browser_json).map_err(|_| AuthError::MissingCookie)?;
        if !ytmusicapi::auth::browser::is_browser(&headers) {
            return Err(AuthError::MissingCookie);
        }
        // ytmusic-rs error text is static (never echoes the blob); keep only a
        // static summary here so secrets cannot leak through error paths.
        let yt = YTMusic::with_auth(browser_json)
            .map_err(|_| AuthError::Client("invalid browser headers".to_string()))?;
        debug_assert!(yt.is_authenticated());
        store.save_cookie(browser_json)?;
        Ok((
            Self {
                logged_in: true,
                account_name: None,
            },
            yt,
        ))
    }

    /// Restore a session from the secret store without re-pasting headers.
    /// Returns `Ok(None)` when nothing was stored (still logged out).
    pub fn restore(store: &impl SecretStore) -> Result<Option<(Self, YTMusic)>, AuthError> {
        match store.load_cookie()? {
            None => Ok(None),
            Some(blob) => {
                let yt = YTMusic::with_auth(&blob)
                    .map_err(|_| AuthError::Client("stored headers invalid".to_string()))?;
                Ok(Some((
                    Self {
                        logged_in: true,
                        account_name: None,
                    },
                    yt,
                )))
            }
        }
    }

    /// Log out: delete the keyring entry and drop back to logged-out state.
    pub fn logout(store: &mut impl SecretStore) -> Result<Self, AuthError> {
        store.delete_cookie()?;
        Ok(Self::logged_out())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plausible `browser.json` shape (fake secrets; never hits the network —
    /// `with_auth` only parses + builds the client offline).
    const FAKE_BROWSER_JSON: &str = r#"{
        "authorization": "SAPISIDHASH fake-hash-for-tests",
        "cookie": "__Secure-3PAPISID=fake-test-sapisid; HSID=fake-hsid; SSID=fake-ssid",
        "origin": "https://music.youtube.com",
        "x-goog-visitor-id": "fake-visitor"
    }"#;

    /// Raw browser `Cookie` header value (fake secret; offline only).
    const FAKE_COOKIE_HEADER: &str =
        "VISITOR_INFO1_LIVE=abc; __Secure-3PAPISID=fake-test-sapisid; PREF=f1";

    #[test]
    fn error_display_carries_no_secrets() {
        let errs = [
            AuthError::MissingCookie,
            AuthError::NotLoggedIn,
            AuthError::Client("invalid browser headers".to_string()),
            AuthError::Keyring("No matching credential found".to_string()),
        ];
        for e in errs {
            let msg = e.to_string();
            assert!(!msg.contains("SAPISID") && !msg.contains("fake"));
        }
    }

    #[test]
    fn login_rejects_cookie_less_json() {
        let mut store = MemorySecretStore::default();
        let res = Session::login_with_cookie(&mut store, r#"{"origin": "x"}"#);
        assert_eq!(res.err(), Some(AuthError::MissingCookie));
        // Failed login persists nothing.
        assert_eq!(store.load_cookie().unwrap(), None);
    }

    #[test]
    fn login_stores_cookie_and_logout_clears() {
        let mut store = MemorySecretStore::default();
        let (session, yt) = Session::login_with_cookie(&mut store, FAKE_BROWSER_JSON)
            .expect("fake browser headers log in offline");
        assert!(session.is_logged_in());
        assert!(yt.is_authenticated());
        assert_eq!(store.load_cookie().unwrap().as_deref(), Some(FAKE_BROWSER_JSON));

        let session = Session::logout(&mut store).expect("logout clears store");
        assert!(!session.is_logged_in());
        assert_eq!(store.load_cookie().unwrap(), None);
    }

    #[test]
    fn cookie_header_normalizes_and_accepts_prefix() {
        let normalized = normalize_cookie_header(
            "Cookie: VISITOR_INFO1_LIVE=abc;  __Secure-3PAPISID=fake-test-sapisid; PREF=f1 ",
        )
        .expect("signed-in header normalizes");
        assert_eq!(
            normalized,
            "VISITOR_INFO1_LIVE=abc; __Secure-3PAPISID=fake-test-sapisid; PREF=f1"
        );

        // Malformed/empty pairs are dropped; a duplicate name keeps the last.
        let normalized = normalize_cookie_header(
            "__Secure-3PAPISID=one; broken; =nope; __Secure-3PAPISID=two",
        )
        .expect("duplicate proof still validates");
        assert_eq!(normalized, "__Secure-3PAPISID=two");
    }

    #[test]
    fn cookie_header_rejects_without_secure_proof() {
        for bad in [
            "",
            "   ",
            "not a header at all",
            "VISITOR_INFO1_LIVE=abc; PREF=f1",
            "SAPISID=only-the-old-cookie",
            "__Secure-3PAPISID=",
            "cookie: only-a-cookie-no-proof",
        ] {
            assert_eq!(
                normalize_cookie_header(bad).err(),
                Some(AuthError::MissingCookie),
                "header must reject: {bad}"
            );
        }
    }

    #[test]
    fn cookie_only_login_builds_browser_auth_and_stores() {
        let mut store = MemorySecretStore::default();
        let (session, yt) = login_from_cookie_header(&mut store, FAKE_COOKIE_HEADER)
            .expect("cookie-only header logs in offline");
        assert!(session.is_logged_in());
        assert!(yt.is_authenticated());

        let blob = store.load_cookie().unwrap().expect("blob persisted");
        let headers = ytmusicapi::auth::parse_auth_str(&blob).expect("blob re-parses");
        assert!(ytmusicapi::auth::browser::is_browser(&headers));
        assert_eq!(
            headers.get("cookie").map(String::as_str),
            Some("VISITOR_INFO1_LIVE=abc; __Secure-3PAPISID=fake-test-sapisid; PREF=f1")
        );
        assert_eq!(
            headers.get("origin").map(String::as_str),
            Some(MUSIC_ORIGIN)
        );
        // The stored authorization is the per-request placeholder, not a copy
        // of any browser value.
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("SAPISIDHASH 0_0")
        );
    }

    #[test]
    fn invalid_cookie_login_stores_nothing() {
        let mut store = MemorySecretStore::default();
        let res = login_from_cookie_header(&mut store, "PREF=f1");
        assert_eq!(res.err(), Some(AuthError::MissingCookie));
        assert_eq!(store.load_cookie().unwrap(), None);
    }

    #[test]
    fn login_outputs_carry_no_secret_bytes() {
        // Fake secret that must appear NOWHERE in Debug/Display/log output.
        const SECRET: &str = "fake-test-sapisid";
        let mut store = MemorySecretStore::default();
        let (_session, yt) = login_from_cookie_header(&mut store, FAKE_COOKIE_HEADER).unwrap();

        // AuthError paths are static.
        for e in [
            AuthError::MissingCookie,
            AuthError::NotLoggedIn,
            AuthError::Client("invalid browser headers".to_string()),
            AuthError::Keyring("No matching credential found".to_string()),
        ] {
            assert!(!format!("{e:?}").contains(SECRET));
            assert!(!e.to_string().contains(SECRET));
        }
        // Session Debug holds identity only.
        let session = Session::logged_out();
        assert!(!format!("{session:?}").contains(SECRET));
        // ProbeReport render (offline mock signals) is secret-free.
        let signals = crate::premium_gate::PremiumSignals {
            logged_in: yt.is_authenticated(),
            home_ok: false,
            premium_badge: None,
            explore_top_songs: None,
            ads_absent: None,
            topbar_premium: None,
            hires_audio: None,
            playability: crate::premium_gate::Playability::Unknown,
        };
        let mut gate = crate::premium_gate::PremiumGate::new_logged_out();
        gate.apply(&signals, std::time::SystemTime::now());
        let report =
            crate::premium_gate::ProbeReport::from_signals(&signals, &gate);
        assert!(!report.render().contains(SECRET));
        assert!(!format!("{report:?}").contains(SECRET));
        assert!(!report.to_string().contains(SECRET));
    }

    fn info_dict(v: serde_json::Value) -> ytmusicapi::JsonDict {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn account_name_parses_live_info_shapes() {
        // Live `get_account_info` shape: `{accountName: <simpleText>}`.
        let live = info_dict(serde_json::json!({
            "accountName": "Test User",
            "channelId": {"thumbnails": [{"url": "https://img/a.jpg"}]},
        }));
        assert_eq!(account_name_from_info(&live).as_deref(), Some("Test User"));

        // Padded names trim; nested `{simpleText}` also parses defensively.
        let padded = info_dict(serde_json::json!({"accountName": "  Ada  "}));
        assert_eq!(account_name_from_info(&padded).as_deref(), Some("Ada"));
        let nested =
            info_dict(serde_json::json!({"accountName": {"simpleText": "Nested"}}));
        assert_eq!(account_name_from_info(&nested).as_deref(), Some("Nested"));

        // Missing / null / empty / non-string names yield None (callers keep
        // the generic "Signed in" label — never a fake name).
        for bad in [
            serde_json::json!({}),
            serde_json::json!({"accountName": null}),
            serde_json::json!({"accountName": ""}),
            serde_json::json!({"accountName": "   "}),
            serde_json::json!({"accountName": 42}),
            serde_json::json!({"accountName": {"runs": [{"text": "X"}]}}),
        ] {
            assert_eq!(account_name_from_info(&info_dict(bad)), None);
        }
    }
}
