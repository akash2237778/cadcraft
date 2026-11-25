//! Touch screens (tablets, phones, touch laptops) on the drawing area.
//!
//! A touch reaches the app twice: as raw [`egui::Event::Touch`] events for every finger, and as
//! the primary pointer for the first finger (eframe's web runner presses the primary button where
//! the finger lands, moves it with the finger and releases it, followed by `PointerGone`, when the
//! finger lifts). The drawing area already zooms with a pinch, because egui folds a pinch into
//! `zoom_delta`. This module adds what a mouse user gets from the other buttons:
//!
//! - Two or more fingers pan the view, as a middle-button drag does.
//! - Press and hold acts as a right-click: Enter while a command asks for input, the shortcut menu
//!   (or Repeat, per the right-click options) otherwise. The hold is timed here, just past egui's
//!   click time so its lift is never a click, not left to egui's own "long touch": in Chromium
//!   that missed about half the holds, and the lift then came as the end of a drag, which places a
//!   point. A browser draws frames only when asked and a
//!   resting finger sends nothing, so frames are asked for while one finger rests on the drawing.
//!   egui's long touch, when it comes, is the same hold: it never acts twice.
//! - The fingers of a pinch, a pan or a hold never pick: the first finger's press, click, drag and
//!   lift belong to the gesture, not to the drawing.
//! - A one-finger drag still opens the press-and-drag selection window, as with the mouse.
//! - With no keyboard, a small bar on the drawing area offers Enter, Esc, Undo, Redo and Zoom
//!   Extents. It appears after the first touch and stays for the session.

use egui::{Pos2, Rect, Vec2, vec2};

use crate::CadApp;

/// How often a frame is asked for while one finger rests on the drawing, so a hold is noticed.
const HOLD_POLL: std::time::Duration = std::time::Duration::from_millis(100);
/// A finger resting this much longer than egui's click time is a hold (seconds). Past the click
/// time, so the hold's lift is never a click (a click outside the shortcut menu would close it).
pub const HOLD_MARGIN: f64 = 0.02;
/// A finger that moves further than this (points) is a drag, not a hold.
const HOLD_SLOP: f32 = 8.0;

/// Touch input carried from frame to frame.
#[derive(Clone, Debug, Default)]
pub struct TouchState {
    /// Fingers on the screen, as counted from the touch events.
    down: usize,
    /// The fingers down since the first one landed became a pinch, a pan or a hold.
    gesture: bool,
    /// When and where the first finger of this touch sequence landed.
    started: Option<(f64, Pos2)>,
    /// That finger moved too far for a hold.
    moved: bool,
    /// This sequence's hold already acted.
    held: bool,
    /// A touch was seen in this session: the touch bar shows.
    pub seen: bool,
}

/// What this frame's touches mean for the drawing area.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TouchFrame {
    /// Screen offset to pan the view by (two or more fingers moving together).
    pub pan: Vec2,
    /// The primary pointer's press, click, drag and release this frame belong to a gesture: they
    /// must not pick a point, select or start a selection window.
    pub suppress: bool,
    /// A hold happened this frame: act on it as a right-click, once.
    pub hold: bool,
    /// egui's long touch this frame repeats a hold that already acted: ignore it.
    pub stale_long_touch: bool,
}

