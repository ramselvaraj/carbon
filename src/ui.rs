//! Window chrome: panels, tab bar, status bar. Content views live in `ui_editor`, `ui_side`, `ui_overlay`.

use crate::app::{App, TabKind};
use crate::cmds::Cmd;
use crate::config::ViewMode;
use crate::editor;
use crate::icons::{self, Icon};
use crate::theme::with_alpha;
use eframe::egui::{
    self, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Layout, Margin, Panel, Rect, RichText, ScrollArea, Sense, Stroke,
    Ui,
};

pub fn draw(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    app.handle_shortcuts(&ctx);

    if app.vault.is_none() {
        crate::ui_overlay::welcome(app, ui);
        crate::ui_overlay::overlays(app, &ctx);
        return;
    }

    let th = app.theme.clone();
    let op = app.cfg.background_opacity;

    // ---- status bar ------------------------------------------------------------------------
    Panel::bottom("status")
        .exact_size(26.0)
        .show_separator_line(false)
        .frame(Frame::new().fill(with_alpha(th.bg_dark, op)).inner_margin(Margin::symmetric(10, 0)))
        .show(ui, |ui| status_bar(app, ui));

    // ---- left sidebar ----------------------------------------------------------------------
    if app.cfg.left_sidebar {
        let r = Panel::left("left")
            .resizable(true)
            .default_size(app.cfg.left_width)
            .size_range(170.0..=520.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(with_alpha(th.bg_dark, op)).inner_margin(Margin::ZERO))
            .show(ui, |ui| crate::ui_side::left_panel(app, ui));
        app.cfg.left_width = r.response.rect.width();
    }

    // ---- right sidebar ---------------------------------------------------------------------
    if app.cfg.right_sidebar {
        let r = Panel::right("right")
            .resizable(true)
            .default_size(app.cfg.right_width)
            .size_range(170.0..=520.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(with_alpha(th.bg_dark, op)).inner_margin(Margin::ZERO))
            .show(ui, |ui| crate::ui_side::right_panel(app, ui));
        app.cfg.right_width = r.response.rect.width();
    }

    // ---- main area -------------------------------------------------------------------------
    egui::CentralPanel::default().frame(Frame::new().fill(with_alpha(th.bg, op))).show(ui, |ui| {
        tab_bar(app, ui);
        crate::ui_editor::main_view(app, ui);
    });

    toast(app, &ctx);
    crate::ui_overlay::overlays(app, &ctx);
}

pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str, selected: bool, th: &crate::theme::Theme) -> egui::Response {
    icons::button(ui, icon, tip, selected, th)
}

fn toast(app: &mut App, ctx: &egui::Context) {
    let Some((msg, t0)) = &app.toast else { return };
    if t0.elapsed().as_secs_f32() > 3.0 {
        app.toast = None;
        return;
    }
    let th = &app.theme;
    egui::Area::new(egui::Id::new("toast")).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -44.0)).order(egui::Order::Tooltip).interactable(false).show(ctx, |ui| {
        Frame::new()
            .fill(with_alpha(th.bg_darker, 0.96))
            .stroke(Stroke::new(1.0, with_alpha(th.fg, 0.15)))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(14, 8))
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(msg).color(th.fg_bright)).wrap_mode(egui::TextWrapMode::Extend));
            });
    });
}

// ---------------------------------------------------------------------------------------------
// Tab bar
// ---------------------------------------------------------------------------------------------

