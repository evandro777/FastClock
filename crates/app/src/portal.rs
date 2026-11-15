//! The desktop's file chooser (`org.freedesktop.portal.FileChooser`), to pick the alarm sound. It works the same
//! natively and inside Flatpak, where the app cannot see the user's files and the portal hands back a path it
//! exposes. Blocks until the user answers: call it from a thread.
use std::collections::HashMap;
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

/// Only what `paplay`/`pw-play` (libsndfile) can read: an m4a would be accepted by an `audio/*` filter and then
/// play nothing.
const PATTERNS: [&str; 5] = ["*.wav", "*.ogg", "*.oga", "*.flac", "*.mp3"];

/// `Ok(None)` when the user cancelled.
pub fn pick_sound(title: &str, filter_name: &str) -> Result<Option<String>, String> {
    let err = |e: zbus::Error| e.to_string();
    let conn = Connection::session().map_err(err)?;
    let token = format!("fastclock{}", std::process::id());
    let sender = conn
        .unique_name()
        .map_or_else(String::new, |n| n.trim_start_matches(':').replace('.', "_"));
    // Subscribe before the call: a fast answer would otherwise be lost.
    let request = Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        format!("/org/freedesktop/portal/desktop/request/{sender}/{token}"),
        "org.freedesktop.portal.Request",
    )
    .map_err(err)?;
    let mut responses = request.receive_signal("Response").map_err(err)?;
    let filter: Vec<(u32, String)> = PATTERNS.iter().map(|p| (0, (*p).to_string())).collect();
    let options: HashMap<&str, Value> = HashMap::from([
        ("handle_token", Value::from(token)),
        ("filters", Value::from(vec![(filter_name.to_string(), filter)])),
    ]);
    Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.FileChooser",
    )
    .map_err(err)?
    .call::<_, _, OwnedObjectPath>("OpenFile", &("", title, options))
    .map_err(err)?;
    let msg = responses.next().ok_or("the portal sent no answer")?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize().map_err(|e| e.to_string())?;
    if code != 0 {
        return Ok(None);
    }
    let uris = results
        .get("uris")
        .and_then(|v| Vec::<String>::try_from(v.try_clone().ok()?).ok())
        .unwrap_or_default();
    Ok(uris.first().and_then(|u| uri_to_path(u)))
}

/// `file:///home/a/My%20song.ogg` -> `/home/a/My song.ogg`.
fn uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::uri_to_path;

    #[test]
    fn a_file_uri_becomes_a_path() {
        assert_eq!(
            uri_to_path("file:///home/a/My%20song%C3%A9.ogg").as_deref(),
            Some("/home/a/My songé.ogg")
        );
        assert_eq!(
            uri_to_path("file:///a/100%").as_deref(),
            Some("/a/100%"),
            "a lone % is kept"
        );
        assert_eq!(uri_to_path("https://x/y.ogg"), None);
    }
}
