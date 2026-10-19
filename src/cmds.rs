//! Every user-invokable command: palette entries, shortcuts and their implementations.

use crate::app::{App, LeftTab, Overlay, Prompt, PromptKind, RightTab, TabKind};
use crate::config::ViewMode;
use crate::editor::{self, LinePrefix, Sel};
use eframe::egui::{self, Key, Modifiers};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    NewNote,
    NewFolder,
    NewTab,
    CloseTab,
    ReopenTab,
    NextTab,
    PrevTab,
    QuickSwitcher,
    Palette,
    SearchVault,
    FindInNote,
    ReplaceInNote,
    ToggleLeft,
    ToggleRight,
    Back,
    Forward,
    ViewLive,
    ViewSource,
    ViewReading,
    ToggleReading,
    Save,
    OpenVault,
    SwitchVault,
    Settings,
    Bold,
    Italic,
    Strike,
    Highlight,
    InlineCode,
    CodeBlock,
    InsertLink,
    InsertWikilink,
    Heading(usize),
    Bullet,
    Numbered,
    Task,
    Quote,
    Indent,
    Outdent,
    RenameNote,
    DeleteNote,
    RevealInFiles,
    OpenInFileManager,
    CopyPath,
    PinTab,
    ShowFiles,
    ShowBacklinks,
    ShowOutline,
    ShowOutgoing,
    ShowTags,
    PasteImage,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    OpenCursorShader,
}

pub const ALL: &[(Cmd, &str, &str)] = &[
    (Cmd::QuickSwitcher, "Open quick switcher", "Ctrl+O"),
    (Cmd::Palette, "Open command palette", "Ctrl+P"),
    (Cmd::NewNote, "Create new note", "Ctrl+N"),
    (Cmd::NewFolder, "Create new folder", ""),
    (Cmd::NewTab, "New tab", "Ctrl+T"),
    (Cmd::CloseTab, "Close current tab", "Ctrl+W"),
    (Cmd::ReopenTab, "Reopen closed tab", "Ctrl+Shift+T"),
    (Cmd::NextTab, "Go to next tab", "Ctrl+Tab"),
    (Cmd::PrevTab, "Go to previous tab", "Ctrl+Shift+Tab"),
    (Cmd::PinTab, "Pin / unpin tab", ""),
    (Cmd::Back, "Navigate back", "Alt+Left"),
    (Cmd::Forward, "Navigate forward", "Alt+Right"),
    (Cmd::SearchVault, "Search in all files", "Ctrl+Shift+F"),
    (Cmd::FindInNote, "Find in current note", "Ctrl+F"),
    (Cmd::ReplaceInNote, "Find and replace in current note", "Ctrl+H"),
    (Cmd::ToggleLeft, "Toggle left sidebar", "Ctrl+\\"),
    (Cmd::ToggleRight, "Toggle right sidebar", "Ctrl+Shift+\\"),
    (Cmd::ShowFiles, "Show file explorer", ""),
    (Cmd::ShowTags, "Show tags", ""),
    (Cmd::ShowBacklinks, "Show backlinks", ""),
    (Cmd::ShowOutgoing, "Show outgoing links", ""),
    (Cmd::ShowOutline, "Show outline", ""),
    (Cmd::ToggleReading, "Toggle reading / editing view", "Ctrl+E"),
    (Cmd::ViewLive, "Editing view: Live preview", ""),
    (Cmd::ViewSource, "Editing view: Source mode", ""),
    (Cmd::ViewReading, "Reading view", ""),
    (Cmd::Save, "Save current note", "Ctrl+S"),
    (Cmd::Bold, "Toggle bold", "Ctrl+B"),
    (Cmd::Italic, "Toggle italic", "Ctrl+I"),
    (Cmd::Strike, "Toggle strikethrough", "Ctrl+Shift+X"),
    (Cmd::Highlight, "Toggle highlight", "Ctrl+Shift+H"),
    (Cmd::InlineCode, "Toggle inline code", ""),
    (Cmd::CodeBlock, "Insert code block", ""),
    (Cmd::InsertLink, "Insert Markdown link", "Ctrl+K"),
    (Cmd::InsertWikilink, "Insert [[wikilink]]", ""),
    (Cmd::Heading(1), "Heading 1", "Ctrl+1"),
    (Cmd::Heading(2), "Heading 2", "Ctrl+2"),
    (Cmd::Heading(3), "Heading 3", "Ctrl+3"),
    (Cmd::Heading(4), "Heading 4", ""),
    (Cmd::Bullet, "Toggle bullet list", "Ctrl+Shift+8"),
    (Cmd::Numbered, "Toggle numbered list", "Ctrl+Shift+7"),
    (Cmd::Task, "Toggle task list", "Ctrl+Shift+9"),
    (Cmd::Quote, "Toggle blockquote", "Ctrl+Shift+."),
    (Cmd::Indent, "Indent", "Tab"),
    (Cmd::Outdent, "Unindent", "Shift+Tab"),
    (Cmd::PasteImage, "Paste image / files from clipboard", ""),
    (Cmd::RenameNote, "Rename current file", "F2"),
    (Cmd::DeleteNote, "Delete current file", ""),
    (Cmd::RevealInFiles, "Reveal current file in explorer", ""),
    (Cmd::OpenInFileManager, "Show in system file manager", ""),
    (Cmd::CopyPath, "Copy file path", ""),
    (Cmd::SwitchVault, "Switch vault", "Ctrl+Shift+V"),
    (Cmd::OpenVault, "Open folder as vault…", ""),
    (Cmd::Settings, "Open settings", "Ctrl+,"),
    (Cmd::OpenCursorShader, "Edit custom cursor shader", ""),
    (Cmd::ZoomIn, "Zoom in", "Ctrl+="),
    (Cmd::ZoomOut, "Zoom out", "Ctrl+-"),
    (Cmd::ZoomReset, "Reset zoom", "Ctrl+0"),
];

