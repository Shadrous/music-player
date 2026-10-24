//! Browse/search skeleton over `ytmusic-rs` (P2).
//!
//! Handover notes:
//! - EVERY fetch calls [`check_gate`] first: only
//!   [`GateStatus::Premium`](crate::premium_gate::GateStatus::Premium) proceeds
//!   (see [`PremiumGate::can_fetch_playback_url`](crate::premium_gate::PremiumGate::can_fetch_playback_url)).
//!   Blocked calls return [`FetchError`] without touching the network.
//! - Live `fetch_*` wrappers take `&YTMusic` and parse with the same
//!   `from_json` constructors the mocks use. The QML model slots
//!   (`TrackModel::load_home` / `search`, ...) reach them through the
//!   `live_*` entry points, which restore the keyring session into an
//!   ephemeral client ([`live_client`]) and return Rust-decided status
//!   lines. Mock entry points (`*_mock`) are offline-test-only — the live
//!   path never falls back to mocks (failures clear + report).
//! - Legal: these endpoints read browse/search metadata only — never
//!   playback URLs, media bytes, or ad logic (see `crate::legal`).

pub mod home;
pub mod library;
pub mod search;

use crate::premium_gate::{GateStatus, PremiumGate};

/// Why a browse/search fetch did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    /// Logged out — sign in first.
    LoggedOut,
    /// Authenticated but not Premium (or ambiguous probe) — upgrade required.
    UpgradeRequired,
    /// Gate passed but the InnerTube call failed (transport only; the value
    /// carries no secrets).
    Transport,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoggedOut => write!(f, "not logged in"),
            Self::UpgradeRequired => write!(f, "YouTube Music Premium is required"),
            Self::Transport => write!(f, "request failed"),
        }
    }
}

impl std::error::Error for FetchError {}

/// THE fetch gate: call before ANY browse/search fetch. Maps the cached
/// [`GateStatus`] to `Ok` (Premium only) or a [`FetchError`].
pub fn check_gate(gate: &PremiumGate) -> Result<(), FetchError> {
    if gate.can_fetch_playback_url() {
        return Ok(());
    }
    match gate.status() {
        GateStatus::Premium => Ok(()),
        GateStatus::LoggedOut => Err(FetchError::LoggedOut),
        GateStatus::UpgradeRequired => Err(FetchError::UpgradeRequired),
    }
}

/// Rebuild a fetch gate from the QML `gateStatus` string.
///
/// Thin IPC only: the authoritative decision lives in `Backend.gate` (set by
/// the live fail-closed probe); QML forwards its string form blindly as the
/// `gate_status` arg of the `load_home` / `search` slots, and Rust re-checks
/// it here BEFORE any keyring or network touch. Only `"premium"` passes;
/// `"upgradeRequired"` maps to UpgradeRequired, anything else (including
/// `""`) maps to LoggedOut. Pure, unit-tested.
pub fn gate_from_status_str(s: &str) -> PremiumGate {
    use crate::premium_gate::{Playability, PremiumSignals};
    use std::time::SystemTime;
    let mut gate = PremiumGate::new_logged_out();
    match s {
        "premium" => {
            // Mirrors a passing probe (the real `Backend.gate` already earned
            // this string via `apply_probe`); lets the `fetch_*` wrappers
            // below run their own gate check on the live client.
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
        }
        "upgradeRequired" => {
            gate.apply(
                &PremiumSignals {
                    logged_in: true,
                    home_ok: false,
                    premium_badge: None,
                    explore_top_songs: None,
                    ads_absent: None,
                    topbar_premium: None,
                    hires_audio: None,
                    playability: Playability::Unknown,
                },
                SystemTime::now(),
            );
        }
        _ => {}
    }
    gate
}

/// User-facing message for a fetch failure. Carries no secrets, no URLs —
/// safe to log and to bind in QML status labels.
pub fn fetch_error_message(e: FetchError) -> String {
    match e {
        FetchError::LoggedOut => "Sign in to load your music.".to_string(),
        FetchError::UpgradeRequired => crate::legal::upgrade_message(),
        FetchError::Transport => {
            "Couldn't load music. Check your connection and try again.".to_string()
        }
    }
}

/// Restore the keyring session into an ephemeral live `YTMusic` client plus
/// its gate. Gate first (blocked returns before any keyring/network touch),
/// then restore: a missing/invalid store reads as LoggedOut. The client is
/// dropped after the fetch; the secret never leaves the keyring except into
/// client memory and is never logged. `Err` is a display-ready status line.
pub(crate) fn live_client(
    gate_status: &str,
) -> Result<(ytmusicapi::YTMusic, PremiumGate), String> {
    let gate = gate_from_status_str(gate_status);
    check_gate(&gate).map_err(fetch_error_message)?;
    let store = crate::auth::KeyringStore;
    match crate::auth::Session::restore(&store) {
        Ok(Some((session, yt))) => {
            if !session.is_logged_in() || !yt.is_authenticated() {
                return Err(fetch_error_message(FetchError::LoggedOut));
            }
            Ok((yt, gate))
        }
        Ok(None) | Err(_) => Err(fetch_error_message(FetchError::LoggedOut)),
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
        assert!(gate.can_fetch_playback_url());
        gate
    }

    #[test]
    fn gate_blocks_before_any_fetch() {
        assert_eq!(check_gate(&PremiumGate::new_logged_out()), Err(FetchError::LoggedOut));

        let mut gate = PremiumGate::new_logged_out();
        gate.apply(
            &PremiumSignals {
                logged_in: true,
                home_ok: true,
                premium_badge: Some(false),
                explore_top_songs: None,
                ads_absent: None,
                topbar_premium: None,
                hires_audio: None,
                playability: Playability::Playable,
            },
            SystemTime::now(),
        );
        assert_eq!(check_gate(&gate), Err(FetchError::UpgradeRequired));
    }

    #[test]
    fn premium_passes_the_fetch_gate() {
        assert_eq!(check_gate(&premium_gate()), Ok(()));
    }

    #[test]
    fn gate_status_string_roundtrips_fail_closed() {
        // Only the Premium string passes; everything else blocks WITHOUT
        // touching the network (offline: constructors only, no client).
        assert!(check_gate(&gate_from_status_str("premium")).is_ok());
        assert_eq!(
            check_gate(&gate_from_status_str("upgradeRequired")),
            Err(FetchError::UpgradeRequired)
        );
        for blocked in ["loggedOut", "", "bogus", "Premium", "PREMIUM"] {
            assert_eq!(
                check_gate(&gate_from_status_str(blocked)),
                Err(FetchError::LoggedOut),
                "must block: {blocked:?}"
            );
        }
    }

    #[test]
    fn fetch_error_display_carries_no_secrets() {
        for e in [FetchError::LoggedOut, FetchError::UpgradeRequired, FetchError::Transport] {
            let msg = e.to_string();
            assert!(!msg.contains("cookie") && !msg.contains("SAPISID"));
        }
    }
}
