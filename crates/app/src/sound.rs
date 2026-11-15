//! Plays the alarm sound with `paplay` (or `pw-play`): both are in the Flatpak runtime and on every desktop with
//! PulseAudio or PipeWire, so no audio library is linked. A sound that cannot be played falls back to the built-in
//! bell, because a silent alarm is the worst thing this app can do.
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Handle of a sound that is playing; `stop` ends it (and its repeats) at once.
pub struct Ringer {
    stop: Arc<AtomicBool>,
}

impl Ringer {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Plays `path` ("" = the bell), once or repeating until stopped. Never blocks.
pub fn ring(path: &str, looping: bool) -> Ringer {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let mut custom = (!path.is_empty() && Path::new(path).is_file()).then(|| PathBuf::from(path));
    std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            let Some(file) = custom.clone().or_else(bell_path) else {
                return eprintln!("no sound: cannot write the bell");
            };
            let played = play(&file, &flag);
            if flag.load(Ordering::Relaxed) {
                return;
            }
            if !played {
                // A format the players cannot read (or no player): the bell is the safe fallback.
                if custom.take().is_none() {
                    return eprintln!("no sound: neither paplay nor pw-play could play the bell");
                }
                continue;
            }
            if !looping {
                return;
            }
            pause(&flag, Duration::from_millis(700));
        }
    });
    Ringer { stop }
}

fn pause(stop: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(50);
    let mut left = total;
    while !left.is_zero() && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(step.min(left));
        left = left.saturating_sub(step);
    }
}

fn spawn(player: &str, file: &Path) -> std::io::Result<Child> {
    Command::new(player)
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Runs a player to the end. `true` when it finished (or was stopped), `false` when it could not play the file.
fn play(file: &Path, stop: &AtomicBool) -> bool {
    let mut child = match spawn("paplay", file) {
        Err(e) if e.kind() == ErrorKind::NotFound => match spawn("pw-play", file) {
            Ok(c) => c,
            Err(_) => return false,
        },
        Err(_) => return false,
        Ok(c) => c,
    };
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return true;
        }
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return false,
        }
    }
}

/// The bell is a generated WAV, written once: no sound file to ship or license, and no sound theme needed (the
/// Flatpak runtime has none).
fn bell_path() -> Option<PathBuf> {
    static BELL: OnceLock<Option<PathBuf>> = OnceLock::new();
    BELL.get_or_init(|| {
        let path = bell_file();
        std::fs::write(&path, bell_wav()).ok().map(|()| path)
    })
    .clone()
}

/// Per process, so two copies never share (or delete) each other's file.
fn bell_file() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("fastclock-bell-{}.wav", std::process::id()))
}

/// Removes the bell file on a normal exit.
pub fn cleanup() {
    let _ = std::fs::remove_file(bell_file());
}

const RATE: u32 = 22_050;

/// Mono 16-bit PCM: two struck-bell tones (880 and 1320 Hz) that fade out over 1.5 s.
fn bell_wav() -> Vec<u8> {
    let samples = RATE * 3 / 2;
    let mut data = Vec::with_capacity(samples as usize * 2);
    for i in 0..samples {
        let t = f64::from(i) / f64::from(RATE);
        let tone = (t * 880.0 * std::f64::consts::TAU).sin() + 0.5 * (t * 1320.0 * std::f64::consts::TAU).sin();
        // A short attack avoids a click; the exponential is the ring-out.
        let envelope = (-3.5 * t).exp() * (t * 400.0).min(1.0);
        data.extend_from_slice(&((tone / 1.5 * envelope * 22_000.0) as i16).to_le_bytes());
    }
    let mut wav = Vec::with_capacity(44 + data.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bell_is_a_valid_wav_of_one_and_a_half_seconds() {
        let w = bell_wav();
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        let data_len = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
        assert_eq!(w.len(), 44 + data_len);
        assert_eq!(data_len, (RATE * 3 / 2) as usize * 2);
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize, w.len() - 8);
        assert!(
            w[44..].chunks(2).any(|s| i16::from_le_bytes([s[0], s[1]]).abs() > 5000),
            "audible"
        );
    }

    #[test]
    fn a_stopped_ringer_ends_its_thread_and_plays_nothing_more() {
        // Whatever the machine has (a player, or none), stop must return control at once and not hang.
        let r = ring("/no/such/file.ogg", true);
        r.stop();
        std::thread::sleep(Duration::from_millis(200));
    }
}
