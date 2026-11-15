# AGENTS.md: FastClock

Notes for AI agents and contributors. Read this before changing anything.


## What this is

A **light and fast** clock for Linux in Rust + Slint: Alarm, Timer, Stopwatch and a simple Pomodoro. Alarms and timers ring a sound (the built-in bell, or a file the user picks) and raise a desktop notification with Snooze/Stop. Priorities: fast start, low memory, simplicity.

## Layout

```txt
Cargo.toml                workspace, release/fast profiles
crates/core/              UI-independent logic: `settings.rs`, `clock.rs` (`Alarm`, `due`, `Countdown`, `Stopwatch`, `Pomodoro`, time formatting; every function takes "now", so tests need no real clock). No Slint here, so a CLI can reuse it
crates/app/src/           `main.rs` (`App`: all clock state, on the UI thread, reached by `app()`), `notify.rs`, `sound.rs`, `portal.rs`, `tray.rs`, `autostart.rs`, `fixtures.rs` (see "How it rings")
crates/app/               Slint UI + main.rs; build.rs compiles ui/ and bundles translations/
crates/app/src/instance.rs  one window: a second start sends `show` over `$XDG_RUNTIME_DIR/<crate>.sock` and exits; the running one is raised. Identical in every Fast app (this is the master): change it here and copy it to the others
crates/core/src/files.rs  private (0700/0600) atomic writes and `read_or_backup` (a bad file is kept as `<name>.bad.<time>`, never over an older backup). Identical in every Fast app (this is the master): change it here and copy it to the others
packaging/                .desktop, SVG icon, 256px PNG icon (the AppImage .DirIcon), metainfo (AppImage, Flatpak and CI validation)
appimage/build.sh         AppImage + .zsync (host libs, not bundled); CI: appimage.yml
flatpak/                  manifest, build.sh (vendors crates, builds outside the repo: `~/.cache/fastclock-flatpak`), new-signing-key.sh, signing-key.b64 (public half, after you run it); CI: flatpak.yml
docs/                     landing page (`site/index.html`, deployed with the Flatpak remote), `screenshots/`, FLATPAK-SIGNING.md, FLATHUB.md, ICON-STYLE.md and UI-STYLE.md (the shared icon and window rules of the Fast apps; the UI values live in `crates/app/ui/family.slint`, and the About card and Preferences row in `about.slint`; both are copied verbatim into every app, together with `controls.slint` (`TabBar`, `TabItem`, `MenuButton`, `CloseButton`, `StepButton`, `PlusIcon`, `AlertIcon`, `Icon`, `NeutralBadge`, `FilterBar`, `SearchBox`; the Widgets page shows the last four), and the shared icons in `ui/icons/`, of which each app keeps only those it uses)
README.md, BUILD.md       what users read / build, settings and details (keep both in step with behaviour)
dev/gen_po.py             writes the .po files from one table
scripts/ui-snapshot.sh    renders pages on a private Xvfb with canned data (crates/app/src/fixtures.rs, debug builds only, env FASTCLOCK_FIXTURES=empty|single|many, FASTCLOCK_PAGE=0..3 (alarm, timer, stopwatch, pomodoro) or `settings` / `about` / `edit` / `edit-existing` / `ringing` for the dialogs; with no ImageMagick it grabs the window with ffmpeg) to target/ui-snapshots/
```

## Commands

```bash
cargo build --profile fast    # iteration build, no LTO
cargo build --release         # thin LTO
cargo test                    # unit tests
```

System deps: `libfontconfig-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-dev libxcb1-dev`. No GL/Skia/femtovg.

## Code conventions: Editing code

- Use the Edit tool for code changes. Don't splice files with sed, awk, python or heredocs through Bash.
- After editing Rust, run `cargo fmt && cargo clippy -- -D warnings && cargo test`. Re-read the surrounding block to confirm no braces, lines or whole components (e.g. pages) were deleted.
- After changing dependencies (`Cargo.toml` or `Cargo.lock`), also run `cargo audit`: CI runs it and fails on known vulnerabilities (warnings such as "unmaintained" do not fail). Fix with `cargo update -p <crate>`.
- After any change, also run `cargo build --profile fast` so the binary is built and cached for the owner's manual testing.

## Architecture decisions (inherited from FastMoji, measured there)

