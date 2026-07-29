//! Mouse-wheel and trackpad normalization for the fullscreen transcript.
//!
//! Derived from Grok Build's `xai-grok-pager/src/input/mouse.rs` at commit
//! `47348d13ec4508dcfe440e34c6d511bb02998fb2` (Apache-2.0). Astral keeps the
//! same stream, terminal-density, cadence, acceleration, and coast invariants,
//! while leaving Grok-specific settings and diagnostics out of this TUI layer.

use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use codex_terminal_detection::TerminalName;
use codex_terminal_detection::terminal_info;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

const STREAM_GAP: Duration = Duration::from_millis(80);
const REDRAW_CADENCE: Duration = Duration::from_millis(16);
const DEFAULT_EVENTS_PER_TICK: u16 = 3;
const DEFAULT_LINES_PER_TICK: u16 = 3;
const MIN_DELTA_PER_FLUSH: i32 = 6;
const WHEEL_TICK_DETECT_MAX: Duration = Duration::from_millis(12);
const WHEEL_LIKE_MAX_DURATION: Duration = Duration::from_millis(200);
const ACCEL_INTERVAL_FAST_MS: f32 = 8.0;
const ACCEL_INTERVAL_MEDIUM_MS: f32 = 20.0;
const ACCEL_MIN_INTERVAL_MS: f32 = 6.0;
const ACCEL_MULTIPLIER_BASE: f32 = 1.0;
const ACCEL_MULTIPLIER_MEDIUM: f32 = 1.6;
const ACCEL_MULTIPLIER_FAST: f32 = 2.5;
const ACCEL_HISTORY_SIZE: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollDirection {
    Up,
    Down,
}

impl ScrollDirection {
    pub(crate) fn from_mouse_event(event: MouseEvent) -> Option<Self> {
        match event.kind {
            MouseEventKind::ScrollUp => Some(Self::Up),
            MouseEventKind::ScrollDown => Some(Self::Down),
            _ => None,
        }
    }

