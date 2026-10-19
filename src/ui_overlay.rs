//! Modal surfaces: quick switcher, command palette, vault picker, settings, prompts.

use crate::app::{App, Overlay, PromptKind};
use crate::cmds::{Cmd, ALL};
use crate::config::{CursorShape, ThemeMode, ViewMode};
use crate::theme::with_alpha;
use crate::util::fuzzy;
use crate::vault::FileKind;
use eframe::egui::{
    self, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Key, Layout, Margin, Modifiers, RichText, ScrollArea, Sense, Stroke, Ui,
};
use std::path::PathBuf;

fn modal_frame(app: &App) -> Frame {
    Frame::new()
        .fill(with_alpha(app.theme.bg_dark, 0.97))
        .stroke(Stroke::new(1.0, with_alpha(app.theme.fg, 0.14)))
        .corner_radius(10)
        .inner_margin(Margin::same(12))
}

pub fn overlays(app: &mut App, ctx: &egui::Context) {
    match &app.overlay {
        Overlay::None => {}
        Overlay::Switcher { .. } => switcher(app, ctx),
        Overlay::Palette { .. } => palette(app, ctx),
        Overlay::Vaults => vaults(app, ctx),
        Overlay::Settings => settings(app, ctx),
        Overlay::Prompt(_) => prompt(app, ctx),
        Overlay::ConfirmDelete(_) => confirm_delete(app, ctx),
    }
}

fn anchored(ctx: &egui::Context, id: &str, top: f32, width: f32, frame: Frame, add: impl FnOnce(&mut Ui)) -> bool {
    let modal = egui::Modal::new(egui::Id::new(id)).backdrop_color(Color32::from_black_alpha(90)).frame(frame).area(
        egui::Modal::default_area(egui::Id::new(id)).anchor(Align2::CENTER_TOP, vec2(0.0, top)),
    );
    let r = modal.show(ctx, |ui| {
        ui.set_width(width);
        add(ui);
    });
    r.should_close()
}

// ---------------------------------------------------------------------------------------------
// Quick switcher
// ---------------------------------------------------------------------------------------------

struct Item {
    rel: String,
    title: String,
    sub: String,
    badge: Option<String>,
}

