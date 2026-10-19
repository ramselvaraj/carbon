//! The main content area: editor (Live / Source), reader, image and empty-tab views.

use crate::app::{hash_of, App, Find, Suggest, Tab, TabKind};
use crate::config::{CursorShape, ViewMode};
use crate::editor::{self, EditFlags, LinkRef, LiveStyle, Sel};
use crate::reader::{self, ReadCtx};
use crate::theme::with_alpha;
use crate::icons::Icon;
use crate::ui::icon_button;
use crate::util::fuzzy;
use crate::vault::{FileKind, Vault};
use eframe::egui::{
    self, text::CCursor, text_edit::TextBuffer, vec2, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Key, Margin, Modifiers, Rect, RichText, ScrollArea,
    Sense, Stroke, Ui, Vec2,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const IMG_MAX_H: f32 = 520.0;

pub fn main_view(app: &mut App, ui: &mut Ui) {
    let Some(tab) = app.tab() else { return };
    match tab.kind {
        TabKind::Empty => empty_tab(app, ui),
        TabKind::Image => image_tab(app, ui),
        TabKind::External => {}
        TabKind::Note => {
            if tab.view == ViewMode::Reading {
                reader_view(app, ui);
            } else {
                editor_view(app, ui);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

fn column(ui: &Ui, app_readable: bool, size: f32) -> (f32, f32) {
    let avail = ui.available_width();
    let col = if app_readable { (size * 47.0).min(avail - 48.0) } else { avail - 48.0 }.max(280.0);
    (col, ((avail - col) / 2.0).max(20.0))
}

pub fn image_size(nat: (u32, u32), hint: Option<f32>, max_w: f32) -> Vec2 {
    let (nw, nh) = (nat.0.max(1) as f32, nat.1.max(1) as f32);
    let mut w = hint.unwrap_or(nw).min(max_w).max(8.0);
    let mut h = w * nh / nw;
    if h > IMG_MAX_H {
        w *= IMG_MAX_H / h;
        h = IMG_MAX_H;
    }
    vec2(w, h)
}

fn dims_of(cache: &RefCell<&mut HashMap<String, Option<(u32, u32)>>>, vault: &Vault, rel: &str) -> Option<(u32, u32)> {
    if let Some(d) = cache.borrow().get(rel) {
        return *d;
    }
    let d = image::image_dimensions(vault.abs(rel)).ok();
    cache.borrow_mut().insert(rel.to_string(), d);
    d
}

fn empty_tab(app: &mut App, ui: &mut Ui) {
    let th = app.theme.clone();
    let ctx = ui.ctx().clone();
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.28);
        ui.label(RichText::new("No file is open").size(22.0).color(th.fg_dim));
        ui.add_space(18.0);
        let mut run = None;
        for (label, hint, cmd) in [
            ("Create new note", "Ctrl+N", crate::cmds::Cmd::NewNote),
            ("Go to file", "Ctrl+O", crate::cmds::Cmd::QuickSwitcher),
            ("Command palette", "Ctrl+P", crate::cmds::Cmd::Palette),
        ] {
            let r = ui.add(egui::Label::new(RichText::new(format!("{label}   ")).size(15.0).color(th.accent)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
            ui.label(RichText::new(hint).size(11.5).color(th.fg_dim));
            ui.add_space(8.0);
            if r.clicked() {
                run = Some(cmd);
            }
        }
        if ui.add(egui::Label::new(RichText::new("Close tab").size(15.0).color(th.accent)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            run = Some(crate::cmds::Cmd::CloseTab);
        }
        if let Some(c) = run {
            app.run_cmd(&ctx, c);
        }
    });
}

fn image_tab(app: &mut App, ui: &mut Ui) {
    let (Some(v), Some(rel)) = (&app.vault, app.current_rel()) else { return };
    let uri = format!("file://{}", v.abs(&rel).display());
    ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.add(egui::Image::new(uri).max_width(ui.available_width() - 40.0).fit_to_original_size(1.0));
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Reading view
// ---------------------------------------------------------------------------------------------

fn reader_view(app: &mut App, ui: &mut Ui) {
    let App { vault, tabs, active, cfg, theme, docs, .. } = app;
    let Some(vault) = vault.as_ref() else { return };
    let tab = &mut tabs[*active];
    let size = cfg.font_size;
    let h = hash_of(&tab.text);
    if tab.doc.as_ref().map(|d| d.0) != Some(h) {
        tab.doc = Some((h, Arc::new(reader::parse(&tab.text))));
    }
    let doc = tab.doc.as_ref().unwrap().1.clone();
    let rel = tab.path.clone().unwrap_or_default();
    let title = tab.title();
    let mut actions = vec![];
    let mut scroll_to = tab.scroll_heading.take();
    let mut rename = false;
    let (col, side) = column(ui, cfg.readable_line_length, size);

    ScrollArea::vertical().id_salt(("read-scroll", tab.id)).auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(28.0);
        ui.horizontal_top(|ui| {
            ui.add_space(side);
            ui.vertical(|ui| {
                ui.set_width(col);
                if inline_title(ui, &title, size, theme) {
                    rename = true;
                }
                ui.add_space(size * 0.8);
                let mut cx = ReadCtx { vault, theme, size, note_rel: &rel, actions: &mut actions, docs, scroll_to: &mut scroll_to, depth: 0, width: col };
                reader::render_doc(ui, &doc, &mut cx);
                ui.add_space(160.0);
            });
        });
    });
    if scroll_to.is_some() {
        // Heading not found yet (or off-screen on first pass): keep trying next frame.
        tab.scroll_heading = scroll_to;
        ui.ctx().request_repaint();
    }
    if rename {
        let ctx = ui.ctx().clone();
        app.run_cmd(&ctx, crate::cmds::Cmd::RenameNote);
    }
    app.handle_actions(actions);
}

fn inline_title(ui: &mut Ui, title: &str, size: f32, th: &crate::theme::Theme) -> bool {
    let r = ui
        .add(egui::Label::new(RichText::new(title).font(FontId::new(size * 2.0, crate::fonts::bold())).color(th.fg_bright)).sense(Sense::click()))
        .on_hover_text("Double-click to rename");
    r.double_clicked()
}

// ---------------------------------------------------------------------------------------------
// Editing view
// ---------------------------------------------------------------------------------------------

struct EditOut {
    follow: Option<(LinkRef, bool)>,
    rename: bool,
    apply_suggest: Option<usize>,
}

fn editor_view(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let id = app.editor_id();
    let mut out = EditOut { follow: None, rename: false, apply_suggest: None };
    {
        let App { vault, tabs, active, cfg, theme, fx, img_dims, .. } = app;
        let Some(vault) = vault.as_ref() else { return };
        let tab = &mut tabs[*active];
        let th = theme.clone();
        let size = cfg.font_size;

        if tab.find.open {
            find_bar(ui, tab, &ctx, id, &th);
        }

        if editor::load_sel(&ctx, id).is_none() {
            editor::store_sel(&ctx, id, Sel::caret(0));
        }
        let focused = ctx.memory(|m| m.has_focus(id));

        // -- keys owned by the autocomplete popup --------------------------------------------
        if focused {
            if let Some(s) = &mut tab.suggest {
                if !s.items.is_empty() {
                    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                        s.sel = (s.sel + 1) % s.items.len();
                    }
                    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                        s.sel = (s.sel + s.items.len() - 1) % s.items.len();
                    }
                    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::NONE, Key::Tab)) {
                        out.apply_suggest = Some(s.sel);
                    }
                }
                if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                    s.dismissed_at = Some(s.start);
                    s.items.clear();
                }
            }
        }
        if let Some(n) = out.apply_suggest.take() {
            apply_suggestion(&ctx, id, tab, n);
        }

        // -- goto / focus requests ------------------------------------------------------------
        if let Some(line) = tab.goto_line.take() {
            let b = editor::line_start_byte(&tab.text, line);
            let c = editor::char_of(&tab.text, b);
            editor::store_sel(&ctx, id, Sel::caret(c));
            tab.scroll_to_char = Some(c);
            tab.focus_editor = true;
        }
        if tab.focus_editor {
            // Retry for a few frames: a modal that is closing can swallow the first request.
            let tries_id = egui::Id::new(("focus-tries", tab.id));
            let tries: u8 = ctx.data(|d| d.get_temp(tries_id)).unwrap_or(0);
            if ctx.memory(|m| m.has_focus(id)) || tries >= 8 {
                tab.focus_editor = false;
                ctx.data_mut(|d| d.insert_temp(tries_id, 0u8));
            } else {
                ctx.memory_mut(|m| m.request_focus(id));
                ctx.data_mut(|d| d.insert_temp(tries_id, tries + 1));
                ctx.request_repaint();
            }
        }

        // -- editing helpers (Enter, Tab, auto-pair, …) ---------------------------------------
        let changed = editor::preprocess(&ctx, id, &mut tab.text, &EditFlags { focused, popup_open: tab.suggest.as_ref().is_some_and(|s| !s.items.is_empty()) });
        if changed {
            tab.dirty = true;
            tab.last_edit = Some(std::time::Instant::now());
        }

        // -- caret line (drives Live-preview reveal) ------------------------------------------
        if let Some(sel) = editor::load_sel(&ctx, id) {
            let (lo, hi) = (editor::byte_of(&tab.text, sel.lo()), editor::byte_of(&tab.text, sel.hi()));
            let head = editor::byte_of(&tab.text, sel.head);
            tab.active_lines = (editor::line_of(&tab.text, lo), editor::line_of(&tab.text, hi));
            let (ls, _) = editor::line_range(&tab.text, head);
            tab.caret = (editor::line_of(&tab.text, head) + 1, tab.text[ls..head].chars().count() + 1);
        }

        let (col, side) = column(ui, cfg.readable_line_length, size);
        let live = tab.view == ViewMode::Live;
        let rel = tab.path.clone().unwrap_or_default();
        let find_q = if tab.find.open { tab.find.query.clone() } else { String::new() };
        let theme_sig = hash_of(&(th.fg, th.accent, th.bg, th.bg_darker, th.yellow, th.orange, th.green, th.fg_dim, th.fg_bright, cfg.background_opacity.to_bits()));
        let key_extra = hash_of(&(live, tab.active_lines, &find_q, size.to_bits(), theme_sig, col.to_bits()));
        let title = tab.title();
        let (rows, row_h) = (((ui.available_height() / (size * 1.55)).ceil() as usize).max(8), size * 1.55);
        let _ = row_h;
        let dims = RefCell::new(&mut *img_dims);
        let active_lines = tab.active_lines;
        let tab_id = tab.id;
        let view_h = ui.available_height();

        // Rendered-block plan + decorations, rebuilt only when the text or size changes.
        let pkey = hash_of(&(tab.text.as_str(), size.to_bits()));
        if tab.plan_cache.as_ref().map(|p| p.0) != Some(pkey) {
            let plan = if live { editor::plan_blocks(&tab.text, size) } else { vec![] };
            tab.plan_cache = Some((pkey, Arc::new(plan), Arc::new(editor::decorations(&tab.text))));
        }
        let (plan, decos) = {
            let p = tab.plan_cache.as_ref().unwrap();
            (p.1.clone(), p.2.clone())
        };
        let Tab { text, galley_cache, .. } = &mut *tab;
        let image_height = |target: &str, hint: Option<f32>| -> Option<f32> {
            let f = vault.resolve(target, &rel)?;
            if f.kind != FileKind::Image {
                return None;
            }
            let nat = dims_of(&dims, vault, &f.rel)?;
            Some(image_size(nat, hint, col).y + 12.0)
        };

        let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, wrap: f32| -> Arc<egui::Galley> {
            let s = buf.as_str();
            let key = hash_of(&(s, key_extra, wrap.to_bits()));
            if let Some((k, g)) = galley_cache.as_ref() {
                if *k == key {
                    return g.clone();
                }
            }
            let style = LiveStyle { theme: &th, size, live, active: active_lines, find: &find_q, image_height: &image_height };
            let job = editor::build_job(s, &style, wrap);
            let g = ui.fonts_mut(|f| f.layout_job(job));
            *galley_cache = Some((key, g.clone()));
            g
        };

        ui.visuals_mut().text_cursor.stroke = Stroke::NONE; // the GPU caret replaces it
        ui.visuals_mut().text_cursor.blink = false;
        ui.visuals_mut().selection.bg_fill = with_alpha(th.accent, 0.32);

        let mut te_out = None;
        let sa = ScrollArea::vertical().id_salt(("edit-scroll", tab_id)).auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(28.0);
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(col);
                    if inline_title(ui, &title, size, &th) {
                        out.rename = true;
                    }
                    ui.add_space(size * 0.6);
                    let deco_slot = ui.painter().add(egui::Shape::Noop);
                    let o = egui::TextEdit::multiline(text)
                        .id(id)
                        .frame(Frame::NONE)
                        .margin(Margin::ZERO)
                        .desired_width(col)
                        .desired_rows(rows)
                        .lock_focus(true)
                        .layouter(&mut layouter)
                        .show(ui);
                    ui.add_space(view_h * 0.4);
                    let starts = line_starts(text);
                    let rows = RowTable::new(&o.galley);
                    let shapes = deco_shapes(&decos, &starts, &rows, o.galley_pos, col, live, &th);
                    ui.painter().set(deco_slot, egui::Shape::Vec(shapes));
                    if live {
                        paint_overlays(ui, &o.galley, o.galley_pos, text, &starts, &rows, &plan, &decos, active_lines, col, &th, size);
                    }
                    te_out = Some(o);
                });
            });
        });
        let Some(te) = te_out else { return };
        let viewport = sa.inner_rect;
        let galley = te.galley.clone();
        let gpos = te.galley_pos;

        if te.response.changed() {
            tab.dirty = true;
            tab.last_edit = Some(std::time::Instant::now());
            tab.galley_cache = None;
        }

        // -- scroll requests -------------------------------------------------------------------
        if let Some(c) = tab.scroll_to_char.take() {
            let r = galley.pos_from_cursor(CCursor::new(c)).translate(gpos.to_vec2());
            ui.scroll_to_rect(r.expand2(vec2(0.0, 40.0)), Some(Align::Center));
        }

        // -- inline images (Live) -----------------------------------------------------------------
        if live {
            let mut chars = 0usize;
            for (ln, line) in tab.text.split('\n').enumerate() {
                let n = line.chars().count();
                if line.contains("![") && (ln < tab.active_lines.0 || ln > tab.active_lines.1) {
                    if let Some((target, w)) = editor::embed_only(line) {
                        if let Some(f) = vault.resolve(&target, &rel).filter(|f| f.kind == FileKind::Image) {
                            if let Some(nat) = dims_of(&dims, vault, &f.rel) {
                                let r = galley.pos_from_cursor(CCursor::new(chars)).translate(gpos.to_vec2());
                                let sz = image_size(nat, w, col);
                                let rect = Rect::from_min_size(egui::pos2(r.min.x, r.min.y + 4.0), sz);
                                if rect.intersects(viewport) {
                                    egui::Image::new(format!("file://{}", vault.abs(&f.rel).display())).corner_radius(4).paint_at(ui, rect);
                                }
                            }
                        }
                    }
                }
                chars += n + 1;
            }
        }

        // -- click on a link / tag -----------------------------------------------------------------
        if te.response.clicked() {
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                let idx = galley.cursor_from_pos(pos - gpos).index.0;
                let byte = editor::byte_of(&tab.text, idx);
                let (ls, le) = editor::line_range(&tab.text, byte);
                let line_no = editor::line_of(&tab.text, byte);
                let was_hidden = live && (line_no < active_lines.0 || line_no > active_lines.1);
                let ctrl = ctx.input(|i| i.modifiers.command);
                let line_s = tab.text[ls..le].to_string();
                let on_box = editor::task_box(&line_s).is_some_and(|r| r.contains(&(byte - ls)));
                if on_box && (ctrl || was_hidden || tab.view == ViewMode::Source) {
                    let r = editor::task_box(&line_s).unwrap();
                    let at = ls + r.start + 1;
                    let nv = if &tab.text[at..at + 1] == " " { "x" } else { " " };
                    tab.text.replace_range(at..at + 1, nv);
                    tab.dirty = true;
                    tab.last_edit = Some(std::time::Instant::now());
                    tab.galley_cache = None;
                } else if ctrl || was_hidden {
                    let line = &tab.text[ls..le];
                    if let Some(l) = editor::link_at(line, byte - ls) {
                        out.follow = Some((l, ctrl));
                    } else if let Some(t) = editor::tag_at(line, byte - ls) {
                        out.follow = Some((LinkRef::Tag(t), false));
                    }
                }
            }
        }

        // -- task checkbox: Ctrl+click or click in a rendered line toggles via palette not needed --

        // -- wikilink autocomplete ---------------------------------------------------------------
        let has_focus = ctx.memory(|m| m.has_focus(id));
        let mut popup_at = None;
        if has_focus {
            if let Some(r) = te.cursor_range {
                if r.primary == r.secondary {
                    let caret = r.primary.index.0;
                    match editor::wiki_context(&tab.text, caret) {
                        Some((start, q)) if tab.suggest.as_ref().map(|s| s.dismissed_at) != Some(Some(start)) => {
                            let items = suggestions(vault, &q);
                            let prev_sel = tab.suggest.as_ref().filter(|s| s.start == start).map(|s| s.sel).unwrap_or(0);
                            tab.suggest = Some(Suggest { start, sel: prev_sel.min(items.len().saturating_sub(1)), items, dismissed_at: None });
                            let r = galley.pos_from_cursor(r.primary).translate(gpos.to_vec2());
                            popup_at = Some(r.left_bottom());
                        }
                        Some((start, _)) => {
                            tab.suggest = Some(Suggest { start, sel: 0, items: vec![], dismissed_at: Some(start) });
                        }
                        None => tab.suggest = None,
                    }
                } else {
                    tab.suggest = None;
                }
            }
        } else {
            tab.suggest = None;
        }
        if let (Some(pos), Some(s)) = (popup_at, &tab.suggest) {
            if !s.items.is_empty() {
                let mut clicked = None;
                egui::Area::new(egui::Id::new("wiki-suggest")).fixed_pos(pos + vec2(0.0, 6.0)).order(egui::Order::Foreground).show(&ctx, |ui| {
                    Frame::popup(ui.style()).fill(with_alpha(th.bg_darker, 0.97)).show(ui, |ui| {
                        ui.set_min_width(260.0);
                        for (n, (_, disp)) in s.items.iter().enumerate() {
                            let sel = n == s.sel;
                            let r = ui.add(egui::Button::new(RichText::new(disp).color(if sel { th.fg_bright } else { th.fg })).fill(if sel { with_alpha(th.accent, 0.25) } else { Color32::TRANSPARENT }).frame(true).min_size(vec2(260.0, 22.0)));
                            if r.clicked() {
                                clicked = Some(n);
                            }
                        }
                    });
                });
                if let Some(n) = clicked {
                    out.apply_suggest = Some(n);
                }
            }
        }

        // -- GPU caret -----------------------------------------------------------------------------
        if has_focus {
            if let Some(r) = te.cursor_range {
                let c = galley.pos_from_cursor(r.primary);
                let pos = gpos + c.min.to_vec2();
                let cw = {
                    let nxt = galley.pos_from_cursor(CCursor::new(r.primary.index.0 + 1));
                    if (nxt.min.y - c.min.y).abs() < 1.0 && nxt.min.x > c.min.x { nxt.min.x - c.min.x } else { size * 0.6 }
                };
                let h = c.height().max(size);
                let target = match cfg.cursor_shape {
                    CursorShape::Bar => Rect::from_min_size(pos, vec2(2.0, h)),
                    CursorShape::Block => Rect::from_min_size(pos, vec2(cw, h)),
                    CursorShape::Underline => Rect::from_min_size(pos + vec2(0.0, h - 2.0), vec2(cw, 2.0)),
                };
                let dt = ctx.input(|i| i.stable_dt).min(0.05);
                let moving = fx.step(target, dt);
                let idle = fx.since_move();
                // Blink for a few seconds after activity, then hold solid so an idle editor sleeps.
                let blinking = cfg.cursor_blink && idle < 8.0;
                let blink_on = !blinking || ((idle / 0.53) as u32) % 2 == 0 || idle < 0.15;
                fx.paint(ui.painter(), viewport, cfg, th.cursor_rgb(), blink_on);
                if moving || cfg.cursor_continuous {
                    ctx.request_repaint();
                } else if blinking && ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
                    let into = (idle / 0.53).fract() * 0.53;
                    ctx.request_repaint_after(Duration::from_secs_f32((0.53 - into).max(0.02) + 0.005));
                }
            }
        }
    }

    if let Some(n) = out.apply_suggest.take() {
        let t = app.tab_mut().unwrap();
        apply_suggestion(&ctx, id, t, n);
    }
    if let Some((l, new_tab)) = out.follow {
        app.follow_editor_link(l, new_tab);
    }
    if out.rename {
        app.run_cmd(&ctx, crate::cmds::Cmd::RenameNote);
    }
    let _ = (Align2::LEFT_TOP, FontFamily::Monospace);
}