- **Software renderer only** (`winit` + `software`, forced in `main.rs` and in `Cargo.toml` features with `default-features = false`): ~27 MB RSS versus ~108 MB with the GL renderer.
- No async runtime. Scans/work in threads, results handed to the UI thread with `slint::invoke_from_event_loop`.
- Never hold a `RefCell` borrow while calling something that makes Slint re-read a model.
- Keep dependencies few and light.
- Slint notes: a `VerticalLayout` inside a `ScrollView` needs `alignment: start`, or a short list spreads its rows over the window; read-only `TextInput` (one per block) makes text selectable; use `content-y` (not `viewport-y`); a callback named `close` shadows a builtin (use `dismiss`); `FocusScope.capture-key-pressed` intercepts keys before `LineEdit`; a wrapped read-only `TextInput` in a nested layout reports one line of height, so give it room with `min-height`, estimated from the available width (a fixed character count leaves a blank band under lines that still fit on one line); `Text` and `TextInput` have no `padding`, so put the space in the parent layout's `spacing` or `padding`.

## Look

- Native Fluent widgets on purpose: apps follow the system accent and light/dark mode. In Slint 1.18 the `Palette` colours are read-only; only `Palette.color-scheme` can be set (the Appearance setting does that). To change a colour you would have to write your own component.
- Pages: `ui/clock.slint` has `AlarmPage`, `TimerPage`, `StopwatchPage`, `PomodoroPage` (and `DayToggle`); the dialogs (`AlarmDialog`, `RingingDialog`, `SettingsDialog`, `About`) are in `main.slint` because they inherit its `Dialog`. A `.po` context is the component name: `Tr`, `Main`, `AlarmPage`, `TimerPage`, `StopwatchPage`, `PomodoroPage`, `StartPauseReset`, `AlarmDialog`, `RingingDialog`, `SettingsDialog`, `About`.
- The clock face (56 px) is the one size outside `Family`'s type scale, on purpose: a number is the whole page. Everything else uses the scale.
- Keys: `main-keys` (a `FocusScope` around the sidebar and pages) turns Space into start/pause for the open page (`toggle-run`); it only sees keys the focused widget rejected, so Space on a button still presses it, and it needs `main-keys.focus()` at start-up and when a dialog closes (the `changed ...-open` handlers). `Dialog` calls `confirm` on Enter while the dialog itself has focus (`AlarmDialog` saves, `RingingDialog` stops); the label `LineEdit` has its own `accepted`. Verified with real keys on a private Xvfb, not only snapshots.
- `AlarmDialog` sets the time with `TimeStepper` (big zero-padded number, the kit's `StepButton`s, wheel); the timer and Pomodoro show a `ProgressBar` of the time left; the window title carries the running countdown (`clock-title`); an alarm (not a timer) raises the window when it rings.
- Clock numbers are drawn by `Glyphs` (clock.slint): one fixed-width cell per character, cut by the `Digits.split` callback that `main.rs` fills, so a proportional font cannot make a running clock shift. The sidebar is our own `NavItem`s (icon + title, Tab and Space/Enter), not `StandardListView`. `DayToggle` does not flip itself: the alarm dialog owns `days`, because the Weekdays/Weekend buttons set several at once. A `ContextMenuArea` inside a layout takes a slot: put it in a plain `Rectangle` beside its button.
- Strings that Rust builds (weekday names, "Weekdays", phase names, notification texts, tray labels) are functions of the `Tr` global; Rust only joins them.
- Snapshots are 600x450 and cannot scroll. To see more, render with a taller window: put `height = 900` in a `settings.toml` under a scratch `XDG_DATA_HOME` and run the script with that variable set.
- `LANGUAGE` (often `en_US` on a dev machine) beats `LANG`: to check a translation run the script with `LC_ALL=pt_BR.UTF-8 LANGUAGE=`.

## Dialogs

- Dialogs are in-window overlays (the last children of `Main`), not second windows: closing Main always ends the app and `scripts/ui-snapshot.sh` can capture them (`FASTCLOCK_PAGE=settings|about`). They share one shell, `component Dialog` (dim background, Esc, Tab trap, card capped to the window height); a new dialog inherits it and puts its content inside. `SettingsDialog` (burger button in the sidebar header) holds Alarm sound (Choose…/Test/Reset), Snooze minutes, Start at login, Appearance and Language plus an About button; `About` opens from it and closing About returns to Settings (re-creating Settings gives its keys focus back, so Esc keeps working). The About version comes from `CARGO_PKG_VERSION`, the Website button opens `CARGO_PKG_REPOSITORY` with `xdg-open`. A `ScrollView` does not report its content's height, hence the explicit `preferred-height` in the settings card.
- Theme and language are saved the moment they change (`theme-changed` / `language-changed`, skipped when the value equals the stored one so start-up never writes); window size only on a normal exit. An unknown locale falls back to the English source strings (`select_language` in `main.rs`).

## Translations

- UI strings use `@tr()`; strings built in Rust go through the `Tr` global in `ui/main.slint`. Never `format!` user-visible text in Rust.
- `.po` files are generated by `dev/gen_po.py` (edit its table, run it, commit both). Slint's gettext context is the component name (`Main`, `Tr`). A new `@tr` string needs an entry in every language. A string with a count needs its own singular string for `n == 1` ("1 connection", not "1 connections"), with its own entry.
- Language = `settings.language` (`""` = system locale).
- Appearance = `settings.theme` (`""` = follow the system, `"light"`, `"dark"`). The Appearance box sets `Palette.color-scheme` in the `.slint` (`unknown` hands it back to the desktop). `THEMES` and `theme_index` in `crates/app/src/main.rs` keep the order of that list, and a test checks they agree. `Family.dark` reads the drawn background, so it stays right in every state.

## Conventions

- Rust 2024, `rustfmt` max_width 120, clippy `-D warnings`. Comments explain *why*, not what.
- Settings are serde TOML with `#[serde(default)]`; add a unit test for each new option so old files keep loading.
- Settings path: `$XDG_DATA_HOME/FastClock/settings.toml`.
- Update `README.md` when behaviour changes, and this file when you learn something non-obvious.

## Testing

- `cargo test`. Manual harnesses (if any) go in `dev/`, must never capture the whole screen, and must never click or type at hard-coded coordinates.
- Never `pkill -f` with a pattern that appears in your own command line; kill by PID.

### Verifying UI changes

- After any Slint UI change, run `scripts/ui-snapshot.sh <page>` and read the PNGs (empty, single, many) before saying the change is done. It uses a private Xvfb and canned data, so it never reads or changes the real system.
- Specifically check: rows don't stretch when a list has 1 item, badges and buttons are right-aligned, icons are sharp, items with no icon get a placeholder so headers stay aligned, vertical gaps between rows of one kind are even, counts read right at 1 ("1 items"), a name already in a header is not repeated in every row under it, text is selectable as a block, and empty or error states (e.g. "nothing found") don't throw errors.
- Controls in a row are the same height (32 px); tabs, search boxes and dialog cards use the kit's components (`TabBar`, `SearchBox`, the card in `docs/UI-STYLE.md`).
- If you can't launch it (e.g. the flow needs pkexec or a live X11 session), say so explicitly and list what I should test manually.

## GitHub flow and releases

Work on branches, PRs to `master`. CI: `ci.yml` (fmt, clippy, tests, MSRV, metainfo/desktop validation), `appimage.yml` (AppImage on every push/PR; on a `v*` tag it attaches to the release), `flatpak.yml` (bundle on every push/PR; on a `v*` tag it also publishes the signed remote and the landing page to GitHub Pages, then attaches the bundle), `virustotal.yml` (scans the AppImage and the Flatpak after their workflows; needs the `VIRUSTOTAL_API_KEY` secret). `RELEASING.md` has the one-time setup and the steps. Never commit or push on the owner's behalf.

Release gotchas (each one broke a first release):

- Bump the version in `Cargo.toml`, then run `cargo check` and commit `Cargo.lock`: CI uses `--locked`, so a stale lock fails every job.
- The metainfo needs a `<release version="X">` for the tag, or `flatpak.yml` fails.
- `appimagetool` builds `.DirIcon` from a PNG at the AppDir root, not from the SVG. The icon must follow `docs/ICON-STYLE.md` (the same tile shape for every Fast app, flat colors only). Regenerate `packaging/io.github.evandro777.FastClock.png` when the SVG changes: `convert -background none -density 384 packaging/io.github.evandro777.FastClock.svg -resize 256x256 packaging/io.github.evandro777.FastClock.png`.
- The first tag needs the one-time setup in `RELEASING.md`: the signing key and its secret, Pages, and the `v*` tag rule on the `github-pages` environment.

## Flatpak

App id `io.github.evandro777.FastClock`; `flatpak/build.sh --devel` installs `...FastClock.Devel` next to a release. Needs `flatpak-builder` and `org.freedesktop.Sdk//25.08` + `org.freedesktop.Sdk.Extension.rust-stable//25.08`. The runtime version `25.08` appears in five places (the manifest, the container image tag, the rust-stable install and its `GITHUB_PATH` line in `flatpak.yml`, and the header of `flatpak/build.sh`): bump them together.

- Auto-update = our own signed OSTree remote on GitHub Pages (`flatpak.yml`: `bundle` → `pages` → `release`), deployed on `v*` tags only. Every deploy replaces the whole site, so never deploy Pages from another trigger. The public key is `flatpak/signing-key.b64` (base64 of the binary export, made by `flatpak/new-signing-key.sh`; the template ships none); the private half is the secret `FLATPAK_GPG_PRIVATE_KEY`. See `docs/FLATPAK-SIGNING.md`.
- Keep `finish-args` minimal (the manifest says how for X11-only apps and trays). The build vendors crates (`cargo vendor`), so `flatpak/vendor` must not be in the manifest's `skip:`.
- Patterns to copy when an app needs them (FastClipboard has each one working): inside the sandbox `FLATPAK_ID` is set. A single-instance Unix socket must live in `$XDG_RUNTIME_DIR/app/$FLATPAK_ID/`, the only directory shared between `flatpak run`s. A ksni tray needs `.disable_dbus_name(true)` there. Start at login must go through the Background portal (`org.freedesktop.portal.Background.RequestBackground`, blocking, so from a thread), not a file in `~/.config`. A host command (for example `tesseract`) is reachable only through `flatpak-spawn --host`, which needs the user's own opt-in `flatpak override --talk-name=org.freedesktop.Flatpak`: never put that in the manifest.
- **Flathub policy:** manifests must NOT contain AI-generated content and AI must not open the submission PR. Do not write a Flathub manifest or submission text for the owner; `docs/FLATHUB.md` explains the process.

## How it rings

- **One tick** (`slint::Timer` in `App.ticker`, `App::tick`): every 1 s while idle or hidden (alarms are checked per minute: `clock::due(alarm, prev, now)` looks at the minutes in `(prev, now]`, at most 3 h back, so a late tick or a short suspend still rings and nothing rings twice), 250 ms while a timer/Pomodoro runs in a visible window, 100 ms for a running stopwatch. Every click calls `tick()` so the screen and the rate follow at once. Countdowns store a wall-clock end (`SystemTime`, survives suspend); the stopwatch uses `Instant`.
- **Ring:** `App::start_ring` plays the sound (`sound::ring`, looping), shows `RingingDialog`, and sends a critical notification from a thread. Only one ring at a time; a new one replaces the old. A ring has an id: a notification button of an earlier ring is ignored (`ring_action`), and a notification that arrives after its ring ended is closed at once. Stop/Snooze/the 5-minute cap all call `end_ring`, which also closes the notification (`notify::close`). Snooze is kept in memory only. A one-time alarm turns itself off and is saved. A Pomodoro phase end plays once, notifies quietly and leaves the next phase stopped.
- **Sound** is `paplay`, else `pw-play` (both in the Flatpak runtime; no audio crate). The bell is a WAV generated in code and written to `$XDG_RUNTIME_DIR`. A custom file that the player rejects (an m4a, a missing file) falls back to the bell, because a silent alarm is the worst failure. The portal filter lists only formats libsndfile reads.
- **Notifications** (`notify.rs`) are copied from FastSync, where each rule was learned the hard way: the bus connection is kept open (a sender that leaves the bus loses its popup), a `default` action is always declared so a click on the body is reported, and `listen` subscribes before the first `Notify`. Added here: `close(id)` and the urgency argument. Never call it on the UI thread (it blocks).
- **Tray and closing:** closing the window hides it when a tray exists (`App.tray`, ksni), else quits. `main` must end with `ui.show()` + `run_event_loop_until_quit()`, never `ui.run()`, or hiding the window ends the loop and the alarms die. `--hidden` (what Start at login runs) skips `show()` only when there is a tray.
- **Fixture runs** (debug builds, `FASTCLOCK_FIXTURES`) never save alarms (`App.persist`), start the ticker, make sound, notify, open the tray or touch autostart. Live tests (typing real keys) go in a scratch `XDG_DATA_HOME` under `dbus-run-session`, with no sound server: otherwise they ring your speakers, notify and overwrite your alarms.
- **Flatpak:** `--socket=pulseaudio`, `--talk-name=org.freedesktop.Notifications` and `--talk-name=org.kde.StatusNotifierWatcher`; the tray is built with `disable_dbus_name` inside the sandbox; the file chooser and Start at login go through portals.

## Open items

- Not verified on a real desktop: that Cinnamon reports a click on the notification body and on the Snooze/Stop buttons; the portal file chooser (native and Flatpak); the tray icon and Quit; Start at login (native and through the Background portal); sound through the user's real PipeWire/PulseAudio; an alarm after a real suspend.
- `cargo audit` has not been run (not installed where this was written).
- Skipped on purpose: lap times, snooze that survives a restart, a sound per alarm, Pomodoro lengths in the UI (they are in `settings.toml`).
