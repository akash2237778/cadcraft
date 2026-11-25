//! Viewer mode: look at a drawing, on a phone or a tablet above all, without changing it.
//!
//! Nothing that edits is in reach: the menus, tool sets, palettes, file tabs and command line are
//! not shown, keyboard shortcuts are off, and a tap never selects for editing, starts a command,
//! drags a grip or opens a menu. What stays is navigation (pinch, two-finger pan, the wheel, Fit,
//! a double tap fits) and two ways to read sizes:
//!
//! - Inspect (the default): tap an object to see its size: a line's length and angle, a circle's
//!   radius and diameter, an arc's radius, angle and length, a polyline's length (area and
//!   perimeter when closed), a dimension's measured value (and the text it shows when that is
//!   overridden), a text's words, a block's name.
//! - Measure: tap two points; the object snaps apply as when drawing (end, middle, centre,
//!   intersection…). The distance shows on the drawing, and the card adds ΔX, ΔY and the angle.
//!
//! Values use the drawing's own units (LUNITS/LUPREC, AUNITS/AUPREC). The mode is the UI command
//! `ui.viewer` (`{on?, tool?: "inspect" | "measure", exit?: bool}`), also View ▸ Viewer Mode. A
//! page hosting CADCraft that offers its own way out (and keeps the drawing read-only on its side)
//! passes `exit: false` to hide the Exit Viewer button.

use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::{Polyline, Vec2};
use cadcraft_engine::units::{format_angle, format_distance};
use egui::{Align2, Color32, FontId, Stroke, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::canvas::Xf;

/// How far from a tapped point an object may be and still be the one tapped (screen points): a
/// fingertip, not a mouse's pickbox.
const TAP_APERTURE: f64 = 16.0;

/// What a tap does in the viewer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Show the size of the object tapped.
    #[default]
    Inspect,
    /// Measure between two tapped points.
    Measure,
}

/// The viewer's state.
#[derive(Clone, Debug, Default)]
pub struct ViewerState {
    pub on: bool,
    pub tool: Tool,
    /// Measure: the points tapped so far (none, one or two).
    pub points: Vec<Vec2>,
    /// Inspect: what the card says about the object tapped.
    pub info: Option<Info>,
    /// The host page decides when the viewer ends: no Exit Viewer button.
    pub hide_exit: bool,
}

/// The card for an inspected object: a title and labelled values.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub handle: Handle,
    pub title: String,
    pub rows: Vec<(String, String)>,
}

/// Turn the viewer on or off. Turning it on cancels a running command and clears the selection,
/// so nothing half-done carries into it; turning it off leaves an ordinary, unselected drawing.
pub fn set(app: &mut CadApp, on: bool) {
    if on && !app.viewer.on {
        app.cmd.buffer.clear();
        app.cmd.options = None;
        app.session.cancel();
    }
    app.viewer = ViewerState { on, hide_exit: app.viewer.hide_exit, ..ViewerState::default() };
    app.session.set_selection(Vec::new());
    app.session.pending_window = None;
}

fn tool_name(t: Tool) -> &'static str {
    match t {
        Tool::Inspect => "inspect",
        Tool::Measure => "measure",
    }
}

/// `ui.viewer {on?, tool?}`: with no `on`, a toggle; `tool` picks Inspect or Measure (and turns
/// the viewer on). Replies the state: `{on, tool}`.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id != "ui.viewer" {
        return None;
    }
    let tool = match params.get("tool").and_then(Value::as_str) {
        None => None,
        Some("inspect") => Some(Tool::Inspect),
        Some("measure") => Some(Tool::Measure),
        Some(other) => return Some(Err(format!("ui.viewer: unknown tool `{other}` (inspect or measure)"))),
    };
    let on = params.get("on").and_then(Value::as_bool).unwrap_or(if tool.is_some() { true } else { !app.viewer.on });
    set(app, on);
    if let Some(exit) = params.get("exit").and_then(Value::as_bool) {
        app.viewer.hide_exit = !exit;
    }
    if on && let Some(t) = tool {
        app.viewer.tool = t;
    }
    Some(Ok(json!({ "on": app.viewer.on, "tool": tool_name(app.viewer.tool), "exit": !app.viewer.hide_exit })))
}