fn suggestions(vault: &Vault, q: &str) -> Vec<(String, String)> {
    let mut scored: Vec<(i32, String, String)> = vec![];
    for f in vault.files.iter().filter(|f| matches!(f.kind, FileKind::Note | FileKind::Image | FileKind::Pdf)) {
        let name = f.stem();
        if let Some(s) = fuzzy(q, name) {
            let shown = if f.rel.contains('/') && f.kind == FileKind::Note { format!("{name}   ·  {}", f.rel.rsplit_once('/').map(|x| x.0).unwrap_or("")) } else { name.to_string() };
            scored.push((s + if f.kind == FileKind::Note { 5 } else { 0 }, vault.link_name(f), shown));
        } else if let Some(m) = vault.meta.get(&f.rel) {
            if let Some(a) = m.aliases.iter().find(|a| fuzzy(q, a).is_some()) {
                scored.push((fuzzy(q, a).unwrap() - 5, format!("{}|{}", vault.link_name(f), a), format!("{a}  →  {name}")));
            }
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(8).map(|(_, i, d)| (i, d)).collect()
}

fn apply_suggestion(ctx: &egui::Context, id: egui::Id, tab: &mut Tab, n: usize) {
    let Some(s) = &tab.suggest else { return };
    let Some((insert, _)) = s.items.get(n).cloned() else { return };
    let start = s.start;
    let Some(sel) = editor::load_sel(ctx, id) else { return };
    let (a, b) = (editor::byte_of(&tab.text, start), editor::byte_of(&tab.text, sel.head));
    tab.text.replace_range(a..b, &insert);
    let mut caret = start + insert.chars().count();
    let cb = editor::byte_of(&tab.text, caret);
    if tab.text[cb..].starts_with("]]") {
        caret += 2;
    } else {
        tab.text.insert_str(cb, "]]");
        caret += 2;
    }
    editor::store_sel(ctx, id, Sel::caret(caret));
    tab.suggest = None;
    tab.dirty = true;
    tab.last_edit = Some(std::time::Instant::now());
    tab.galley_cache = None;
    tab.focus_editor = true;
}

// ---------------------------------------------------------------------------------------------
// Find / replace bar
// ---------------------------------------------------------------------------------------------

fn find_matches(text: &str, q: &str) -> Vec<(usize, usize)> {
    if q.is_empty() {
        return vec![];
    }
    let (t, q) = (text.to_ascii_lowercase(), q.to_ascii_lowercase());
    let mut out = vec![];
    let mut from = 0;
    while let Some(p) = t[from..].find(&q) {
        let a = from + p;
        out.push((editor::char_of(text, a), editor::char_of(text, a + q.len())));
        from = a + q.len().max(1);
    }
    out
}

fn find_bar(ui: &mut Ui, tab: &mut Tab, ctx: &egui::Context, ed_id: egui::Id, th: &crate::theme::Theme) {
    let fid = egui::Id::new(("find-input", tab.id));
    let last_q_id = egui::Id::new(("find-last", tab.id));
    let matches = find_matches(&tab.text, &tab.find.query);
    let mut select: Option<usize> = None;
    let mut close = false;
    let mut replace_one = false;
    let mut replace_all = false;

    // Query edits jump to the nearest match after the caret.
    let last: String = ctx.data(|d| d.get_temp(last_q_id)).unwrap_or_default();
    if last != tab.find.query {
        ctx.data_mut(|d| d.insert_temp(last_q_id, tab.find.query.clone()));
        if !matches.is_empty() {
            let caret = editor::load_sel(ctx, ed_id).map(|s| s.lo()).unwrap_or(0);
            let i = matches.iter().position(|m| m.0 >= caret).unwrap_or(0);
            tab.find.idx = i;
            select = Some(i);
        }
    }

    Frame::new().fill(with_alpha(th.bg_darker, 0.9)).inner_margin(Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let te = ui.add(egui::TextEdit::singleline(&mut tab.find.query).id(fid).hint_text("Find").desired_width(220.0));
            if tab.find.focus {
                te.request_focus();
                tab.find.focus = false;
            }
            if te.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let back = ui.input(|i| i.modifiers.shift);
                if !matches.is_empty() {
                    tab.find.idx = if back { (tab.find.idx + matches.len() - 1) % matches.len() } else { (tab.find.idx + 1) % matches.len() };
                    select = Some(tab.find.idx);
                }
                te.request_focus();
            }
            if te.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
                close = true;
            }
            let label = if tab.find.query.is_empty() { String::new() } else if matches.is_empty() { "No results".into() } else { format!("{} of {}", tab.find.idx.min(matches.len() - 1) + 1, matches.len()) };
            ui.label(RichText::new(label).size(12.0).color(th.fg_dim));
            if icon_button(ui, Icon::Up, "Previous match", false, th).clicked() && !matches.is_empty() {
                tab.find.idx = (tab.find.idx + matches.len() - 1) % matches.len();
                select = Some(tab.find.idx);
            }
            if icon_button(ui, Icon::Down, "Next match", false, th).clicked() && !matches.is_empty() {
                tab.find.idx = (tab.find.idx + 1) % matches.len();
                select = Some(tab.find.idx);
            }
            if icon_button(ui, Icon::Replace, "Replace", tab.find.show_replace, th).clicked() {
                tab.find.show_replace = !tab.find.show_replace;
            }
            if icon_button(ui, Icon::Close, "Close (Esc)", false, th).clicked() {
                close = true;
            }
        });
        if tab.find.show_replace {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut tab.find.replace).hint_text("Replace with").desired_width(220.0));
                if ui.button("Replace").clicked() {
                    replace_one = true;
                }
                if ui.button("All").clicked() {
                    replace_all = true;
                }
            });
        }
    });

    if replace_one && !matches.is_empty() {
        let (a, b) = matches[tab.find.idx.min(matches.len() - 1)];
        let (ba, bb) = (editor::byte_of(&tab.text, a), editor::byte_of(&tab.text, b));
        tab.text.replace_range(ba..bb, &tab.find.replace);
        tab.dirty = true;
        tab.last_edit = Some(std::time::Instant::now());
        tab.galley_cache = None;
    }
    if replace_all && !matches.is_empty() {
        for &(a, b) in matches.iter().rev() {
            let (ba, bb) = (editor::byte_of(&tab.text, a), editor::byte_of(&tab.text, b));
            tab.text.replace_range(ba..bb, &tab.find.replace);
        }
        tab.dirty = true;
        tab.last_edit = Some(std::time::Instant::now());
        tab.galley_cache = None;
    }
    if let Some(i) = select {
        let (a, b) = matches[i];
        editor::store_sel(ctx, ed_id, Sel { anchor: a, head: b });
        tab.scroll_to_char = Some(a);
    }
    if close {
        tab.find.open = false;
        tab.focus_editor = true;
        tab.galley_cache = None;
    }
    let _: Option<Find> = None;
}

