//! Sidebars: file explorer, vault search, tags; backlinks, outgoing links, outline.

use crate::app::{App, LeftTab, Overlay, Prompt, PromptKind, RightTab, TabKind, TreeRow};
use crate::cmds::Cmd;
use crate::config::ViewMode;
use crate::theme::with_alpha;
use crate::icons::{self, Icon};
use crate::ui::icon_button;
use crate::vault::FileKind;
use eframe::egui::{
    self, text::LayoutJob, text::TextFormat, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Layout, Margin, RichText, ScrollArea, Sense,
    Stroke, Ui,
};
use std::collections::HashMap;

const ROW_H: f32 = 24.0;

pub fn left_panel(app: &mut App, ui: &mut Ui) {
    let th = app.theme.clone();
    let ctx = ui.ctx().clone();

    // Footer first (so it keeps its space), then the tab strip, then the body.
    egui::Panel::bottom("left-footer").exact_size(34.0).show_separator_line(false).frame(egui::Frame::new().inner_margin(Margin::symmetric(8, 4))).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            let name = app.vault.as_ref().map(|v| v.name.clone()).unwrap_or_default();
            let g = ui.painter().layout_no_wrap(name.clone(), FontId::proportional(13.0), th.fg);
            let (rect, r) = ui.allocate_exact_size(vec2(g.size().x + 54.0, 26.0), Sense::click());
            let r = r.on_hover_text("Switch vault (Ctrl+Shift+V)").on_hover_cursor(CursorIcon::PointingHand);
            if r.hovered() {
                ui.painter().rect_filled(rect, CornerRadius::same(5), with_alpha(th.fg, 0.08));
            }
            icons::paint(ui.painter(), egui::pos2(rect.left() + 14.0, rect.center().y), 14.0, Icon::Vault, th.fg_dim);
            ui.painter().galley(egui::pos2(rect.left() + 28.0, rect.center().y - g.size().y / 2.0), g.clone(), th.fg);
            icons::paint(ui.painter(), egui::pos2(rect.left() + 34.0 + g.size().x, rect.center().y), 12.0, Icon::ChevronDown, th.fg_dim);
            if r.clicked() {
                app.overlay = Overlay::Vaults;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, Icon::Gear, "Settings (Ctrl+,)", false, &th).clicked() {
                    app.overlay = Overlay::Settings;
                }
            });
        });
    });

    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(0.0);
        for (tab, glyph, tip) in [(LeftTab::Files, Icon::Folder, "Files"), (LeftTab::Search, Icon::Search, "Search (Ctrl+Shift+F)"), (LeftTab::Tags, Icon::Hash, "Tags")] {
            if icon_button(ui, glyph, tip, app.left_tab == tab, &th).clicked() {
                app.left_tab = tab;
                if tab == LeftTab::Search {
                    app.search.focus = true;
                }
            }
        }
        {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                if icon_button(ui, Icon::SidebarLeft, "Collapse file browser (Ctrl+\\)", true, &th).clicked() {
                    app.cfg.left_sidebar = false;
                }
                if app.left_tab != LeftTab::Files {
                    return;
                }
                if icon_button(ui, Icon::Collapse, "Collapse all", false, &th).clicked() {
                    app.expanded.clear();
                    app.expanded_ver += 1;
                }
                if icon_button(ui, Icon::FolderPlus, "New folder", false, &th).clicked() {
                    app.run_cmd(&ctx, Cmd::NewFolder);
                }
                if icon_button(ui, Icon::FilePlus, "New note (Ctrl+N)", false, &th).clicked() {
                    app.run_cmd(&ctx, Cmd::NewNote);
                }
            });
        }
    });
    ui.add_space(2.0);

    match app.left_tab {
        LeftTab::Files => file_tree(app, ui),
        LeftTab::Search => search_panel(app, ui),
        LeftTab::Tags => tags_panel(app, ui),
    }
}

// ---------------------------------------------------------------------------------------------
// File explorer
// ---------------------------------------------------------------------------------------------