/// The drawing's distance format.
fn dist(app: &CadApp, v: f64) -> String {
    let (lu, lp) = app.session.doc().map(|d| (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4))).unwrap_or((2, 4));
    format_distance(v, lu, lp)
}

/// The drawing's angle format, with at least two decimals: a measured angle is read, not drawn.
fn angle(app: &CadApp, a: f64) -> String {
    let (au, ap) = app.session.doc().map(|d| (d.header.i64("AUNITS", 0), d.header.i64("AUPREC", 0))).unwrap_or((0, 0));
    format_angle(a, au, ap.max(2))
}

/// The angle swept from `start` to `end` counter-clockwise, in (0, 2π].
fn sweep(start: f64, end: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let s = (end - start).rem_euclid(tau);
    if s <= 1e-12 { tau } else { s }
}

/// Text without MTEXT's formatting codes, short enough for the card.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('P') | Some('n') => out.push(' '),
                // A code with a value runs to its `;`.
                Some('f' | 'F' | 'H' | 'C' | 'Q' | 'W' | 'T' | 'A' | 'p') => {
                    for d in chars.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                Some(d) => out.push(d),
                None => {}
            },
            '{' | '}' => {}
            c => out.push(c),
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.chars().count() > 80 { format!("{}…", out.chars().take(80).collect::<String>()) } else { out }
}

/// What the card says about object `h`, or `None` when it is gone.
pub fn info(app: &CadApp, h: Handle) -> Option<Info> {
    let d = app.session.doc().ok()?;
    let e = d.entity(h)?;
    let t = |s: &str| crate::i18n::t(s).to_string();
    let mut rows: Vec<(String, String)> = Vec::new();
    let title = match &e.kind {
        EntityKind::Line(l) => {
            let v = l.b - l.a;
            rows.push((t("Length"), dist(app, v.len())));
            rows.push((t("Angle"), angle(app, l.a.xy().angle_to(l.b.xy()))));
            rows.push(("ΔX".into(), dist(app, v.x)));
            rows.push(("ΔY".into(), dist(app, v.y)));
            t("Line")
        }
        EntityKind::Circle(c) => {
            rows.push((t("Diameter"), dist(app, c.radius * 2.0)));
            rows.push((t("Radius"), dist(app, c.radius)));
            rows.push((t("Circumference"), dist(app, c.radius * std::f64::consts::TAU)));
            t("Circle")
        }
        EntityKind::Arc(a) => {
            let sw = sweep(a.start, a.end);
            rows.push((t("Radius"), dist(app, a.radius)));
            rows.push((t("Diameter"), dist(app, a.radius * 2.0)));
            rows.push((t("Angle"), angle(app, sw)));
            rows.push((t("Arc length"), dist(app, a.radius * sw)));
            rows.push((t("Chord"), dist(app, 2.0 * a.radius * (sw / 2.0).sin().abs())));
            t("Arc")
        }
        EntityKind::Ellipse(el) => {
            let major = el.major.len() * 2.0;
            rows.push((t("Major axis"), dist(app, major)));
            rows.push((t("Minor axis"), dist(app, major * el.ratio)));
            t("Ellipse")
        }
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            if p.closed {
                rows.push((t("Perimeter"), dist(app, pl.len())));
                rows.push((t("Area"), dist(app, pl.area().abs())));
            } else {
                rows.push((t("Length"), dist(app, pl.len())));
            }
            rows.push((t("Vertices"), p.vertices.len().to_string()));
            t("Polyline")
        }
        EntityKind::Dimension(dm) => {
            use cadcraft_engine::doc::DimKind;
            let is_angle = matches!(dm.kind, DimKind::Angular | DimKind::Angular3P);
            let measured = if is_angle { angle(app, dm.measurement) } else { dist(app, dm.measurement) };
            let what = match dm.kind {
                DimKind::Linear { .. } => t("Linear"),
                DimKind::Aligned => t("Aligned"),
                DimKind::Angular | DimKind::Angular3P => t("Angular"),
                DimKind::Diameter => t("Diameter"),
                DimKind::Radius => t("Radius"),
                DimKind::Ordinate { .. } => t("Ordinate"),
                DimKind::ArcLength => t("Arc length"),
            };
            rows.push((t("Measured"), measured.clone()));
            // A text override ("<>" stands for the measurement) can show something else.
            let shown = dm.text.trim();
            if !shown.is_empty() && shown != "<>" {
                rows.push((t("Shown as"), plain(&shown.replace("<>", &measured))));
            }
            rows.push((t("Type"), what));
            rows.push((t("Style"), dm.style.clone()));
            t("Dimension")
        }
        EntityKind::Text(tx) => {
            rows.push((t("Text"), plain(&tx.value)));
            rows.push((t("Height"), dist(app, tx.height)));
            t("Text")
        }
        EntityKind::MText(mt) => {
            rows.push((t("Text"), plain(&mt.contents)));
            rows.push((t("Height"), dist(app, mt.height)));
            t("Text")
        }
        EntityKind::Insert(ins) => {
            rows.push((t("Block"), ins.block.clone()));
            rows.push((t("Scale"), format!("{} × {}", dist(app, ins.scale.x), dist(app, ins.scale.y))));
            t("Block")
        }
        other => t(other.type_name()),
    };
    rows.push((t("Layer"), e.common.layer.clone()));
    Some(Info { handle: h, title, rows })
}

