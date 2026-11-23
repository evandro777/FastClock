//! Tray icon (StatusNotifierItem): Open and Quit. Closing the window hides it to the tray so alarms keep ringing; it is
//! optional: without a tray host or a D-Bus session the app quits when the window closes instead.
use std::sync::OnceLock;

use ksni::{
    Icon, MenuItem, Tray,
    blocking::{Handle, TrayMethods},
    menu::StandardItem,
};

/// The app icon (packaging/*.png): a pixmap, so it shows without being installed in a theme.
const ICON: &[u8] = include_bytes!("../../../packaging/io.github.evandro777.FastClock.png");

/// Already translated, because the tray lives on its own thread and the strings come from Slint's `Tr` global.
pub struct Labels {
    pub open: String,
    pub quit: String,
}

/// What the menu does; each posts work to the UI thread.
pub struct Actions {
    pub open: fn(),
    pub quit: fn(),
}

pub struct FastTray {
    labels: Labels,
    actions: Actions,
}

/// RGBA of the 8-bit PNG, with its size.
fn decode_rgba(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let data = &buf[..frame.buffer_size()];
    let rgba = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        _ => return None,
    };
    Some((frame.width, frame.height, rgba))
}

impl Tray for FastTray {
    fn id(&self) -> String {
        "fastclock".into()
    }

    fn title(&self) -> String {
        "FastClock".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        // Decoded once: the tray host asks for the icon again on every refresh.
        static PIXMAP: OnceLock<Option<Icon>> = OnceLock::new();
        PIXMAP
            .get_or_init(|| {
                let (width, height, rgba) = decode_rgba(ICON)?;
                // StatusNotifier wants ARGB32 in network byte order.
                let data = rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [p[3], p[0], p[1], p[2]])
                    .collect();
                Some(Icon {
                    width: width as i32,
                    height: height as i32,
                    data,
                })
            })
            .iter()
            .cloned()
            .collect()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        (self.actions.open)();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: self.labels.open.clone(),
                icon_name: "alarm-symbolic".into(),
                activate: Box::new(|t: &mut Self| (t.actions.open)()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: self.labels.quit.clone(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| (t.actions.quit)()),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Keep the returned handle alive for as long as the tray should stay.
pub fn spawn(labels: Labels, actions: Actions) -> Option<Handle<FastTray>> {
    FastTray { labels, actions }
        // A sandbox may not own ksni's pid-based bus name; the unique name is enough for the watcher.
        .disable_dbus_name(crate::autostart::in_flatpak())
        .spawn()
        .inspect_err(|e| eprintln!("no tray icon ({e}); closing the window will quit"))
        .ok()
}

/// New labels after a language change.
pub fn set_labels(handle: &Handle<FastTray>, labels: Labels) {
    let _ = handle.update(|t| t.labels = labels);
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_icon_decodes() {
        let (w, h, rgba) = super::decode_rgba(super::ICON).unwrap();
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        assert!(w >= 64 && w == h);
    }
}
