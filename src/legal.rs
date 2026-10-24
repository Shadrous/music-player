//! Legal guardrails for the YouTube Music integration.
//!
//! Handover notes:
//! - All InnerTube traffic goes through `ytmusic-rs` (see `crate::auth`), which
//!   emulates the YouTube Music web client. Use of YouTube Music is governed by
//!   the YouTube Terms of Service ([`YOUTUBE_TERMS_URL`]); it is linked from
//!   every user-facing sign-in / upgrade surface.
//! - Hard prohibitions:
//!   * NO download / persist / export of media bytes: never extract, cache, or
//!     write streaming URLs or audio/video data anywhere.
//!   * NO ad-strip / ad-skip / ad-block logic of any kind.
//!   * Secrets (cookies, SAPISID, tokens) live ONLY in the OS keyring
//!     (see `crate::auth`); never in plaintext files, logs, or telemetry.

/// YouTube Terms of Service (governs YouTube Music use).
pub const YOUTUBE_TERMS_URL: &str = "https://www.youtube.com/t/terms";

/// Google Privacy Policy (covers YouTube Music data handling). Linked next
/// to the ToS on every sign-in / upgrade surface.
pub const GOOGLE_PRIVACY_URL: &str = "https://policies.google.com/privacy";

/// YouTube Music web client — the product whose InnerTube API we emulate.
pub const YTMUSIC_URL: &str = "https://music.youtube.com";

/// Short legal notice for sign-in / upgrade surfaces.
pub const LEGAL_NOTICE: &str = "Uses the YouTube Music InnerTube API via ytmusic-rs. \
    Media bytes are never downloaded, stored, or exported, and ads are never \
    stripped or skipped. See the YouTube Terms of Service for details.";

fn tos_suffix() -> String {
    format!("See {YOUTUBE_TERMS_URL}.")
}

/// Upgrade prompt shown when [`crate::premium_gate::GateStatus::UpgradeRequired`]
/// blocks playback. Keeps the ToS link attached to the upsell.
pub fn upgrade_message() -> String {
    format!(
        "YouTube Music Premium is required for playback in this app. {t} {n}",
        t = tos_suffix(),
        n = LEGAL_NOTICE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_constants_are_populated() {
        for url in [YOUTUBE_TERMS_URL, YTMUSIC_URL] {
            assert!(url.starts_with("https://"), "empty/non-https URL: {url}");
        }
        assert!(!LEGAL_NOTICE.is_empty());
    }

    #[test]
    fn upgrade_message_links_tos() {
        let msg = upgrade_message();
        assert!(msg.contains(YOUTUBE_TERMS_URL), "upsell must link ToS");
        assert!(msg.contains("Premium"));
    }

    #[test]
    fn privacy_link_is_https() {
        assert!(GOOGLE_PRIVACY_URL.starts_with("https://"));
    }
}
