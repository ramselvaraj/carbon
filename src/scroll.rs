//! Touchpad scrolling that feels like other desktop apps: speed-dependent gain (fast swipes travel
//! further) and inertial "fling" after the fingers lift. egui passes touchpad deltas through 1:1 and
//! Wayland/libinput supply no momentum, so we shape the events before egui sees them.

use eframe::egui::{self, vec2, Event, Modifiers, MouseWheelSource, MouseWheelUnit, TouchPhase, Vec2};
use std::time::Instant;

pub struct ScrollFx {
    vel: Vec2, // points / second
    last_input: Option<Instant>,
    last_frame: Instant,
    momentum: bool,
}

impl Default for ScrollFx {
    fn default() -> Self {
        Self { vel: Vec2::ZERO, last_input: None, last_frame: Instant::now(), momentum: false }
    }
}

/// Base gain for slow movement, plus extra gain that grows with swipe speed.
const BASE_GAIN: f32 = 1.8;
const ACCEL: f32 = 0.35; // per (points per frame-ish ms): see `gain`
const MAX_GAIN: f32 = 6.0;
/// Velocity half-life style decay: higher = stops sooner.
const FRICTION: f32 = 2.6;
const MIN_FLING: f32 = 40.0;
const IDLE_BEFORE_FLING: f32 = 0.045;

fn gain(speed_pts_per_s: f32) -> f32 {
    (BASE_GAIN + ACCEL * (speed_pts_per_s / 400.0)).min(MAX_GAIN)
}

impl ScrollFx {
    pub fn process(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().clamp(1.0 / 240.0, 0.05);
        self.last_frame = now;

        let mut got = Vec2::ZERO;
        let mut ended = false;
        for ev in raw.events.iter_mut() {
            if let Event::MouseWheel { unit: MouseWheelUnit::Point, delta, phase, modifiers, source, .. } = ev {
                if modifiers.command || *source == MouseWheelSource::Momentum {
                    continue; // zoom gesture / OS-provided momentum: leave alone
                }
                if matches!(phase, TouchPhase::End | TouchPhase::Cancel) {
                    ended = true;
                }
                let speed = delta.length() / dt;
                *delta *= gain(speed);
                got += *delta;
            }
        }

        if got != Vec2::ZERO {
            // Smooth velocity estimate; a new gesture cancels any running fling.
            self.vel = self.vel * 0.35 + (got / dt) * 0.65;
            self.last_input = Some(now);
            self.momentum = false;
        }
        let idle = self.last_input.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(f32::MAX);
        if !self.momentum && self.last_input.is_some() && self.vel.length() > MIN_FLING && (ended || idle > IDLE_BEFORE_FLING) && got == Vec2::ZERO {
            self.momentum = true;
        }
        if self.momentum {
            if self.vel.length() < MIN_FLING {
                self.momentum = false;
                self.vel = Vec2::ZERO;
                self.last_input = None;
            } else {
                raw.events.push(Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: self.vel * dt,
                    phase: TouchPhase::Move,
                    source: MouseWheelSource::Trackpad,
                    modifiers: Modifiers::NONE,
                });
                self.vel *= (-FRICTION * dt).exp();
                ctx.request_repaint();
            }
        } else if self.last_input.is_some() {
            // Wake up shortly to notice that the fingers stopped.
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            if idle > 0.5 {
                self.last_input = None;
                self.vel = Vec2::ZERO;
            }
        }
        let _ = vec2;
    }
}