impl App {
    /// Global keyboard shortcuts. Runs before any widget sees input.
    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use Key::*;
        let c = Modifiers::COMMAND;
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let table: &[(Modifiers, Key, Cmd)] = &[
            (c, O, Cmd::QuickSwitcher),
            (c, P, Cmd::Palette),
            (c, N, Cmd::NewNote),
            (c, T, Cmd::NewTab),
            (c, W, Cmd::CloseTab),
            (cs, T, Cmd::ReopenTab),
            (c, Tab, Cmd::NextTab),
            (cs, Tab, Cmd::PrevTab),
            (c, PageDown, Cmd::NextTab),
            (c, PageUp, Cmd::PrevTab),
            (Modifiers::ALT, ArrowLeft, Cmd::Back),
            (Modifiers::ALT, ArrowRight, Cmd::Forward),
            (cs, F, Cmd::SearchVault),
            (c, F, Cmd::FindInNote),
            (c, H, Cmd::ReplaceInNote),
            (c, Backslash, Cmd::ToggleLeft),
            (cs, Backslash, Cmd::ToggleRight),
            (c, E, Cmd::ToggleReading),
            (c, S, Cmd::Save),
            (c, B, Cmd::Bold),
            (c, I, Cmd::Italic),
            (cs, X, Cmd::Strike),
            (cs, H, Cmd::Highlight),
            (c, K, Cmd::InsertLink),
            (c, Num1, Cmd::Heading(1)),
            (c, Num2, Cmd::Heading(2)),
            (c, Num3, Cmd::Heading(3)),
            (cs, Num8, Cmd::Bullet),
            (cs, Num7, Cmd::Numbered),
            (cs, Num9, Cmd::Task),
            (cs, Period, Cmd::Quote),
            (cs, V, Cmd::SwitchVault),
            (c, Comma, Cmd::Settings),
            (c, Equals, Cmd::ZoomIn),
            (c, Minus, Cmd::ZoomOut),
            (c, Num0, Cmd::ZoomReset),
            (Modifiers::NONE, F2, Cmd::RenameNote),
        ];
        // Escape closes whatever is on top.
        if !matches!(self.overlay, Overlay::None) && Self::consume(ctx, Modifiers::NONE, Escape) {
            self.overlay = Overlay::None;
            return;
        }
        let editing_cmd = |cmd: Cmd| {
            matches!(
                cmd,
                Cmd::Bold | Cmd::Italic | Cmd::Strike | Cmd::Highlight | Cmd::InsertLink | Cmd::Heading(_) | Cmd::Bullet | Cmd::Numbered | Cmd::Task | Cmd::Quote
            )
        };
        let overlay_open = !matches!(self.overlay, Overlay::None);
        for (m, k, cmd) in table {
            if overlay_open && !matches!(cmd, Cmd::Palette | Cmd::QuickSwitcher) {
                continue;
            }
            if editing_cmd(*cmd) && !self.tab().is_some_and(|t| t.kind == TabKind::Note) {
                continue;
            }
            // Number-row heading shortcuts conflict with tab switching; Alt+N switches tabs below.
            if Self::consume(ctx, *m, *k) {
                self.run_cmd(ctx, *cmd);
                return;
            }
        }
        // Alt+1..9 jump to a tab.
        for (n, k) in [Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9].into_iter().enumerate() {
            if !overlay_open && Self::consume(ctx, Modifiers::ALT, k) {
                let idx = if n == 8 { self.tabs.len().saturating_sub(1) } else { n };
                self.activate(idx);
                return;
            }
        }
        // Screenshots / images on the clipboard: egui hands them to us as `PasteImage` on Ctrl+V.
        let img = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::PasteImage(im) => Some(im.clone()),
                _ => None,
            })
        });
        if let Some(im) = img {
            let id = self.editor_id();
            if !overlay_open && self.tab().is_some_and(|t| t.kind == TabKind::Note && t.view != ViewMode::Reading) && ctx.memory(|m| m.has_focus(id)) {
                let mut rgba = Vec::with_capacity(im.pixels.len() * 4);
                for p in &im.pixels {
                    rgba.extend_from_slice(&p.to_srgba_unmultiplied());
                }
                self.paste_rgba(ctx, im.size[0] as u32, im.size[1] as u32, rgba);
            }
        }
        self.handle_dropped(ctx);
    }

    fn edit_active(&mut self, ctx: &egui::Context, f: impl FnOnce(&mut String, Sel) -> Sel) {
        let id = self.editor_id();
        let Some(t) = self.tabs.get_mut(self.active) else { return };
        if t.kind != TabKind::Note {
            return;
        }
        if t.view == ViewMode::Reading {
            t.view = ViewMode::Live;
        }
        let sel = editor::load_sel(ctx, id).unwrap_or(Sel::caret(t.text.chars().count()));
        let ns = f(&mut t.text, sel);
        editor::store_sel(ctx, id, ns);
        t.dirty = true;
        t.last_edit = Some(Instant::now());
        t.focus_editor = true;
        t.galley_cache = None;
    }

    pub fn run_cmd(&mut self, ctx: &egui::Context, cmd: Cmd) {
        match cmd {
            Cmd::NewNote => {
                let dir = self.new_note_dir();
                let name = self.vault.as_ref().map(|v| v.unique_rel(&dir, "Untitled", ".md")).unwrap_or_default();
                let stem = name.rsplit('/').next().unwrap_or("Untitled").trim_end_matches(".md").to_string();
                if self.vault.is_some() {
                    self.create_note_named(&dir, &stem);
                }
            }
            Cmd::NewFolder => {
                let dir = self.current_rel().and_then(|r| r.rsplit_once('/').map(|x| x.0.to_string())).unwrap_or_default();
                self.overlay = Overlay::Prompt(Prompt { kind: PromptKind::NewFolder { dir }, input: String::new(), focus: true });
            }
            Cmd::NewTab => self.new_tab(),
            Cmd::CloseTab => {
                let i = self.active;
                self.close_tab(i);
            }
            Cmd::ReopenTab => self.reopen_closed(),
            Cmd::NextTab => {
                if !self.tabs.is_empty() {
                    let n = (self.active + 1) % self.tabs.len();
                    self.activate(n);
                }
            }
            Cmd::PrevTab => {
                if !self.tabs.is_empty() {
                    let n = (self.active + self.tabs.len() - 1) % self.tabs.len();
                    self.activate(n);
                }
            }
            Cmd::QuickSwitcher => {
                self.overlay = if matches!(self.overlay, Overlay::Switcher { .. }) { Overlay::None } else { Overlay::Switcher { query: String::new(), sel: 0 } };
            }
            Cmd::Palette => {
                self.overlay = if matches!(self.overlay, Overlay::Palette { .. }) { Overlay::None } else { Overlay::Palette { query: String::new(), sel: 0 } };
            }
            Cmd::SearchVault => {
                self.cfg.left_sidebar = true;
                self.left_tab = LeftTab::Search;
                self.search.focus = true;
                // Seed with the current selection's text if any is cheap to get: keep previous query.
            }
            Cmd::FindInNote | Cmd::ReplaceInNote => {
                if let Some(t) = self.tab_mut() {
                    if t.kind == TabKind::Note {
                        if t.view == ViewMode::Reading {
                            t.view = ViewMode::Live;
                        }
                        t.find.open = true;
                        t.find.focus = true;
                        t.find.show_replace = cmd == Cmd::ReplaceInNote;
                    }
                }
            }
            Cmd::ToggleLeft => self.cfg.left_sidebar = !self.cfg.left_sidebar,
            Cmd::ToggleRight => self.cfg.right_sidebar = !self.cfg.right_sidebar,
            Cmd::Back => self.nav(true),
            Cmd::Forward => self.nav(false),
            Cmd::ViewLive => self.set_view(ViewMode::Live),
            Cmd::ViewSource => self.set_view(ViewMode::Source),
            Cmd::ViewReading => self.set_view(ViewMode::Reading),
            Cmd::ToggleReading => {
                let v = self.tab().map(|t| t.view);
                self.set_view(if v == Some(ViewMode::Reading) { self.cfg.default_view.max_edit() } else { ViewMode::Reading });
            }
            Cmd::Save => {
                self.flush_all();
                self.toast("Saved");
            }
            Cmd::OpenVault => self.pick_vault_dialog(),
            Cmd::SwitchVault => self.overlay = Overlay::Vaults,
            Cmd::Settings => self.overlay = Overlay::Settings,
            Cmd::Bold => self.edit_active(ctx, |t, s| editor::wrap_selection(t, s, "**", "**")),
            Cmd::Italic => self.edit_active(ctx, |t, s| editor::wrap_selection(t, s, "*", "*")),
            Cmd::Strike => self.edit_active(ctx, |t, s| editor::wrap_selection(t, s, "~~", "~~")),
            Cmd::Highlight => self.edit_active(ctx, |t, s| editor::wrap_selection(t, s, "==", "==")),
            Cmd::InlineCode => self.edit_active(ctx, |t, s| editor::wrap_selection(t, s, "`", "`")),
            Cmd::CodeBlock => self.edit_active(ctx, |t, s| editor::wrap_selection(t, s, "```\n", "\n```")),
            Cmd::InsertLink => self.edit_active(ctx, |t, s| {
                if s.empty() {
                    let ns = editor::wrap_selection(t, s, "[", "]()");
                    Sel::caret(ns.head)
                } else {
                    let ns = editor::wrap_selection(t, s, "[", "]()");
                    Sel::caret(ns.hi() + 2)
                }
            }),
            Cmd::InsertWikilink => self.edit_active(ctx, |t, s| editor::wrap_selection(t, s, "[[", "]]")),
            Cmd::Heading(n) => self.edit_active(ctx, move |t, s| editor::toggle_line_prefix(t, s, LinePrefix::Heading(n))),
            Cmd::Bullet => self.edit_active(ctx, |t, s| editor::toggle_line_prefix(t, s, LinePrefix::Bullet)),
            Cmd::Numbered => self.edit_active(ctx, |t, s| editor::toggle_line_prefix(t, s, LinePrefix::Number)),
            Cmd::Task => self.edit_active(ctx, |t, s| editor::toggle_line_prefix(t, s, LinePrefix::Task)),
            Cmd::Quote => self.edit_active(ctx, |t, s| editor::toggle_line_prefix(t, s, LinePrefix::Quote)),
            Cmd::Indent => self.edit_active(ctx, |t, s| editor::indent_lines(t, s, false)),
            Cmd::Outdent => self.edit_active(ctx, |t, s| editor::indent_lines(t, s, true)),
            Cmd::PasteImage => {
                if !self.paste_attachments(ctx) {
                    self.toast("No image or file on the clipboard");
                }
            }
            Cmd::RenameNote => {
                if let Some(rel) = self.current_rel() {
                    let name = rel.rsplit('/').next().unwrap_or(&rel);
                    let stem = if self.tab().is_some_and(|t| t.kind == TabKind::Note) { name.trim_end_matches(".md") } else { name };
                    self.overlay = Overlay::Prompt(Prompt { kind: PromptKind::Rename { rel: rel.clone() }, input: stem.to_string(), focus: true });
                }
            }
            Cmd::DeleteNote => {
                if let Some(rel) = self.current_rel() {
                    self.overlay = Overlay::ConfirmDelete(rel);
                }
            }
            Cmd::RevealInFiles => {
                if let Some(rel) = self.current_rel() {
                    self.cfg.left_sidebar = true;
                    self.left_tab = LeftTab::Files;
                    self.expand_to(&rel);
                }
            }
            Cmd::OpenInFileManager => {
                if let (Some(rel), Some(v)) = (self.current_rel(), &self.vault) {
                    let p = v.abs(&rel);
                    let _ = open::that_detached(p.parent().unwrap_or(&v.root));
                }
            }
            Cmd::CopyPath => {
                if let (Some(rel), Some(v)) = (self.current_rel(), &self.vault) {
                    ctx.copy_text(v.abs(&rel).to_string_lossy().to_string());
                    self.toast("Path copied");
                }
            }
            Cmd::PinTab => {
                if let Some(t) = self.tab_mut() {
                    t.pinned = !t.pinned;
                }
                self.session_dirty = true;
            }
            Cmd::ShowFiles => {
                self.cfg.left_sidebar = true;
                self.left_tab = LeftTab::Files;
            }
            Cmd::ShowTags => {
                self.cfg.left_sidebar = true;
                self.left_tab = LeftTab::Tags;
            }
            Cmd::ShowBacklinks => {
                self.cfg.right_sidebar = true;
                self.right_tab = RightTab::Backlinks;
            }
            Cmd::ShowOutgoing => {
                self.cfg.right_sidebar = true;
                self.right_tab = RightTab::Outgoing;
            }
            Cmd::ShowOutline => {
                self.cfg.right_sidebar = true;
                self.right_tab = RightTab::Outline;
            }
            Cmd::ZoomIn => {
                self.cfg.font_size = (self.cfg.font_size + 1.0).min(32.0);
                self.cfg.save();
            }
            Cmd::ZoomOut => {
                self.cfg.font_size = (self.cfg.font_size - 1.0).max(10.0);
                self.cfg.save();
            }
            Cmd::ZoomReset => {
                self.cfg.font_size = 16.0;
                self.cfg.save();
            }
            Cmd::OpenCursorShader => {
                self.cfg.cursor_effect = "custom".into();
                self.cfg.save();
                self.fx.reload(&self.cfg);
                let _ = open::that_detached(crate::cursor_fx::custom_shader_path());
                self.toast("Editing ~/.config/carbon/cursor.wgsl — saves hot-reload instantly");
            }
        }
    }

    fn set_view(&mut self, v: ViewMode) {
        if let Some(t) = self.tab_mut() {
            if t.kind == TabKind::Note {
                t.view = v;
                t.galley_cache = None;
                t.focus_editor = v != ViewMode::Reading;
            }
        }
        self.fx.snap();
        self.session_dirty = true;
    }
}

impl ViewMode {
    /// The editing mode to return to from Reading.
    pub fn max_edit(self) -> ViewMode {
        if self == ViewMode::Reading { ViewMode::Live } else { self }
    }
}
