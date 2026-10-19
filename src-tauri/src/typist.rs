//! Sends a typing plan to another window as real key presses, and pauses when the user takes
//! focus away, clicks, or presses Esc. Runs on its own thread; the UI talks to it through
//! [`Control`] messages and hears back through `typer-status` events.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use device_query::{DeviceQuery, DeviceState, Keycode};
use enigo::{Direction, Enigo, Keyboard, Settings};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use typing_plan::{Action, Key, Plan};

use crate::focus::{self, Target};

pub const STATUS_EVENT: &str = "typer-status";

/// How often to look at the keyboard, mouse and focus while waiting.
const POLL: Duration = Duration::from_millis(10);
/// Delay after the target is chosen, so the click that chose it is over.
const SETTLE: Duration = Duration::from_millis(500);

pub enum Control {
    Pause,
    Resume,
    Stop,
    SetWpm(u32),
    SetTypoRate(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PauseReason {
    Escape,
    Click,
    FocusLost,
    Requested,
    /// The OS refused the key press (e.g. the target runs elevated, or the screen locked).
    Blocked,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Status {
    /// Waiting for the user to click into the window to type in.
    Waiting,
    #[serde(rename_all = "camelCase")]
    Typing { position: usize, total: usize, wpm: f64 },
    #[serde(rename_all = "camelCase")]
    Paused { position: usize, reason: PauseReason },
    #[serde(rename_all = "camelCase")]
    Done { target: Target },
    #[serde(rename_all = "camelCase")]
    Stopped { position: usize },
    Failed { message: String },
}

pub struct Options {
    pub pause_on_click: bool,
    /// The window typed into last time; Start goes straight back to it if it still exists.
    pub remembered: Option<Target>,
}

enum Interrupt {
    Pause(PauseReason),
    Stop,
}

struct Typist {
    app: AppHandle,
    rx: Receiver<Control>,
    opts: Options,
    enigo: Enigo,
    device: DeviceState,
    target: Option<Target>,
    total: usize,
    /// Everything before this is typed correctly.
    position: usize,
    /// Scratch chars typed and not yet erased (a typo in progress).
    dirty: usize,
    /// No key has been pressed since the last progress mark.
    at_boundary: bool,
    /// A pause asked for in the middle of a key sequence that must not be split.
    pending: Option<Interrupt>,
    mouse_down: bool,
    wpm_clock: WpmClock,
    last_emit: Instant,
}

pub fn run(app: AppHandle, plan: Plan, rx: Receiver<Control>, opts: Options) {
    let enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => {
            emit(&app, Status::Failed { message: format!("Can't send key presses: {e}") });
            return;
        }
    };
    let total = plan.len();
    let mut typist = Typist {
        app,
        rx,
        opts,
        enigo,
        device: DeviceState::new(),
        target: None,
        total,
        position: 0,
        dirty: 0,
        at_boundary: true,
        pending: None,
        mouse_down: false,
        wpm_clock: WpmClock::default(),
        last_emit: Instant::now(),
    };
    typist.mouse_down = typist.mouse_pressed();
    typist.run(plan);
}

fn emit(app: &AppHandle, status: Status) {
    let _ = app.emit(STATUS_EVENT, status);
}

impl Typist {
    fn run(&mut self, mut plan: Plan) {
        let mut resuming = false;
        loop {
            let target = match self.acquire_target(resuming) {
                Ok(t) => t,
                Err(Interrupt::Stop) => return self.status(Status::Stopped { position: self.position }),
                Err(Interrupt::Pause(reason)) => {
                    self.status(Status::Paused { position: self.position, reason });
                    match self.wait_for_resume() {
                        true => {
                            resuming = true;
                            continue;
                        }
                        false => return self.status(Status::Stopped { position: self.position }),
                    }
                }
            };
            let same_window = self.target == Some(target);
            self.target = Some(target);
            if resuming {
                // A pause can land mid-typo; erase the wrong keys first, but only in the same
                // window — in another box that would delete the user's own text.
                if same_window {
                    for _ in 0..self.dirty {
                        let _ = self.tap(enigo::Key::Backspace, 0.04);
                        thread::sleep(Duration::from_millis(60));
                    }
                }
                self.dirty = 0;
                plan.seek(self.position);
            }

            match self.type_plan(&mut plan) {
                Ok(()) => return self.status(Status::Done { target }),
                Err(Interrupt::Stop) => return self.status(Status::Stopped { position: self.position }),
                Err(Interrupt::Pause(reason)) => {
                    self.wpm_clock.pause();
                    self.status(Status::Paused { position: self.position, reason });
                    if !self.wait_for_resume() {
                        return self.status(Status::Stopped { position: self.position });
                    }
                    resuming = true;
                }
            }
        }
    }

    /// The window to type into: the one paused in or remembered from last time if it can be
    /// brought back, otherwise whichever window the user clicks into next.
    fn acquire_target(&mut self, resuming: bool) -> Result<Target, Interrupt> {
        let previous = if resuming { self.target } else { self.opts.remembered };
        if let Some(t) = previous.filter(|t| focus::activate(*t)) {
            let deadline = Instant::now() + Duration::from_secs(1);
            while Instant::now() < deadline {
                if focus::foreground() == Some(t) {
                    thread::sleep(Duration::from_millis(250));
                    return Ok(t);
                }
                self.poll_controls_only()?;
                thread::sleep(POLL);
            }
        }

        self.status(Status::Waiting);
        loop {
            self.poll_controls_only()?;
            if let Some(t) = focus::foreground().filter(|t| !t.is_own()) {
                // Let the click that picked the window finish.
                while self.mouse_pressed() {
                    self.poll_controls_only()?;
                    thread::sleep(POLL);
                }
                self.mouse_down = false;
                thread::sleep(SETTLE);
                if focus::foreground() == Some(t) {
                    return Ok(t);
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn type_plan(&mut self, plan: &mut Plan) -> Result<(), Interrupt> {
        self.wpm_clock.resume();
        self.at_boundary = true;
        while let Some(action) = plan.next() {
            match action {
                Action::Wait(s) => self.sleep(s, plan)?,
                Action::Progress(p) => {
                    self.position = p;
                    self.at_boundary = true;
                    self.wpm_clock.typed(1);
                    if let Some(i) = self.pending.take() {
                        return Err(i);
                    }
                    self.report_progress(false);
                }
                Action::Char { ch, hold } => {
                    self.before_key(plan)?;
                    self.type_char(ch, hold)?;
                }
                Action::Scratch { ch, hold } => {
                    self.before_key(plan)?;
                    self.type_char(ch, hold)?;
                    self.dirty += 1;
                }
                Action::Erase { hold } => {
                    self.before_key(plan)?;
                    self.tap(enigo::Key::Backspace, hold)?;
                    self.dirty = self.dirty.saturating_sub(1);
                }
                Action::Key { key, shift, hold } => {
                    self.before_key(plan)?;
                    self.press_key(key, shift, hold)?;
                }
            }
        }
        self.report_progress(true);
        Ok(())
    }

    /// Right before a key goes out: never type into the wrong window, and don't turn the
    /// user's held Ctrl/Alt/Win into a shortcut.
    fn before_key(&mut self, plan: &mut Plan) -> Result<(), Interrupt> {
        self.check(plan)?;
        if focus::foreground() != self.target {
            return Err(Interrupt::Pause(PauseReason::FocusLost));
        }
        while self.modifier_held() {
            self.check(plan)?;
            thread::sleep(POLL);
        }
        self.at_boundary = false;
        Ok(())
    }

    fn sleep(&mut self, seconds: f64, plan: &mut Plan) -> Result<(), Interrupt> {
        let end = Instant::now() + Duration::from_secs_f64(seconds);
        loop {
            self.check(plan)?;
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            thread::sleep(left.min(POLL));
        }
    }

    /// Look for anything that should stop typing. Interrupts act right away at safe points
    /// (between characters, or mid-typo where erasing is tracked); inside a sequence like a
    /// code-mode new line they wait for the next progress mark.
    fn check(&mut self, plan: &mut Plan) -> Result<(), Interrupt> {
        let found = match self.poll(Some(plan)) {
            Ok(()) => return Ok(()),
            Err(i) => i,
        };
        if matches!(found, Interrupt::Stop) || self.at_boundary || self.dirty > 0 {
            return Err(found);
        }
        self.pending.get_or_insert(found);
        Ok(())
    }

    fn poll_controls_only(&mut self) -> Result<(), Interrupt> {
        self.poll(None)
    }

    fn poll(&mut self, plan: Option<&mut Plan>) -> Result<(), Interrupt> {
        let mut plan = plan;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Control::Stop => return Err(Interrupt::Stop),
                Control::Pause => return Err(Interrupt::Pause(PauseReason::Requested)),
                Control::Resume => {}
                Control::SetWpm(w) => {
                    if let Some(p) = plan.as_deref_mut() {
                        p.set_wpm(w);
                    }
                }
                Control::SetTypoRate(r) => {
                    if let Some(p) = plan.as_deref_mut() {
                        p.set_typo_rate(r);
                    }
                }
            }
        }
        if self.device.get_keys().contains(&Keycode::Escape) {
            return Err(Interrupt::Pause(PauseReason::Escape));
        }
        let down = self.mouse_pressed();
        let clicked = down && !self.mouse_down;
        self.mouse_down = down;
        if clicked && self.opts.pause_on_click && plan.is_some() {
            return Err(Interrupt::Pause(PauseReason::Click));
        }
        Ok(())
    }

    /// Block until Resume (true) or Stop (false). Settings changes while paused are kept.
    fn wait_for_resume(&mut self) -> bool {
        loop {
            match self.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Control::Resume) => {
                    self.pending = None;
                    self.mouse_down = self.mouse_pressed();
                    return true;
                }
                Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn mouse_pressed(&self) -> bool {
        self.device.get_mouse().button_pressed.iter().any(|b| *b)
    }

    fn modifier_held(&self) -> bool {
        use Keycode::*;
        self.device
            .get_keys()
            .iter()
            .any(|k| matches!(k, LControl | RControl | LAlt | RAlt | LMeta | RMeta | Command | RCommand))
    }

    /// Plain letters, digits and space go out as real key presses (held for `hold`), with
    /// Shift for capitals. Anything else goes in as Unicode text, which works on any keyboard
    /// layout without knowing where the symbol lives.
    fn type_char(&mut self, ch: char, hold: f64) -> Result<(), Interrupt> {
        let result = if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == ' ' {
            self.hold(enigo::Key::Unicode(ch), hold)
        } else if ch.is_ascii_uppercase() {
            self.shifted(|t| t.hold(enigo::Key::Unicode(ch.to_ascii_lowercase()), hold))
        } else if ch == '\t' {
            self.hold(enigo::Key::Tab, hold)
        } else {
            self.enigo.text(&ch.to_string())
        };
        result.map_err(|_| Interrupt::Pause(PauseReason::Blocked))
    }

    fn press_key(&mut self, key: Key, shift: bool, hold: f64) -> Result<(), Interrupt> {
        let key = match key {
            Key::Enter => enigo::Key::Return,
            Key::Tab => enigo::Key::Tab,
            Key::Backspace => enigo::Key::Backspace,
            Key::Delete => enigo::Key::Delete,
            Key::Home => enigo::Key::Home,
            Key::End => enigo::Key::End,
        };
        let result = if shift { self.shifted(|t| t.hold(key, hold)) } else { self.hold(key, hold) };
        result.map_err(|_| Interrupt::Pause(PauseReason::Blocked))
    }

    fn tap(&mut self, key: enigo::Key, hold: f64) -> Result<(), Interrupt> {
        self.hold(key, hold).map_err(|_| Interrupt::Pause(PauseReason::Blocked))
    }

    fn hold(&mut self, key: enigo::Key, hold: f64) -> enigo::InputResult<()> {
        self.enigo.key(key, Direction::Press)?;
        thread::sleep(Duration::from_secs_f64(hold));
        // Always release, so a key is never left held down.
        self.enigo.key(key, Direction::Release)
    }

    fn shifted(
        &mut self,
        f: impl FnOnce(&mut Self) -> enigo::InputResult<()>,
    ) -> enigo::InputResult<()> {
        self.enigo.key(enigo::Key::Shift, Direction::Press)?;
        thread::sleep(Duration::from_millis(15));
        let result = f(self);
        let released = self.enigo.key(enigo::Key::Shift, Direction::Release);
        result.and(released)
    }

    fn report_progress(&mut self, force: bool) {
        if force || self.last_emit.elapsed() >= Duration::from_millis(50) {
            self.last_emit = Instant::now();
            let status =
                Status::Typing { position: self.position, total: self.total, wpm: self.wpm_clock.wpm() };
            self.status(status);
        }
    }

    fn status(&self, status: Status) {
        emit(&self.app, status);
    }
}

/// Measured speed over the last stretch of typing, not counting time paused.
#[derive(Default)]
struct WpmClock {
    running_since: Option<Instant>,
    banked: Duration,
    chars: usize,
}

impl WpmClock {
    fn resume(&mut self) {
        self.running_since.get_or_insert_with(Instant::now);
    }

    fn pause(&mut self) {
        if let Some(t) = self.running_since.take() {
            self.banked += t.elapsed();
        }
    }

    fn typed(&mut self, n: usize) {
        self.chars += n;
    }

    fn wpm(&self) -> f64 {
        let elapsed = self.banked + self.running_since.map_or(Duration::ZERO, |t| t.elapsed());
        let minutes = elapsed.as_secs_f64() / 60.0;
        if minutes < 1.0 / 60.0 { 0.0 } else { self.chars as f64 / 5.0 / minutes }
    }
}
