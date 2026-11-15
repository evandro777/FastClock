//! Clock logic with no UI and no I/O: when an alarm is due, the Pomodoro cycle, timers and the text of a time.
//! Every function takes "now" as an argument so the tests need no real clock.
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Alarm {
    pub hour: u8,
    pub minute: u8,
    pub label: String,
    /// Bit 0 = Monday ... bit 6 = Sunday. 0 rings once, then switches itself off.
    pub days: u8,
    pub enabled: bool,
}

impl Default for Alarm {
    fn default() -> Self {
        Self {
            hour: 7,
            minute: 0,
            label: String::new(),
            days: 0,
            enabled: true,
        }
    }
}

impl Alarm {
    /// Hand-edited files may hold nonsense.
    pub fn normalize(&mut self) {
        self.hour = self.hour.min(23);
        self.minute = self.minute.min(59);
        self.days &= 0x7f;
    }
}

const DAY: i64 = 1440;
/// A tick that finds more than this many minutes passed (a long suspend) only looks at the last of them: an alarm
/// from yesterday must not ring now.
const MAX_CATCH_UP: i64 = 180;

/// Minutes since 1970-01-01 00:00 on the wall clock (the time zone's offset included).
pub fn local_minutes(t: SystemTime) -> i64 {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let time = secs as libc::time_t;
    // SAFETY: `tm` is plain data that localtime_r fills; both pointers are valid for the call.
    let offset = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&time, &mut tm).is_null() {
            0
        } else {
            tm.tm_gmtoff
        }
    };
    (secs as i64 + offset as i64) / 60
}

/// Whether the alarm's minute falls in `(prev, now]` (minutes as from `local_minutes`), on a day it repeats.
/// A tick that is a little late, or a short suspend, still rings it, and the window never overlaps the last one.
pub fn due(alarm: &Alarm, prev: i64, now: i64) -> bool {
    if !alarm.enabled {
        return false;
    }
    let target = i64::from(alarm.hour) * 60 + i64::from(alarm.minute);
    (prev.max(now - MAX_CATCH_UP) + 1..=now).any(|m| {
        // 1970-01-01 was a Thursday, and bit 0 is Monday.
        let weekday = ((m.div_euclid(DAY) + 3).rem_euclid(7)) as u8;
        m.rem_euclid(DAY) == target && (alarm.days == 0 || alarm.days >> weekday & 1 == 1)
    })
}

/// Minutes from `now` until the first enabled alarm rings (the current minute does not count: it is ringing or
/// rang already), or `None` when no alarm is on.
pub fn next_ring(alarms: &[Alarm], now: i64) -> Option<i64> {
    let day = now.div_euclid(DAY);
    alarms
        .iter()
        .filter(|a| a.enabled)
        .flat_map(|a| {
            let target = i64::from(a.hour) * 60 + i64::from(a.minute);
            // Today and the next 7 days cover every weekday (and a once-alarm that is past today rings tomorrow).
            (0..=7).filter_map(move |d| {
                let at = (day + d) * DAY + target;
                let weekday = ((day + d + 3).rem_euclid(7)) as u8;
                (at > now && (a.days == 0 || a.days >> weekday & 1 == 1)).then_some(at - now)
            })
        })
        .min()
}

/// `7:05` style; the app shows the 24 h clock the family uses.
pub fn fmt_time(hour: u8, minute: u8) -> String {
    format!("{hour:02}:{minute:02}")
}

/// `05:00` or `1:23:45`. Rounds up, so a countdown shows 00:01 until it really is over.
pub fn fmt_countdown(left: Duration) -> String {
    let s = left.as_millis().div_ceil(1000) as u64;
    hms(s)
}

/// `01:23.4` or `1:02:03.4` (tenths cut, not rounded, so it never runs ahead).
pub fn fmt_stopwatch(elapsed: Duration) -> String {
    format!("{}.{}", hms(elapsed.as_secs()), elapsed.subsec_millis() / 100)
}

fn hms(s: u64) -> String {
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// A countdown that survives suspend: the end is a wall-clock time (`Instant` stops while the machine sleeps).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Countdown {
    Idle,
    Running(SystemTime),
    Paused(Duration),
}

