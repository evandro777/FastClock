use fastclock_core::{
    clock::{self, Alarm, Countdown, Pomodoro, ROUNDS, Stopwatch},
    settings::Settings,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant, SystemTime},
};

slint::include_modules!();

mod autostart;
#[cfg(debug_assertions)]
mod fixtures;
mod instance;
mod notify;
mod portal;
mod sound;
mod tray;

/// An alarm or timer stops ringing by itself after this long: nobody is there, and the neighbours are.
const RING_CAP: Duration = Duration::from_secs(5 * 60);

/// What is ringing.
enum Kind {
    Alarm(Alarm),
    Timer(Duration),
}

struct Ring {
    /// Tells a notification button of this ring from one of an earlier ring that is already over.
    id: u64,
    /// The alarm to wake again on Snooze; `None` for a timer.
    alarm: Option<Alarm>,
    ringer: sound::Ringer,
    notification: Option<u32>,
    started: Instant,
}

/// An alarm that Snooze put to sleep. Not saved: a restart forgets it.
struct Snoozed {
    alarm: Alarm,
    at: SystemTime,
}

/// Everything the clock does between clicks. Lives on the UI thread only (`app()`); other threads post to it with
/// `upgrade_in_event_loop`.
struct App {
    ui: slint::Weak<Main>,
    settings: Rc<RefCell<Settings>>,
    /// False for fixture runs: they must never write the real settings file.
    persist: bool,
    alarms: Rc<VecModel<AlarmRow>>,
    /// The alarm the editor is on; `None` for a new one.
    editing: Option<usize>,
    timer: Countdown,
    timer_total: Duration,
    stopwatch: Stopwatch,
    pomodoro: Pomodoro,
    pomodoro_clock: Countdown,
    last_minute: i64,
    snoozed: Vec<Snoozed>,
    ring: Option<Ring>,
    next_ring: u64,
    test: Option<sound::Ringer>,
    ticker: slint::Timer,
    tick_ms: u64,
    tray: Option<ksni::blocking::Handle<tray::FastTray>>,
}

thread_local! {
    static APP: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
}

fn app() -> Rc<RefCell<App>> {
    APP.with(|a| a.borrow().clone().expect("the app is set up before any callback runs"))
}

fn minutes(m: u32) -> Duration {
    Duration::from_secs(u64::from(m) * 60)
}

impl App {
    fn save(&self) {
        if self.persist {
            self.settings.borrow().save();
        }
    }

    fn sound(&self) -> String {
        self.settings.borrow().sound.clone()
    }

    fn pomodoro_length(&self) -> Duration {
        let s = self.settings.borrow();
        minutes(self.pomodoro.minutes(s.work_min, s.short_min, s.long_min))
    }

    // ---- what the window shows ----

    fn alarm_row(alarm: &Alarm, tr: &Tr) -> AlarmRow {
        let days = match alarm.days {
            0 => tr.invoke_once(),
            0x7f => tr.invoke_every_day(),
            0b001_1111 => tr.invoke_weekdays(),
            mask => (0..7)
                .filter(|i| mask >> i & 1 == 1)
                .map(|i| tr.invoke_day(i).to_string())
                .collect::<Vec<_>>()
                .join(", ")
                .into(),
        };
        AlarmRow {
            time: clock::fmt_time(alarm.hour, alarm.minute).into(),
            label: alarm.label.as_str().into(),
            days,
            enabled: alarm.enabled,
        }
    }

    fn refresh_alarms(&self, ui: &Main) {
        let rows: Vec<AlarmRow> = {
            let tr = ui.global::<Tr>();
            self.settings
                .borrow()
                .alarms
                .iter()
                .map(|a| Self::alarm_row(a, &tr))
                .collect()
        };
        self.alarms.set_vec(rows);
    }

    fn countdown_state(c: &Countdown) -> i32 {
        match c {
            Countdown::Idle => 0,
            Countdown::Running(_) => 1,
            Countdown::Paused(_) => 2,
        }
    }