fn rebuild_tree(app: &mut App) {
    let Some(v) = &app.vault else { return };
    let key = (v.fs_version, app.expanded_ver);
    if key == app.tree_key {
        return;
    }
    app.tree_key = key;
    let mut kids: HashMap<&str, (Vec<&str>, Vec<usize>)> = HashMap::new();
    for d in &v.dirs {
        let parent = d.rsplit_once('/').map(|x| x.0).unwrap_or("");
        kids.entry(parent).or_default().0.push(d);
    }
    for (i, f) in v.files.iter().enumerate() {
        let parent = f.rel.rsplit_once('/').map(|x| x.0).unwrap_or("");
        kids.entry(parent).or_default().1.push(i);
    }
    fn walk(v: &crate::vault::Vault, kids: &HashMap<&str, (Vec<&str>, Vec<usize>)>, dir: &str, depth: usize, expanded: &std::collections::HashSet<String>, out: &mut Vec<TreeRow>) {
        let Some((dirs, files)) = kids.get(dir) else { return };
        for d in dirs {
            let open = expanded.contains(*d);
            out.push(TreeRow { rel: d.to_string(), name: d.rsplit('/').next().unwrap_or(d).to_string(), depth, is_dir: true, open, kind: None });
            if open {
                walk(v, kids, d, depth + 1, expanded, out);
            }
        }
        for &i in files {
            let f = &v.files[i];
            out.push(TreeRow { rel: f.rel.clone(), name: f.name.clone(), depth, is_dir: false, open: false, kind: Some(f.kind) });
        }
    }
    let mut rows = vec![];
    walk(v, &kids, "", 0, &app.expanded, &mut rows);
    app.tree_rows = rows;
}