impl Countdown {
    pub fn start(total: Duration, now: SystemTime) -> Self {
        Self::Running(now + total)
    }

    pub fn left(&self, now: SystemTime) -> Option<Duration> {
        match *self {
            Self::Idle => None,
            Self::Running(end) => Some(end.duration_since(now).unwrap_or_default()),
            Self::Paused(d) => Some(d),
        }
    }

    pub fn pause(&mut self, now: SystemTime) {
        if let Self::Running(_) = self {
            *self = Self::Paused(self.left(now).unwrap_or_default());
        }
    }

    pub fn resume(&mut self, now: SystemTime) {
        if let Self::Paused(d) = *self {
            *self = Self::Running(now + d);
        }
    }

    pub fn finished(&self, now: SystemTime) -> bool {
        matches!(self, Self::Running(end) if *end <= now)
    }
}

/// Elapsed time that stops and resumes; ticks only need a monotonic clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stopwatch {
    base: Duration,
    since: Option<Instant>,
}

impl Stopwatch {
    pub fn running(&self) -> bool {
        self.since.is_some()
    }

    /// Stopped at `elapsed` (the UI fixtures use it).
    pub fn paused_at(elapsed: Duration) -> Self {
        Self {
            base: elapsed,
            since: None,
        }
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        self.base + self.since.map_or(Duration::ZERO, |s| now.saturating_duration_since(s))
    }

    pub fn start(&mut self, now: Instant) {
        self.since.get_or_insert(now);
    }

