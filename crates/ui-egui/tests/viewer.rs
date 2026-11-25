//! Viewer mode: the drawing without the editing UI; a tap shows an object's size, Measure snaps
//! to the drawing; and nothing a viewer does (taps, double taps, drags, a held finger, keys)
//! changes the drawing.

use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::viewer::Tool;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// A drawing with a 3-4-5 line (length 50) and a horizontal dimension of 40 under it.
fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("line", json!({ "points": [[0.0, 0.0], [30.0, 40.0]] })).expect("line");
    app.run("dimlinear", json!({ "p1": [60.0, 0.0], "p2": [100.0, 0.0], "at": [80.0, -10.0] })).expect("dimension");
    let mut h = Harness::builder().with_size(vec2(1000.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.state_mut().run("ui.viewer", json!({ "on": true })).expect("viewer on");
    h.run_steps(3);
    let _ = h.state_mut().session.zoom_extents();
    h.run_steps(3);
    h
}

fn screen(h: &Harness<'_, CadApp>, x: f64, y: f64) -> Pos2 {
    h.state().canvas.xf.expect("the canvas is laid out").to_screen(Vec2::new(x, y))
}

fn click(h: &mut Harness<'_, CadApp>, at: Pos2) {
    h.hover_at(at);
    h.step();
    h.event(Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.event(Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(3);
}

fn row(h: &Harness<'_, CadApp>, key: &str) -> Option<String> {
    h.state().viewer.info.as_ref()?.rows.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

/// The drawing's revision: every change bumps it.
fn revision(h: &Harness<'_, CadApp>) -> u64 {
    h.state().session.state().map(|s| s.revision).unwrap_or(0)
}

#[test]
fn the_viewer_shows_the_drawing_without_the_editing_ui() {
    let h = harness();
    assert!(h.query_by_label("Exit Viewer").is_some(), "the viewer's own bar");
    assert!(h.query_by_label("Measure").is_some());
    assert!(h.query_by_label("File").is_none(), "no menu bar");
    assert!(h.query_by_label("Tool Sets").is_none(), "no tool sets");
}

#[test]
fn tapping_a_line_shows_its_length_and_angle() {
    let mut h = harness();
    let at = screen(&h, 15.0, 20.0);
    click(&mut h, at);
    assert_eq!(h.state().viewer.info.as_ref().map(|i| i.title.as_str()), Some("Line"));
    assert_eq!(row(&h, "Length").as_deref(), Some("50.0000"));
    assert_eq!(row(&h, "ΔX").as_deref(), Some("30.0000"));
    assert!(h.query_by_label("50.0000").is_some(), "the card shows it");
}

#[test]
fn tapping_a_dimension_shows_its_measured_value() {
    let mut h = harness();
    // The dimension line runs at y = -10 between x = 60 and 100.
    let at = screen(&h, 80.0, -10.0);
    click(&mut h, at);
    assert_eq!(h.state().viewer.info.as_ref().map(|i| i.title.as_str()), Some("Dimension"), "{:?}", h.state().viewer.info);
    assert_eq!(row(&h, "Measured").as_deref(), Some("40.0000"));
}

#[test]
fn measure_snaps_to_the_line_ends() {
    let mut h = harness();
    h.state_mut().run("ui.viewer", json!({ "tool": "measure" })).expect("measure");
    h.run_steps(2);
    assert_eq!(h.state().viewer.tool, Tool::Measure);
    // A few points off each end: the endpoint snap puts the measurement on the ends.
    let at = screen(&h, 0.0, 0.0) + vec2(3.0, -2.0);
    click(&mut h, at);
    let at = screen(&h, 30.0, 40.0) + vec2(-2.0, 3.0);
    click(&mut h, at);
    let pts = h.state().viewer.points.clone();
    assert_eq!(pts.len(), 2);
    let d = pts[0].dist(pts[1]);
    assert!((d - 50.0).abs() < 1e-6, "snapped to the ends: {pts:?}");
    assert!(h.query_by_label("50.0000").is_some(), "the distance shows");
}

#[test]
fn nothing_a_viewer_does_changes_the_drawing() {
    let mut h = harness();
    let before = revision(&h);
    let n = h.state().session.doc().map(|d| d.entity_count()).unwrap_or(0);
    let on_line = screen(&h, 15.0, 20.0);
    // Tap (selects for the highlight), then Delete, Backspace and Ctrl+Z.
    click(&mut h, on_line);
    for key in [egui::Key::Delete, egui::Key::Backspace] {
        h.key_press(key);
        h.run_steps(2);
    }
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(2);
    // A double click (edits text or a hatch elsewhere), a drag across the line (a grip or a
    // window), a right-click (Enter or a menu).
    h.event(Event::PointerButton { pos: on_line, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.event(Event::PointerButton { pos: on_line, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.event(Event::PointerButton { pos: on_line, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.event(Event::PointerButton { pos: on_line, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(2);
    h.drag_at(on_line);
    h.hover_at(on_line + vec2(120.0, 60.0));
    h.step();
    h.drop_at(on_line + vec2(120.0, 60.0));
    h.run_steps(2);
    h.event(Event::PointerButton { pos: on_line, button: egui::PointerButton::Secondary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.event(Event::PointerButton { pos: on_line, button: egui::PointerButton::Secondary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(2);
    assert!(!egui::Popup::is_any_open(&h.ctx), "no menu opened");
    assert!(h.state().session.running.is_none(), "no command started");
    assert_eq!(h.state().session.doc().map(|d| d.entity_count()).unwrap_or(0), n, "nothing deleted or added");
    assert_eq!(revision(&h), before, "the drawing did not change");
}

#[test]
fn a_host_page_can_hide_the_exit_button() {
    let mut h = harness();
    let r = h.state_mut().run("ui.viewer", json!({ "on": true, "exit": false })).expect("viewer");
    assert_eq!(r["exit"], false);
    h.run_steps(2);
    assert!(h.query_by_label("Exit Viewer").is_none(), "the host page offers the way out");
    assert!(h.state().viewer.on);
}

#[test]
fn leaving_the_viewer_gives_the_editing_ui_back() {
    let mut h = harness();
    h.get_by_label("Exit Viewer").click();
    h.run_steps(3);
    assert!(!h.state().viewer.on);
    assert!(h.query_by_label("Exit Viewer").is_none());
    assert!(h.state().session.selection().is_empty(), "nothing left selected");
}