/// Char index at which each line starts.
fn line_starts(text: &str) -> Vec<usize> {
    let mut v = Vec::with_capacity(text.len() / 32 + 1);
    let mut c = 0;
    for line in text.split('\n') {
        v.push(c);
        c += line.chars().count() + 1;
    }
    v
}

/// Maps char indices to the vertical extent of their galley row (galley space).
struct RowTable {
    starts: Vec<usize>,
    tops: Vec<f32>,
    bots: Vec<f32>,
}

impl RowTable {
    fn new(g: &egui::Galley) -> Self {
        let mut t = RowTable { starts: Vec::with_capacity(g.rows.len()), tops: Vec::with_capacity(g.rows.len()), bots: Vec::with_capacity(g.rows.len()) };
        let mut c = 0usize;
        for pr in &g.rows {
            t.starts.push(c);
            t.tops.push(pr.min_y());
            t.bots.push(pr.max_y());
            c += pr.char_count_including_newline().0;
        }
        t
    }

    fn y_of(&self, ch: usize) -> (f32, f32) {
        if self.starts.is_empty() {
            return (0.0, 0.0);
        }
        let i = match self.starts.binary_search(&ch) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        (self.tops[i], self.bots[i])
    }
}

/// Backgrounds drawn under the text: code blocks, callout cards, quote bars.
fn deco_shapes(decos: &[editor::Deco], starts: &[usize], rows: &RowTable, gpos: egui::Pos2, col: f32, live: bool, th: &crate::theme::Theme) -> Vec<egui::Shape> {
    let mut out = vec![];
    for d in decos {
        if d.last >= starts.len() {
            continue;
        }
        let last_line_end = if d.last + 1 < starts.len() { starts[d.last + 1].saturating_sub(1) } else { usize::MAX / 2 };
        let top = rows.y_of(starts[d.first]).0;
        let bot = rows.y_of(last_line_end).1;
        let x0 = gpos.x - 12.0;
        let x1 = gpos.x + col + 12.0;
        let rect = Rect::from_min_max(egui::pos2(x0, gpos.y + top - 4.0), egui::pos2(x1, gpos.y + bot + 4.0));
        match &d.kind {
            editor::DecoKind::Code(_) => out.push(egui::Shape::rect_filled(rect, 6.0, with_alpha(th.bg_darker, 0.55))),
            editor::DecoKind::Callout(k) if live => {
                let (c, _) = crate::reader::callout_style(k, th);
                out.push(egui::Shape::rect_filled(rect, 6.0, with_alpha(c, 0.10)));
            }
            editor::DecoKind::Quote if live => {
                let r = Rect::from_min_max(egui::pos2(gpos.x - 10.0, gpos.y + top), egui::pos2(gpos.x - 7.0, gpos.y + bot));
                out.push(egui::Shape::rect_filled(r, 1.5, with_alpha(th.accent, 0.6)));
            }
            _ => {}
        }
    }
    out
}