/// Read this frame's touch events. `long_touched` is the drawing area's long touch (egui's
/// press-and-hold), which ends the touch sequence's picking like a second finger does.
pub fn frame(state: &mut TouchState, ctx: &egui::Context, long_touched: bool) -> TouchFrame {
    let hold_secs = ctx.options(|o| o.input_options.max_click_duration) + HOLD_MARGIN;
    let (events, multi, any_touches, now) = ctx.input(|i| {
        let events: Vec<(egui::TouchPhase, Pos2)> = i
            .raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Touch { phase, pos, .. } => Some((*phase, *pos)),
                _ => None,
            })
            .collect();
        (events, i.multi_touch(), i.any_touches(), i.time)
    });
    let mut lifted = false;
    for (phase, pos) in &events {
        state.seen = true;
        match phase {
            egui::TouchPhase::Start => {
                if state.down == 0 {
                    state.gesture = false;
                    state.started = Some((now, *pos));
                    state.moved = false;
                    state.held = false;
                }
                state.down = state.down.saturating_add(1);
                if state.down >= 2 {
                    state.gesture = true;
                }
            }
            egui::TouchPhase::Move => {
                if state.started.is_some_and(|(_, at)| at.distance(*pos) > HOLD_SLOP) {
                    state.moved = true;
                }
            }
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                state.down = state.down.saturating_sub(1);
                lifted = true;
            }
        }
    }
    // A lost end event must not leave a finger counted forever.
    if events.is_empty() && !any_touches {
        state.down = 0;
    }
    // One finger resting long enough, still down this frame: a hold.
    let hold = state.down == 1 && !state.gesture && !state.moved && !state.held && state.started.is_some_and(|(t0, _)| now - t0 >= hold_secs);
    if hold {
        state.held = true;
        state.gesture = true;
    }
    let stale_long_touch = long_touched && state.held && !hold;
    if long_touched && !state.held {
        // egui noticed the hold first: that is the hold.
        state.held = true;
        state.gesture = true;
    }
    if state.down == 1 && !state.gesture {
        ctx.request_repaint_after(HOLD_POLL);
    }
    let pan = match multi {
        Some(m) if m.num_touches >= 2 && m.translation_delta.is_finite() => m.translation_delta,
        _ => Vec2::ZERO,
    };
    let hold = hold || (long_touched && !stale_long_touch);
    TouchFrame { pan, suppress: state.gesture && (state.down > 0 || lifted), hold, stale_long_touch }
}

/// The touch bar's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchButton {
    Enter,
    Escape,
    Undo,
    Redo,
    ZoomExtents,
}

impl TouchButton {
    pub const ALL: [TouchButton; 5] = [Self::Enter, Self::Escape, Self::Undo, Self::Redo, Self::ZoomExtents];

    fn label(self) -> &'static str {
        match self {
            Self::Enter => crate::tl!("Enter"),
            Self::Escape => crate::tl!("Esc"),
            Self::Undo => crate::tl!("Undo"),
            Self::Redo => crate::tl!("Redo"),
            Self::ZoomExtents => crate::tl!("Zoom Extents"),
        }
    }
}

/// Do what the button stands for, as the keyboard or the menu does it.
pub fn press(app: &mut CadApp, button: TouchButton) {
    match button {
        // Enter on the command line: submit what is typed, end or repeat the command.
        TouchButton::Enter => crate::cmdline::submit(app),
        // Esc on the command line: drop what is typed and cancel the command.
        TouchButton::Escape => {
            app.cmd.buffer.clear();
            app.cmd.preedit.clear();
            app.cmd.options = None;
            app.session.cancel();
        }
        TouchButton::Undo => {
            let _ = app.run("u", serde_json::json!({}));
        }
        TouchButton::Redo => {
            let _ = app.run("redo", serde_json::json!({}));
        }
        TouchButton::ZoomExtents => {
            let _ = app.session.zoom_extents();
        }
    }
}

/// Below this drawing-area width (points) the touch bar is a column, not a row.
const NARROW: f32 = 560.0;

