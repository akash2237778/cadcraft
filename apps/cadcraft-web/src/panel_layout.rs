//! Browser panel layout storage, independent of drawings and native workspace files.

use cadcraft_ui_egui::{UiState, docking};
use serde_json::{Value, json};

#[cfg(target_arch = "wasm32")]
pub const KEY: &str = "cadcraft.panel-layout";
const MAX_BYTES: usize = 64 * 1024;

/// Serialize only validated panel placement and visibility, with a bounded record size.
pub fn save(ui: &UiState) -> Result<String, String> {
    docking::validate(&ui.docking)?;
    let saved = json!({
        "version": docking::LAYOUT_VERSION,
        "docking": ui.docking,
        "showToolsets": ui.show_toolsets,
        "showPalettes": ui.show_palettes,
    })
    .to_string();
    if saved.len() > MAX_BYTES {
        return Err("panel layout exceeds 64 KiB".into());
    }
    Ok(saved)
}

/// Validate the entire optional record before changing any UI field.
pub fn restore(ui: &mut UiState, saved: &str) -> Result<(), String> {
    if saved.len() > MAX_BYTES {
        return Err("panel layout exceeds 64 KiB".into());
    }
    let value: Value = serde_json::from_str(saved).map_err(|error| error.to_string())?;
    if value.get("version").and_then(Value::as_u64) != Some(u64::from(docking::LAYOUT_VERSION)) {
        return Err("unsupported panel layout version".into());
    }
    let workspace: docking::Workspace =
        serde_json::from_value(value.get("docking").cloned().ok_or("missing panel layout")?).map_err(|error| error.to_string())?;
    docking::validate(&workspace)?;
    let tools = value.get("showToolsets").and_then(Value::as_bool).ok_or("invalid Tool Sets visibility")?;
    let palettes = value.get("showPalettes").and_then(Value::as_bool).ok_or("invalid palette visibility")?;
    ui.docking = workspace;
    ui.show_toolsets = tools;
    ui.show_palettes = palettes;
    Ok(())
}

/// Below this width (CSS pixels, a phone held upright) the docked Tool Sets and palettes would
/// leave the drawing only a sliver of the screen.
pub const COMPACT_WIDTH: f64 = 760.0;

/// With no saved layout, a narrow screen starts with the Tool Sets and palettes hidden, so the
/// drawing gets the screen (they come back from the Window menu). A saved layout, the user's own
/// choice, is left alone; so is a width the browser could not tell (not finite or not positive).
pub fn compact_default(ui: &mut UiState, has_saved_layout: bool, width_css: f64) {
    if has_saved_layout || !width_css.is_finite() || width_css <= 0.0 || width_css >= COMPACT_WIDTH {
        return;
    }
    ui.show_toolsets = false;
    ui.show_palettes = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;
    use cadcraft_ui_egui::{CadApp, Services};

    #[test]
    fn a_phone_starts_with_the_drawing_on_the_whole_screen() {
        let mut ui = CadApp::new(Session::default(), Services::default()).ui;
        assert!(ui.show_toolsets && ui.show_palettes, "the desktop default shows both");
        compact_default(&mut ui, false, 390.0);
        assert!(!ui.show_toolsets && !ui.show_palettes, "a phone hides both");
    }

    #[test]
    fn a_saved_layout_or_a_wide_or_unknown_screen_keeps_the_panels() {
        for (saved, width) in [(true, 390.0), (false, 1280.0), (false, COMPACT_WIDTH), (false, f64::NAN), (false, 0.0), (false, -5.0)] {
            let mut ui = CadApp::new(Session::default(), Services::default()).ui;
            compact_default(&mut ui, saved, width);
            assert!(ui.show_toolsets && ui.show_palettes, "saved={saved} width={width}");
        }
    }

    #[test]
    fn floating_and_hidden_panels_survive_storage_without_replacing_drawing_state() {
        let mut source = CadApp::new(Session::default(), Services::default());
        source.run("ui.dock", json!({"operation":"float","panel":"toolSets","rect":[120,90,340,410]})).unwrap();
        source.run("ui.dock", json!({"operation":"close","panel":"properties"})).unwrap();
        let saved = save(&source.ui).unwrap();
        let mut destination = CadApp::new(Session::default(), Services::default());
        destination.session.start("line").unwrap();
        let before = destination.session.doc().unwrap().clone();
        restore(&mut destination.ui, &saved).unwrap();
        assert_eq!(destination.ui.docking, source.ui.docking);
        assert_eq!(destination.ui.show_toolsets, source.ui.show_toolsets);
        assert_eq!(destination.ui.show_palettes, source.ui.show_palettes);
        assert_eq!(destination.session.running.as_ref().map(|command| command.id.as_str()), Some("line"));
        assert_eq!(destination.session.doc().unwrap(), &before);
    }

    #[test]
    fn malformed_or_unsupported_storage_is_rejected_atomically() {
        let mut ui = UiState::default();
        let before = serde_json::to_value(&ui).unwrap();
        let valid: Value = serde_json::from_str(&save(&ui).unwrap()).unwrap();
        for (field, bad) in [
            ("version", json!(0)),
            ("version", json!(docking::LAYOUT_VERSION + 1)),
            ("docking", json!({})),
            ("showToolsets", json!("false")),
            ("showPalettes", Value::Null),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = bad;
            assert!(restore(&mut ui, &invalid.to_string()).is_err());
            assert_eq!(serde_json::to_value(&ui).unwrap(), before);
        }
        for invalid in ["not json".to_string(), " ".repeat(MAX_BYTES + 1)] {
            assert!(restore(&mut ui, &invalid).is_err());
            assert_eq!(serde_json::to_value(&ui).unwrap(), before);
        }
    }

    #[test]
    fn storage_cannot_remove_the_document_view() {
        let mut ui = UiState::default();
        let before = serde_json::to_value(&ui).unwrap();
        let mut invalid = ui.clone();
        invalid.docking.layout.root = None;
        assert!(save(&invalid).is_err());
        let mut record: Value = serde_json::from_str(&save(&ui).unwrap()).unwrap();
        record["docking"] = serde_json::to_value(&invalid.docking).unwrap();
        assert!(restore(&mut ui, &record.to_string()).is_err());
        assert_eq!(serde_json::to_value(&ui).unwrap(), before);
    }
}