fn text_w(ui: &Ui, s: &str, font: &FontId) -> f32 {
    ui.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x)
}

/// Everything Live preview draws *on top of* hidden source: tables, properties, rules, bullets,
/// checkboxes and code-language labels. Lines/blocks that contain the caret stay raw.
#[allow(clippy::too_many_arguments)]
fn paint_overlays(
    ui: &Ui,
    galley: &egui::Galley,
    gpos: egui::Pos2,
    text: &str,
    starts: &[usize],
    rows: &RowTable,
    plan: &[editor::BlockPlan],
    decos: &[editor::Deco],
    active: (usize, usize),
    col: f32,
    th: &crate::theme::Theme,
    size: f32,
) {
    let clip = ui.clip_rect();
    let painter = ui.painter();
    let intersects = |first: usize, last: usize| active.0 <= last && active.1 >= first;
    let line_y = |ln: usize| -> (f32, f32) {
        let (t, b) = rows.y_of(starts[ln]);
        (gpos.y + t, gpos.y + b)
    };

    // ---- planned blocks -------------------------------------------------------------------
    for b in plan {
        if intersects(b.first, b.last) || b.first >= starts.len() {
            continue;
        }
        let (y, _) = line_y(b.first);
        let rect = Rect::from_min_size(egui::pos2(gpos.x, y), vec2(col, b.height));
        if !rect.intersects(clip) {
            continue;
        }
        match &b.kind {
            editor::BlockKind::Rule => {
                let yy = rect.center().y;
                painter.hline(rect.x_range(), yy, Stroke::new(1.5, with_alpha(th.fg, 0.22)));
            }
            editor::BlockKind::Props(props) => paint_props(ui, rect, props, th, size),
            editor::BlockKind::Table(t) => paint_table(ui, rect, t, th, size),
        }
    }

    // ---- code-language labels ---------------------------------------------------------------
    for d in decos {
        if let editor::DecoKind::Code(lang) = &d.kind {
            if lang.is_empty() || intersects(d.first, d.last) || d.first >= starts.len() {
                continue;
            }
            let (y, _) = line_y(d.first);
            if y > clip.bottom() || y < clip.top() - 40.0 {
                continue;
            }
            painter.text(egui::pos2(gpos.x + col + 4.0, y + 3.0), Align2::RIGHT_TOP, lang, FontId::proportional(size * 0.72), with_alpha(th.fg_dim, 0.9));
        }
    }

    // ---- bullets and checkboxes on inactive list lines ---------------------------------------
    let hidden = |ln: usize| -> bool {
        plan.iter().any(|b| b.first <= ln && ln <= b.last && !intersects(b.first, b.last))
            || decos.iter().any(|d| matches!(d.kind, editor::DecoKind::Code(_)) && d.first <= ln && ln <= d.last)
    };
    for (ln, line) in text.split('\n').enumerate() {
        if ln >= starts.len() {
            break;
        }
        if ln >= active.0 && ln <= active.1 {
            continue;
        }
        let t = line.trim_start();
        if !t.starts_with(['-', '*', '+']) {
            continue;
        }
        let (y0, y1) = line_y(ln);
        if y1 < clip.top() {
            continue;
        }
        if y0 > clip.bottom() {
            break;
        }
        let ind = line.len() - t.len();
        let Some(m) = editor::list_marker(t) else { continue };
        if m.ordered.is_some() || hidden(ln) {
            continue;
        }
        let ind_chars = line[..ind].chars().count();
        let ci = starts[ln] + ind_chars;
        let a = galley.pos_from_cursor(CCursor::new(ci));
        let b = galley.pos_from_cursor(CCursor::new(ci + 1));
        let cx = gpos.x + (a.min.x + b.min.x.max(a.min.x + 4.0)) / 2.0;
        let cy = (y0 + y1) / 2.0 + size * 0.03;
        let depth = line[..ind].chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>() / 2;
        if let Some(r) = editor::task_box(line) {
            let c0 = starts[ln] + line[..r.start].chars().count();
            let p0 = galley.pos_from_cursor(CCursor::new(c0));
            let p1 = galley.pos_from_cursor(CCursor::new(c0 + 3));
            let center = egui::pos2(gpos.x + (p0.min.x + p1.min.x) / 2.0, cy);
            let bx = Rect::from_center_size(center, Vec2::splat(size * 0.92));
            if line[r.start + 1..r.start + 2] != *" " {
                painter.rect_filled(bx, 4.0, th.accent);
                let s = bx.width() * 0.22;
                painter.line(vec![center + vec2(-s * 1.3, 0.0), center + vec2(-s * 0.3, s), center + vec2(s * 1.4, -s)], Stroke::new(1.8, th.bg));
            } else {
                painter.rect_stroke(bx, 4.0, Stroke::new(1.3, with_alpha(th.fg, 0.55)), egui::StrokeKind::Inside);
            }
        } else {
            if depth % 2 == 0 {
                painter.circle_filled(egui::pos2(cx, cy), size * 0.13, th.fg_dim);
            } else {
                painter.circle_stroke(egui::pos2(cx, cy), size * 0.12, Stroke::new(1.3, th.fg_dim));
            }
        }
    }
}

