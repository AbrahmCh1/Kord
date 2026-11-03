use anyhow::Result;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};

fn sessions() -> Result<Vec<Session>> {
    let manager = SessionManager::RequestAsync()?.join()?;
    Ok(manager.GetSessions()?.into_iter().collect())
}

fn is_playing(session: &Session) -> bool {
    session
        .GetPlaybackInfo()
        .and_then(|info| info.PlaybackStatus())
        .is_ok_and(|status| status == PlaybackStatus::Playing)
}

pub fn pause_playing() -> Result<Vec<String>> {
    let mut paused = Vec::new();
    for session in sessions()? {
        if is_playing(&session) && session.TryPauseAsync()?.join()? {
            paused.push(session.SourceAppUserModelId()?.to_string());
        }
    }
    Ok(paused)
}

/// Presses "play" on the session Windows considers current.
pub fn play_current() -> Result<()> {
    let manager = SessionManager::RequestAsync()?.join()?;
    if let Ok(session) = manager.GetCurrentSession()
        && !is_playing(&session)
    {
        session.TryPlayAsync()?.join()?;
    }
    Ok(())
}

pub fn resume(players: &[String]) -> Result<()> {
    for session in sessions()? {
        let id = session.SourceAppUserModelId()?.to_string();
        if players.contains(&id) && !is_playing(&session) {
            session.TryPlayAsync()?.join()?;
        }
    }
    Ok(())
}
