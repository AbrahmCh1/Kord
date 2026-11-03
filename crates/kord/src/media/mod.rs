//! System-wide media control, used by ear detection.
//!
//! Instead of sending a play/pause key (which toggles and could *start* music),
//! Kord asks the OS which players are playing, pauses exactly those and later
//! resumes exactly those.
//!
//! - Windows: Global System Media Transport Controls (what the volume flyout uses).
//! - Linux: MPRIS over the D-Bus session bus (Spotify, browsers, VLC, mpv…).
//!
//! These calls block for a few milliseconds; run them off the async executor.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use self::linux as platform;
#[cfg(windows)]
use self::windows as platform;

/// Pauses every player that is currently playing and returns their ids.
pub fn pause_playing() -> anyhow::Result<Vec<String>> {
    #[cfg(any(windows, target_os = "linux"))]
    return platform::pause_playing();
    #[cfg(not(any(windows, target_os = "linux")))]
    Ok(Vec::new())
}

/// Presses "play" on the current player, e.g. after the headphones connect.
pub fn play_current() -> anyhow::Result<()> {
    #[cfg(any(windows, target_os = "linux"))]
    return platform::play_current();
    #[cfg(not(any(windows, target_os = "linux")))]
    Ok(())
}

/// Resumes the players returned by an earlier [`pause_playing`].
pub fn resume(players: &[String]) -> anyhow::Result<()> {
    if players.is_empty() {
        return Ok(());
    }
    #[cfg(any(windows, target_os = "linux"))]
    return platform::resume(players);
    #[cfg(not(any(windows, target_os = "linux")))]
    Ok(())
}