fn switcher_items(app: &App, q: &str) -> Vec<Item> {
    let Some(v) = &app.vault else { return vec![] };
    let mk = |f: &crate::vault::VFile| Item {
        rel: f.rel.clone(),
        title: f.stem().to_string(),
        sub: f.rel.rsplit_once('/').map(|x| x.0.to_string()).unwrap_or_default(),
        badge: if f.kind != FileKind::Note { f.name.rsplit_once('.').map(|x| x.1.to_string()) } else { None },
    };
    let current = app.current_rel();
    if q.is_empty() {
        let mut out: Vec<Item> = vec![];
        for r in &app.recent {
            if Some(r) != current.as_ref() {
                if let Some(f) = v.file(r) {
                    out.push(mk(f));
                }
            }
        }
        for f in &v.files {
            if out.len() >= 40 {
                break;
            }
            if f.kind == FileKind::Note && !out.iter().any(|i| i.rel == f.rel) && Some(&f.rel) != current.as_ref() {
                out.push(mk(f));
            }
        }
        return out;
    }
    let mut scored: Vec<(i32, Item)> = vec![];
    for f in v.files.iter().filter(|f| matches!(f.kind, FileKind::Note | FileKind::Image | FileKind::Pdf)) {
        let by_name = fuzzy(q, f.stem()).map(|s| s + 30);
        let by_path = fuzzy(q, &f.rel);
        let by_alias = v.meta.get(&f.rel).and_then(|m| m.aliases.iter().filter_map(|a| fuzzy(q, a)).max());
        if let Some(s) = [by_name, by_path, by_alias].into_iter().flatten().max() {
            let mut it = mk(f);
            if by_name.is_none() && by_alias.is_some() {
                if let Some(a) = v.meta.get(&f.rel).and_then(|m| m.aliases.iter().find(|a| fuzzy(q, a).is_some())) {
                    it.sub = format!("alias: {a}");
                }
            }
            let recency = app.recent.iter().position(|r| r == &f.rel).map(|p| (20 - p as i32).max(0)).unwrap_or(0);
            scored.push((s + recency, it));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(40).map(|x| x.1).collect()
}

fn switcher(app: &mut App, ctx: &egui::Context) {
    let Overlay::Switcher { query, sel } = &app.overlay else { return };
    let (mut q, mut s) = (query.clone(), *sel);
    let items = switcher_items(app, &q);
    let th = app.theme.clone();
    let n = items.len();
    let creatable = !q.trim().is_empty() && !items.iter().any(|i| i.title.eq_ignore_ascii_case(q.trim()));
    let total = n + creatable as usize;

    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) && total > 0 {
        s = (s + 1) % total;
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) && total > 0 {
        s = (s + total - 1) % total;
    }
    let ctrl_enter = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
    let enter = ctrl_enter || ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    let shift_enter = !enter && ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Enter));
    let mut chosen: Option<usize> = if enter { Some(s) } else { None };
    let create = shift_enter;

    let mut changed_query = false;
    let frame = modal_frame(app);
    let close = anchored(ctx, "switcher", 90.0, 640.0, frame, |ui| {
        let te = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Find or create a note…").desired_width(f32::INFINITY).font(FontId::proportional(16.0)).frame(Frame::NONE).margin(Margin::symmetric(6, 6)));
        te.request_focus();
        changed_query = te.changed();
        ui.add_space(2.0);
        ui.separator();
        ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
            for (i, it) in items.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                if i == s {
                    ui.painter().rect_filled(rect, CornerRadius::same(6), with_alpha(th.accent, 0.22));
                    resp.scroll_to_me(None);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::same(6), with_alpha(th.fg, 0.07));
                }
                ui.painter().text(egui::pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, &it.title, FontId::proportional(14.5), th.fg_bright);
                let mut right = rect.right() - 10.0;
                if let Some(b) = &it.badge {
                    ui.painter().text(egui::pos2(right, rect.center().y), Align2::RIGHT_CENTER, b, FontId::proportional(11.0), th.fg_dim);
                    right -= 34.0;
                }
                ui.painter().text(egui::pos2(right, rect.center().y), Align2::RIGHT_CENTER, &it.sub, FontId::proportional(12.0), th.fg_dim);
                if resp.clicked() {
                    chosen = Some(i);
                }
                if resp.hovered() && ui.input(|i| i.pointer.delta().length() > 0.5) {
                    s = i;
                }
            }
            if creatable {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                if s == n {
                    ui.painter().rect_filled(rect, CornerRadius::same(6), with_alpha(th.accent, 0.22));
                }
                ui.painter().text(egui::pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, format!("Create “{}”", q.trim()), FontId::proportional(14.0), th.accent);
                ui.painter().text(egui::pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, "Shift+Enter", FontId::proportional(11.5), th.fg_dim);
                if resp.clicked() {
                    chosen = Some(n);
                }
            }
            if total == 0 {
                ui.add_space(8.0);
                ui.label(RichText::new("No matches").color(th.fg_dim));
            }
        });
    });

    if changed_query {
        s = 0;
    }
    if close {
        app.overlay = Overlay::None;
        return;
    }
    if create && creatable {
        let name = q.trim().to_string();
        app.overlay = Overlay::None;
        let dir = app.new_note_dir();
        app.create_note_named(&dir, &name);
        return;
    }
    if let Some(i) = chosen {
        app.overlay = Overlay::None;
        if i < n {
            app.open_path(&items[i].rel.clone(), ctrl_enter, None);
        } else if creatable {
            let dir = app.new_note_dir();
            app.create_note_named(&dir, q.trim());
        }
        return;
    }
    app.overlay = Overlay::Switcher { query: q, sel: s.min(total.saturating_sub(1)) };
}

