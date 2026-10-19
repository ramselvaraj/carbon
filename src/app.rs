//! Application state and behaviour. Drawing lives in `ui.rs`.

use crate::config::{Config, Session, SessionTab, ViewMode};
use crate::cursor_fx::CursorFx;
use crate::editor::{self, LinkRef, Sel};
use crate::hot::{Hot, HotEvent};
use crate::reader::{Action, Doc, DocCache};
use crate::search::{self, Hit, SearchHandle};
use crate::theme::Theme;
use crate::vault::{self, FileKind, Heading, Link, NoteMeta, Vault};
use eframe::egui::{self, Key, Modifiers};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LeftTab {
    Files,
    Search,
    Tags,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Backlinks,
    Outgoing,
    Outline,
}

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub query: String,
    pub replace: String,
    pub show_replace: bool,
    pub focus: bool,
    pub idx: usize,
}

pub struct Suggest {
    pub start: usize,
    pub items: Vec<(String, String)>,
    pub sel: usize,
    pub dismissed_at: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    Empty,
    Note,
    Image,
    External,
}

pub struct Tab {
    pub id: u64,
    pub path: Option<String>,
    pub kind: TabKind,
    pub text: String,
    pub dirty: bool,
    pub last_edit: Option<Instant>,
    pub disk_mtime: Option<SystemTime>,
    pub view: ViewMode,
    pub history: Vec<String>,
    pub hpos: usize,
    pub pinned: bool,
    pub find: Find,
    pub scroll_heading: Option<String>,
    pub goto_line: Option<usize>,
    pub scroll_to_char: Option<usize>,
    pub doc: Option<(u64, Arc<Doc>)>,
    pub active_lines: (usize, usize),
    pub suggest: Option<Suggest>,
    pub galley_cache: Option<(u64, Arc<egui::Galley>)>,
    pub plan_cache: Option<(u64, Arc<Vec<crate::editor::BlockPlan>>, Arc<Vec<crate::editor::Deco>>)>,
    pub focus_editor: bool,
    pub caret: (usize, usize),
}

impl Tab {
    pub fn new(id: u64, view: ViewMode) -> Self {
        Tab {
            id,
            path: None,
            kind: TabKind::Empty,
            text: String::new(),
            dirty: false,
            last_edit: None,
            disk_mtime: None,
            view,
            history: vec![],
            hpos: 0,
            pinned: false,
            find: Find::default(),
            scroll_heading: None,
            goto_line: None,
            scroll_to_char: None,
            doc: None,
            active_lines: (0, 0),
            suggest: None,
            galley_cache: None,
            plan_cache: None,
            focus_editor: false,
            caret: (1, 1),
        }
    }

    pub fn title(&self) -> String {
        match &self.path {
            Some(p) => {
                let n = p.rsplit('/').next().unwrap_or(p);
                n.strip_suffix(".md").unwrap_or(n).to_string()
            }
            None => "New tab".into(),
        }
    }
}

pub enum PromptKind {
    NewNote { dir: String },
    NewFolder { dir: String },
    Rename { rel: String },
}

pub struct Prompt {
    pub kind: PromptKind,
    pub input: String,
    pub focus: bool,
}

pub enum Overlay {
    None,
    Switcher { query: String, sel: usize },
    Palette { query: String, sel: usize },
    Vaults,
    Settings,
    Prompt(Prompt),
    ConfirmDelete(String),
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub case: bool,
    pub results: Vec<Hit>,
    pub handle: Option<SearchHandle>,
    pub generation: Arc<AtomicU64>,
    pub focus: bool,
    pub running: bool,
    pub last_run: String,
}

#[derive(Clone)]
pub struct TreeRow {
    pub rel: String,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub open: bool,
    pub kind: Option<FileKind>,
}

pub enum DialogResult {
    Vault(PathBuf),
}

pub struct App {
    pub cfg: Config,
    pub theme: Theme,
    pub fx: CursorFx,
    pub hot: Hot,
    pub vault: Option<Vault>,
    pub index_rx: Option<Receiver<(String, NoteMeta)>>,
    pub watcher: Option<(RecommendedWatcher, Receiver<PathBuf>)>,
    pub fs_pending: Option<Instant>,
    pub fs_changed: HashSet<String>,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub next_tab_id: u64,
    pub closed: Vec<(String, ViewMode)>,
    pub recent: Vec<String>,
    pub left_tab: LeftTab,
    pub right_tab: RightTab,
    pub expanded: HashSet<String>,
    pub expanded_ver: u64,
    pub tree_rows: Vec<TreeRow>,
    pub tree_key: (u64, u64),
    pub overlay: Overlay,
    pub search: SearchState,
    pub toast: Option<(String, Instant)>,
    pub docs: DocCache,
    pub img_dims: HashMap<String, Option<(u32, u32)>>,
    pub backlinks_cache: Option<(String, u64, Vec<(String, Link, String)>)>,
    pub outline_cache: Option<(u64, Vec<Heading>)>,
    pub dialog_rx: Option<Receiver<DialogResult>>,
    pub clipboard: Option<arboard::Clipboard>,
    pub last_title: String,
    pub session_dirty: bool,
    pub ctx: egui::Context,
    pub obsidian_vaults: Vec<PathBuf>,
    /// Frames left in which cached text layouts must be rebuilt after a theme change
    /// (egui re-rasterises its font atlas one pass after the visuals change).
    pub theme_settle: u8,
}