    /// Puts the timer, stopwatch and Pomodoro on screen.
    fn show(&self, ui: &Main, now: SystemTime, instant: Instant) {
        let tr = ui.global::<Tr>();
        // How much of a countdown is left, 0 to 1, for the thin bar under the number.
        let fraction = |left: Duration, total: Duration| {
            if total.is_zero() {
                0.0
            } else {
                (left.as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0)
            }
        };
        let timer_left = self.timer.left(now);
        let timer_text = timer_left.map(clock::fmt_countdown).unwrap_or_default();
        ui.set_timer_state(Self::countdown_state(&self.timer));
        ui.set_timer_text(timer_text.as_str().into());
        ui.set_timer_progress(fraction(timer_left.unwrap_or_default(), self.timer_total));
        let elapsed = self.stopwatch.elapsed(instant);
        ui.set_sw_state(if self.stopwatch.running() {
            1
        } else {
            i32::from(!elapsed.is_zero()) * 2
        });
        ui.set_sw_text(clock::fmt_stopwatch(elapsed).into());
        ui.set_pomo_state(Self::countdown_state(&self.pomodoro_clock));
        let length = self.pomodoro_length();
        let left = self.pomodoro_clock.left(now).unwrap_or(length);
        let pomo_text = clock::fmt_countdown(left);
        ui.set_pomo_text(pomo_text.as_str().into());
        ui.set_pomo_progress(fraction(left, length));
        // The window title (taskbar, Alt-Tab) carries the time of whatever is counting down.
        let running = |c: &Countdown| matches!(c, Countdown::Running(_));
        ui.set_clock_title(if running(&self.timer) {
            timer_text.as_str().into()
        } else if running(&self.pomodoro_clock) {
            pomo_text.as_str().into()
        } else {
            Default::default()
        });
        let next = clock::next_ring(&self.settings.borrow().alarms, clock::local_minutes(now));
        ui.set_next_alarm(match next {
            None => Default::default(),
            Some(m) if m < 60 => tr.invoke_next_min(m as i32),
            Some(m) if m < 1440 => tr.invoke_next_h_min((m / 60) as i32, (m % 60) as i32),
            Some(m) => tr.invoke_next_d_h((m / 1440) as i32, (m % 1440 / 60) as i32),
        });
        let p = self.pomodoro;
        ui.set_pomo_working(p.working);
        ui.set_pomo_done(i32::from(if p.working { p.round - 1 } else { p.round }));
        ui.set_pomo_phase(match (p.working, p.round) {
            (true, _) => tr.invoke_phase_work(),
            (false, ROUNDS) => tr.invoke_phase_long_break(),
            (false, _) => tr.invoke_phase_break(),
        });
        ui.set_pomo_round(tr.invoke_round(i32::from(p.round), i32::from(ROUNDS)));
    }

    // ---- the clock ----

    /// Called by the timer, and after every click so the screen and the tick rate follow at once.
    fn tick(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let (now, instant) = (SystemTime::now(), Instant::now());

        let minute = clock::local_minutes(now);
        if minute != self.last_minute {
            let prev = std::mem::replace(&mut self.last_minute, minute);
            let mut fired = None;
            let mut once_done = false;
            for a in self.settings.borrow_mut().alarms.iter_mut() {
                if clock::due(a, prev, minute) {
                    fired.get_or_insert_with(|| a.clone());
                    // A one-time alarm switches itself off, so it does not ring again next week.
                    if a.days == 0 {
                        a.enabled = false;
                        once_done = true;
                    }
                }
            }
            if once_done {
                self.save();
                self.refresh_alarms(&ui);
            }
            if let Some(a) = fired {
                self.start_ring(&ui, Kind::Alarm(a));
            }
        }
        if let Some(i) = self.snoozed.iter().position(|s| s.at <= now) {
            let s = self.snoozed.remove(i);
            self.start_ring(&ui, Kind::Alarm(s.alarm));
        }
        if self.timer.finished(now) {
            self.timer = Countdown::Idle;
            self.start_ring(&ui, Kind::Timer(self.timer_total));
        }
        if self.pomodoro_clock.finished(now) {
            self.phase_done(&ui);
        }
        if self.ring.as_ref().is_some_and(|r| r.started.elapsed() > RING_CAP) {
            self.end_ring(&ui);
        }

        self.show(&ui, now, instant);
        // Fast only while a visible number is moving: a hidden window or an idle clock needs one tick a second
        // (for the alarms), and a wake-up ten times a second would be a waste of power.
        let visible = ui.window().is_visible();
        let moving = |c: &Countdown| matches!(c, Countdown::Running(_));
        let wanted = if visible && self.stopwatch.running() {
            100
        } else if visible && (moving(&self.timer) || moving(&self.pomodoro_clock)) {
            250
        } else {
            1000
        };
        if wanted != self.tick_ms {
            self.tick_ms = wanted;
            self.ticker.set_interval(Duration::from_millis(wanted));
        }
    }