// ---------------------------------------------------------------------------------------------
// Command palette
// ---------------------------------------------------------------------------------------------

fn palette(app: &mut App, ctx: &egui::Context) {
    let Overlay::Palette { query, sel } = &app.overlay else { return };
    let (mut q, mut s) = (query.clone(), *sel);
    let th = app.theme.clone();
    let mut list: Vec<(i32, Cmd, &str, &str)> = ALL
        .iter()
        .filter_map(|(c, name, key)| {
            // Hide editing commands when there's no note to edit.
            fuzzy(&q, name).map(|sc| (sc, *c, *name, *key))
        })
        .collect();
    if q.is_empty() {
        // keep declared order
    } else {
        list.sort_by(|a, b| b.0.cmp(&a.0));
    }
    let n = list.len();
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) && n > 0 {
        s = (s + 1) % n;
    }
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) && n > 0 {
        s = (s + n - 1) % n;
    }
    let mut run: Option<Cmd> = None;
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) && n > 0 {
        run = Some(list[s.min(n - 1)].1);
    }
    let mut changed = false;
    let frame = modal_frame(app);
    let close = anchored(ctx, "palette", 90.0, 640.0, frame, |ui| {
        let te = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Run a command…").desired_width(f32::INFINITY).font(FontId::proportional(16.0)).frame(Frame::NONE).margin(Margin::symmetric(6, 6)));
        te.request_focus();
        changed = te.changed();
        ui.separator();
        ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
            for (i, (_, cmd, name, key)) in list.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
                if i == s {
                    ui.painter().rect_filled(rect, CornerRadius::same(6), with_alpha(th.accent, 0.22));
                    resp.scroll_to_me(None);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::same(6), with_alpha(th.fg, 0.07));
                }
                ui.painter().text(egui::pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, name, FontId::proportional(14.0), th.fg_bright);
                ui.painter().text(egui::pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, key, FontId::proportional(12.0), th.fg_dim);
                if resp.clicked() {
                    run = Some(*cmd);
                }
            }
        });
    });
    if changed {
        s = 0;
    }
    if close {
        app.overlay = Overlay::None;
        return;
    }
    if let Some(c) = run {
        app.overlay = Overlay::None;
        app.run_cmd(ctx, c);
        return;
    }
    app.overlay = Overlay::Palette { query: q, sel: s.min(n.saturating_sub(1)) };
}

// ---------------------------------------------------------------------------------------------
// Vaults
// ---------------------------------------------------------------------------------------------

fn vault_row(ui: &mut Ui, app_theme: &crate::theme::Theme, p: &PathBuf, current: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
    if current {
        ui.painter().rect_filled(rect, CornerRadius::same(6), with_alpha(app_theme.accent, 0.20));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(6), with_alpha(app_theme.fg, 0.07));
    }
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    ui.painter().text(egui::pos2(rect.left() + 12.0, rect.center().y - 7.0), Align2::LEFT_CENTER, name, FontId::proportional(14.5), app_theme.fg_bright);
    ui.painter().text(egui::pos2(rect.left() + 12.0, rect.center().y + 9.0), Align2::LEFT_CENTER, p.to_string_lossy(), FontId::proportional(11.0), app_theme.fg_dim);
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn vault_lists(app: &App) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let recent: Vec<PathBuf> = app.cfg.recent_vaults.iter().filter(|p| p.is_dir()).cloned().collect();
    let others: Vec<PathBuf> = app.obsidian_vaults.iter().filter(|p| !recent.contains(p)).cloned().collect();
    (recent, others)
}

