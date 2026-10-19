//! Omarchy theme integration. Colours come from
//! `~/.local/state/omarchy/current/theme/colors.toml` and are re-read whenever the theme changes.
//! Background fills carry the configured opacity; text colours are always fully opaque,
//! so Hyprland's blur shows through the window without dimming any content.

use crate::config::Config;
use eframe::egui::{self, Color32, CornerRadius, Stroke};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub dark: bool,
    pub bg: Color32,
    pub bg_dark: Color32,
    pub bg_darker: Color32,
    pub bg_light: Color32,
    pub fg: Color32,
    pub fg_dim: Color32,
    pub fg_bright: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub red: Color32,
    pub yellow: Color32,
    pub green: Color32,
    pub cyan: Color32,
    pub blue: Color32,
    pub magenta: Color32,
    pub orange: Color32,
}

pub fn omarchy_current_dir() -> PathBuf {
    // Test hook / non-standard installs.
    if let Some(p) = std::env::var_os("CARBON_OMARCHY_DIR") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_default();
    base.join("omarchy/current")
}

fn hex(s: &str) -> Option<Color32> {
    let s = s.trim().trim_start_matches('#');
    if s.len() < 6 {
        return None;
    }
    let v = u32::from_str_radix(&s[..6], 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Same colour with a given opacity (premultiplied correctly).
pub fn with_alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

impl Default for Theme {
    fn default() -> Self {
        let g = |h: &str| hex(h).unwrap();
        Self::from_palette(
            "default".into(),
            true,
            |k| {
                Some(g(match k {
                    "background" => "#1a1b1e",
                    "dark_background" => "#141517",
                    "darker_background" => "#0e0f10",
                    "lighter_background" => "#26272b",
                    "foreground" => "#d4d4d8",
                    "bright_foreground" => "#f4f4f5",
                    "dark_foreground" => "#6b6d75",
                    "accent" | "blue" => "#8aa4ff",
                    "selection" => "#34364a",
                    "red" => "#f2777a",
                    "yellow" => "#e5c07b",
                    "green" => "#98c379",
                    "cyan" => "#56b6c2",
                    "magenta" => "#c678dd",
                    "orange" => "#d19a66",
                    _ => return None,
                }))
            },
        )
    }
}

impl Theme {
    fn from_palette(name: String, dark: bool, get: impl Fn(&str) -> Option<Color32>) -> Self {
        let d = |k: &str, fallback: Color32| get(k).unwrap_or(fallback);
        let bg = d("background", Color32::from_rgb(26, 27, 30));
        let fg = d("foreground", Color32::from_rgb(212, 212, 216));
        let blue = d("blue", d("accent", Color32::from_rgb(138, 164, 255)));
        Self {
            name,
            dark,
            bg,
            bg_dark: d("dark_background", mix(bg, Color32::BLACK, 0.2)),
            bg_darker: d("darker_background", mix(bg, Color32::BLACK, 0.4)),
            bg_light: d("lighter_background", mix(bg, fg, 0.08)),
            fg,
            fg_dim: mix(fg, bg, 0.45),
            fg_bright: d("bright_foreground", fg),
            muted: d("muted", d("dark_foreground", mix(fg, bg, 0.6))),
            accent: d("accent", blue),
            selection: d("selection", mix(bg, fg, 0.18)),
            red: d("red", Color32::from_rgb(242, 119, 122)),
            yellow: d("yellow", Color32::from_rgb(229, 192, 123)),
            green: d("green", Color32::from_rgb(152, 195, 121)),
            cyan: d("cyan", Color32::from_rgb(86, 182, 194)),
            blue,
            magenta: d("magenta", Color32::from_rgb(198, 120, 221)),
            orange: d("orange", d("yellow", Color32::from_rgb(209, 154, 102))),
        }
    }

    /// Reads the live Omarchy theme; falls back to a built-in palette if Omarchy isn't there.
    pub fn load() -> Self {
        let dir = omarchy_current_dir();
        let name = std::fs::read_to_string(dir.join("theme.name"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "default".into());
        let Ok(text) = std::fs::read_to_string(dir.join("theme/colors.toml")) else {
            return Self::default();
        };
        let mut map = std::collections::HashMap::new();
        for line in text.lines() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
            }
        }
        let dark = map.get("mode").map(|m| m != "light").unwrap_or(true);
        Self::from_palette(name, dark, |k| map.get(k).and_then(|v| hex(v)))
    }

    pub fn cursor_rgb(&self) -> [u8; 3] {
        let c = self.fg_bright;
        [c.r(), c.g(), c.b()]
    }

    pub fn apply(&self, ctx: &egui::Context, cfg: &Config) {
        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        let op = cfg.background_opacity;
        // Popups/menus need to stay legible even on a very see-through window.
        let pop = op.max(0.94);

        v.panel_fill = with_alpha(self.bg_dark, op);
        v.window_fill = with_alpha(self.bg_dark, pop);
        v.extreme_bg_color = with_alpha(self.bg_darker, op.max(0.6));
        v.faint_bg_color = with_alpha(self.fg, 0.04);
        v.code_bg_color = with_alpha(self.bg_darker, 0.55);
        v.window_stroke = Stroke::new(1.0, with_alpha(self.fg, 0.14));
        v.window_corner_radius = CornerRadius::same(8);
        v.menu_corner_radius = CornerRadius::same(6);
        v.hyperlink_color = self.accent;
        v.selection.bg_fill = with_alpha(self.selection, 1.0);
        v.selection.stroke = Stroke::new(1.0, self.fg_bright);
        v.override_text_color = None;
        v.popup_shadow = egui::Shadow::NONE;
        v.window_shadow = egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(90) };

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = with_alpha(self.bg_dark, op);
        w.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
        w.noninteractive.bg_stroke = Stroke::new(1.0, with_alpha(self.fg, 0.10));
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.fg);
        w.inactive.bg_fill = with_alpha(self.fg, 0.07);
        w.inactive.weak_bg_fill = with_alpha(self.fg, 0.06);
        w.inactive.bg_stroke = Stroke::NONE;
        w.inactive.fg_stroke = Stroke::new(1.0, self.fg);
        w.hovered.bg_fill = with_alpha(self.fg, 0.12);
        w.hovered.weak_bg_fill = with_alpha(self.fg, 0.10);
        w.hovered.bg_stroke = Stroke::new(1.0, with_alpha(self.fg, 0.2));
        w.hovered.fg_stroke = Stroke::new(1.5, self.fg_bright);
        w.active.bg_fill = with_alpha(self.accent, 0.35);
        w.active.weak_bg_fill = with_alpha(self.accent, 0.30);
        w.active.bg_stroke = Stroke::new(1.0, self.accent);
        w.active.fg_stroke = Stroke::new(1.5, self.fg_bright);
        w.open.bg_fill = with_alpha(self.fg, 0.10);
        w.open.weak_bg_fill = with_alpha(self.fg, 0.10);
        w.open.fg_stroke = Stroke::new(1.0, self.fg_bright);
        for ws in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            ws.corner_radius = CornerRadius::same(5);
        }
        ctx.set_visuals(v);
    }
}