fn paint_table(ui: &Ui, rect: Rect, t: &editor::Table, th: &crate::theme::Theme, size: f32) {
    let painter = ui.painter();
    let ncols = t.head.len().max(t.rows.iter().map(|r| r.len()).max().unwrap_or(0)).max(1);
    let rh = editor::table_row_h(size);
    let pad = 14.0;
    let body = FontId::new(size, FontFamily::Proportional);
    let head = FontId::new(size, crate::fonts::bold());
    let mut widths = vec![60.0f32; ncols];
    for (c, w) in widths.iter_mut().enumerate() {
        if let Some(h) = t.head.get(c) {
            *w = w.max(text_w(ui, h, &head) + pad * 2.0);
        }
        for r in &t.rows {
            if let Some(cell) = r.get(c) {
                *w = w.max(text_w(ui, cell, &body) + pad * 2.0);
            }
        }
    }
    let total: f32 = widths.iter().sum();
    if total > rect.width() {
        let k = rect.width() / total;
        widths.iter_mut().for_each(|w| *w *= k);
    }
    let total: f32 = widths.iter().sum();
    let nrows = t.rows.len() + 1;
    let tbl = Rect::from_min_size(rect.min + vec2(0.0, 2.0), vec2(total, nrows as f32 * rh));
    painter.rect_filled(Rect::from_min_size(tbl.min, vec2(total, rh)), CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, with_alpha(th.fg, 0.07));
    let line = Stroke::new(1.0, with_alpha(th.fg, 0.16));
    painter.rect_stroke(tbl, 6.0, line, egui::StrokeKind::Inside);
    for r in 1..nrows {
        painter.hline(tbl.x_range(), tbl.top() + r as f32 * rh, line);
    }
    let mut x = tbl.left();
    for (c, w) in widths.iter().enumerate() {
        if c > 0 {
            painter.vline(x, tbl.y_range(), line);
        }
        let align = t.aligns.get(c).copied().unwrap_or(-1);
        for r in 0..nrows {
            let (txt, font, color) = if r == 0 { (t.head.get(c), &head, th.fg_bright) } else { (t.rows[r - 1].get(c), &body, th.fg) };
            let Some(txt) = txt else { continue };
            let cell = Rect::from_min_size(egui::pos2(x, tbl.top() + r as f32 * rh), vec2(*w, rh));
            let p = painter.with_clip_rect(cell.shrink2(vec2(2.0, 0.0)).intersect(ui.clip_rect()));
            let (px, anchor) = match align {
                0 => (cell.center().x, Align2::CENTER_CENTER),
                1 => (cell.right() - pad, Align2::RIGHT_CENTER),
                _ => (cell.left() + pad, Align2::LEFT_CENTER),
            };
            p.text(egui::pos2(px, cell.center().y), anchor, txt, font.clone(), color);
        }
        x += w;
    }
}