fn vault_sections(app: &mut App, ui: &mut Ui) -> Option<PathBuf> {
    let th = app.theme.clone();
    let cur = app.vault.as_ref().map(|v| v.root.clone());
    let (recent, others) = vault_lists(app);
    let mut pick = None;
    if !recent.is_empty() {
        ui.label(RichText::new("Recent").size(11.5).color(th.fg_dim));
        for p in &recent {
            if vault_row(ui, &th, p, cur.as_ref() == Some(p)) {
                pick = Some(p.clone());
            }
        }
    }
    if !others.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new("Known to Obsidian").size(11.5).color(th.fg_dim));
        for p in &others {
            if vault_row(ui, &th, p, false) {
                pick = Some(p.clone());
            }
        }
    }
    pick
}

fn vaults(app: &mut App, ctx: &egui::Context) {
    let frame = modal_frame(app);
    let mut pick = None;
    let mut dialog = false;
    let close = anchored(ctx, "vaults", 120.0, 520.0, frame, |ui| {
        ui.label(RichText::new("Vaults").size(17.0).color(app.theme.fg_bright));
        ui.add_space(8.0);
        ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
            pick = vault_sections(app, ui);
        });
        ui.add_space(10.0);
        if ui.button("Open folder as vault…").clicked() {
            dialog = true;
        }
    });
    if dialog {
        app.overlay = Overlay::None;
        app.pick_vault_dialog();
    } else if let Some(p) = pick {
        app.open_vault(p);
    } else if close {
        app.overlay = Overlay::None;
    }
}

pub fn welcome(app: &mut App, ui: &mut Ui) {
    let th = app.theme.clone();
    let op = app.cfg.background_opacity;
    egui::CentralPanel::default().frame(Frame::new().fill(with_alpha(th.bg, op))).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.16).max(30.0));
            ui.label(RichText::new("Carbon").font(FontId::new(44.0, crate::fonts::bold())).color(th.fg_bright));
            ui.label(RichText::new("Fast, GPU-accelerated Markdown for Obsidian vaults").size(14.0).color(th.fg_dim));
            ui.add_space(26.0);
            if ui.add(egui::Button::new(RichText::new("Open folder as vault").size(15.0)).min_size(vec2(240.0, 38.0))).clicked() {
                app.pick_vault_dialog();
            }
            ui.add_space(22.0);
            ui.allocate_ui_with_layout(vec2(520.0, 360.0), Layout::top_down(Align::Min), |ui| {
                ScrollArea::vertical().show(ui, |ui| {
                    if let Some(p) = vault_sections(app, ui) {
                        app.open_vault(p);
                    }
                });
            });
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------------------------

fn prompt(app: &mut App, ctx: &egui::Context) {
    let Overlay::Prompt(p) = &mut app.overlay else { return };
    let title = match &p.kind {
        PromptKind::NewNote { .. } => "New note",
        PromptKind::NewFolder { .. } => "New folder",
        PromptKind::Rename { .. } => "Rename",
    };
    let mut input = std::mem::take(&mut p.input);
    let mut focus = p.focus;
    let mut submit = false;
    let th = app.theme.clone();
    let frame = modal_frame(app);
    let close = anchored(ctx, "prompt", 160.0, 420.0, frame, |ui| {
        ui.label(RichText::new(title).size(15.0).color(th.fg_bright));
        ui.add_space(8.0);
        let te = ui.add(egui::TextEdit::singleline(&mut input).desired_width(f32::INFINITY));
        if focus {
            te.request_focus();
            // Select the whole name so typing replaces it (rename).
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), te.id) {
                st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(input.chars().count()))));
                st.store(ui.ctx(), te.id);
            }
            focus = false;
        }
        if te.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            submit = true;
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                submit = true;
            }
        });
    });
    let Overlay::Prompt(p) = &mut app.overlay else { return };
    p.input = input.clone();
    p.focus = focus;
    if submit && !input.trim().is_empty() {
        let kind = std::mem::replace(&mut p.kind, PromptKind::NewFolder { dir: String::new() });
        app.overlay = Overlay::None;
        match kind {
            PromptKind::NewNote { dir } => app.create_note_named(&dir, &input),
            PromptKind::NewFolder { dir } => {
                if let Some(v) = &mut app.vault {
                    let name = input.trim().to_string();
                    match v.create_folder(&dir, &name) {
                        Ok(rel) => app.expand_to(&format!("{rel}/x")),
                        Err(e) => app.toast(format!("Couldn't create folder: {e}")),
                    }
                }
            }
            PromptKind::Rename { rel } => app.rename_path(&rel, &input),
        }
    } else if close {
        app.overlay = Overlay::None;
    }
}