    pub fn pause(&mut self, now: Instant) {
        self.base = self.elapsed(now);
        self.since = None;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Where the Pomodoro cycle is: work rounds 1..=4, a break after each (the 4th is the long one).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pomodoro {
    pub working: bool,
    pub round: u8,
}

pub const ROUNDS: u8 = 4;

impl Default for Pomodoro {
    fn default() -> Self {
        Self {
            working: true,
            round: 1,
        }
    }
}

impl Pomodoro {
    pub fn next(self) -> Self {
        match (self.working, self.round) {
            (true, r) => Self {
                working: false,
                round: r,
            },
            (false, r) => Self {
                working: true,
                round: r % ROUNDS + 1,
            },
        }
    }

    /// Minutes of this phase, given the three lengths from the settings.
    pub fn minutes(self, work: u32, short: u32, long: u32) -> u32 {
        match (self.working, self.round) {
            (true, _) => work,
            (false, ROUNDS) => long,
            (false, _) => short,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Monday 2024-01-01 00:00 UTC is minute 28_401_120 (19_723 days * 1440), a Monday.
    const MON: i64 = 19_723 * DAY;

    fn alarm(h: u8, m: u8, days: u8) -> Alarm {
        Alarm {
            hour: h,
            minute: m,
            days,
            ..Alarm::default()
        }
    }

    #[test]
    fn rings_in_the_minute_it_is_due_once() {
        let a = alarm(7, 30, 0);
        let t = MON + 7 * 60 + 30;
        assert!(!due(&a, t - 2, t - 1));
        assert!(due(&a, t - 1, t));
        assert!(!due(&a, t, t + 1), "the window never overlaps the last one");
    }

    #[test]
    fn a_late_tick_or_short_suspend_still_rings() {
        let a = alarm(7, 30, 0);
        assert!(due(&a, MON + 7 * 60 + 20, MON + 7 * 60 + 45));
        // Asleep for a day: 07:30 (twice passed) is too old to ring when the machine wakes at 11:00.
        assert!(!due(&a, MON + 7 * 60, MON + DAY + 11 * 60));
    }

    #[test]
    fn weekdays_pick_the_days() {
        // Tuesday and Thursday only: bits 1 and 3.
        let a = alarm(8, 0, 0b0001010);
        assert!(!due(&a, MON + 8 * 60 - 1, MON + 8 * 60), "Monday");
        assert!(due(&a, MON + DAY + 8 * 60 - 1, MON + DAY + 8 * 60), "Tuesday");
        assert!(due(&a, MON + 3 * DAY + 8 * 60 - 1, MON + 3 * DAY + 8 * 60), "Thursday");
    }

    #[test]
    fn crossing_midnight_finds_an_alarm_at_00_00() {
        let a = alarm(0, 0, 0);
        assert!(due(&a, MON + DAY - 1, MON + DAY));
    }

    #[test]
    fn next_ring_counts_minutes_to_the_soonest_enabled_alarm() {
        let at = |h: i64, m: i64| MON + h * 60 + m;
        let alarms = [
            alarm(8, 0, 0),
            alarm(7, 30, 0),
            Alarm {
                enabled: false,
                ..alarm(6, 0, 0)
            },
        ];
        assert_eq!(next_ring(&alarms, at(7, 0)), Some(30));
        // The minute itself is not "next": tomorrow's 07:30 is 24 h away, and 08:00 is 30 minutes after it.
        assert_eq!(next_ring(&alarms, at(7, 30)), Some(30));
        // After the last alarm of the day, a once-alarm rings tomorrow.
        assert_eq!(next_ring(&alarms, at(9, 0)), Some(22 * 60 + 30));
        assert_eq!(
            next_ring(
                &[Alarm {
                    enabled: false,
                    ..alarm(6, 0, 0)
                }],
                at(0, 0)
            ),
            None
        );
        assert_eq!(next_ring(&[], at(0, 0)), None);
    }

    #[test]
    fn next_ring_skips_to_the_right_weekday() {
        // Saturday and Sunday only, asked on Monday 12:00: Saturday 08:00 is 5 days minus 4 hours away.
        let weekend = alarm(8, 0, 0b110_0000);
        assert_eq!(next_ring(&[weekend], MON + 12 * 60), Some(5 * DAY - 4 * 60));
    }

    #[test]
    fn a_disabled_alarm_is_silent() {
        let a = Alarm {
            enabled: false,
            ..alarm(7, 30, 0)
        };
        assert!(!due(&a, MON + 7 * 60 + 29, MON + 7 * 60 + 30));
    }

    #[test]
    fn pomodoro_cycles_work_break_with_a_long_fourth_break() {
        let mut p = Pomodoro::default();
        let mut seen = Vec::new();
        for _ in 0..9 {
            seen.push((p.working, p.round, p.minutes(25, 5, 15)));
            p = p.next();
        }
        assert_eq!(
            seen,
            [
                (true, 1, 25),
                (false, 1, 5),
                (true, 2, 25),
                (false, 2, 5),
                (true, 3, 25),
                (false, 3, 5),
                (true, 4, 25),
                (false, 4, 15),
                (true, 1, 25)
            ]
        );
    }

    #[test]
    fn times_read_as_clocks() {
        assert_eq!(fmt_countdown(Duration::from_secs(300)), "05:00");
        assert_eq!(fmt_countdown(Duration::from_millis(100)), "00:01");
        assert_eq!(fmt_countdown(Duration::from_secs(3725)), "1:02:05");
        assert_eq!(fmt_stopwatch(Duration::from_millis(83_450)), "01:23.4");
        assert_eq!(fmt_time(7, 5), "07:05");
    }

    #[test]
    fn countdown_pauses_and_finishes() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let mut c = Countdown::start(Duration::from_secs(10), t0);
        let t4 = t0 + Duration::from_secs(4);
        c.pause(t4);
        assert_eq!(c.left(t4 + Duration::from_secs(99)), Some(Duration::from_secs(6)));
        c.resume(t4 + Duration::from_secs(99));
        assert!(!c.finished(t4 + Duration::from_secs(99)));
        assert!(c.finished(t4 + Duration::from_secs(106)));
    }

    #[test]
    fn stopwatch_keeps_its_time_across_pauses() {
        let t0 = Instant::now();
        let mut s = Stopwatch::default();
        s.start(t0);
        s.pause(t0 + Duration::from_secs(3));
        assert_eq!(s.elapsed(t0 + Duration::from_secs(50)), Duration::from_secs(3));
        s.start(t0 + Duration::from_secs(50));
        assert_eq!(s.elapsed(t0 + Duration::from_secs(52)), Duration::from_secs(5));
    }
}