/// The touch bar at the drawing area's top left, under the viewport label (the ViewCube has the
/// top right and the command line the bottom), once a touch was seen. Buttons are finger-sized.
pub fn bar(app: &mut CadApp, ctx: &egui::Context, rect: Rect) {
    if !app.canvas.touch.seen {
        return;
    }
    let mut pressed = None;
    egui::Area::new(egui::Id::unique("cc_touch_bar"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::LEFT_TOP)
        .fixed_pos(rect.left_top() + vec2(8.0, 28.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let mut buttons = |ui: &mut egui::Ui| {
                    for b in TouchButton::ALL {
                        let button = egui::Button::new(b.label()).min_size(vec2(44.0, 36.0));
                        if ui.add(button).clicked() {
                            pressed = Some(b);
                        }
                    }
                };
                // A narrow drawing area (a phone held upright) has no room for a row beside the
                // ViewCube: the buttons go down the left edge instead.
                if rect.width() < NARROW {
                    ui.vertical(&mut buttons);
                } else {
                    ui.horizontal(&mut buttons);
                }
            });
        });
    if let Some(b) = pressed {
        press(app, b);
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run one frame at `time` with `events`, then read the touches like the drawing area does.
    fn at(ctx: &egui::Context, state: &mut TouchState, time: f64, events: Vec<egui::Event>, long_touched: bool) -> TouchFrame {
        let input = egui::RawInput { time: Some(time), events, ..Default::default() };
        let mut out = TouchFrame::default();
        let mut full = ctx.run_ui(input, |ui| out = frame(state, ui.ctx(), long_touched));
        // No renderer here: drop the frame's texture updates (egui insists they are handled).
        full.textures_delta.clear();
        out
    }

    /// The hold time with egui's default options.
    const HOLD_SECS: f64 = 0.8 + HOLD_MARGIN;

    fn finger(id: u64, phase: egui::TouchPhase, x: f32) -> egui::Event {
        egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(id), phase, pos: egui::pos2(x, 100.0), force: None }
    }

    #[test]
    fn a_resting_finger_is_a_hold_once_after_hold_secs() {
        let ctx = egui::Context::default();
        let mut s = TouchState::default();
        assert!(!at(&ctx, &mut s, 1.0, vec![finger(1, egui::TouchPhase::Start, 100.0)], false).hold);
        assert!(!at(&ctx, &mut s, 1.0 + HOLD_SECS - 0.05, vec![], false).hold, "not yet");
        let f = at(&ctx, &mut s, 1.0 + HOLD_SECS + 0.01, vec![], false);
        assert!(f.hold && f.suppress, "a hold, and the finger is no longer a pick");
        assert!(!at(&ctx, &mut s, 1.0 + HOLD_SECS + 0.2, vec![], false).hold, "only once");
        let lift = at(&ctx, &mut s, 2.0, vec![finger(1, egui::TouchPhase::End, 100.0)], false);
        assert!(lift.suppress, "its lift places nothing");
        assert!(!at(&ctx, &mut s, 2.1, vec![], false).suppress, "and the next input is free again");
    }

    #[test]
    fn a_moving_finger_or_a_second_finger_is_not_a_hold() {
        let ctx = egui::Context::default();
        let mut s = TouchState::default();
        at(&ctx, &mut s, 1.0, vec![finger(1, egui::TouchPhase::Start, 100.0)], false);
        at(&ctx, &mut s, 1.1, vec![finger(1, egui::TouchPhase::Move, 100.0 + HOLD_SLOP + 1.0)], false);
        assert!(!at(&ctx, &mut s, 2.0, vec![], false).hold, "a drag");
        at(&ctx, &mut s, 2.1, vec![finger(1, egui::TouchPhase::End, 120.0)], false);
        at(&ctx, &mut s, 3.0, vec![finger(1, egui::TouchPhase::Start, 100.0)], false);
        let two = at(&ctx, &mut s, 3.1, vec![finger(2, egui::TouchPhase::Start, 200.0)], false);
        assert!(two.suppress, "two fingers are a gesture");
        assert!(!at(&ctx, &mut s, 4.0, vec![], false).hold, "a pinch or pan, not a hold");
    }

    #[test]
    fn egui_long_touch_after_the_hold_acted_is_ignored_and_before_it_is_the_hold() {
        let ctx = egui::Context::default();
        let mut s = TouchState::default();
        at(&ctx, &mut s, 1.0, vec![finger(1, egui::TouchPhase::Start, 100.0)], false);
        assert!(at(&ctx, &mut s, 1.0 + HOLD_SECS + 0.01, vec![], false).hold);
        let late = at(&ctx, &mut s, 1.85, vec![], true);
        assert!(!late.hold && late.stale_long_touch, "egui's long touch repeats the hold");
        at(&ctx, &mut s, 2.0, vec![finger(1, egui::TouchPhase::End, 100.0)], false);
        at(&ctx, &mut s, 3.0, vec![finger(1, egui::TouchPhase::Start, 100.0)], false);
        let first = at(&ctx, &mut s, 3.3, vec![], true);
        assert!(first.hold && !first.stale_long_touch, "egui noticing it first is the hold");
        assert!(!at(&ctx, &mut s, 3.0 + HOLD_SECS + 0.1, vec![], false).hold, "which then does not act again");
    }
}