fn confirm_delete(app: &mut App, ctx: &egui::Context) {
    let Overlay::ConfirmDelete(rel) = &app.overlay else { return };
    let rel = rel.clone();
    let th = app.theme.clone();
    let frame = modal_frame(app);
    let mut yes = false;
    let mut no = false;
    let close = anchored(ctx, "confirm-delete", 160.0, 400.0, frame, |ui| {
        ui.label(RichText::new("Delete file?").size(15.0).color(th.fg_bright));
        ui.add_space(6.0);
        ui.label(RichText::new(format!("“{rel}” will be moved to the trash.")).color(th.fg_dim));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("Delete").color(th.red))).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                yes = true;
            }
            if ui.button("Cancel").clicked() {
                no = true;
            }
        });
    });
    if yes {
        app.overlay = Overlay::None;
        app.delete_path(&rel);
    } else if no || close {
        app.overlay = Overlay::None;
    }
}

// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

fn settings(app: &mut App, ctx: &egui::Context) {
    let th = app.theme.clone();
    let frame = modal_frame(app);
    let mut dirty = false;
    let mut reapply = false;
    let mut edit_shader = false;
    let close = anchored(ctx, "settings", 70.0, 560.0, frame, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Settings").size(18.0).color(th.fg_bright));
        });
        ui.add_space(6.0);
        ScrollArea::vertical().max_height((ctx.content_rect().height() - 180.0).clamp(240.0, 640.0)).min_scrolled_height((ctx.content_rect().height() - 180.0).clamp(240.0, 640.0)).auto_shrink([false, false]).show(ui, |ui| {
            let head = |ui: &mut Ui, t: &str| {
                ui.add_space(12.0);
                ui.label(RichText::new(t).size(12.0).color(th.fg_dim));
                ui.separator();
            };
            let row = |ui: &mut Ui, label: &str, add: &mut dyn FnMut(&mut Ui)| {
                ui.horizontal(|ui| {
                    ui.set_min_height(28.0);
                    ui.label(label);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| add(ui));
                });
            };
            let cfg = &mut app.cfg;

            head(ui, "APPEARANCE");
            row(ui, &format!("Theme  ·  following Omarchy ({})", th.name), &mut |ui| {
                ui.label(RichText::new("hot-reloads").size(11.5).color(th.fg_dim));
            });
            row(ui, "Background opacity  (Hyprland blur shows through)", &mut |ui| {
                if ui.add(egui::Slider::new(&mut cfg.background_opacity, 0.3..=1.0).fixed_decimals(2)).changed() {
                    dirty = true;
                    reapply = true;
                }
            });
            row(ui, "Font size", &mut |ui| {
                dirty |= ui.add(egui::Slider::new(&mut cfg.font_size, 11.0..=28.0).fixed_decimals(0)).changed();
            });
            row(ui, "Interface scale", &mut |ui| {
                if ui.add(egui::Slider::new(&mut cfg.ui_scale, 0.8..=1.8).fixed_decimals(2)).drag_stopped() {
                    ctx.set_zoom_factor(cfg.ui_scale);
                    dirty = true;
                }
            });
            row(ui, "Readable line length", &mut |ui| {
                dirty |= ui.checkbox(&mut cfg.readable_line_length, "").changed();
            });

            head(ui, "EDITOR");
            row(ui, "Default view for new tabs", &mut |ui| {
                egui::ComboBox::from_id_salt("defview").selected_text(match cfg.default_view {
                    ViewMode::Live => "Live preview",
                    ViewMode::Source => "Source",
                    ViewMode::Reading => "Reading",
                }).show_ui(ui, |ui| {
                    dirty |= ui.selectable_value(&mut cfg.default_view, ViewMode::Live, "Live preview").changed();
                    dirty |= ui.selectable_value(&mut cfg.default_view, ViewMode::Source, "Source").changed();
                    dirty |= ui.selectable_value(&mut cfg.default_view, ViewMode::Reading, "Reading").changed();
                });
            });
            row(ui, "Auto-save delay (seconds)", &mut |ui| {
                dirty |= ui.add(egui::Slider::new(&mut cfg.autosave_secs, 0.3..=10.0).fixed_decimals(1)).changed();
            });

            head(ui, "CURSOR (GPU SHADER)");
            row(ui, "Effect", &mut |ui| {
                egui::ComboBox::from_id_salt("fx").selected_text(cfg.cursor_effect.clone()).show_ui(ui, |ui| {
                    for (name, _) in crate::cursor_fx::PRESETS {
                        dirty |= ui.selectable_value(&mut cfg.cursor_effect, name.to_string(), *name).changed();
                    }
                    dirty |= ui.selectable_value(&mut cfg.cursor_effect, "custom".to_string(), "custom (~/.config/carbon/cursor.wgsl)").changed();
                });
            });
            row(ui, "Shape", &mut |ui| {
                egui::ComboBox::from_id_salt("shape").selected_text(format!("{:?}", cfg.cursor_shape)).show_ui(ui, |ui| {
                    for s in [CursorShape::Bar, CursorShape::Block, CursorShape::Underline] {
                        dirty |= ui.selectable_value(&mut cfg.cursor_shape, s, format!("{s:?}")).changed();
                    }
                });
            });
            row(ui, "Blink", &mut |ui| {
                dirty |= ui.checkbox(&mut cfg.cursor_blink, "").changed();
            });
            row(ui, "Animate every frame (for time-based custom shaders)", &mut |ui| {
                dirty |= ui.checkbox(&mut cfg.cursor_continuous, "").changed();
            });
            row(ui, "Colour", &mut |ui| {
                let follow = cfg.cursor_color.is_none();
                let mut f = follow;
                if ui.checkbox(&mut f, "follow theme").changed() {
                    cfg.cursor_color = if f { None } else { Some(th.cursor_rgb()) };
                    dirty = true;
                }
                if let Some(c) = &mut cfg.cursor_color {
                    dirty |= ui.color_edit_button_srgb(c).changed();
                }
            });
            row(ui, "Write your own shader", &mut |ui| {
                if ui.button("Open cursor.wgsl").clicked() {
                    edit_shader = true;
                }
            });

            head(ui, "FILES");
            if let Some(v) = &app.vault {
                let att = v.attachment_dir("Folder/Note.md");
                let desc = match &v.cfg.attachment_folder {
                    Some(s) if !s.is_empty() => format!("From vault settings: “{s}”"),
                    _ => "Default: an “attachments” folder beside each note".to_string(),
                };
                let _ = att;
                row(ui, "Pasted images and dropped files", &mut |ui| {
                    ui.label(RichText::new(desc.clone()).size(12.0).color(th.fg_dim));
                });
                row(ui, "Vault", &mut |ui| {
                    ui.label(RichText::new(v.root.to_string_lossy()).size(12.0).color(th.fg_dim));
                });
            }
            let _ = ThemeMode::Dark;
        });
    });
    if reapply {
        app.theme.apply(ctx, &app.cfg);
    }
    if dirty {
        if app.fx.effect_changed(&app.cfg) {
            app.fx.reload(&app.cfg);
        }
        app.cfg.save();
    }
    if edit_shader {
        app.overlay = Overlay::None;
        app.run_cmd(ctx, Cmd::OpenCursorShader);
    } else if close {
        app.overlay = Overlay::None;
    }
}