fn file_tree(app: &mut App, ui: &mut Ui) {
    rebuild_tree(app);
    let th = app.theme.clone();
    let rows = app.tree_rows.clone();
    let current = app.current_rel();
    let mut open: Option<(String, bool)> = None;
    let mut toggle: Option<String> = None;
    let mut mv: Option<(String, String)> = None;
    let mut overlay: Option<Overlay> = None;
    let mut cmd: Option<Cmd> = None;
    let mut reveal: Option<String> = None;
    let mut copy: Option<String> = None;

    let avail_h = ui.available_height();
    let out = ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            let row = &rows[i];
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
            let selected = !row.is_dir && current.as_deref() == Some(row.rel.as_str());
            let mut hl_drop = false;
            if row.is_dir {
                if let Some(p) = resp.dnd_hover_payload::<String>() {
                    hl_drop = *p != row.rel && !p.starts_with(&format!("{}/", row.rel));
                }
                if let Some(p) = resp.dnd_release_payload::<String>() {
                    let name = p.rsplit('/').next().unwrap_or(&p);
                    if *p != row.rel && !row.rel.starts_with(&format!("{p}/")) {
                        mv = Some(((*p).clone(), format!("{}/{}", row.rel, name)));
                    }
                }
            }
            resp.dnd_set_drag_payload(row.rel.clone());
            let r = rect.shrink2(vec2(4.0, 1.0));
            if selected {
                ui.painter().rect_filled(r, CornerRadius::same(5), with_alpha(th.accent, 0.22));
            } else if hl_drop {
                ui.painter().rect_filled(r, CornerRadius::same(5), with_alpha(th.accent, 0.28));
            } else if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(5), with_alpha(th.fg, 0.07));
            }
            let x = rect.left() + 10.0 + row.depth as f32 * 14.0;
            let mid = rect.center().y;
            if row.is_dir {
                icons::paint(ui.painter(), egui::pos2(x + 5.0, mid), 12.0, if row.open { Icon::ChevronDown } else { Icon::ChevronRight }, th.fg_dim);
            }
            let (label, col) = if row.is_dir {
                (row.name.clone(), th.fg)
            } else if row.kind == Some(FileKind::Note) {
                (row.name.trim_end_matches(".md").to_string(), if selected { th.fg_bright } else { th.fg })
            } else {
                (row.name.clone(), th.fg_dim)
            };
            let tx = x + if row.is_dir { 16.0 } else { 4.0 };
            let job = {
                let mut j = LayoutJob::simple_singleline(label, FontId::proportional(13.5), col);
                j.wrap.max_width = (rect.right() - tx - 6.0).max(20.0);
                j.wrap.max_rows = 1;
                j.wrap.break_anywhere = true;
                j
            };
            let g = ui.painter().layout_job(job);
            ui.painter().galley(egui::pos2(tx, mid - g.size().y / 2.0), g, col);

            if resp.clicked() {
                if row.is_dir {
                    toggle = Some(row.rel.clone());
                } else {
                    open = Some((row.rel.clone(), ui.input(|i| i.modifiers.command)));
                }
            }
            if resp.middle_clicked() && !row.is_dir {
                open = Some((row.rel.clone(), true));
            }
            resp.context_menu(|ui| {
                let dir_for_new = if row.is_dir { row.rel.clone() } else { row.rel.rsplit_once('/').map(|x| x.0.to_string()).unwrap_or_default() };
                if !row.is_dir {
                    if ui.button("Open in new tab").clicked() {
                        open = Some((row.rel.clone(), true));
                        ui.close();
                    }
                    ui.separator();
                }
                if ui.button("New note").clicked() {
                    overlay = Some(Overlay::Prompt(Prompt { kind: PromptKind::NewNote { dir: dir_for_new.clone() }, input: String::new(), focus: true }));
                    ui.close();
                }
                if ui.button("New folder").clicked() {
                    overlay = Some(Overlay::Prompt(Prompt { kind: PromptKind::NewFolder { dir: dir_for_new.clone() }, input: String::new(), focus: true }));
                    ui.close();
                }
                ui.separator();
                if ui.button("Rename…").clicked() {
                    let input = if row.kind == Some(FileKind::Note) { row.name.trim_end_matches(".md").to_string() } else { row.name.clone() };
                    overlay = Some(Overlay::Prompt(Prompt { kind: PromptKind::Rename { rel: row.rel.clone() }, input, focus: true }));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    overlay = Some(Overlay::ConfirmDelete(row.rel.clone()));
                    ui.close();
                }
                ui.separator();
                if ui.button("Copy path").clicked() {
                    copy = Some(row.rel.clone());
                    ui.close();
                }
                if ui.button("Show in system file manager").clicked() {
                    reveal = Some(row.rel.clone());
                    ui.close();
                }
            });
        }
    });

    // Drop on the empty area below the rows → move to vault root.
    let used = out.content_size.y.min(avail_h);
    let rest = (avail_h - used).max(0.0);
    if rest > 4.0 {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), rest), Sense::click());
        let _ = rect;
        if let Some(p) = resp.dnd_release_payload::<String>() {
            let name = p.rsplit('/').next().unwrap_or(&p).to_string();
            if p.contains('/') {
                mv = Some(((*p).clone(), name));
            }
        }
        resp.context_menu(|ui| {
            if ui.button("New note").clicked() {
                overlay = Some(Overlay::Prompt(Prompt { kind: PromptKind::NewNote { dir: String::new() }, input: String::new(), focus: true }));
                ui.close();
            }
            if ui.button("New folder").clicked() {
                overlay = Some(Overlay::Prompt(Prompt { kind: PromptKind::NewFolder { dir: String::new() }, input: String::new(), focus: true }));
                ui.close();
            }
        });
    }

    if let Some(rel) = toggle {
        if !app.expanded.remove(&rel) {
            app.expanded.insert(rel);
        }
        app.expanded_ver += 1;
        app.session_dirty = true;
    }
    if let Some((rel, new_tab)) = open {
        app.open_path(&rel, new_tab, None);
    }
    if let Some((a, b)) = mv {
        app.move_to(&a, &b);
    }
    if let Some(o) = overlay {
        app.overlay = o;
    }
    if let Some(c) = cmd.take() {
        let ctx = ui.ctx().clone();
        app.run_cmd(&ctx, c);
    }
    if let (Some(rel), Some(v)) = (reveal, &app.vault) {
        let p = v.abs(&rel);
        let _ = open::that_detached(if p.is_dir() { p.clone() } else { p.parent().unwrap_or(&v.root).to_path_buf() });
    }
    if let (Some(rel), Some(v)) = (copy, &app.vault) {
        ui.ctx().copy_text(v.abs(&rel).to_string_lossy().to_string());
        app.toast("Path copied");
    }
    // Keep the selected file visible when it's revealed.
    let _ = &th;
}

// ---------------------------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------------------------

fn highlight_job(line: &str, terms: &[String], th: &crate::theme::Theme, case: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::proportional(12.5);
    let base = TextFormat { font_id: font.clone(), color: th.fg_dim, ..Default::default() };
    let hay: String = if case { line.to_string() } else { line.to_ascii_lowercase() };
    let mut marks: Vec<(usize, usize)> = vec![];
    for t in terms {
        let t = if case { t.clone() } else { t.to_ascii_lowercase() };
        if t.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(p) = hay[from..].find(&t) {
            marks.push((from + p, from + p + t.len()));
            from += p + t.len();
        }
    }
    marks.sort();
    let mut pos = 0;
    for (a, b) in marks {
        if a < pos || !line.is_char_boundary(a) || !line.is_char_boundary(b) {
            continue;
        }
        job.append(&line[pos..a], 0.0, base.clone());
        job.append(&line[a..b], 0.0, TextFormat { color: th.fg_bright, background: with_alpha(th.yellow, 0.35), ..base.clone() });
        pos = b;
    }
    job.append(&line[pos..], 0.0, base);
    job.wrap.max_width = f32::INFINITY;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job
}