fn paint_props(ui: &Ui, rect: Rect, props: &[(String, crate::vault::PropValue)], th: &crate::theme::Theme, size: f32) {
    let painter = ui.painter();
    let panel = Rect::from_min_size(rect.min + vec2(0.0, 2.0), vec2(rect.width(), rect.height() - 8.0));
    painter.rect_filled(panel, 8.0, with_alpha(th.bg_darker, 0.4));
    painter.rect_stroke(panel, 8.0, Stroke::new(1.0, with_alpha(th.fg, 0.08)), egui::StrokeKind::Inside);
    let rh = editor::prop_row_h(size);
    let key_font = FontId::proportional(size * 0.9);
    let val_font = FontId::proportional(size * 0.95);
    let chip_font = FontId::proportional(size * 0.85);
    let vx = panel.left() + (panel.width() * 0.22).clamp(90.0, 160.0);
    for (i, (k, v)) in props.iter().enumerate() {
        let cy = panel.top() + 12.0 + rh * (i as f32 + 0.5);
        painter.text(egui::pos2(panel.left() + 14.0, cy), Align2::LEFT_CENTER, k, key_font.clone(), th.fg_dim);
        match v {
            crate::vault::PropValue::Text(t) => {
                painter.text(egui::pos2(vx, cy), Align2::LEFT_CENTER, t, val_font.clone(), th.fg);
            }
            crate::vault::PropValue::List(items) => {
                let mut x = vx;
                let is_tag = k.eq_ignore_ascii_case("tags") || k.eq_ignore_ascii_case("tag");
                for it in items {
                    let label = if is_tag { format!("#{}", it.trim_start_matches('#')) } else { it.clone() };
                    let w = text_w(ui, &label, &chip_font) + 16.0;
                    let chip = Rect::from_center_size(egui::pos2(x + w / 2.0, cy), vec2(w, rh * 0.74));
                    painter.rect_filled(chip, rh * 0.37, with_alpha(th.accent, 0.16));
                    painter.text(chip.center(), Align2::CENTER_CENTER, label, chip_font.clone(), th.accent);
                    x += w + 6.0;
                }
            }
        }
    }
}
