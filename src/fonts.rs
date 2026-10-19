//! System fonts via fontconfig: a readable sans for prose and the Omarchy monospace for code.
//! Falls back to egui's bundled fonts if anything is missing.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

fn fc(pattern: &str) -> Option<Vec<u8>> {
    let out = std::process::Command::new("fc-match").args(["-f", "%{file}", pattern]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    std::fs::read(String::from_utf8_lossy(&out.stdout).trim()).ok()
}

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}
pub fn italic() -> FontFamily {
    FontFamily::Name("italic".into())
}
pub fn bold_italic() -> FontFamily {
    FontFamily::Name("bolditalic".into())
}

pub fn family(b: bool, i: bool) -> FontFamily {
    match (b, i) {
        (false, false) => FontFamily::Proportional,
        (true, false) => bold(),
        (false, true) => italic(),
        (true, true) => bold_italic(),
    }
}

pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let default_prop = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let default_mono = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

    let add = |defs: &mut FontDefinitions, key: &str, pattern: &str, family: FontFamily, fallback: &[String]| {
        let mut list = vec![];
        if let Some(bytes) = fc(pattern) {
            defs.font_data.insert(key.to_string(), Arc::new(FontData::from_owned(bytes)));
            list.push(key.to_string());
        }
        list.extend(fallback.iter().cloned());
        defs.families.insert(family, list);
    };

    add(&mut defs, "carbon-sans", "sans-serif", FontFamily::Proportional, &default_prop);
    add(&mut defs, "carbon-sans-b", "sans-serif:bold", bold(), &default_prop);
    add(&mut defs, "carbon-sans-i", "sans-serif:italic", italic(), &default_prop);
    add(&mut defs, "carbon-sans-bi", "sans-serif:bold:italic", bold_italic(), &default_prop);
    add(&mut defs, "carbon-mono", "monospace", FontFamily::Monospace, &default_mono);

    ctx.set_fonts(defs);
}