fn search_panel(app: &mut App, ui: &mut Ui) {
    let th = app.theme.clone();
    let mut open: Option<(String, Option<usize>)> = None;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let w = ui.available_width() - 44.0;
        let te = ui.add(egui::TextEdit::singleline(&mut app.search.query).hint_text("Search…  (file: path: tag:)").desired_width(w));
        if app.search.focus {
            te.request_focus();
            app.search.focus = false;
        }
        let case_btn = egui::Button::new(RichText::new("Aa").size(12.0).color(if app.search.case { th.fg_bright } else { th.fg_dim }))
            .fill(if app.search.case { with_alpha(th.accent, 0.25) } else { Color32::TRANSPARENT })
            .min_size(vec2(26.0, 24.0));
        if ui.add(case_btn).on_hover_text("Match case").clicked() {
            app.search.case = !app.search.case;
            app.search.last_run.clear();
        }
    });
    if app.search.query != app.search.last_run {
        app.run_search();
    }
    ui.add_space(4.0);
    let terms: Vec<String> = crate::search::parse_query(&app.search.query, app.search.case).terms;
    let status = if app.search.query.trim().is_empty() {
        String::new()
    } else if app.search.running {
        format!("{} results…", app.search.results.len())
    } else {
        format!("{} result{}", app.search.results.len(), if app.search.results.len() == 1 { "" } else { "s" })
    };
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(status).size(11.5).color(th.fg_dim));
    });
    let case = app.search.case;
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for hit in &app.search.results {
            let stem = hit.rel.rsplit('/').next().unwrap_or(&hit.rel).trim_end_matches(".md").to_string();
            let folder = hit.rel.rsplit_once('/').map(|x| x.0).unwrap_or("");
            ui.add_space(4.0);
            let r = ui.horizontal(|ui| {
                ui.add_space(10.0);
                let r = ui.add(egui::Label::new(RichText::new(stem).size(13.5).color(th.fg_bright)).sense(Sense::click()).truncate()).on_hover_cursor(CursorIcon::PointingHand);
                if !folder.is_empty() {
                    ui.label(RichText::new(folder).size(11.0).color(th.fg_dim));
                }
                r
            });
            if r.inner.clicked() {
                open = Some((hit.rel.clone(), None));
            }
            for (ln, text) in &hit.lines {
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    let job = highlight_job(text, &terms, &th, case);
                    let l = ui.add(egui::Label::new(job).sense(Sense::click())).on_hover_cursor(CursorIcon::PointingHand);
                    if l.clicked() {
                        open = Some((hit.rel.clone(), Some(*ln)));
                    }
                });
            }
        }
    });
    if let Some((rel, line)) = open {
        app.open_path(&rel, false, None);
        if let (Some(l), Some(t)) = (line, app.tab_mut()) {
            if t.kind == TabKind::Note {
                if t.view == ViewMode::Reading {
                    t.view = ViewMode::Live;
                }
                t.goto_line = Some(l);
            }
        }
    }
}

fn tags_panel(app: &mut App, ui: &mut Ui) {
    let th = app.theme.clone();
    let tags = app.vault.as_ref().map(|v| v.tags()).unwrap_or_default();
    let mut pick: Option<String> = None;
    if tags.is_empty() {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(RichText::new("No tags found").color(th.fg_dim));
        });
    }
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (tag, notes) in &tags {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
            let r = rect.shrink2(vec2(4.0, 1.0));
            if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(5), with_alpha(th.fg, 0.07));
            }
            ui.painter().text(egui::pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, format!("#{tag}"), FontId::proportional(13.5), th.accent);
            ui.painter().text(egui::pos2(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, notes.len().to_string(), FontId::proportional(12.0), th.fg_dim);
            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                pick = Some(tag.clone());
            }
        }
    });
    if let Some(t) = pick {
        app.search.query = format!("tag:#{t}");
        app.left_tab = LeftTab::Search;
    }
}

// ---------------------------------------------------------------------------------------------
// Right sidebar
// ---------------------------------------------------------------------------------------------