/// How far a measured point reaches for an object snap (screen points): a fingertip.
const SNAP_APERTURE: f64 = 24.0;

/// The point a measurement takes for a tap at `raw`: the drawing's object snaps (OSMODE) within
/// a fingertip, as when a command asks for a point (the drawing area only snaps then).
fn snapped(app: &mut CadApp, raw: Vec2, xf: &Xf, now: f64) -> Vec2 {
    let scale = xf.scale.max(1e-12);
    let aperture = app.session.settings.aperture.max(SNAP_APERTURE) / scale;
    let q =
        cadcraft_engine::snap::tracking::CursorQuery { base: None, deferred: false, corner: false, aperture, tol: 6.0 / scale, now, shift: false };
    app.session.snap_cursor(raw, &q).point
}

/// A tap on the drawing in the viewer, at `raw` (the point under the finger).
pub fn tap(app: &mut CadApp, raw: Vec2, xf: &Xf, now: f64) {
    match app.viewer.tool {
        Tool::Inspect => {
            let aperture = app.session.pixel_size() * TAP_APERTURE;
            let space = app.session.space();
            let hit = app.session.pick_object(&space, raw, aperture).ok().flatten();
            app.viewer.info = hit.and_then(|h| info(app, h));
            // The selection only highlights the object: nothing in the viewer edits it.
            app.session.set_selection(app.viewer.info.as_ref().map(|i| vec![i.handle]).unwrap_or_default());
        }
        Tool::Measure => {
            if app.viewer.points.len() >= 2 {
                app.viewer.points.clear();
            }
            let at = snapped(app, raw, xf, now);
            app.viewer.points.push(at);
        }
    }
}

/// The two measured points, once both are tapped.
pub fn measured(v: &ViewerState) -> Option<(Vec2, Vec2)> {
    match v.points.as_slice() {
        [a, b] => Some((*a, *b)),
        _ => None,
    }
}

/// Draw the measurement on the drawing: the points, the line between them and its length.
pub fn overlay(app: &CadApp, painter: &egui::Painter, xf: &Xf) {
    if !app.viewer.on || app.viewer.tool != Tool::Measure {
        return;
    }
    let accent = Color32::from_rgb(255, 196, 0);
    let pts: Vec<egui::Pos2> = app.viewer.points.iter().map(|p| xf.to_screen(*p)).collect();
    if let [a, b] = pts.as_slice() {
        painter.line_segment([*a, *b], Stroke::new(2.0, accent));
        if let Some((p, q)) = measured(&app.viewer) {
            let label = dist(app, p.dist(q));
            let mid = a.lerp(*b, 0.5);
            let galley = painter.layout_no_wrap(label, FontId::proportional(18.0), Color32::BLACK);
            let r = Align2::CENTER_BOTTOM.anchor_size(mid - vec2(0.0, 8.0), galley.size()).expand(5.0);
            painter.rect_filled(r, 4.0, accent);
            painter.galley(r.min + vec2(5.0, 5.0), galley, Color32::BLACK);
        }
    }
    for p in &pts {
        painter.circle(*p, 6.0, Color32::TRANSPARENT, Stroke::new(2.0, accent));
        painter.circle_filled(*p, 2.0, accent);
    }
}

