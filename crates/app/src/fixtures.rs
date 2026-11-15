//! Canned data for `scripts/ui-snapshot.sh` (`FASTCLOCK_FIXTURES=empty|single|many`): every page can be rendered with zero,
//! one or many rows without ringing, saving or touching the real system. Nothing here starts a clock: the timer,
//! stopwatch and Pomodoro are shown paused.
use crate::{App, Main};
use fastclock_core::clock::{Alarm, Countdown, Pomodoro, Stopwatch};
use std::time::{Duration, Instant, SystemTime};

/// 0 empty, 1 single, 2 many; `None` when the variable is unset or unknown.
pub fn mode() -> Option<usize> {
    match std::env::var("FASTCLOCK_FIXTURES").ok()?.as_str() {
        "empty" => Some(0),
        "single" => Some(1),
        "many" => Some(2),
        _ => None,
    }
}

fn alarm(hour: u8, minute: u8, label: &str, days: u8, enabled: bool) -> Alarm {
    Alarm {
        hour,
        minute,
        label: label.into(),
        days,
        enabled,
    }
}

pub fn apply(ui: &Main, app: &mut App, mode: usize) {
    let secs = Duration::from_secs;
    app.settings.borrow_mut().alarms = match mode {
        0 => vec![],
        1 => vec![alarm(7, 30, "Wake up", 0b001_1111, true)],
        // A long label (it must be cut, not widen the row), every kind of day summary, on and off.
        _ => vec![
            alarm(6, 30, "Gym", 0b001_1111, true),
            alarm(7, 0, "", 0, true),
            alarm(
                8,
                15,
                "Standup with the whole team about the quarterly planning and the budget",
                0b000_1010,
                true,
            ),
            alarm(12, 0, "Lunch", 0x7f, false),
            alarm(18, 45, "Pick up the kids", 0b001_0101, true),
            alarm(22, 0, "Read", 0b110_0000, true),
        ],
    };
    app.timer_total = secs([300, 300, 7200][mode]);
    app.timer = [
        Countdown::Idle,
        Countdown::Paused(secs(299)),
        Countdown::Paused(secs(3725)),
    ][mode];
    app.stopwatch = Stopwatch::paused_at(
        [
            Duration::ZERO,
            Duration::from_millis(83_400),
            Duration::from_millis(3_723_400),
        ][mode],
    );
    app.pomodoro = [
        Pomodoro::default(),
        Pomodoro {
            working: true,
            round: 2,
        },
        Pomodoro {
            working: false,
            round: 4,
        },
    ][mode];
    app.pomodoro_clock = [
        Countdown::Idle,
        Countdown::Paused(secs(24 * 60 + 59)),
        Countdown::Paused(secs(6 * 60 + 32)),
    ][mode];
    // The timer page's input row, for the empty mode.
    ui.set_timer_minutes(5);
    ui.set_sound_name(
        [
            "",
            "chime.ogg",
            "a-very-long-sound-file-name-that-has-to-be-cut-off-in-the-preferences.ogg",
        ][mode]
            .into(),
    );
    app.refresh_alarms(ui);
    app.show(ui, SystemTime::now(), Instant::now());
}

/// `FASTCLOCK_PAGE=<page>` (0 alarm, 1 timer, 2 stopwatch, 3 pomodoro) or one of `settings`, `about`, `edit`, `ringing`.
pub fn open_page(ui: &Main, app: &mut App) {
    match std::env::var("FASTCLOCK_PAGE").as_deref() {
        Ok("settings") => ui.set_settings_open(true),
        Ok("about") => ui.set_about_open(true),
        Ok("edit") => app.open_editor(ui, None),
        Ok("edit-existing") => app.open_editor(ui, Some(0)),
        Ok("ringing") => {
            ui.set_ring_title("Wake up".into());
            ui.set_ring_body("07:30".into());
            ui.set_ring_can_snooze(true);
            ui.set_ringing(true);
        }
        Ok(p) => ui.set_page(
            p.parse()
                .unwrap_or_else(|_| panic!("FASTCLOCK_PAGE={p} is not a page number or a dialog name")),
        ),
        Err(_) => {}
    }
}