fn tab_bar(app: &mut App, ui: &mut Ui) {
    let th = app.theme.clone();
    let op = app.cfg.background_opacity;
    let bar_h = 36.0;
    let (bar_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), bar_h), Sense::hover());
    ui.painter().rect_filled(bar_rect, 0.0, with_alpha(th.bg_darker, op));
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(bar_rect.shrink2(vec2(6.0, 4.0))).layout(Layout::left_to_right(Align::Center)));

    // File browser toggle (also Ctrl+\\).
    if icon_button(&mut bar, Icon::SidebarLeft, if app.cfg.left_sidebar { "Collapse file browser (Ctrl+\\)" } else { "Show file browser (Ctrl+\\)" }, app.cfg.left_sidebar, &th).clicked() {
        app.cfg.left_sidebar = !app.cfg.left_sidebar;
    }
    bar.add_space(4.0);
    // Back / forward.
    let (can_back, can_fwd) = app.tab().map(|t| (t.hpos > 0, t.hpos + 1 < t.history.len())).unwrap_or((false, false));
    bar.add_enabled_ui(can_back, |ui| {
        if icon_button(ui, Icon::Back, "Navigate back (Alt+Left)", false, &th).clicked() {
            app.nav(true);
        }
    });
    bar.add_enabled_ui(can_fwd, |ui| {
        if icon_button(ui, Icon::Forward, "Navigate forward (Alt+Right)", false, &th).clicked() {
            app.nav(false);
        }
    });
    bar.add_space(4.0);

    let right_w = 118.0;
    let tabs_w = (bar.available_width() - right_w).max(60.0);
    let mut close: Option<usize> = None;
    let mut activate: Option<usize> = None;
    let mut reorder: Option<(usize, usize)> = None;
    let mut new_tab = false;
    let mut cmds: Vec<Cmd> = vec![];

    bar.allocate_ui_with_layout(vec2(tabs_w, 28.0), Layout::left_to_right(Align::Center), |ui| {
        ScrollArea::horizontal().id_salt("tabs-scroll").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let n = app.tabs.len();
            for i in 0..n {
                let (title, pinned, dirty, kind_empty) = {
                    let t = &app.tabs[i];
                    (t.title(), t.pinned, t.dirty, t.kind == TabKind::Empty)
                };
                let active = i == app.active;
                let font = FontId::proportional(13.0);
                let galley = ui.painter().layout_no_wrap(title.clone(), font.clone(), th.fg);
                let w = if pinned { 36.0 } else { (galley.size().x + 44.0).clamp(96.0, 200.0) };
                let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click_and_drag());
                let hovered = resp.hovered();
                let bg = if active {
                    with_alpha(th.bg, op.max(0.5))
                } else if hovered {
                    with_alpha(th.fg, 0.07)
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
                if active {
                    ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, with_alpha(th.fg, 0.08)), egui::StrokeKind::Inside);
                }
                let col = if active { th.fg_bright } else if kind_empty { th.fg_dim } else { th.fg };
                // Right-hand slot: close button / dirty dot / pin.
                let slot = Rect::from_center_size(rect.right_center() - vec2(14.0, 0.0), vec2(18.0, 18.0));
                let text_max = rect.width() - if pinned { 0.0 } else { 38.0 };
                if pinned {
                    icons::paint(ui.painter(), rect.center(), 13.0, Icon::Pin, th.fg_dim);
                } else {
                    let job = {
                        let mut j = egui::text::LayoutJob::simple_singleline(title.clone(), font.clone(), col);
                        j.wrap.max_width = text_max;
                        j.wrap.max_rows = 1;
                        j.wrap.break_anywhere = true;
                        j
                    };
                    let g = ui.painter().layout_job(job);
                    let pos = egui::pos2(rect.left() + 12.0, rect.center().y - g.size().y / 2.0);
                    ui.painter().galley(pos, g, col);
                    let slot_hover = ui.rect_contains_pointer(slot);
                    if hovered || active {
                        if slot_hover {
                            ui.painter().rect_filled(slot, CornerRadius::same(4), with_alpha(th.fg, 0.14));
                        }
                        icons::paint(ui.painter(), slot.center(), 11.0, Icon::Close, if slot_hover { th.fg_bright } else { th.fg_dim });
                    } else if dirty {
                        ui.painter().circle_filled(slot.center(), 3.0, th.accent);
                    }
                    if resp.clicked() && slot_hover {
                        close = Some(i);
                    } else if resp.clicked() {
                        activate = Some(i);
                    }
                }
                if pinned && resp.clicked() {
                    activate = Some(i);
                }
                if resp.middle_clicked() && !pinned {
                    close = Some(i);
                }
                // Reorder by dragging.
                resp.dnd_set_drag_payload(i);
                if let Some(from) = resp.dnd_hover_payload::<usize>() {
                    if *from != i {
                        let x = if *from < i { rect.right() } else { rect.left() };
                        ui.painter().vline(x, rect.y_range(), Stroke::new(2.0, th.accent));
                    }
                }
                if let Some(from) = resp.dnd_release_payload::<usize>() {
                    reorder = Some((*from, i));
                }
                resp.context_menu(|ui| {
                    if ui.button("Close").clicked() {
                        close = Some(i);
                        ui.close();
                    }
                    if ui.button("Close others").clicked() {
                        activate = Some(i);
                        cmds.push(Cmd::CloseTab); // handled below using sentinel
                        ui.close();
                    }
                    if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                        activate = Some(i);
                        cmds.push(Cmd::PinTab);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Copy file path").clicked() {
                        activate = Some(i);
                        cmds.push(Cmd::CopyPath);
                        ui.close();
                    }
                    if ui.button("Show in file explorer").clicked() {
                        activate = Some(i);
                        cmds.push(Cmd::RevealInFiles);
                        ui.close();
                    }
                });
            }
            if icon_button(ui, Icon::Plus, "New tab (Ctrl+T)", false, &th).clicked() {
                new_tab = true;
            }
        });
    });

    // Right-hand controls.
    bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if icon_button(ui, Icon::SidebarRight, "Toggle right sidebar", app.cfg.right_sidebar, &th).clicked() {
            app.cfg.right_sidebar = !app.cfg.right_sidebar;
        }
        if app.tab().is_some_and(|t| t.kind == TabKind::Note) {
            let reading = app.tab().map(|t| t.view == ViewMode::Reading).unwrap_or(false);
            if icon_button(ui, if reading { Icon::Pencil } else { Icon::Book }, if reading { "Switch to editing (Ctrl+E)" } else { "Switch to reading (Ctrl+E)" }, false, &th).clicked() {
                cmds.push(Cmd::ToggleReading);
            }
        }
    });

    if let Some((from, to)) = reorder {
        if from != to && from < app.tabs.len() && to < app.tabs.len() {
            let active_id = app.tabs[app.active].id;
            let t = app.tabs.remove(from);
            app.tabs.insert(to, t);
            app.active = app.tabs.iter().position(|t| t.id == active_id).unwrap_or(0);
            app.session_dirty = true;
        }
    }
    if let Some(i) = activate {
        app.activate(i);
    }
    let ctx = ui.ctx().clone();
    for c in cmds {
        if c == Cmd::CloseTab {
            // "Close others" keeps the activated tab.
            let keep = app.tabs[app.active].id;
            let ids: Vec<u64> = app.tabs.iter().filter(|t| t.id != keep && !t.pinned).map(|t| t.id).collect();
            for id in ids {
                if let Some(p) = app.tabs.iter().position(|t| t.id == id) {
                    app.close_tab(p);
                }
            }
        } else {
            app.run_cmd(&ctx, c);
        }
    }
    if let Some(i) = close {
        app.close_tab(i);
    }
    if new_tab {
        app.new_tab();
    }
}

