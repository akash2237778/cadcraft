//! Touch screens on the drawing area: two fingers pan (a pinch already zooms), press and hold in a
//! command is Enter, the fingers of a gesture never pick, and the touch bar (Enter, Esc, Undo,
//! Redo, Zoom Extents) appears after the first touch. A tap placing a point, a one-finger drag
//! selecting with a window and a hold opening the shortcut menu when idle worked before; their
//! tests keep them working.
//!
//! The events are the ones eframe's web runner sends for a finger: the primary pointer follows
//! the first finger (pressed where it lands, released with `PointerGone` when it lifts), every
//! finger also arrives as a raw `Touch` event, and a second finger landing or lifting presses or
//! releases the primary pointer again at the first finger's position.

use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Pos2, TouchDeviceId, TouchId, TouchPhase, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("line", json!({ "points": [[2.0, 2.0], [8.0, 6.0]] })).expect("draw a line");
    let mut h = Harness::builder().with_size(vec2(1280.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

/// The drawing area's middle, in screen points.
fn middle(h: &Harness<'_, CadApp>) -> Pos2 {
    h.state().canvas.rect.expect("the canvas is laid out").center()
}

/// The screen point of a drawing point.
fn screen(h: &Harness<'_, CadApp>, x: f64, y: f64) -> Pos2 {
    h.state().canvas.xf.expect("the canvas is laid out").to_screen(Vec2::new(x, y))
}

fn touch(id: u64, phase: TouchPhase, pos: Pos2) -> Event {
    Event::Touch { device_id: TouchDeviceId(1), id: TouchId(id), phase, pos, force: None }
}

fn primary(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

/// Fingers on the screen, sending what eframe's web runner sends.
struct Fingers {
    /// (id, position) of the fingers down; the first is the primary pointer.
    down: Vec<(u64, Pos2)>,
}

impl Fingers {
    fn new() -> Self {
        Self { down: Vec::new() }
    }

    fn primary_pos(&self) -> Option<Pos2> {
        self.down.first().map(|f| f.1)
    }

    fn land(&mut self, h: &mut Harness<'_, CadApp>, id: u64, pos: Pos2) {
        self.down.push((id, pos));
        if let Some(p) = self.primary_pos() {
            h.event(primary(p, true));
        }
        h.event(touch(id, TouchPhase::Start, pos));
    }

    fn slide(&mut self, h: &mut Harness<'_, CadApp>, id: u64, pos: Pos2) {
        let first = self.down.first().is_some_and(|f| f.0 == id);
        if let Some(f) = self.down.iter_mut().find(|f| f.0 == id) {
            f.1 = pos;
        }
        if first {
            h.event(Event::PointerMoved(pos));
        }
        h.event(touch(id, TouchPhase::Move, pos));
    }

    fn lift(&mut self, h: &mut Harness<'_, CadApp>, id: u64) {
        let Some(i) = self.down.iter().position(|f| f.0 == id) else { return };
        let (_, pos) = self.down[i];
        // The primary pointer is released at the first finger, whichever finger lifts.
        if let Some(p) = self.primary_pos() {
            h.event(primary(p, false));
            h.event(Event::PointerGone);
        }
        self.down.remove(i);
        h.event(touch(id, TouchPhase::End, pos));
    }
}

/// A quick tap: one finger lands and lifts.
fn tap(h: &mut Harness<'_, CadApp>, at: Pos2) {
    let mut f = Fingers::new();
    f.land(h, 1, at);
    h.step();
    f.lift(h, 1);
    h.run_steps(2);
}

/// Two fingers moving together by `by` in `steps` frames.
fn two_finger_drag(h: &mut Harness<'_, CadApp>, at: Pos2, by: egui::Vec2, steps: u32) {
    let mut f = Fingers::new();
    let (a, b) = (at - vec2(40.0, 0.0), at + vec2(40.0, 0.0));
    f.land(h, 1, a);
    h.step();
    f.land(h, 2, b);
    h.step();
    for s in 1..=steps {
        let d = by * (s as f32 / steps as f32);
        f.slide(h, 1, a + d);
        f.slide(h, 2, b + d);
        h.step();
    }
    f.lift(h, 2);
    h.step();
    f.lift(h, 1);
    h.run_steps(2);
}

fn prompt(h: &Harness<'_, CadApp>) -> Option<String> {
    h.state().session.current_prompt().map(|p| p.message)
}

fn entity_count(h: &Harness<'_, CadApp>) -> usize {
    h.state().session.doc().map(|d| d.entity_count()).unwrap_or(0)
}

#[test]
fn a_tap_places_a_point() {
    let mut h = harness();
    h.state_mut().start("line");
    h.run_steps(2);
    let first = prompt(&h);
    let at = middle(&h);
    tap(&mut h, at);
    assert_ne!(prompt(&h), first, "the tap answered the first-point prompt");
}

#[test]
fn two_fingers_pan_the_view() {
    let mut h = harness();
    let before = h.state().session.state().expect("drawing").view();
    let at = middle(&h);
    two_finger_drag(&mut h, at, vec2(120.0, 0.0), 6);
    let after = h.state().session.state().expect("drawing").view();
    assert!(after.center.x < before.center.x - 1e-6, "moving the fingers right shows what is left of the view: {before:?} -> {after:?}");
    // egui's pinch factor is an f32: a parallel move leaves only rounding noise in the zoom.
    assert!((after.height / before.height - 1.0).abs() < 1e-5, "a parallel two-finger move pans without zooming: {before:?} -> {after:?}");
    assert!(h.state().session.selection().is_empty(), "panning selects nothing");
    assert!(h.state().session.pending_window.is_none(), "and leaves no selection window open");
}

#[test]
fn the_fingers_of_a_pan_place_no_point() {
    let mut h = harness();
    h.state_mut().start("line");
    h.run_steps(2);
    let first = prompt(&h);
    let at = middle(&h);
    two_finger_drag(&mut h, at, vec2(0.0, 80.0), 5);
    assert_eq!(prompt(&h), first, "still asking for the first point: no finger of the gesture was a pick");
    assert_eq!(entity_count(&h), 1, "nothing was drawn");
}

#[test]
fn press_and_hold_in_a_command_is_enter() {
    let mut h = harness();
    h.state_mut().start("line");
    h.run_steps(2);
    let a = middle(&h);
    tap(&mut h, a);
    tap(&mut h, a + vec2(100.0, 0.0));
    assert_eq!(entity_count(&h), 2, "one segment drawn");
    assert!(h.state().session.running.is_some(), "LINE asks for the next point");
    // Hold a finger still for over a second (four 0.25 s frames).
    let mut f = Fingers::new();
    f.land(&mut h, 1, a + vec2(0.0, 80.0));
    h.run_steps(5);
    f.lift(&mut h, 1);
    h.run_steps(2);
    assert!(h.state().session.running.is_none(), "the hold ended LINE like Enter");
    assert_eq!(entity_count(&h), 2, "and the lift placed no point");
}

/// Like a browser: a frame is drawn only when the app asked for one, after the delay it asked for
/// (or at the next input event). The harness clock is set by hand so a resting finger really
/// produces no frames unless the app asks. A browser's timer can fire the frame a little early:
/// egui asks for one frame at its hold threshold (0.8 s), and one that lands just before it saw
/// no hold, drew nothing more, and the lift then placed a point (seen in Chromium).
struct Browser {
    now: f64,
}

/// How early a browser's timer may fire a requested frame (seconds).
const EARLY: f64 = 0.002;

impl Browser {
    /// Deliver `events` at the current time, one frame each.
    fn input(&mut self, h: &mut Harness<'_, CadApp>, send: impl FnOnce(&mut Harness<'_, CadApp>)) {
        self.now += 1.0 / 60.0;
        let now = self.now;
        h.input_mut().time = Some(now);
        send(h);
    }

    /// Let `secs` pass, drawing only the frames the app asks for.
    fn wait(&mut self, h: &mut Harness<'_, CadApp>, secs: f64) {
        let end = self.now + secs;
        loop {
            let asked = h.output().viewport_output.get(&egui::ViewportId::ROOT).map_or(f64::INFINITY, |v| v.repaint_delay.as_secs_f64());
            let next = self.now + (asked - EARLY).max(1.0 / 60.0);
            if next > end {
                break;
            }
            self.now = next;
            h.input_mut().time = Some(next);
            h.step();
        }
        self.now = end;
    }
}

#[test]
fn a_hold_in_a_browser_is_enter_even_though_a_resting_finger_sends_nothing() {
    let mut h = Harness::builder().with_size(vec2(1280.0, 760.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        {
            let mut app = CadApp::new(Session::new(), Services::default());
            app.run("line", json!({ "points": [[2.0, 2.0], [8.0, 6.0]] })).expect("draw a line");
            app
        },
    );
    let mut web = Browser { now: 0.0 };
    for _ in 0..5 {
        web.input(&mut h, |h| h.step());
    }
    web.input(&mut h, |h| h.state_mut().start("line"));
    web.input(&mut h, |h| h.step());
    let a = middle(&h);
    // The segment is drawn with ordinary frames; only the hold runs on the browser's terms.
    tap(&mut h, a);
    tap(&mut h, a + vec2(100.0, 0.0));
    web.now += 1.0;
    let mut f = Fingers::new();
    assert_eq!(entity_count(&h), 2, "two taps drew one segment");
    web.input(&mut h, |h| f.land(h, 1, a + vec2(0.0, 80.0)));
    web.wait(&mut h, 1.2);
    web.input(&mut h, |h| f.lift(h, 1));
    web.wait(&mut h, 0.3);
    assert!(h.state().session.running.is_none(), "the hold ended LINE like Enter");
    assert_eq!(entity_count(&h), 2, "and its lift placed no point");
}

/// The drawing area times the hold itself (just past egui's 0.8 s click time): the menu opens while
/// the finger still rests, and the lift (no click, being past the click time) leaves it open.
#[test]
fn a_hold_opens_the_shortcut_menu_and_the_lift_leaves_it_open() {
    let mut h = Harness::builder().with_size(vec2(1280.0, 760.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        CadApp::new(Session::new(), Services::default()),
    );
    let mut web = Browser { now: 0.0 };
    for _ in 0..5 {
        web.input(&mut h, |h| h.step());
    }
    let at = screen(&h, 1.0, 7.0);
    let mut f = Fingers::new();
    web.input(&mut h, |h| f.land(h, 1, at));
    web.wait(&mut h, 0.9);
    assert!(egui::Popup::is_any_open(&h.ctx), "the menu is open while the finger still rests");
    web.input(&mut h, |h| f.lift(h, 1));
    web.wait(&mut h, 0.3);
    assert!(egui::Popup::is_any_open(&h.ctx), "and stays open when it lifts");
}

#[test]
fn press_and_hold_when_idle_opens_the_shortcut_menu() {
    let mut h = harness();
    let at = screen(&h, 1.0, 7.0);
    let mut f = Fingers::new();
    f.land(&mut h, 1, at);
    h.run_steps(5);
    assert!(egui::Popup::is_any_open(&h.ctx), "the hold opened the shortcut menu");
    f.lift(&mut h, 1);
    h.run_steps(2);
    assert!(h.state().session.selection().is_empty(), "and selected nothing");
    assert!(h.state().session.pending_window.is_none(), "nor left a selection window open");
}

#[test]
fn a_one_finger_drag_selects_with_a_window() {
    let mut h = harness();
    let (from, to) = (screen(&h, 1.0, 7.0), screen(&h, 9.0, 1.0));
    let mut f = Fingers::new();
    f.land(&mut h, 1, from);
    h.step();
    for s in 1..=6 {
        f.slide(&mut h, 1, from + (to - from) * (s as f32 / 6.0));
        h.step();
    }
    f.lift(&mut h, 1);
    h.run_steps(3);
    assert_eq!(h.state().session.selection().len(), 1, "the window closed where the finger lifted and caught the line");
}

#[test]
fn the_touch_bar_appears_after_a_touch_and_esc_cancels() {
    let mut h = harness();
    assert!(!h.state().canvas.touch.seen && h.query_by_label("Esc").is_none(), "no touch bar for mouse users");
    h.state_mut().start("circle");
    h.run_steps(2);
    let at = screen(&h, 1.0, 7.0);
    tap(&mut h, at);
    assert!(h.state().session.running.is_some(), "CIRCLE asks for the radius");
    h.get_by_label("Esc").click();
    h.run_steps(2);
    assert!(h.state().session.running.is_none(), "Esc on the touch bar cancelled the command");
    assert!(h.query_by_label("Esc").is_some(), "the bar stays");
}