pub fn right_panel(app: &mut App, ui: &mut Ui) {
    let th = app.theme.clone();
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing.x = 2.0;
        for (tab, glyph, tip) in [(RightTab::Backlinks, Icon::Link, "Backlinks"), (RightTab::Outgoing, Icon::Outgoing, "Outgoing links"), (RightTab::Outline, Icon::Outline, "Outline")] {
            if icon_button(ui, glyph, tip, app.right_tab == tab, &th).clicked() {
                app.right_tab = tab;
            }
        }
    });
    ui.add_space(2.0);
    let Some(rel) = app.current_rel() else { return };
    if app.tab().is_none_or(|t| t.kind != TabKind::Note) {
        return;
    }
    let mut open: Option<(String, Option<usize>)> = None;
    let mut goto_heading: Option<(String, usize)> = None;
    match app.right_tab {
        RightTab::Backlinks => {
            let links = app.backlinks_for(&rel);
            section_title(ui, &th, &format!("Linked mentions ({})", links.len()));
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if links.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(RichText::new("No backlinks found").color(th.fg_dim));
                    });
                }
                let mut last = String::new();
                for (src, l, ctx_line) in &links {
                    if *src != last {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            let name = src.rsplit('/').next().unwrap_or(src).trim_end_matches(".md");
                            if ui.add(egui::Label::new(RichText::new(name).color(th.fg_bright).size(13.5)).sense(Sense::click()).truncate()).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                open = Some((src.clone(), None));
                            }
                        });
                        last = src.clone();
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        let job = highlight_job(ctx_line, &[], &th, true);
                        if ui.add(egui::Label::new(job).sense(Sense::click())).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            open = Some((src.clone(), Some(l.line)));
                        }
                    });
                }
            });
        }
        RightTab::Outgoing => {
            let items: Vec<(String, Option<String>)> = app
                .vault
                .as_ref()
                .and_then(|v| v.meta.get(&rel).map(|m| (v, m)))
                .map(|(v, m)| {
                    let mut seen = std::collections::HashSet::new();
                    m.links
                        .iter()
                        .filter(|l| !l.target.is_empty() && seen.insert(l.target.clone()))
                        .map(|l| (l.target.clone(), v.resolve(&l.target, &rel).map(|f| f.rel.clone())))
                        .collect()
                })
                .unwrap_or_default();
            section_title(ui, &th, &format!("Outgoing links ({})", items.len()));
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (target, resolved) in &items {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        let col = if resolved.is_some() { th.fg } else { th.fg_dim };
                        let r = ui.add(egui::Label::new(RichText::new(target).color(col).size(13.5)).sense(Sense::click()).truncate()).on_hover_cursor(CursorIcon::PointingHand);
                        if resolved.is_none() {
                            ui.label(RichText::new("not created").size(11.0).color(th.red));
                        }
                        if r.clicked() {
                            open = Some((resolved.clone().unwrap_or_else(|| format!("\0{target}")), None));
                        }
                    });
                }
            });
        }
        RightTab::Outline => {
            let heads = app.outline();
            section_title(ui, &th, "Outline");
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if heads.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(RichText::new("No headings found").color(th.fg_dim));
                    });
                }
                for h in &heads {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), CornerRadius::same(4), with_alpha(th.fg, 0.07));
                    }
                    let x = rect.left() + 12.0 + (h.level.saturating_sub(1)) as f32 * 14.0;
                    ui.painter().text(egui::pos2(x, rect.center().y), Align2::LEFT_CENTER, &h.text, FontId::proportional(13.0), if h.level == 1 { th.fg_bright } else { th.fg });
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        goto_heading = Some((h.text.clone(), h.line));
                    }
                }
            });
        }
    }
    if let Some((r, line)) = open {
        if let Some(name) = r.strip_prefix('\0') {
            let name = name.to_string();
            app.open_link(&name, None, false);
        } else {
            app.open_path(&r, false, None);
            if let (Some(l), Some(t)) = (line, app.tab_mut()) {
                if t.kind == TabKind::Note {
                    if t.view == ViewMode::Reading {
                        t.view = ViewMode::Live;
                    }
                    t.goto_line = Some(l);
                }
            }
        }
    }
    if let Some((text, line)) = goto_heading {
        if let Some(t) = app.tab_mut() {
            if t.view == ViewMode::Reading {
                t.scroll_heading = Some(text);
            } else {
                t.goto_line = Some(line);
            }
        }
    }
    let _ = Stroke::NONE;
    let _: Option<Color32> = None;
}

fn section_title(ui: &mut Ui, th: &crate::theme::Theme, t: &str) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(t).size(11.5).color(th.fg_dim));
    });
    ui.add_space(2.0);
}
