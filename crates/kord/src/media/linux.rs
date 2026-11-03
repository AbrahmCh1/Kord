use anyhow::Result;
use zbus::blocking::{Connection, Proxy, fdo::DBusProxy};

const PLAYER_PREFIX: &str = "org.mpris.MediaPlayer2.";

fn player<'a>(conn: &Connection, name: &'a str) -> zbus::Result<Proxy<'a>> {
    Proxy::new(conn, name, "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2.Player")
}

fn is_playing(player: &Proxy<'_>) -> bool {
    player.get_property::<String>("PlaybackStatus").is_ok_and(|status| status == "Playing")
}

pub fn pause_playing() -> Result<Vec<String>> {
    let conn = Connection::session()?;
    let names = DBusProxy::new(&conn)?.list_names()?;
    let mut paused = Vec::new();
    for name in names.iter().map(|n| n.as_str()).filter(|n| n.starts_with(PLAYER_PREFIX)) {
        let Ok(proxy) = player(&conn, name) else { continue };
        if is_playing(&proxy) && proxy.call_method("Pause", &()).is_ok() {
            paused.push(name.to_owned());
        }
    }
    Ok(paused)
}

/// Presses "play" on the first paused player.
pub fn play_current() -> Result<()> {
    let conn = Connection::session()?;
    let names = DBusProxy::new(&conn)?.list_names()?;
    for name in names.iter().map(|n| n.as_str()).filter(|n| n.starts_with(PLAYER_PREFIX)) {
        let Ok(proxy) = player(&conn, name) else { continue };
        if proxy.get_property::<String>("PlaybackStatus").is_ok_and(|status| status == "Paused") {
            proxy.call_method("Play", &())?;
            break;
        }
    }
    Ok(())
}

pub fn resume(players: &[String]) -> Result<()> {
    let conn = Connection::session()?;
    for name in players {
        // The player may have quit in the meantime; that's fine.
        if let Ok(proxy) = player(&conn, name)
            && !is_playing(&proxy)
        {
            let _ = proxy.call_method("Play", &());
        }
    }
    Ok(())
}