    fn start_ring(&mut self, ui: &Main, kind: Kind) {
        self.end_ring(ui);
        let tr = ui.global::<Tr>();
        let (title, body, alarm): (slint::SharedString, slint::SharedString, _) = match kind {
            Kind::Alarm(a) => {
                let title = if a.label.is_empty() {
                    tr.invoke_alarm()
                } else {
                    a.label.as_str().into()
                };
                (title, clock::fmt_time(a.hour, a.minute).into(), Some(a))
            }
            Kind::Timer(total) => (tr.invoke_timer_done(), clock::fmt_countdown(total).into(), None),
        };
        let id = self.next_ring;
        self.next_ring += 1;
        let mut actions = vec![("stop", tr.invoke_stop().to_string())];
        if alarm.is_some() {
            actions.insert(0, ("snooze", tr.invoke_snooze().to_string()));
        }
        ui.set_ring_title(title.clone());
        ui.set_ring_body(body.clone());
        ui.set_ring_can_snooze(alarm.is_some());
        ui.set_ringing(true);
        // An alarm brings the window up, like a phone's; a timer ringing in the background stays a notification.
        if alarm.is_some() {
            instance::raise(ui.window());
        }
        self.ring = Some(Ring {
            id,
            alarm,
            ringer: sound::ring(&self.sound(), true),
            notification: None,
            started: Instant::now(),
        });
        self.notify(id, &title, &body, actions, true);
    }