    fn sign(self) -> i32 {
        match self {
            Self::Up => -1,
            Self::Down => 1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollConfig {
    events_per_tick: u16,
    wheel_lines_per_tick: u16,
    trackpad_lines_per_tick: u16,
    accel_interval_fast_ms: f32,
    accel_interval_medium_ms: f32,
    trackpad_detect_max_interval_ms: f32,
    viewport_height: u16,
}

impl ScrollConfig {
    pub(crate) fn detected(viewport_height: u16) -> Self {
        let terminal = terminal_info();
        Self::for_terminal(
            terminal.name,
            terminal.multiplexer.is_some(),
            viewport_height,
        )
    }

    fn for_terminal(terminal: TerminalName, remultiplexed: bool, viewport_height: u16) -> Self {
        let events_per_tick = if remultiplexed {
            1
        } else {
            match terminal {
                TerminalName::Iterm2 | TerminalName::WezTerm | TerminalName::VsCode => 1,
                TerminalName::AppleTerminal
                | TerminalName::Ghostty
                | TerminalName::WarpTerminal
                | TerminalName::Kitty
                | TerminalName::Alacritty
                | TerminalName::Konsole
                | TerminalName::GnomeTerminal
                | TerminalName::Vte
                | TerminalName::WindowsTerminal
                | TerminalName::Dumb
                | TerminalName::Unknown => DEFAULT_EVENTS_PER_TICK,
            }
        };
        let wheel_lines_per_tick = if remultiplexed {
            1
        } else if matches!(terminal, TerminalName::Iterm2 | TerminalName::WezTerm) {
            1
        } else {
            DEFAULT_LINES_PER_TICK
        };
        let vscode = terminal == TerminalName::VsCode && !remultiplexed;
        Self {
            events_per_tick,
            wheel_lines_per_tick,
            trackpad_lines_per_tick: if vscode { 15 } else { DEFAULT_LINES_PER_TICK },
            accel_interval_fast_ms: if vscode { 25.0 } else { ACCEL_INTERVAL_FAST_MS },
            accel_interval_medium_ms: if vscode {
                50.0
            } else {
                ACCEL_INTERVAL_MEDIUM_MS
            },
            trackpad_detect_max_interval_ms: if vscode { 60.0 } else { 30.0 },
            viewport_height,
        }
    }

    fn flush_cap(self) -> i32 {
        (i32::from(self.viewport_height) / 2).max(MIN_DELTA_PER_FLUSH)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamKind {
    Unknown,
    Wheel,
    Trackpad,
}

#[derive(Debug)]
pub(crate) struct MouseScrollState {
    stream: Option<ScrollStream>,
    last_flush_at: Instant,
    carry_lines: f32,
    carry_direction: Option<ScrollDirection>,
}

impl Default for MouseScrollState {
    fn default() -> Self {
        Self::new_at(Instant::now())
    }
}

impl MouseScrollState {
    fn new_at(now: Instant) -> Self {
        Self {
            stream: None,
            last_flush_at: now,
            carry_lines: 0.0,
            carry_direction: None,
        }
    }

    pub(crate) fn on_scroll_event(
        &mut self,
        direction: ScrollDirection,
        config: ScrollConfig,
    ) -> i32 {
        self.on_scroll_event_at(Instant::now(), direction, config)
    }

    fn on_scroll_event_at(
        &mut self,
        now: Instant,
        direction: ScrollDirection,
        config: ScrollConfig,
    ) -> i32 {
        let mut lines = 0;
        if let Some(mut stream) = self.stream.take() {
            let direction_flipped = stream.direction != direction;
            if now.duration_since(stream.last) > STREAM_GAP || direction_flipped {
                lines += self.finalize_stream(now, &mut stream, direction_flipped);
            } else {
                self.stream = Some(stream);
            }
        }

        if self.stream.is_none() {
            if self.carry_direction != Some(direction) {
                self.carry_lines = 0.0;
                self.carry_direction = Some(direction);
            }
            self.stream = Some(ScrollStream::new(now, direction, config));
        }
        let carry_lines = self.carry_lines;
        let stream = self.stream.as_mut().expect("scroll stream inserted above");
        stream.push_event(now);
        stream.maybe_promote_kind(now);
        if now.duration_since(self.last_flush_at) >= REDRAW_CADENCE || stream.just_promoted {
            lines += Self::flush_lines(&mut self.last_flush_at, carry_lines, now, stream);
            stream.just_promoted = false;
        }
        lines
    }

    pub(crate) fn on_tick(&mut self) -> i32 {
        self.on_tick_at(Instant::now())
    }

    fn on_tick_at(&mut self, now: Instant) -> i32 {
        let Some(mut stream) = self.stream.take() else {
            return 0;
        };
        let gap_expired = now.duration_since(stream.last) > STREAM_GAP;
        if gap_expired && stream.flushable_now(self.carry_lines) == 0 {
            return self.finalize_stream(now, &mut stream, false);
        }
        let lines = if now.duration_since(self.last_flush_at) >= REDRAW_CADENCE {
            Self::flush_lines(&mut self.last_flush_at, self.carry_lines, now, &mut stream)
        } else {
            0
        };
        self.stream = Some(stream);
        lines
    }

    pub(crate) fn clock_deadline(&self, now: Instant) -> Option<Duration> {
        self.stream.as_ref()?;
        Some(self.next_tick_in(now).unwrap_or(Duration::ZERO))
    }

    pub(crate) fn cancel(&mut self) {
        self.stream = None;
        self.carry_lines = 0.0;
        self.carry_direction = None;
    }

    fn finalize_stream(
        &mut self,
        now: Instant,
        stream: &mut ScrollStream,
        cancel_backlog: bool,
    ) -> i32 {
        let desired_before = stream.desired_lines(self.carry_lines);
        stream.finalize_kind();
        stream.limit_finalize_reprice(desired_before, self.carry_lines);
        let lines = if cancel_backlog {
            0
        } else {
            Self::flush_lines(&mut self.last_flush_at, self.carry_lines, now, stream)
        };
        if stream.kind == StreamKind::Trackpad {
            let remainder = stream.desired_lines(self.carry_lines) - stream.applied_lines as f32;
            self.carry_lines = remainder.fract();
        } else {
            self.carry_lines = 0.0;
        }
        lines
    }

    fn flush_lines(
        last_flush_at: &mut Instant,
        carry_lines: f32,
        now: Instant,
        stream: &mut ScrollStream,
    ) -> i32 {
        let delta = stream.flushable_now(carry_lines);
        if delta == 0 {
            return 0;
        }
        if stream.coasting() {
            stream.coast_spent += delta.abs();
        }
        stream.applied_lines = stream.applied_lines.saturating_add(delta);
        stream.events_at_flush = stream.event_count;
        *last_flush_at = now;
        delta
    }

    fn next_tick_in(&self, now: Instant) -> Option<Duration> {
        let stream = self.stream.as_ref()?;
        let gap = now.duration_since(stream.last);
        let flushable = stream.flushable_now(self.carry_lines) != 0;
        let until_flush = REDRAW_CADENCE.saturating_sub(now.duration_since(self.last_flush_at));
        if gap > STREAM_GAP {
            return flushable.then_some(until_flush);
        }
        let mut next = STREAM_GAP.saturating_sub(gap);
        if flushable {
            next = next.min(until_flush);
        }
        Some(next)
    }
}

#[derive(Clone, Debug)]
struct ScrollStream {
    start: Instant,
    last: Instant,
    direction: ScrollDirection,
    event_count: usize,
    accumulated_events: i32,
    applied_lines: i32,
    config: ScrollConfig,
    kind: StreamKind,
    just_promoted: bool,
    interval_history: VecDeque<f32>,
    interval_sum: f32,
    accel_weighted_events: f32,
    events_at_flush: usize,
    coast_spent: i32,
}

impl ScrollStream {
    fn new(now: Instant, direction: ScrollDirection, config: ScrollConfig) -> Self {
        Self {
            start: now,
            last: now,
            direction,
            event_count: 0,
            accumulated_events: 0,
            applied_lines: 0,
            config,
            kind: StreamKind::Unknown,
            just_promoted: false,
            interval_history: VecDeque::with_capacity(ACCEL_HISTORY_SIZE),
            interval_sum: 0.0,
            accel_weighted_events: 0.0,
            events_at_flush: 0,
            coast_spent: 0,
        }
    }

    fn push_event(&mut self, now: Instant) {
        let interval_ms = now.duration_since(self.last).as_secs_f32() * 1000.0;
        if self.event_count > 0 && interval_ms >= ACCEL_MIN_INTERVAL_MS {
            self.interval_history.push_back(interval_ms);
            self.interval_sum += interval_ms;
            if self.interval_history.len() > ACCEL_HISTORY_SIZE
                && let Some(old) = self.interval_history.pop_front()
            {
                self.interval_sum -= old;
            }
        }
        self.last = now;
        self.event_count = self.event_count.saturating_add(1);
        self.accumulated_events = self
            .accumulated_events
            .saturating_add(self.direction.sign());
        self.accel_weighted_events += self.direction.sign() as f32 * self.interval_accel();
        if self.kind == StreamKind::Trackpad {
            self.clamp_trackpad_demand();
        }
    }

    fn maybe_promote_kind(&mut self, now: Instant) {
        if self.kind != StreamKind::Unknown {
            return;
        }
        let events_per_tick = usize::from(self.config.events_per_tick.max(1));
        if events_per_tick <= 1
            && self.event_count > 2
            && self
                .avg_interval_ms()
                .is_some_and(|avg| avg < self.config.trackpad_detect_max_interval_ms)
        {
            self.kind = StreamKind::Trackpad;
            self.clamp_trackpad_demand();
        } else if events_per_tick >= 2
            && self.event_count >= events_per_tick
            && now.duration_since(self.start) <= WHEEL_TICK_DETECT_MAX
        {
            self.kind = StreamKind::Wheel;
            self.just_promoted = true;
        }
    }

    fn finalize_kind(&mut self) {
        if self.kind != StreamKind::Unknown {
            return;
        }
        let duration = self.last.duration_since(self.start);
        self.kind = if self.config.events_per_tick <= 1
            && self.event_count <= 2
            && duration <= WHEEL_LIKE_MAX_DURATION
        {
            StreamKind::Wheel
        } else {
            StreamKind::Trackpad
        };
    }

    fn avg_interval_ms(&self) -> Option<f32> {
        (!self.interval_history.is_empty())
            .then(|| self.interval_sum / self.interval_history.len() as f32)
    }

    fn interval_accel(&self) -> f32 {
        let Some(avg) = self.avg_interval_ms() else {
            return ACCEL_MULTIPLIER_BASE;
        };
        let fast = self.config.accel_interval_fast_ms;
        let medium = self.config.accel_interval_medium_ms;
        let raw = if avg <= fast {
            ACCEL_MULTIPLIER_FAST
        } else if avg <= medium {
            let t = (avg - fast) / (medium - fast);
            ACCEL_MULTIPLIER_FAST + t * (ACCEL_MULTIPLIER_MEDIUM - ACCEL_MULTIPLIER_FAST)
        } else {
            ACCEL_MULTIPLIER_BASE
        };
        raw.clamp(ACCEL_MULTIPLIER_BASE, DEFAULT_LINES_PER_TICK as f32)
    }

    fn trackpad_line_rate(&self) -> f32 {
        f32::from(self.config.trackpad_lines_per_tick) / f32::from(DEFAULT_EVENTS_PER_TICK)
    }

    fn clamp_trackpad_demand(&mut self) {
        let rate = self.trackpad_line_rate();
        let raw_lines = self.accumulated_events.abs() as f32 * rate;
        let honorable = (self.applied_lines.abs() + self.config.flush_cap()) as f32;
        let ceiling = raw_lines.max(honorable);
        if self.accel_weighted_events.abs() * rate > ceiling {
            self.accel_weighted_events = self.accel_weighted_events.signum() * ceiling / rate;
        }
    }

    fn limit_finalize_reprice(&mut self, desired_before: f32, carry_lines: f32) {
        if self.kind != StreamKind::Trackpad {
            return;
        }
        let rate = self.trackpad_line_rate();
        if self.desired_lines(carry_lines).abs() > desired_before.abs() {
            self.accel_weighted_events = (desired_before - carry_lines) / rate;
        }
    }

    fn desired_lines(&self, carry_lines: f32) -> f32 {
        if self.kind == StreamKind::Trackpad {
            self.accel_weighted_events * self.trackpad_line_rate() + carry_lines
        } else {
            self.accumulated_events as f32 * f32::from(self.config.wheel_lines_per_tick)
                / f32::from(self.config.events_per_tick.max(1))
        }
    }

    fn effective_pending(&self, carry_lines: f32) -> i32 {
        let mut desired = self.desired_lines(carry_lines).trunc() as i32;
        let wheel_like = self.kind == StreamKind::Wheel
            || (self.kind == StreamKind::Unknown && self.config.events_per_tick <= 1);
        if wheel_like && desired == 0 && self.accumulated_events != 0 {
            desired = self.accumulated_events.signum();
        }
        let delta = desired - self.applied_lines;
        if self.accumulated_events > 0 {
            delta.max(0)
        } else {
            delta.min(0)
        }
    }

    fn coasting(&self) -> bool {
        self.event_count == self.events_at_flush
    }

    fn flushable_now(&self, carry_lines: f32) -> i32 {
        let pending = self.effective_pending(carry_lines);
        let cap = self.config.flush_cap();
        let magnitude = if self.coasting() {
            let taper = (pending.abs() / 2).max(i32::from(
                self.config
                    .wheel_lines_per_tick
                    .max(self.config.trackpad_lines_per_tick),
            ));
            pending
                .abs()
                .min(taper)
                .min((cap - self.coast_spent).max(0))
        } else {
            pending.abs().min(cap)
        };
        pending.signum() * magnitude
    }
}

#[cfg(test)]
#[path = "mouse_scroll_tests.rs"]
mod tests;