// ---------------------------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------------------------

fn status_bar(app: &mut App, ui: &mut Ui) {
    let th = app.theme.clone();
    let dim = |s: String| RichText::new(s).size(12.0).color(th.fg_dim);
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        // Left: indexing progress / shader problems.
        if app.index_rx.is_some() {
            ui.label(dim("Indexing…".into()));
        }
        if let Some(e) = app.fx.error() {
            let first = e.lines().next().unwrap_or("").to_string();
            ui.label(RichText::new(format!("cursor shader error: {first}")).size(12.0).color(th.red)).on_hover_text(e);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(t) = app.tab() {
                if t.kind == TabKind::Note {
                    let words = editor::count_words(&t.text);
                    let chars = t.text.chars().count();
                    ui.label(dim(format!("{words} words")));
                    ui.label(dim(format!("{chars} characters")));
                    ui.label(dim(format!("Ln {}, Col {}", t.caret.0, t.caret.1)));
                    let mode = match t.view {
                        ViewMode::Live => "Live preview",
                        ViewMode::Source => "Source",
                        ViewMode::Reading => "Reading",
                    };
                    let r = ui.add(egui::Label::new(dim(mode.to_string())).sense(Sense::click())).on_hover_cursor(CursorIcon::PointingHand);
                    if r.clicked() {
                        let next = match t.view {
                            ViewMode::Live => ViewMode::Source,
                            ViewMode::Source => ViewMode::Reading,
                            ViewMode::Reading => ViewMode::Live,
                        };
                        let ctx = ui.ctx().clone();
                        app.run_cmd(&ctx, match next {
                            ViewMode::Live => Cmd::ViewLive,
                            ViewMode::Source => Cmd::ViewSource,
                            ViewMode::Reading => Cmd::ViewReading,
                        });
                    }
                    if let Some(rel) = app.current_rel() {
                        let n = app.backlinks_for(&rel).len();
                        let r = ui.add(egui::Label::new(dim(format!("{n} backlink{}", if n == 1 { "" } else { "s" }))).sense(Sense::click())).on_hover_cursor(CursorIcon::PointingHand);
                        if r.clicked() {
                            let ctx = ui.ctx().clone();
                            app.run_cmd(&ctx, Cmd::ShowBacklinks);
                        }
                    }
                }
            }
        });
    });
}
