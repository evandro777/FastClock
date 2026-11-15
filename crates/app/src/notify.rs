//! A desktop notification (`org.freedesktop.Notifications`), for what the user must not miss while the window sits
//! in the tray. Blocks on D-Bus: call it from a thread, never the UI thread.
use std::{collections::HashMap, sync::Mutex};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::Value,
};

/// What a click does: gets the action's key (`default` is a click on the notification itself).
type Handler = Box<dyn Fn(&str) + Send>;

/// Best effort: no session bus or no notification server means no notification, never an error. `actions` are
/// (key, label) buttons; `default` is also sent so a click on the body is reported (its label is not shown).
/// Returns the server's id, which `close` takes. `critical` stays on screen until dismissed (alarms, timers).
pub fn send(
    summary: &str,
    body: &str,
    actions: Vec<(&'static str, String)>,
    critical: bool,
    on_action: impl Fn(&str) + Send + 'static,
) -> Option<u32> {
    try_send(summary, body, actions, critical, Box::new(on_action))
        .inspect_err(|e| eprintln!("no notification ({e})"))
        .ok()
}

/// Takes a notification off the screen (its buttons would do nothing once the ring is over). Best effort.
pub fn close(id: u32) {
    HANDLERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_default()
        .remove(&id);
    if let Some(bus) = BUS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = bus.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "CloseNotification",
            &(id,),
        );
    }
}

/// Kept open: the desktop drops a notification whose sender leaves the bus at once (it showed up for `gdbus`, which
/// lingers, but not for a connection closed right after the call). Not opened until the first notification.
static BUS: Mutex<Option<Connection>> = Mutex::new(None);

/// The click handler of every notification still on screen, by the id the server gave it.
static HANDLERS: Mutex<Option<HashMap<u32, Handler>>> = Mutex::new(None);

/// Listens for clicks. The match rules are in place when this returns (before the first `Notify`), so no click is
/// missed; one thread per signal, for as long as the connection lives.
fn listen(bus: &Connection) -> zbus::Result<()> {
    let proxy = Proxy::new(
        bus,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )?;
    let clicks = proxy.receive_signal("ActionInvoked")?;
    let closed = proxy.receive_signal("NotificationClosed")?;
    std::thread::spawn(move || {
        for m in clicks {
            if let Ok((id, key)) = m.body().deserialize::<(u32, String)>() {
                // Taken out while it runs: a handler may send a notification itself.
                let h = HANDLERS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_mut()
                    .and_then(|h| h.remove(&id));
                if let Some(h) = h {
                    h(&key);
                    // A second button on the same notification may still be clicked.
                    HANDLERS
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get_or_insert_default()
                        .insert(id, h);
                }
            }
        }
    });
    std::thread::spawn(move || {
        for m in closed {
            if let Ok((id, _)) = m.body().deserialize::<(u32, u32)>() {
                HANDLERS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_or_insert_default()
                    .remove(&id);
            }
        }
    });
    Ok(())
}

fn try_send(
    summary: &str,
    body: &str,
    actions: Vec<(&'static str, String)>,
    critical: bool,
    handler: Handler,
) -> zbus::Result<u32> {
    let mut bus = BUS.lock().unwrap_or_else(|e| e.into_inner());
    if bus.is_none() {
        let c = Connection::session()?;
        listen(&c)?;
        *bus = Some(c);
    }
    let mut flat = vec!["default".to_string(), String::new()];
    for (key, label) in actions {
        flat.extend([key.to_string(), label]);
    }
    let reply = bus.as_ref().expect("just set").call_method(
        Some("org.freedesktop.Notifications"),
        "/org/freedesktop/Notifications",
        Some("org.freedesktop.Notifications"),
        "Notify",
        &(
            "FastClock",
            0u32,
            // A stock theme icon: our own app icon is only in the theme once installed (not for a dev binary).
            "alarm-symbolic",
            summary,
            body,
            flat,
            HashMap::from([
                // Critical stays until dismissed: an alarm that scrolls away unseen is a missed alarm.
                ("urgency", Value::from(if critical { 2u8 } else { 1u8 })),
                // Lets the shell tie the popup to FastClock's launcher entry.
                ("desktop-entry", Value::from("io.github.evandro777.FastClock")),
            ]),
            -1i32,
        ),
    )?;
    let id: u32 = reply.body().deserialize()?;
    HANDLERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_default()
        .insert(id, handler);
    Ok(id)
}

#[cfg(test)]
mod tests {
    /// Shows a real notification: `cargo test notification -- --ignored`. Needs a desktop session.
    #[test]
    #[ignore]
    fn shows_a_real_notification() {
        super::try_send(
            "FastClock test",
            "If you see this, notifications work.",
            vec![("stop", "Stop".into())],
            false,
            Box::new(|_| {}),
        )
        .unwrap();
    }
}