    /// Sends the notification on a thread (D-Bus blocks). Its buttons come back through `ring_action`.
    fn notify(&self, ring_id: u64, title: &str, body: &str, actions: Vec<(&'static str, String)>, critical: bool) {
        let (title, body, weak) = (title.to_string(), body.to_string(), self.ui.clone());
        std::thread::spawn(move || {
            let clicks = weak.clone();
            let id = notify::send(&title, &body, actions, critical, move |key| {
                let key = key.to_string();
                let _ = clicks.upgrade_in_event_loop(move |ui| ring_action(&ui, ring_id, &key));
            });
            let Some(id) = id else { return };
            // If the ring ended while the notification was being sent, take it back down.
            let _ = weak.upgrade_in_event_loop(move |_| {
                let app = app();
                let mut app = app.borrow_mut();
                match app.ring.as_mut().filter(|r| r.id == ring_id) {
                    Some(r) => r.notification = Some(id),
                    None => {
                        std::thread::spawn(move || notify::close(id));
                    }
                }
            });
        });
    }

    fn end_ring(&mut self, ui: &Main) {
        if let Some(r) = self.ring.take() {
            r.ringer.stop();
            if let Some(id) = r.notification {
                std::thread::spawn(move || notify::close(id));
            }
        }
        ui.set_ringing(false);
    }

    fn snooze(&mut self, ui: &Main) {
        let Some(alarm) = self.ring.as_ref().and_then(|r| r.alarm.clone()) else {
            return;
        };
        let at = SystemTime::now() + minutes(self.settings.borrow().snooze_min);
        self.snoozed.push(Snoozed { alarm, at });
        self.end_ring(ui);
    }

    /// A Pomodoro phase ended: one sound, a quiet notification, and the next phase waits for the user to start it.
    fn phase_done(&mut self, ui: &Main) {
        let was_working = self.pomodoro.working;
        self.pomodoro = self.pomodoro.next();
        self.pomodoro_clock = Countdown::Idle;
        let tr = ui.global::<Tr>();
        let title = if was_working {
            tr.invoke_work_done()
        } else {
            tr.invoke_break_done()
        };
        let next = if self.pomodoro.working {
            tr.invoke_phase_work()
        } else if self.pomodoro.round == ROUNDS {
            tr.invoke_phase_long_break()
        } else {
            tr.invoke_phase_break()
        };
        self.test = Some(sound::ring(&self.sound(), false));
        // Ring id 0 is never used by a real ring, so its buttons (it has none) can never match.
        self.notify(0, &title, &next, Vec::new(), false);
    }

    // ---- alarm editor ----

    fn open_editor(&mut self, ui: &Main, editing: Option<usize>) {
        let alarm = editing
            .and_then(|i| self.settings.borrow().alarms.get(i).cloned())
            .unwrap_or_else(|| {
                // A new alarm starts at the next full hour: the likeliest thing to want, and never in the past.
                let hour = (clock::local_minutes(SystemTime::now()).rem_euclid(1440) / 60 + 1) % 24;
                Alarm {
                    hour: hour as u8,
                    minute: 0,
                    ..Alarm::default()
                }
            });
        self.editing = editing;
        ui.set_edit_new(editing.is_none());
        ui.set_edit_hour(i32::from(alarm.hour));
        ui.set_edit_minute(i32::from(alarm.minute));
        ui.set_edit_label(alarm.label.as_str().into());
        let days: Vec<bool> = (0..7).map(|i| alarm.days >> i & 1 == 1).collect();
        ui.set_edit_days(ModelRc::new(VecModel::from(days)));
        ui.set_edit_open(true);
    }

    fn save_alarm(&mut self, ui: &Main) {
        let days = ui.get_edit_days();
        let alarm = Alarm {
            hour: ui.get_edit_hour().clamp(0, 23) as u8,
            minute: ui.get_edit_minute().clamp(0, 59) as u8,
            label: ui.get_edit_label().trim().chars().take(60).collect(),
            days: (0..7).fold(0u8, |m, i| m | u8::from(days.row_data(i).unwrap_or(false)) << i),
            // Saving an alarm means the user wants it on.
            enabled: true,
        };
        {
            let mut s = self.settings.borrow_mut();
            match self.editing.filter(|i| *i < s.alarms.len()) {
                Some(i) => s.alarms[i] = alarm,
                None => s.alarms.push(alarm),
            }
            s.alarms.sort_by_key(|a| (a.hour, a.minute));
        }
        self.editing = None;
        self.save();
        self.refresh_alarms(ui);
        ui.set_edit_open(false);
    }

    fn delete_alarm(&mut self, ui: &Main) {
        if let Some(i) = self.editing {
            self.delete_alarm_at(ui, i);
        }
        ui.set_edit_open(false);
    }

    /// Also the row's own menu, which works with no editor open.
    fn delete_alarm_at(&mut self, ui: &Main, index: usize) {
        {
            let mut s = self.settings.borrow_mut();
            if index < s.alarms.len() {
                s.alarms.remove(index);
            }
        }
        self.editing = None;
        self.save();
        self.refresh_alarms(ui);
    }

    fn toggle_alarm(&mut self, index: usize, enabled: bool) {
        let row = {
            let mut s = self.settings.borrow_mut();
            let Some(a) = s.alarms.get_mut(index) else { return };
            a.enabled = enabled;
            self.alarms.row_data(index).map(|r| AlarmRow { enabled, ..r })
        };
        if let Some(r) = row {
            self.alarms.set_row_data(index, r);
        }
        self.save();
    }

    // ---- preferences ----

    fn set_sound(&mut self, ui: &Main, path: &str) {
        self.settings.borrow_mut().sound = path.to_string();
        self.save();
        ui.set_sound_name(
            Path::new(path)
                .file_name()
                .map_or_else(Default::default, |n| n.to_string_lossy().as_ref().into()),
        );
    }

    fn tray_labels(ui: &Main) -> tray::Labels {
        let tr = ui.global::<Tr>();
        tray::Labels {
            open: tr.invoke_tray_open().to_string(),
            quit: tr.invoke_tray_quit().to_string(),
        }
    }
}

/// A button of a notification (or its body, `default`) was pressed. Ignored when that ring is already over.
fn ring_action(ui: &Main, ring_id: u64, key: &str) {
    let app = app();
    let mut app = app.borrow_mut();
    if app.ring.as_ref().is_none_or(|r| r.id != ring_id) {
        return;
    }
    match key {
        "snooze" => app.snooze(ui),
        "stop" => app.end_ring(ui),
        _ => {
            drop(app);
            instance::raise(ui.window());
        }
    }
}

fn tray_open() {
    let _ = slint::invoke_from_event_loop(|| {
        let ui = app().borrow().ui.upgrade();
        if let Some(ui) = ui {
            instance::raise(ui.window());
        }
    });
}

fn tray_quit() {
    let _ = slint::invoke_from_event_loop(|| {
        let _ = slint::quit_event_loop();
    });
}

/// Wires a Slint callback to a method of `App` that needs the window.
macro_rules! on {
    ($ui:expr, $set:ident, |$app:ident, $w:ident $(, $arg:ident)*| $body:expr) => {{
        let weak = $ui.as_weak();
        $ui.$set(move |$($arg),*| {
            let Some($w) = weak.upgrade() else { return };
            let app = app();
            #[allow(unused_mut)]
            let mut $app = app.borrow_mut();
            $body
        });
    }};
}

fn main() {
    if std::env::args().skip(1).any(|a| a == "-V" || a == "--version") {
        return println!("fastclock {}", env!("CARGO_PKG_VERSION"));
    }
    // Fixtures (scripts/ui-snapshot.sh) never touch a real running instance. Debug builds only, like the fixtures.
    #[cfg(debug_assertions)]
    let fixture_run = fixtures::mode().is_some();
    #[cfg(not(debug_assertions))]
    let fixture_run = false;
    // One window: a copy is already open, so it comes to the front and this one leaves before setting anything up.
    if !fixture_run && instance::send("show") {
        return;
    }
    // Forced so the 100 MB GL renderer is never pulled in by accident.
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()
        .expect("cannot start the Slint software renderer");

    let settings = Settings::load();
    let ui = Main::new().unwrap();
    // Take the socket now that the window exists; a later start sends "show" and this window is raised.
    if !fixture_run {
        let weak = ui.as_weak();
        let raise = move |_command: String| {
            let _ = weak.upgrade_in_event_loop(|ui| instance::raise(ui.window()));
        };
        if instance::claim("show", raise) {
            return;
        }
    }
    select_language(&language(&settings));
    ui.set_language_index(LANGUAGES.iter().position(|l| *l == settings.language).unwrap_or(0) as i32);
    ui.set_color_scheme_index(theme_index(&settings.theme));
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    ui.set_snooze_min(settings.snooze_min as i32);
    ui.set_autostart(!fixture_run && autostart::is_enabled());
    ui.set_sound_name(
        Path::new(&settings.sound)
            .file_name()
            .map_or_else(Default::default, |n| n.to_string_lossy().as_ref().into()),
    );
    let settings = Rc::new(RefCell::new(settings));

    let alarms = Rc::new(VecModel::<AlarmRow>::default());
    ui.set_alarms(alarms.clone().into());
    let ticker = slint::Timer::default();
    let tray = (!fixture_run)
        .then(|| {
            tray::spawn(
                App::tray_labels(&ui),
                tray::Actions {
                    open: tray_open,
                    quit: tray_quit,
                },
            )
        })
        .flatten();
    let has_tray = tray.is_some();
    let (pomodoro, now) = (Pomodoro::default(), SystemTime::now());
    let state = Rc::new(RefCell::new(App {
        ui: ui.as_weak(),
        settings: settings.clone(),
        persist: !fixture_run,
        alarms,
        editing: None,
        timer: Countdown::Idle,
        timer_total: Duration::ZERO,
        stopwatch: Stopwatch::default(),
        pomodoro,
        pomodoro_clock: Countdown::Idle,
        // One minute back: an alarm of the minute the app starts in (autostart at 07:00:05) still rings.
        last_minute: clock::local_minutes(now) - 1,
        snoozed: Vec::new(),
        ring: None,
        next_ring: 1,
        test: None,
        ticker,
        tick_ms: 1000,
        tray,
    }));
    APP.with(|a| *a.borrow_mut() = Some(state.clone()));
    {
        let app = state.borrow();
        app.refresh_alarms(&ui);
        app.show(&ui, now, Instant::now());
    }
    // With fixtures (scripts/ui-snapshot.sh) nothing rings, saves or runs: only canned data is shown.
    // Debug builds only: a release build must not have a hidden fake-data switch.
    #[cfg(debug_assertions)]
    if let Some(mode) = fixtures::mode() {
        fixtures::apply(&ui, &mut state.borrow_mut(), mode);
    }
    if !fixture_run {
        let ticker_app = state.clone();
        state
            .borrow()
            .ticker
            .start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
                ticker_app.borrow_mut().tick();
            });
    }

    // The clock faces draw one fixed-width cell per character (see `Glyphs` in clock.slint).
    ui.global::<Digits>().on_split(|text| {
        ModelRc::new(VecModel::from(
            text.chars()
                .map(|c| c.to_string().into())
                .collect::<Vec<slint::SharedString>>(),
        ))
    });
    on!(ui, on_alarm_add, |a, ui| a.open_editor(&ui, None));
    on!(ui, on_alarm_edit, |a, ui, i| a
        .open_editor(&ui, usize::try_from(i).ok()));
    // `tick()` after each change refreshes the "Next alarm in ..." line.
    on!(ui, on_alarm_toggled, |a, _ui, i, on| {
        if let Ok(i) = usize::try_from(i) {
            a.toggle_alarm(i, on);
        }
        a.tick();
    });
    on!(ui, on_alarm_delete, |a, ui, i| {
        if let Ok(i) = usize::try_from(i) {
            a.delete_alarm_at(&ui, i);
        }
        a.tick();
    });
    on!(ui, on_edit_save, |a, ui| {
        a.save_alarm(&ui);
        a.tick();
    });
    on!(ui, on_edit_delete, |a, ui| {
        a.delete_alarm(&ui);
        a.tick();
    });

    on!(ui, on_timer_start, |a, ui| {
        let secs = u64::from(ui.get_timer_hours().max(0) as u32) * 3600
            + u64::from(ui.get_timer_minutes().max(0) as u32) * 60
            + u64::from(ui.get_timer_seconds().max(0) as u32);
        if secs > 0 {
            a.timer_total = Duration::from_secs(secs);
            a.timer = Countdown::start(a.timer_total, SystemTime::now());
        }
        a.tick();
    });
    on!(ui, on_timer_pause, |a, _ui| {
        a.timer.pause(SystemTime::now());
        a.tick();
    });
    on!(ui, on_timer_resume, |a, _ui| {
        a.timer.resume(SystemTime::now());
        a.tick();
    });
    on!(ui, on_timer_reset, |a, _ui| {
        a.timer = Countdown::Idle;
        a.tick();
    });

    on!(ui, on_sw_start, |a, _ui| {
        a.stopwatch.start(Instant::now());
        a.tick();
    });
    on!(ui, on_sw_pause, |a, _ui| {
        a.stopwatch.pause(Instant::now());
        a.tick();
    });
    on!(ui, on_sw_resume, |a, _ui| {
        a.stopwatch.start(Instant::now());
        a.tick();
    });
    on!(ui, on_sw_reset, |a, _ui| {
        a.stopwatch.reset();
        a.tick();
    });

    on!(ui, on_pomo_start, |a, _ui| {
        a.pomodoro_clock = Countdown::start(a.pomodoro_length(), SystemTime::now());
        a.tick();
    });
    on!(ui, on_pomo_pause, |a, _ui| {
        a.pomodoro_clock.pause(SystemTime::now());
        a.tick();
    });
    on!(ui, on_pomo_resume, |a, _ui| {
        a.pomodoro_clock.resume(SystemTime::now());
        a.tick();
    });
    on!(ui, on_pomo_reset, |a, _ui| {
        a.pomodoro = Pomodoro::default();
        a.pomodoro_clock = Countdown::Idle;
        a.tick();
    });
    on!(ui, on_pomo_skip, |a, _ui| {
        let running = matches!(a.pomodoro_clock, Countdown::Running(_));
        a.pomodoro = a.pomodoro.next();
        a.pomodoro_clock = if running {
            Countdown::start(a.pomodoro_length(), SystemTime::now())
        } else {
            Countdown::Idle
        };
        a.tick();
    });

    on!(ui, on_ring_stop, |a, ui| a.end_ring(&ui));
    on!(ui, on_ring_snooze, |a, ui| a.snooze(&ui));

    on!(ui, on_pick_sound, |_a, ui| {
        let tr = ui.global::<Tr>();
        let (title, filter) = (tr.invoke_pick_title().to_string(), tr.invoke_pick_filter().to_string());
        let weak = ui.as_weak();
        std::thread::spawn(move || match portal::pick_sound(&title, &filter) {
            Ok(Some(path)) => {
                let _ = weak.upgrade_in_event_loop(move |ui| app().borrow_mut().set_sound(&ui, &path));
            }
            Ok(None) => {}
            Err(e) => eprintln!("cannot choose a sound: {e}"),
        });
    });
    on!(ui, on_reset_sound, |a, ui| a.set_sound(&ui, ""));
    on!(ui, on_test_sound, |a, _ui| {
        if let Some(t) = a.test.take() {
            t.stop();
        }
        a.test = Some(sound::ring(&a.sound(), false));
    });
    on!(ui, on_snooze_changed, |a, _ui, v| {
        a.settings.borrow_mut().snooze_min = v.clamp(1, 120) as u32;
        a.save();
    });
    on!(ui, on_autostart_changed, |_a, ui, on| {
        let weak = ui.as_weak();
        // The Background portal may show the desktop's own question, so it runs on a thread.
        std::thread::spawn(move || {
            if let Err(e) = autostart::set(on) {
                eprintln!("cannot change start at login: {e}");
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_autostart(!on));
            }
        });
    });

    // Saved as soon as it changes: a killed session would otherwise lose the choice.
    ui.on_theme_changed({
        let settings = settings.clone();
        move |index| {
            let mut s = settings.borrow_mut();
            let theme = theme_name(index);
            // Also fires once at start-up; saving then would rewrite the file (and, with fixtures, the real one).
            if s.theme != theme {
                s.theme = theme.into();
                s.save();
            }
        }
    });
    ui.on_language_changed({
        let (settings, weak) = (settings.clone(), ui.as_weak());
        move |index| {
            let code = usize::try_from(index)
                .ok()
                .and_then(|i| LANGUAGES.get(i))
                .copied()
                .unwrap_or("");
            {
                let mut s = settings.borrow_mut();
                // Same guard as the theme: the start-up notification must not rewrite the file.
                if s.language == code {
                    return;
                }
                s.language = code.into();
                s.save();
                select_language(&language(&s));
            }
            // The strings Rust built (alarm days, tray menu) are in the old language until redone.
            if let Some(ui) = weak.upgrade() {
                let app = app();
                let app = app.borrow();
                app.refresh_alarms(&ui);
                app.show(&ui, SystemTime::now(), Instant::now());
                if let Some(t) = &app.tray {
                    tray::set_labels(t, App::tray_labels(&ui));
                }
            }
        }
    });
    ui.on_open_website(|| {
        // Waiting in a thread reaps the child; a dropped `Child` would stay a zombie until exit.
        std::thread::spawn(|| {
            if let Err(e) = std::process::Command::new("xdg-open")
                .arg(env!("CARGO_PKG_REPOSITORY"))
                .status()
            {
                eprintln!("cannot open the website: {e}");
            }
        });
    });
    // Opens a page or an overlay directly, for the same script.
    #[cfg(debug_assertions)]
    fixtures::open_page(&ui, &mut state.borrow_mut());

    {
        let s = settings.borrow();
        ui.window()
            .set_size(slint::LogicalSize::new(s.width as f32, s.height as f32));
    }
    // Closing the window hides it: alarms keep ringing and the tray brings it back. Without a tray there would be no
    // way back to a hidden window, so then it quits.
    ui.window().on_close_requested({
        let (weak, settings) = (ui.as_weak(), settings.clone());
        move || {
            if let Some(ui) = weak.upgrade() {
                save_size(&ui, &settings);
            }
            if !has_tray {
                let _ = slint::quit_event_loop();
            }
            slint::CloseRequestResponse::HideWindow
        }
    });
    // --hidden (start at login) goes straight to the tray; without one there would be nothing to click.
    if !(has_tray && std::env::args().any(|a| a == "--hidden")) {
        ui.show().unwrap();
    }
    // Not `ui.run()`: that ends when the window is hidden, and the alarms would die with it.
    slint::run_event_loop_until_quit().unwrap();
    // Normal exit only; the snapshot script kills the app, so fixtures never overwrite the real file.
    save_size(&ui, &settings);
    let ui_handle = app();
    let mut a = ui_handle.borrow_mut();
    if let Some(r) = a.ring.take() {
        r.ringer.stop();
    }
    sound::cleanup();
}

