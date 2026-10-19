//! The typing plan: which keys to press and when, so text comes out at the chosen speed with a
//! human rhythm (uneven timing, pauses after punctuation, the odd typo that gets fixed).
//!
//! Pure logic with no OS calls. The app's typist walks the [`Plan`] iterator and turns each
//! [`Action`] into real key presses. Positions are indices into `text.chars()` after
//! [`normalize`], so the UI must index the same way (`Array.from(text)` in JS).

mod keyboard;
mod rng;

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

pub use rng::Rng;

/// Keys other than characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Enter,
    Tab,
    Backspace,
    Delete,
    Home,
    End,
}

/// One step for the typist. Times are in seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Do nothing for this long.
    Wait(f64),
    /// Type a character of the text, holding its key down this long.
    Char { ch: char, hold: f64 },
    /// Type a character that is wrong (or typed before a mistake was noticed). It will be
    /// erased by a later [`Action::Erase`]; the typist counts these so a pause mid-typo can
    /// clean up.
    Scratch { ch: char, hold: f64 },
    /// Backspace over one [`Action::Scratch`] character.
    Erase { hold: f64 },
    /// Press a key, with Shift held if `shift`.
    Key { key: Key, shift: bool, hold: f64 },
    /// Everything before this position is now typed correctly: a safe point to pause.
    Progress(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanConfig {
    /// Target speed in words per minute (a word is 5 characters).
    pub wpm: u32,
    /// Chance that a letter comes out wrong, 0.0..=0.2. Around 0.01–0.03 looks natural.
    pub typo_rate: f64,
    /// Shift+Enter for new lines (chat apps where Enter sends).
    pub shift_enter: bool,
    /// Code editor mode: keep the text's own indentation and drop brackets the editor
    /// closed by itself.
    pub code_mode: bool,
}

impl Default for PlanConfig {
    fn default() -> Self {
        PlanConfig { wpm: 60, typo_rate: 0.02, shift_enter: false, code_mode: false }
    }
}

/// Line endings become `\n`, so positions match between the UI and the plan.
pub fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Pauses and warm-up make real speed lower than raw key speed; this brings the measured
/// speed back to the WPM setting. Tuned by the `long_text_lands_near_target_wpm` test.
const SPEED_CALIBRATION: f64 = 0.80;
/// Fixing a typo costs time; keys go this much faster per unit of typo rate to compensate,
/// up to `MAX_TYPO_SPEEDUP` so it never looks inhuman.
const TYPO_COST: f64 = 8.0;
const MAX_TYPO_SPEEDUP: f64 = 1.5;
/// Keys typed after a (re)start before reaching full speed.
const WARM_UP_KEYS: f64 = 8.0;

pub struct Plan {
    text: Vec<char>,
    cfg: PlanConfig,
    rng: Rng,
    pos: usize,
    queue: VecDeque<Action>,
    rhythm: f64,
    prev: Option<char>,
    last_hold: f64,
    keys_since_start: u32,
}

impl Plan {
    pub fn new(text: &str, cfg: PlanConfig, seed: u64) -> Self {
        let mut plan = Plan {
            text: normalize(text).chars().collect(),
            cfg,
            rng: Rng::new(seed),
            pos: 0,
            queue: VecDeque::new(),
            rhythm: 1.0,
            prev: None,
            last_hold: 0.0,
            keys_since_start: 0,
        };
        plan.set_wpm(cfg.wpm);
        plan.set_typo_rate(cfg.typo_rate);
        plan
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn config(&self) -> PlanConfig {
        self.cfg
    }

    /// Takes effect from the next key.
    pub fn set_wpm(&mut self, wpm: u32) {
        self.cfg.wpm = wpm.clamp(5, 300);
    }

    /// Takes effect from the next key.
    pub fn set_typo_rate(&mut self, rate: f64) {
        self.cfg.typo_rate = if rate.is_finite() { rate.clamp(0.0, 0.2) } else { 0.0 };
    }

    /// Plan from `pos` onwards, easing in again like a person picking back up.
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos.min(self.text.len());
        self.queue.clear();
        self.prev = None;
        self.last_hold = 0.0;
        self.keys_since_start = 0;
        self.rhythm = 1.0;
        if self.cfg.code_mode && self.pos > 0 && self.text[self.pos - 1] == '\n' {
            // Resuming at the start of a line: the editor's auto-indent may still be there.
            self.replace_auto_indent();
        }
    }

    /// Seconds per character at the set speed, before rhythm and pauses.
    fn nominal(&self) -> f64 {
        let typo_speedup = (1.0 + TYPO_COST * self.cfg.typo_rate).min(MAX_TYPO_SPEEDUP);
        60.0 / (self.cfg.wpm as f64 * 5.0) / typo_speedup
    }

    fn base(&self) -> f64 {
        self.nominal() * SPEED_CALIBRATION
    }

    /// Pauses scale with speed: slow typists pause longer. 1.0 at 100 WPM.
    fn pace(&self) -> f64 {
        self.nominal() / 0.12
    }

    fn plan_next(&mut self) {
        let i = self.pos;
        let ch = self.text[i];
        if ch == '\n' {
            self.newline(i);
        } else {
            if self.typo_allowed(i) && self.rng.chance(self.cfg.typo_rate) {
                self.typo(i);
            }
            let (wait, hold) = self.timing(ch, i);
            self.queue.push_back(Action::Wait(wait));
            self.queue.push_back(Action::Char { ch, hold });
        }
        self.pos += 1;
        self.queue.push_back(Action::Progress(self.pos));
    }

    /// Wait before the next key press and how long to hold it.
    fn timing(&mut self, ch: char, i: usize) -> (f64, f64) {
        let hold = self.rng.uniform(0.03, 0.08).min(self.base() * 0.4);
        let wait = (self.interval(ch, i) - self.last_hold).max(0.005);
        self.prev = Some(ch);
        self.last_hold = hold;
        self.keys_since_start += 1;
        (wait, hold)
    }

    /// Time from the previous key press to this one.
    fn interval(&mut self, ch: char, i: usize) -> f64 {
        let base = self.base();

        // Slow drift around 1.0: focus comes and goes.
        self.rhythm += self.rng.gauss(0.03) + (1.0 - self.rhythm) * 0.05;
        self.rhythm = self.rhythm.clamp(0.75, 1.35);
        let warm_up = 1.0 + 0.6 * (-(self.keys_since_start as f64) / WARM_UP_KEYS).exp();
        let mut t = base * self.rhythm * warm_up * self.rng.gauss(0.28).exp();

        if let Some(prev) = self.prev {
            let (fa, fb) = (keyboard::finger(prev), keyboard::finger(ch));
            t *= if prev == ch {
                0.85
            } else if keyboard::is_fast_bigram(prev, ch) {
                0.75
            } else if fa.is_some() && fa == fb {
                1.25 // same finger has to travel
            } else if let (Some(a), Some(b)) = (fa, fb) {
                if (a < 4) != (b < 4) { 0.9 } else { 1.0 } // other hand is already in place
            } else {
                1.0
            };
        }
        if keyboard::needs_shift(ch) {
            t += self.rng.uniform(0.03, 0.09);
        } else if ch.is_ascii_digit() {
            t *= 1.25;
        } else if !ch.is_alphanumeric() && ch != ' ' {
            t *= 1.3;
        }
        let t = t.clamp(base * 0.5, base * 4.0);

        let mut pause = match self.prev {
            Some('.' | '!' | '?') => self.rng.uniform(0.25, 0.7),
            Some(',' | ';' | ':') => self.rng.uniform(0.08, 0.3),
            Some('\n') => self.rng.uniform(0.3, 0.9),
            Some(' ') => {
                let r = self.rng.f64();
                if r < 0.015 {
                    self.rng.uniform(0.7, 2.0) // thinking
                } else if r < 0.13 {
                    self.rng.uniform(0.05, 0.3) // hesitation
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        let word = self.word_starting_at(i);
        if word > 5 {
            pause += self.rng.uniform(0.0, 0.025) * (word - 5) as f64; // planning a long word
        }
        t + pause * self.pace()
    }

    /// Length of the word starting at `i`, or 0 if none starts there.
    fn word_starting_at(&self, i: usize) -> usize {
        if i > 0 && self.text[i - 1].is_alphanumeric() {
            return 0;
        }
        self.text[i..].iter().take_while(|c| c.is_alphanumeric()).count()
    }

    fn newline(&mut self, i: usize) {
        if !self.cfg.code_mode {
            let (wait, hold) = self.timing('\n', i);
            self.queue.push_back(Action::Wait(wait));
            self.queue.push_back(Action::Key { key: Key::Enter, shift: self.cfg.shift_enter, hold });
            return;
        }
        let prev = (i > 0).then(|| self.text[i - 1]);
        if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            // An autocomplete pop-up may be open and would swallow Enter; a space closes it.
            let (wait, hold) = self.timing(' ', i);
            self.queue.push_back(Action::Wait(wait));
            self.queue.push_back(Action::Char { ch: ' ', hold });
            self.queue.push_back(Action::Wait(self.rng.uniform(0.04, 0.09)));
            self.queue.push_back(Action::Key { key: Key::Backspace, shift: false, hold });
        }
        if prev.is_some_and(|c| "{[(".contains(c)) {
            // The editor may have closed the bracket after the cursor ("{|}"). Select it so
            // Enter replaces it; the text brings its own closing bracket later.
            self.queue.push_back(Action::Wait(self.rng.uniform(0.03, 0.06)));
            self.queue.push_back(Action::Key { key: Key::End, shift: true, hold: 0.03 });
        }
        let (wait, hold) = self.timing('\n', i);
        self.queue.push_back(Action::Wait(wait));
        self.queue.push_back(Action::Key { key: Key::Enter, shift: false, hold });
        self.replace_auto_indent_at(i + 1);
    }

    fn replace_auto_indent(&mut self) {
        self.replace_auto_indent_at(self.pos);
    }

    /// After Enter the editor may indent the new line by itself. Select that indent
    /// (Shift+Home) so the next key typed replaces it and the line gets exactly the text's
    /// indentation.
    fn replace_auto_indent_at(&mut self, next: usize) {
        self.queue.push_back(Action::Wait(self.rng.uniform(0.03, 0.06)));
        self.queue.push_back(Action::Key { key: Key::Home, shift: true, hold: 0.03 });
        if self.text.get(next) == Some(&'\t') {
            // Tab on a selection would indent it instead of replacing it.
            self.queue.push_back(Action::Key { key: Key::Delete, shift: false, hold: 0.03 });
        }
    }

    fn typo_allowed(&self, i: usize) -> bool {
        let ch = self.text[i];
        // Plain letters only: one Backspace may erase more than one char of an emoji or an
        // accented letter in some apps, and editors pair up brackets and quotes.
        ch.is_ascii_alphabetic()
    }

    /// Type something wrong at `i`, notice, and erase it. The correct char follows.
    fn typo(&mut self, i: usize) {
        let ch = self.text[i];
        let next = self.text.get(i + 1).copied().filter(|c| c.is_ascii_alphabetic());
        let mut wrong: Vec<char> = Vec::new();
        let r = self.rng.f64();
        match next {
            // Swapped letters: "hte" for "the".
            Some(n) if r < 0.2 && !n.eq_ignore_ascii_case(&ch) => {
                wrong.push(n);
                wrong.push(ch);
            }
            // Doubled letter.
            _ if r < 0.3 => {
                wrong.push(ch);
                wrong.push(ch);
            }
            // Neighbouring key.
            _ => {
                let near = keyboard::neighbors(ch.to_ascii_lowercase());
                let near: Vec<char> = near.chars().collect();
                if near.is_empty() {
                    return;
                }
                let mut w = self.rng.pick(&near);
                if ch.is_ascii_uppercase() {
                    w = w.to_ascii_uppercase();
                }
                wrong.push(w);
            }
        }
        if wrong.len() == 1 {
            // Often a letter or two more go in before the mistake is noticed.
            let extra = if self.rng.chance(0.4) { 1 + self.rng.chance(0.3) as usize } else { 0 };
            wrong.extend(self.text[i + 1..].iter().take(extra).take_while(|c| c.is_ascii_alphabetic()));
        }

        for &w in &wrong {
            let (wait, hold) = self.timing(w, i);
            self.queue.push_back(Action::Wait(wait));
            self.queue.push_back(Action::Scratch { ch: w, hold });
        }
        let notice = self.rng.uniform(0.2, 0.5) * self.pace();
        self.queue.push_back(Action::Wait(notice));
        for _ in &wrong {
            let hold = self.rng.uniform(0.03, 0.06);
            self.queue.push_back(Action::Erase { hold });
            self.queue.push_back(Action::Wait(self.rng.uniform(0.07, 0.14)));
        }
        self.prev = None;
    }
}

impl Iterator for Plan {
    type Item = Action;

    fn next(&mut self) -> Option<Action> {
        if self.queue.is_empty() && self.pos < self.text.len() {
            self.plan_next();
        }
        self.queue.pop_front()
    }
}

/// What a sequence of actions leaves in a plain text box. For tests and previews.
pub fn simulate(actions: impl IntoIterator<Item = Action>) -> String {
    let mut out: Vec<char> = Vec::new();
    for a in actions {
        match a {
            Action::Char { ch, .. } | Action::Scratch { ch, .. } => out.push(ch),
            Action::Erase { .. } | Action::Key { key: Key::Backspace, .. } => {
                out.pop();
            }
            Action::Key { key: Key::Enter, .. } => out.push('\n'),
            Action::Key { key: Key::Tab, .. } => out.push('\t'),
            _ => {}
        }
    }
    out.into_iter().collect()
}

/// Total time the actions take, in seconds.
pub fn duration(actions: &[Action]) -> f64 {
    actions
        .iter()
        .map(|a| match a {
            Action::Wait(s) => *s,
            Action::Char { hold, .. }
            | Action::Scratch { hold, .. }
            | Action::Erase { hold }
            | Action::Key { hold, .. } => *hold,
            Action::Progress(_) => 0.0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROSE: &str = "The quick brown fox jumps over the lazy dog. Then it naps, briefly, \
        in the warm afternoon sun! Typing practice works best with ordinary sentences like \
        these ones, which mix short and longer words.\nA new line starts here.";

    fn run(text: &str, cfg: PlanConfig, seed: u64) -> Vec<Action> {
        Plan::new(text, cfg, seed).collect()
    }

    #[test]
    fn types_the_exact_text_with_and_without_typos() {
        for seed in 0..50 {
            for typo_rate in [0.0, 0.05, 0.2] {
                let cfg = PlanConfig { typo_rate, ..Default::default() };
                assert_eq!(simulate(run(PROSE, cfg, seed)), PROSE, "seed {seed} typos {typo_rate}");
            }
        }
    }

    #[test]
    fn handles_unicode_and_line_endings() {
        let text = "Café ½ — 😀 naïve\r\nnext\rline";
        let cfg = PlanConfig { typo_rate: 0.2, ..Default::default() };
        assert_eq!(simulate(run(text, cfg, 7)), normalize(text));
    }

    #[test]
    fn no_typos_when_rate_is_zero() {
        let cfg = PlanConfig { typo_rate: 0.0, ..Default::default() };
        assert!(!run(PROSE, cfg, 1).iter().any(|a| matches!(a, Action::Scratch { .. })));
    }

    #[test]
    fn progress_reaches_the_end_in_order() {
        let actions = run(PROSE, PlanConfig::default(), 3);
        let marks: Vec<usize> = actions
            .iter()
            .filter_map(|a| if let Action::Progress(p) = a { Some(*p) } else { None })
            .collect();
        assert_eq!(marks, (1..=PROSE.chars().count()).collect::<Vec<_>>());
    }

    #[test]
    fn scratch_is_always_erased_before_next_progress() {
        let cfg = PlanConfig { typo_rate: 0.2, ..Default::default() };
        let mut dirty = 0i32;
        for a in run(PROSE, cfg, 11) {
            match a {
                Action::Scratch { .. } => dirty += 1,
                Action::Erase { .. } => dirty -= 1,
                Action::Progress(_) => assert_eq!(dirty, 0),
                _ => {}
            }
            assert!(dirty >= 0);
        }
    }

    #[test]
    fn long_text_lands_near_target_wpm() {
        let text = PROSE.repeat(8);
        for (wpm, typo_rate) in [(40, 0.0), (80, 0.0), (80, 0.03), (120, 0.02)] {
            let mut total = 0.0;
            for seed in 0..10 {
                total += duration(&run(&text, PlanConfig { wpm, typo_rate, ..Default::default() }, seed));
            }
            let measured = text.chars().count() as f64 / 5.0 / (total / 10.0 / 60.0);
            let err = (measured - wpm as f64).abs() / wpm as f64;
            assert!(err < 0.15, "target {wpm} wpm, typos {typo_rate}: measured {measured:.1}");
        }
    }

    #[test]
    fn seek_resumes_from_position() {
        let mut plan = Plan::new("hello world", PlanConfig { typo_rate: 0.0, ..Default::default() }, 0);
        plan.seek(6);
        assert_eq!(simulate(plan), "world");
    }

    fn keys(actions: &[Action]) -> Vec<(Key, bool)> {
        actions
            .iter()
            .filter_map(|a| if let Action::Key { key, shift, .. } = a { Some((*key, *shift)) } else { None })
            .collect()
    }

    #[test]
    fn code_mode_selects_auto_indent_and_closed_brackets() {
        let cfg = PlanConfig { code_mode: true, typo_rate: 0.0, ..Default::default() };
        let actions = run("fn main() {\n    run();\n}", cfg, 0);
        assert_eq!(
            keys(&actions),
            vec![
                (Key::End, true), // select the "}" the editor added after "{"
                (Key::Enter, false),
                (Key::Home, true), // select the editor's auto-indent
                (Key::Enter, false),
                (Key::Home, true),
            ]
        );
    }

    #[test]
    fn code_mode_closes_autocomplete_after_a_word() {
        let cfg = PlanConfig { code_mode: true, typo_rate: 0.0, ..Default::default() };
        let actions = run("let x = foo\n\tbar", cfg, 0);
        assert_eq!(
            keys(&actions),
            vec![(Key::Backspace, false), (Key::Enter, false), (Key::Home, true), (Key::Delete, false)]
        );
        assert_eq!(simulate(actions), "let x = foo\n\tbar");
    }

    #[test]
    fn shift_enter_option() {
        let cfg = PlanConfig { shift_enter: true, ..Default::default() };
        assert_eq!(keys(&run("a\nb", cfg, 0)), vec![(Key::Enter, true)]);
    }
}