/// The viewer's bar across the top: the drawing's name, Fit, the two tools and Exit.
pub fn top_bar(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    let name = app.session.state().map(|s| s.title.clone()).unwrap_or_default();
    egui::Panel::top("cc_viewer_bar").exact_size(48.0).frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(8, 6))).show(
        ui,
        |ui| {
            ui.horizontal_centered(|ui| {
                let big = |s: &str| egui::Button::new(egui::RichText::new(s).size(16.0)).min_size(vec2(44.0, 36.0));
                if ui.add(big(crate::tl!("Fit"))).clicked() {
                    let _ = app.session.zoom_extents();
                }
                let inspect = app.viewer.tool == Tool::Inspect;
                if ui.add(big(crate::tl!("Inspect")).selected(inspect)).clicked() {
                    app.viewer.tool = Tool::Inspect;
                    app.viewer.points.clear();
                }
                if ui.add(big(crate::tl!("Measure")).selected(!inspect)).clicked() {
                    app.viewer.tool = Tool::Measure;
                    app.viewer.info = None;
                    app.session.set_selection(Vec::new());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !app.viewer.hide_exit && ui.add(big(crate::tl!("Exit Viewer"))).clicked() {
                        set(app, false);
                    }
                    ui.add(egui::Label::new(egui::RichText::new(name).size(14.0)).truncate());
                });
            });
        },
    );
}

/// The card at the bottom: what was tapped, or how to use the tool.
pub fn card(app: &mut CadApp, ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let mut clear = false;
    egui::Area::new(egui::Id::unique("cc_viewer_card"))
        .order(egui::Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(screen.center_bottom() - vec2(0.0, 12.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_max_width((screen.width() - 32.0).clamp(200.0, 420.0));
                let value = |ui: &mut egui::Ui, k: &str, v: &str| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(k).size(14.0).weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(v).size(18.0).strong());
                        });
                    });
                };
                match (app.viewer.tool, &app.viewer.info, measured(&app.viewer)) {
                    (Tool::Inspect, Some(info), _) => {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&info.title).size(16.0).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("✕").clicked() {
                                    clear = true;
                                }
                            });
                        });
                        for (k, v) in &info.rows {
                            value(ui, k, v);
                        }
                    }
                    (Tool::Inspect, None, _) => {
                        ui.label(
                            egui::RichText::new(crate::tl!(
                                "Tap an object to see its size. Pinch to zoom, drag two fingers to move, double tap to fit."
                            ))
                            .size(14.0),
                        );
                    }
                    (Tool::Measure, _, Some((a, b))) => {
                        let d = b - a;
                        value(ui, crate::tl!("Distance"), &dist(app, a.dist(b)));
                        value(ui, "ΔX", &dist(app, d.x));
                        value(ui, "ΔY", &dist(app, d.y));
                        value(ui, crate::tl!("Angle"), &angle(app, a.angle_to(b)));
                        ui.label(egui::RichText::new(crate::tl!("Tap again to start a new measurement.")).size(13.0).weak());
                    }
                    (Tool::Measure, _, None) => {
                        let msg = if app.viewer.points.is_empty() {
                            crate::tl!("Tap the first point. Points snap to ends, middles, centres and intersections.")
                        } else {
                            crate::tl!("Tap the second point.")
                        };
                        ui.label(egui::RichText::new(msg).size(14.0));
                    }
                }
            });
        });
    if clear {
        app.viewer.info = None;
        app.session.set_selection(Vec::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_is_counter_clockwise_and_never_zero() {
        let pi = std::f64::consts::PI;
        assert!((sweep(0.0, pi / 2.0) - pi / 2.0).abs() < 1e-12);
        assert!((sweep(pi / 2.0, 0.0) - 1.5 * pi).abs() < 1e-12);
        assert!((sweep(1.0, 1.0) - 2.0 * pi).abs() < 1e-12, "a full turn, not nothing");
    }

    #[test]
    fn mtext_codes_are_dropped_for_the_card() {
        assert_eq!(plain("{\\fArial|b1;Hole}\\PØ12 H7"), "Hole Ø12 H7");
        assert_eq!(plain("\\H2.5;Big"), "Big");
    }
}