fn save_size(ui: &Main, settings: &RefCell<Settings>) {
    let size = ui.window().size().to_logical(ui.window().scale_factor());
    let mut settings = settings.borrow_mut();
    settings.width = size.width.round() as u32;
    settings.height = size.height.round() as u32;
    settings.theme = theme_name(ui.get_color_scheme_index()).into();
    settings.save();
}

/// `settings.language` for each entry of the Language selector: system, English, Português.
const LANGUAGES: [&str; 3] = ["", "en", "pt_BR"];

/// An unknown locale (`de_DE`) leaves the previous language selected, so fall back to the English source strings
/// explicitly: otherwise going from Português back to System on such a desktop would stay Portuguese.
fn select_language(code: &str) {
    if slint::select_bundled_translation(code).is_err() {
        let _ = slint::select_bundled_translation("");
    }
}

/// `settings.theme` for each entry of the Appearance list: system, light, dark.
const THEMES: [&str; 3] = ["", "light", "dark"];

/// The Appearance list index of a `settings.theme` value; anything unknown follows the system.
fn theme_index(theme: &str) -> i32 {
    THEMES.iter().position(|t| *t == theme).unwrap_or(0) as i32
}

/// The `settings.theme` value for an Appearance list index.
fn theme_name(index: i32) -> &'static str {
    THEMES.get(index as usize).copied().unwrap_or("")
}

/// `settings.language`, or the system locale (`LC_ALL`/`LC_MESSAGES`/`LANGUAGE`/`LANG`, minus `.UTF-8`).
fn language(settings: &Settings) -> String {
    if !settings.language.is_empty() {
        return settings.language.clone();
    }
    ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.is_empty())
        .map(|v| locale_code(&v).to_string())
        .unwrap_or_default()
}

/// `pt_BR.UTF-8` / `pt_BR:en` / `pt_BR@euro` -> `pt_BR`.
fn locale_code(v: &str) -> &str {
    v.split(['.', ':', '@']).next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_and_their_indexes_agree() {
        for (i, t) in THEMES.iter().enumerate() {
            assert_eq!(theme_index(t), i as i32);
            assert_eq!(theme_name(i as i32), *t);
        }
        assert_eq!(theme_index("bogus"), 0);
    }

    #[test]
    fn locale_code_drops_encoding_fallbacks_and_modifier() {
        assert_eq!(locale_code("pt_BR:en"), "pt_BR");
        assert_eq!(locale_code("pt_BR.UTF-8"), "pt_BR");
        assert_eq!(locale_code("pt_BR@euro"), "pt_BR");
        assert_eq!(locale_code("C"), "C");
    }
}