pub fn hash_of<T: Hash + ?Sized>(t: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

/// Vaults Obsidian itself knows about (`~/.config/obsidian/obsidian.json`).
fn obsidian_known_vaults() -> Vec<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    let Some(base) = base else { return vec![] };
    let Ok(s) = std::fs::read_to_string(base.join("obsidian/obsidian.json")) else { return vec![] };
    let Ok(j) = serde_json::from_str::<serde_json::Value>(&s) else { return vec![] };
    let mut v: Vec<(u64, PathBuf)> = j
        .get("vaults")
        .and_then(|v| v.as_object())
        .map(|o| {
            o.values()
                .filter_map(|e| Some((e.get("ts").and_then(|t| t.as_u64()).unwrap_or(0), PathBuf::from(e.get("path")?.as_str()?))))
                .filter(|(_, p)| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|x| x.1).collect()
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let cfg = Config::load();
        let theme = Theme::load();
        theme.apply(&cc.egui_ctx, &cfg);
        cc.egui_ctx.set_zoom_factor(cfg.ui_scale);
        let mut app = App {
            fx: CursorFx::new(cc, &cfg),
            hot: Hot::new(cc.egui_ctx.clone()),
            cfg,
            theme,
            vault: None,
            index_rx: None,
            watcher: None,
            fs_pending: None,
            fs_changed: HashSet::new(),
            tabs: vec![],
            active: 0,
            next_tab_id: 1,
            closed: vec![],
            recent: vec![],
            left_tab: LeftTab::Files,
            right_tab: RightTab::Backlinks,
            expanded: HashSet::new(),
            expanded_ver: 0,
            tree_rows: vec![],
            tree_key: (u64::MAX, u64::MAX),
            overlay: Overlay::None,
            search: SearchState::default(),
            toast: None,
            docs: DocCache::default(),
            img_dims: HashMap::new(),
            backlinks_cache: None,
            outline_cache: None,
            dialog_rx: None,
            clipboard: None,
            last_title: String::new(),
            session_dirty: false,
            ctx: cc.egui_ctx.clone(),
            obsidian_vaults: obsidian_known_vaults(),
            theme_settle: 0,
        };
        // Optional `carbon /path/to/vault` argument, otherwise reopen the last vault.
        let arg = std::env::args().nth(1).map(PathBuf::from).filter(|p| p.is_dir());
        if let Some(p) = arg.or_else(|| app.cfg.recent_vaults.first().cloned().filter(|p| p.is_dir())) {
            app.open_vault(p);
        }
        app
    }

    // -----------------------------------------------------------------------------------------
    // Vaults
    // -----------------------------------------------------------------------------------------

    pub fn open_vault(&mut self, path: PathBuf) {
        self.flush_all();
        self.save_session();
        let path = path.canonicalize().unwrap_or(path);
        let vault = Vault::open(path.clone());
        let notes: Vec<String> = vault.files.iter().filter(|f| f.kind == FileKind::Note).map(|f| f.rel.clone()).collect();
        self.index_rx = Some(vault::spawn_index(path.clone(), notes));
        self.cfg.remember_vault(&path);

        // Watch the vault for outside changes (git sync, Obsidian itself, other editors).
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        let w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if ev.kind.is_access() {
                    return;
                }
                for p in ev.paths {
                    let _ = tx.send(p);
                }
                ctx.request_repaint();
            }
        });
        self.watcher = w.ok().and_then(|mut w| w.watch(&path, RecursiveMode::Recursive).ok().map(|_| (w, rx)));

        self.vault = Some(vault);
        self.tabs.clear();
        self.active = 0;
        self.closed.clear();
        self.recent.clear();
        self.search = SearchState::default();
        self.backlinks_cache = None;
        self.tree_key = (u64::MAX, u64::MAX);

        let sess = Session::load(&path);
        self.expanded = sess.expanded.into_iter().collect();
        self.expanded_ver += 1;
        let mut restored = 0;
        for st in &sess.tabs {
            let exists = self.vault.as_ref().is_some_and(|v| v.file(&st.path).is_some());
            if exists {
                self.open_path(&st.path, true, None);
                if let Some(t) = self.tabs.last_mut() {
                    t.pinned = st.pinned;
                    if let Some(v) = st.view {
                        t.view = v;
                    }
                }
                restored += 1;
            }
        }
        if restored > 0 {
            self.active = sess.active.min(self.tabs.len() - 1);
        } else {
            self.new_tab();
        }
        self.overlay = Overlay::None;
    }

    pub fn save_session(&self) {
        let Some(v) = &self.vault else { return };
        let sess = Session {
            tabs: self
                .tabs
                .iter()
                .filter_map(|t| t.path.as_ref().map(|p| SessionTab { path: p.clone(), pinned: t.pinned, view: Some(t.view) }))
                .collect(),
            active: self.active,
            expanded: self.expanded.iter().cloned().collect(),
        };
        sess.save(&v.root);
    }

    pub fn pick_vault_dialog(&mut self) {
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            if let Some(p) = rfd::FileDialog::new().set_title("Open folder as vault").pick_folder() {
                let _ = tx.send(DialogResult::Vault(p));
            }
            ctx.request_repaint();
        });
        self.dialog_rx = Some(rx);
    }

    // -----------------------------------------------------------------------------------------
    // Tabs and files
    // -----------------------------------------------------------------------------------------

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
        self.ctx.request_repaint_after(Duration::from_millis(3200));
    }

    pub fn tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    pub fn new_tab(&mut self) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(Tab::new(id, self.cfg.default_view));
        self.active = self.tabs.len() - 1;
        self.session_dirty = true;
    }

    pub fn activate(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.active = i;
            self.fx.snap();
            self.session_dirty = true;
            if self.tabs[i].view != ViewMode::Reading {
                self.tabs[i].focus_editor = true;
            }
        }
    }

    pub fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.save_tab(i);
        let t = self.tabs.remove(i);
        if let Some(p) = t.path {
            self.closed.push((p, t.view));
        }
        if self.tabs.is_empty() {
            self.new_tab();
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if i < self.active {
            self.active -= 1;
        }
        self.fx.snap();
        self.session_dirty = true;
    }

    pub fn reopen_closed(&mut self) {
        if let Some((p, v)) = self.closed.pop() {
            self.open_path(&p, true, None);
            if let Some(t) = self.tab_mut() {
                t.view = v;
            }
        }
    }

    /// Open `rel` — focus an existing tab, reuse the current one, or open a new tab.
    pub fn open_path(&mut self, rel: &str, new_tab: bool, heading: Option<String>) {
        let Some(vault) = &self.vault else { return };
        let Some(file) = vault.file(rel) else { return };
        let rel = file.rel.clone();
        let kind = file.kind;

        if kind == FileKind::Pdf || kind == FileKind::Media || kind == FileKind::Canvas {
            let _ = open::that_detached(vault.abs(&rel));
            return;
        }
        // Already open somewhere → just go there.
        if let Some(i) = self.tabs.iter().position(|t| t.path.as_deref() == Some(rel.as_str())) {
            self.activate(i);
            self.apply_target(heading);
            self.touch_recent(&rel);
            return;
        }
        let reuse = !new_tab
            && self.tabs.get(self.active).is_some_and(|t| !t.pinned)
            && !self.tabs.is_empty();
        if reuse {
            self.save_tab(self.active);
        } else {
            self.new_tab();
        }
        let i = self.active;
        self.load_into(i, &rel);
        // History: truncate forward entries, then push.
        let t = &mut self.tabs[i];
        t.history.truncate(t.hpos + usize::from(!t.history.is_empty()));
        t.history.push(rel.clone());
        t.hpos = t.history.len() - 1;
        self.apply_target(heading);
        self.touch_recent(&rel);
        self.fx.snap();
        self.session_dirty = true;
    }

    fn apply_target(&mut self, heading: Option<String>) {
        let Some(h) = heading.filter(|h| !h.is_empty()) else { return };
        let Some(t) = self.tab_mut() else { return };
        if let Some(rest) = h.strip_prefix('^') {
            let _ = rest; // block references: land on the note
            return;
        }
        if t.view == ViewMode::Reading {
            t.scroll_heading = Some(h);
        } else if let Some(line) = vault::parse_note(&t.text).headings.iter().find(|x| x.text.eq_ignore_ascii_case(h.trim())).map(|x| x.line) {
            t.goto_line = Some(line);
        }
    }

    fn touch_recent(&mut self, rel: &str) {
        self.recent.retain(|r| r != rel);
        self.recent.insert(0, rel.to_string());
        self.recent.truncate(50);
    }

    fn load_into(&mut self, i: usize, rel: &str) {
        let Some(vault) = &self.vault else { return };
        let abs = vault.abs(rel);
        let kind = vault.file(rel).map(|f| f.kind).unwrap_or(FileKind::Note);
        let default_view = self.cfg.default_view;
        let t = &mut self.tabs[i];
        t.path = Some(rel.to_string());
        t.dirty = false;
        t.doc = None;
        t.galley_cache = None;
        t.suggest = None;
        t.find = Find::default();
        t.scroll_heading = None;
        t.goto_line = None;
        t.disk_mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
        match kind {
            FileKind::Note => {
                t.kind = TabKind::Note;
                t.text = std::fs::read_to_string(&abs).unwrap_or_default();
                if t.view == ViewMode::Reading && t.history.is_empty() {
                    t.view = default_view;
                }
                if t.text.trim().is_empty() && t.view == ViewMode::Reading {
                    t.view = ViewMode::Live;
                }
                t.focus_editor = t.view != ViewMode::Reading;
                if t.view != ViewMode::Reading {
                    t.goto_line = Some(0);
                }
            }
            FileKind::Image => {
                t.kind = TabKind::Image;
                t.text.clear();
            }
            _ => {
                t.kind = TabKind::External;
                t.text.clear();
            }
        }
    }

    pub fn nav(&mut self, back: bool) {
        let i = self.active;
        let Some(t) = self.tabs.get_mut(i) else { return };
        let target = if back {
            if t.hpos == 0 {
                return;
            }
            t.hpos - 1
        } else {
            if t.hpos + 1 >= t.history.len() {
                return;
            }
            t.hpos + 1
        };
        let rel = t.history[target].clone();
        self.save_tab(i);
        self.load_into(i, &rel);
        self.tabs[i].hpos = target;
        self.fx.snap();
        self.session_dirty = true;
    }

    pub fn save_tab(&mut self, i: usize) {
        let Some(vault) = &mut self.vault else { return };
        let Some(t) = self.tabs.get_mut(i) else { return };
        if !t.dirty || t.kind != TabKind::Note {
            return;
        }
        let Some(rel) = t.path.clone() else { return };
        match std::fs::write(vault.abs(&rel), &t.text) {
            Ok(()) => {
                t.dirty = false;
                t.last_edit = None;
                t.disk_mtime = std::fs::metadata(vault.abs(&rel)).and_then(|m| m.modified()).ok();
                let meta = vault::parse_note(&t.text);
                vault.set_meta(rel, meta);
            }
            Err(e) => {
                let msg = format!("Couldn't save: {e}");
                self.toast(msg);
            }
        }
    }

    pub fn flush_all(&mut self) {
        for i in 0..self.tabs.len() {
            self.save_tab(i);
        }
    }

    pub fn current_rel(&self) -> Option<String> {
        self.tab().and_then(|t| t.path.clone())
    }

    // -----------------------------------------------------------------------------------------
    // Links
    // -----------------------------------------------------------------------------------------

    pub fn new_note_dir(&self) -> String {
        let Some(v) = &self.vault else { return String::new() };
        match v.cfg.new_file_location.as_deref() {
            Some("current") => self.current_rel().and_then(|r| r.rsplit_once('/').map(|x| x.0.to_string())).unwrap_or_default(),
            Some("folder") => v.cfg.new_file_folder.clone().unwrap_or_default().trim_matches('/').to_string(),
            _ => String::new(),
        }
    }

    pub fn open_link(&mut self, target: &str, heading: Option<String>, new_tab: bool) {
        let from = self.current_rel().unwrap_or_default();
        let Some(vault) = &self.vault else { return };
        if let Some(f) = vault.resolve(target, &from) {
            let rel = f.rel.clone();
            self.open_path(&rel, new_tab, heading);
            return;
        }
        // Unresolved: create the note (like Obsidian), next to the link's path if it has one.
        let (dir, stem) = match target.rsplit_once('/') {
            Some((d, s)) => (d.to_string(), s.to_string()),
            None => (self.new_note_dir(), target.to_string()),
        };
        if stem.trim().is_empty() {
            return;
        }
        let stem = stem.trim_end_matches(".md").to_string();
        let res = self.vault.as_mut().unwrap().create_note(&dir, &stem, "");
        match res {
            Ok(rel) => {
                self.open_path(&rel, new_tab, heading);
                self.toast(format!("Created {rel}"));
            }
            Err(e) => self.toast(format!("Couldn't create note: {e}")),
        }
    }

    pub fn handle_actions(&mut self, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Open { target, heading, new_tab } => self.open_link(&target, heading, new_tab),
                Action::Url(u) => {
                    let _ = open::that_detached(u);
                }
                Action::Tag(t) => {
                    self.left_tab = LeftTab::Search;
                    self.cfg.left_sidebar = true;
                    self.search.query = format!("tag:#{t}");
                    self.search.focus = false;
                }
                Action::ToggleTask(off) => {
                    if let Some(t) = self.tab_mut() {
                        if t.text.is_char_boundary(off) && (t.text[off..].starts_with(' ') || t.text[off..].starts_with('x') || t.text[off..].starts_with('X')) {
                            let new = if t.text[off..].starts_with(' ') { "x" } else { " " };
                            t.text.replace_range(off..off + 1, new);
                            t.dirty = true;
                            t.last_edit = Some(Instant::now());
                            t.doc = None;
                        }
                    }
                }
                Action::OpenFile(rel) => self.open_path(&rel, true, None),
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Attachments: paste screenshots and dropped files
    // -----------------------------------------------------------------------------------------

    /// Insert `![[file]]` (or markdown form) at the caret of the active note.
    fn insert_embeds(&mut self, ctx: &egui::Context, rels: Vec<String>) {
        let id = self.editor_id();
        let Some(vault) = &self.vault else { return };
        let Some(t) = self.tabs.get_mut(self.active) else { return };
        let Some(note) = t.path.clone() else { return };
        let mut ins = String::new();
        for (n, r) in rels.iter().enumerate() {
            if let Some(f) = vault.file(r) {
                let is_img = f.kind == FileKind::Image;
                if n > 0 {
                    ins.push('\n');
                }
                ins.push_str(&vault.make_link(f, &note, is_img, None));
            }
        }
        let sel = editor::load_sel(ctx, id).unwrap_or(Sel::caret(t.text.chars().count()));
        let (a, b) = (editor::byte_of(&t.text, sel.lo()), editor::byte_of(&t.text, sel.hi()));
        t.text.replace_range(a..b, &ins);
        editor::store_sel(ctx, id, Sel::caret(sel.lo() + ins.chars().count()));
        t.dirty = true;
        t.last_edit = Some(Instant::now());
    }

    pub fn editor_id(&self) -> egui::Id {
        match self.tab() {
            Some(t) => egui::Id::new(("editor", t.id, t.path.clone())),
            None => egui::Id::new("editor-none"),
        }
    }

    /// Ctrl+V with an image (or copied files) on the clipboard: store next to the note and link it.
    pub fn paste_attachments(&mut self, ctx: &egui::Context) -> bool {
        let Some(note) = self.current_rel() else { return false };
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        let Some(cb) = self.clipboard.as_mut() else { return false };
        // Text on the clipboard → leave it to the normal paste path.
        if cb.get_text().map(|t| !t.is_empty()).unwrap_or(false) {
            return false;
        }
        let mut saved: Vec<String> = vec![];
        if let Ok(img) = cb.get_image() {
            self.paste_rgba(ctx, img.width as u32, img.height as u32, img.bytes.into_owned());
            return true;
        } else if let Ok(files) = cb.get().file_list() {
            for f in files {
                saved.extend(self.import_file(&note, &f));
            }
        }
        if saved.is_empty() {
            return false;
        }
        let n = saved.len();
        self.insert_embeds(ctx, saved);
        self.toast(if n == 1 { "Pasted attachment".to_string() } else { format!("Pasted {n} attachments") });
        true
    }

    /// Encode raw RGBA as PNG, store it in the note's attachment folder and link it at the caret.
    pub fn paste_rgba(&mut self, ctx: &egui::Context, w: u32, h: u32, rgba: Vec<u8>) {
        let Some(note) = self.current_rel() else { return };
        let Some(buf) = image::RgbaImage::from_raw(w, h, rgba) else { return };
        let mut png = std::io::Cursor::new(Vec::new());
        if buf.write_to(&mut png, image::ImageFormat::Png).is_err() {
            return;
        }
        let name = format!("Pasted image {}.png", chrono::Local::now().format("%Y%m%d%H%M%S"));
        match self.vault.as_mut().unwrap().save_attachment(&note, &name, png.get_ref()) {
            Ok(rel) => {
                let dir = rel.rsplit_once('/').map(|x| x.0.to_string()).unwrap_or_default();
                self.insert_embeds(ctx, vec![rel]);
                self.toast(format!("Pasted image to {}", if dir.is_empty() { "vault root".to_string() } else { dir }));
            }
            Err(e) => self.toast(format!("Couldn't save image: {e}")),
        }
    }

    fn import_file(&mut self, note: &str, path: &std::path::Path) -> Option<String> {
        let vault = self.vault.as_mut()?;
        // Files already inside the vault are linked in place.
        if let Some(rel) = vault.rel_of(path) {
            if vault.file(&rel).is_some() {
                return Some(rel);
            }
        }
        let name = path.file_name()?.to_string_lossy().to_string();
        vault::kind_of(&name)?;
        let bytes = std::fs::read(path).ok()?;
        vault.save_attachment(note, &name, &bytes).ok()
    }

    pub fn handle_dropped(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if dropped.is_empty() {
            return;
        }
        let Some(note) = self.current_rel() else {
            self.toast("Open a note first, then drop files into it");
            return;
        };
        let mut rels = vec![];
        for p in dropped {
            rels.extend(self.import_file(&note, &p));
        }
        if !rels.is_empty() {
            self.insert_embeds(ctx, rels);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Background plumbing
    // -----------------------------------------------------------------------------------------

    pub fn pump(&mut self, ctx: &egui::Context) {
        // Hot reload: theme / config / cursor shader.
        for e in self.hot.poll() {
            match e {
                HotEvent::Theme => {
                    let t = Theme::load();
                    if t != self.theme {
                        self.theme = t;
                        self.theme.apply(ctx, &self.cfg);
                        self.theme_settle = 3;
                        self.toast(format!("Theme: {}", self.theme.name));
                    }
                }
                HotEvent::Shader => self.fx.reload(&self.cfg),
                HotEvent::Config => {
                    let new = Config::load();
                    if serde_json::to_string(&new).ok() != serde_json::to_string(&self.cfg).ok() {
                        let (left, right) = (self.cfg.left_sidebar, self.cfg.right_sidebar);
                        let recent = self.cfg.recent_vaults.clone();
                        self.cfg = new;
                        self.cfg.left_sidebar = left;
                        self.cfg.right_sidebar = right;
                        self.cfg.recent_vaults = recent;
                        self.theme.apply(ctx, &self.cfg);
                        ctx.set_zoom_factor(self.cfg.ui_scale);
                        if self.fx.effect_changed(&self.cfg) {
                            self.fx.reload(&self.cfg);
                        }
                    }
                }
            }
        }

        if self.theme_settle > 0 {
            self.theme_settle -= 1;
            for t in &mut self.tabs {
                t.galley_cache = None;
            }
            ctx.request_repaint();
        }

        // Folder dialog result.
        if let Some(rx) = &self.dialog_rx {
            if let Ok(DialogResult::Vault(p)) = rx.try_recv() {
                self.dialog_rx = None;
                self.open_vault(p);
            }
        }

        // Streaming index.
        if let Some(rx) = &self.index_rx {
            let mut n = 0;
            let mut done = false;
            loop {
                match rx.try_recv() {
                    Ok((rel, m)) => {
                        if let Some(v) = &mut self.vault {
                            v.set_meta(rel, m);
                        }
                        n += 1;
                        if n >= 400 {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        ctx.request_repaint_after(Duration::from_millis(40));
                        break;
                    }
                    Err(_) => {
                        done = true;
                        break;
                    }
                }
            }
            if n >= 400 {
                ctx.request_repaint();
            }
            if done {
                self.index_rx = None;
            }
        }

        // Filesystem changes.
        if let Some((_, rx)) = &self.watcher {
            let mut any = false;
            while let Ok(p) = rx.try_recv() {
                if let Some(v) = &self.vault {
                    if let Some(rel) = v.rel_of(&p) {
                        if !rel.split('/').any(|s| s.starts_with('.')) {
                            self.fs_changed.insert(rel);
                            any = true;
                        }
                    }
                }
            }
            if any {
                self.fs_pending = Some(Instant::now());
            }
        }
        if let Some(t0) = self.fs_pending {
            if t0.elapsed() >= Duration::from_millis(200) {
                self.fs_pending = None;
                self.apply_fs_changes();
            } else {
                ctx.request_repaint_after(Duration::from_millis(220));
            }
        }

        // Autosave.
        let wait = Duration::from_secs_f32(self.cfg.autosave_secs.max(0.2));
        let mut next: Option<Duration> = None;
        for i in 0..self.tabs.len() {
            if self.tabs[i].dirty {
                if let Some(t0) = self.tabs[i].last_edit {
                    if t0.elapsed() >= wait && ctx.input(|i| !i.pointer.any_down()) {
                        self.save_tab(i);
                    } else {
                        let rem = wait.saturating_sub(t0.elapsed()).max(Duration::from_millis(50));
                        next = Some(next.map_or(rem, |n| n.min(rem)));
                    }
                }
            }
        }
        if let Some(n) = next {
            ctx.request_repaint_after(n);
        }

        if self.session_dirty {
            self.session_dirty = false;
            self.save_session();
        }

        // Window title.
        let title = match (self.tab(), &self.vault) {
            (Some(t), Some(v)) if t.path.is_some() => format!("{} - {} - Carbon", t.title(), v.name),
            (_, Some(v)) => format!("{} - Carbon", v.name),
            _ => "Carbon".to_string(),
        };
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn apply_fs_changes(&mut self) {
        let changed: Vec<String> = self.fs_changed.drain().collect();
        let Some(vault) = &mut self.vault else { return };
        vault.rescan();
        for rel in &changed {
            let abs = vault.abs(rel);
            if rel.ends_with(".md") && abs.is_file() {
                if let Ok(text) = std::fs::read_to_string(&abs) {
                    let m = vault::parse_note(&text);
                    vault.set_meta(rel.clone(), m);
                }
            }
        }
        // Reload clean tabs whose file changed on disk; drop tabs whose file vanished.
        let mut gone = vec![];
        for (i, t) in self.tabs.iter_mut().enumerate() {
            let Some(rel) = t.path.clone() else { continue };
            let abs = vault.abs(&rel);
            let mtime = std::fs::metadata(&abs).and_then(|m| m.modified()).ok();
            if mtime.is_none() {
                gone.push(i);
                continue;
            }
            if mtime != t.disk_mtime && !t.dirty && t.kind == TabKind::Note {
                if let Ok(s) = std::fs::read_to_string(&abs) {
                    t.text = s;
                    t.doc = None;
                    t.galley_cache = None;
                }
                t.disk_mtime = mtime;
            }
        }
        for i in gone.into_iter().rev() {
            let t = &mut self.tabs[i];
            if t.dirty {
                // Keep unsaved edits; they'll be written back (recreating the file).
                continue;
            }
            self.tabs.remove(i);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len().saturating_sub(1);
            }
        }
        if self.tabs.is_empty() {
            self.new_tab();
        }
    }

    // -----------------------------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------------------------

    pub fn run_search(&mut self) {
        let Some(v) = &self.vault else { return };
        let q = search::parse_query(&self.search.query, self.search.case);
        self.search.results.clear();
        self.search.last_run = self.search.query.clone();
        if q.is_empty() {
            self.search.handle = None;
            self.search.running = false;
            return;
        }
        let notes: Vec<(String, FileKind)> = v.files.iter().filter(|f| matches!(f.kind, FileKind::Note | FileKind::Image | FileKind::Pdf)).map(|f| (f.rel.clone(), f.kind)).collect();
        let h = search::spawn(v.root.clone(), notes, v.meta.clone(), q, self.search.generation.clone());
        self.search.handle = Some(h);
        self.search.running = true;
    }

    pub fn poll_search(&mut self, ctx: &egui::Context) {
        let Some(h) = &self.search.handle else { return };
        let mut got = false;
        while let Ok((id, hit)) = h.rx.try_recv() {
            if id == h.id {
                self.search.results.push(hit);
                got = true;
            }
        }
        if self.search.running {
            // The worker drops its sender when finished.
            if matches!(h.rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)) {
                self.search.running = false;
            } else {
                ctx.request_repaint_after(Duration::from_millis(30));
            }
        }
        if got {
            ctx.request_repaint();
        }
    }

    // -----------------------------------------------------------------------------------------
    // File operations (tree / palette)
    // -----------------------------------------------------------------------------------------

    pub fn create_note_named(&mut self, dir: &str, name: &str) {
        let stem = name.trim().trim_end_matches(".md").to_string();
        if stem.is_empty() {
            return;
        }
        let (dir, stem) = match stem.rsplit_once('/') {
            Some((d, s)) => (if dir.is_empty() { d.to_string() } else { format!("{dir}/{d}") }, s.to_string()),
            None => (dir.to_string(), stem),
        };
        let Some(v) = &mut self.vault else { return };
        match v.create_note(&dir, &stem, "") {
            Ok(rel) => {
                if !dir.is_empty() {
                    self.expand_to(&rel);
                }
                self.open_path(&rel, false, None);
                if let Some(t) = self.tab_mut() {
                    t.view = ViewMode::Live;
                    t.focus_editor = true;
                }
            }
            Err(e) => self.toast(format!("Couldn't create note: {e}")),
        }
    }

    pub fn expand_to(&mut self, rel: &str) {
        let mut p = String::new();
        let parts: Vec<&str> = rel.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            if !p.is_empty() {
                p.push('/');
            }
            p.push_str(part);
            self.expanded.insert(p.clone());
        }
        self.expanded_ver += 1;
    }

    pub fn rename_path(&mut self, old: &str, new_name: &str) {
        let Some(v) = &mut self.vault else { return };
        let is_dir = v.root.join(old).is_dir();
        let dir = old.rsplit_once('/').map(|x| x.0).unwrap_or("");
        let mut name = new_name.trim().to_string();
        if name.is_empty() || name.contains('\\') {
            return;
        }
        if !is_dir && vault::kind_of(&name).is_none() {
            // Keep the original extension if the user typed a bare name.
            let ext = old.rsplit_once('.').map(|x| format!(".{}", x.1)).unwrap_or_default();
            name.push_str(&ext);
        }
        let new = if dir.is_empty() { name } else { format!("{dir}/{name}") };
        self.move_to(old, &new);
    }

    pub fn move_to(&mut self, old: &str, new: &str) {
        self.flush_all();
        let Some(v) = &mut self.vault else { return };
        match v.move_path(old, new) {
            Ok(changed) => {
                // Re-point open tabs.
                let prefix = format!("{old}/");
                for t in &mut self.tabs {
                    if let Some(p) = &t.path {
                        if p == old {
                            t.path = Some(new.to_string());
                        } else if let Some(r) = p.strip_prefix(&prefix) {
                            t.path = Some(format!("{new}/{r}"));
                        }
                        for h in &mut t.history {
                            if h == old {
                                *h = new.to_string();
                            }
                        }
                    }
                }
                // Reload tabs whose text was rewritten for link updates.
                let ch: HashSet<String> = changed.into_iter().collect();
                let n = ch.len();
                for t in &mut self.tabs {
                    if t.path.as_ref().is_some_and(|p| ch.contains(p)) {
                        if let (Some(p), Some(v)) = (&t.path, &self.vault) {
                            if let Ok(s) = std::fs::read_to_string(v.abs(p)) {
                                t.text = s;
                                t.doc = None;
                                t.galley_cache = None;
                                t.disk_mtime = std::fs::metadata(v.abs(p)).and_then(|m| m.modified()).ok();
                            }
                        }
                    }
                }
                // Re-index touched notes.
                if let Some(v) = &mut self.vault {
                    for rel in ch.iter().chain(std::iter::once(&new.to_string())) {
                        if rel.ends_with(".md") {
                            if let Ok(s) = std::fs::read_to_string(v.abs(rel)) {
                                v.set_meta(rel.clone(), vault::parse_note(&s));
                            }
                        }
                    }
                }
                self.session_dirty = true;
                if n > 0 {
                    self.toast(format!("Updated links in {n} note{}", if n == 1 { "" } else { "s" }));
                }
            }
            Err(e) => self.toast(e),
        }
    }

    pub fn delete_path(&mut self, rel: &str) {
        self.flush_all();
        let Some(v) = &mut self.vault else { return };
        match v.delete(rel) {
            Ok(()) => {
                let prefix = format!("{rel}/");
                let victims: Vec<usize> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.path.as_ref().is_some_and(|p| p == rel || p.starts_with(&prefix)))
                    .map(|(i, _)| i)
                    .collect();
                for i in victims.into_iter().rev() {
                    self.tabs[i].dirty = false;
                    self.close_tab(i);
                }
                self.toast("Moved to trash");
            }
            Err(e) => self.toast(format!("Couldn't delete: {e}")),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Cached derived data
    // -----------------------------------------------------------------------------------------

    pub fn backlinks_for(&mut self, rel: &str) -> Vec<(String, Link, String)> {
        let Some(v) = &self.vault else { return vec![] };
        if let Some((r, ver, data)) = &self.backlinks_cache {
            if r == rel && *ver == v.version {
                return data.clone();
            }
        }
        let data: Vec<(String, Link, String)> = v
            .backlinks(rel)
            .into_iter()
            .map(|(src, l)| {
                let ctx = std::fs::read_to_string(v.abs(&src))
                    .ok()
                    .and_then(|s| s.lines().nth(l.line).map(|x| x.trim().to_string()))
                    .unwrap_or_default();
                (src, l, ctx)
            })
            .collect();
        self.backlinks_cache = Some((rel.to_string(), v.version, data.clone()));
        data
    }

    pub fn outline(&mut self) -> Vec<Heading> {
        let Some(t) = self.tab() else { return vec![] };
        if t.kind != TabKind::Note {
            return vec![];
        }
        let h = hash_of(&t.text);
        if let Some((k, d)) = &self.outline_cache {
            if *k == h {
                return d.clone();
            }
        }
        let d = vault::parse_note(&t.text).headings;
        self.outline_cache = Some((h, d.clone()));
        d
    }

    // -----------------------------------------------------------------------------------------
    // Link-at-click helper for the editor
    // -----------------------------------------------------------------------------------------

    pub fn follow_editor_link(&mut self, link: LinkRef, new_tab: bool) {
        match link {
            LinkRef::Wiki { target, heading, .. } => self.open_link(&target, heading, new_tab),
            LinkRef::Url(u) => {
                let _ = open::that_detached(u);
            }
            LinkRef::Tag(t) => self.handle_actions(vec![Action::Tag(t)]),
        }
    }

    pub fn consume(ctx: &egui::Context, m: Modifiers, k: Key) -> bool {
        ctx.input_mut(|i| i.consume_key(m, k))
    }
}

impl eframe::App for App {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn logic(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        if std::env::var_os("CARBON_FPS").is_some() {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            static T: std::sync::OnceLock<std::sync::Mutex<Instant>> = std::sync::OnceLock::new();
            let t = T.get_or_init(|| std::sync::Mutex::new(Instant::now()));
            N.fetch_add(1, Ordering::Relaxed);
            let mut t = t.lock().unwrap();
            if t.elapsed() >= Duration::from_secs(2) {
                eprintln!("frames/2s: {}", N.swap(0, Ordering::Relaxed));
                *t = Instant::now();
            }
        }
        self.pump(ctx);
        self.poll_search(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        crate::ui::draw(self, ui);
    }

    fn on_exit(&mut self) {
        self.flush_all();
        self.save_session();
        self.cfg.save();
    }
}
