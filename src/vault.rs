//! Vault model: file listing, link/tag/heading index, Obsidian-style link resolution,
//! attachments and rename-with-link-updates. Paths are vault-relative with `/` separators.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Note,
    Image,
    Pdf,
    Canvas,
    Media,
}

pub fn kind_of(name: &str) -> Option<FileKind> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "md" => FileKind::Note,
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "avif" => FileKind::Image,
        "pdf" => FileKind::Pdf,
        "canvas" | "base" => FileKind::Canvas,
        "mp3" | "wav" | "m4a" | "ogg" | "flac" | "webm" | "3gp" | "mp4" | "ogv" | "mov" | "mkv" => FileKind::Media,
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub struct VFile {
    pub rel: String,
    pub name: String,
    pub kind: FileKind,
}

impl VFile {
    pub fn stem(&self) -> &str {
        if self.kind == FileKind::Note {
            self.name.strip_suffix(".md").unwrap_or(&self.name)
        } else {
            &self.name
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Link {
    pub target: String,
    #[allow(dead_code)]
    pub heading: Option<String>,
    pub line: usize,
    #[allow(dead_code)]
    pub embed: bool,
}

#[derive(Clone, Debug)]
pub struct Heading {
    pub level: usize,
    pub text: String,
    pub line: usize,
}

#[derive(Clone, Debug, Default)]
pub struct NoteMeta {
    pub links: Vec<Link>,
    pub tags: Vec<String>,
    pub headings: Vec<Heading>,
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct VaultCfg {
    /// Raw `attachmentFolderPath` from `.obsidian/app.json`.
    pub attachment_folder: Option<String>,
    pub use_markdown_links: bool,
    pub new_file_location: Option<String>,
    pub new_file_folder: Option<String>,
}

pub struct Vault {
    pub root: PathBuf,
    pub name: String,
    pub cfg: VaultCfg,
    pub files: Vec<VFile>,
    pub dirs: Vec<String>,
    by_key: HashMap<String, Vec<usize>>,
    by_rel: HashMap<String, usize>,
    by_alias: HashMap<String, Vec<usize>>,
    pub meta: HashMap<String, NoteMeta>,
    /// Bumped whenever the index changes (cheap cache invalidation for the UI).
    pub version: u64,
    /// Bumped only when the file/folder listing changes.
    pub fs_version: u64,
}

// ---------------------------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------------------------

fn scan_dir(root: &Path, rel: &str, files: &mut Vec<VFile>, dirs: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(root.join(rel)) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if ft.is_dir() {
            dirs.push(r.clone());
            scan_dir(root, &r, files, dirs);
        } else if let Some(kind) = kind_of(&name) {
            files.push(VFile { rel: r, name, kind });
        }
    }
}

impl Vault {
    pub fn open(root: PathBuf) -> Vault {
        let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Vault".into());
        let mut v = Vault {
            root,
            name,
            cfg: VaultCfg::default(),
            files: vec![],
            dirs: vec![],
            by_key: HashMap::new(),
            by_rel: HashMap::new(),
            by_alias: HashMap::new(),
            meta: HashMap::new(),
            version: 0,
            fs_version: 0,
        };
        v.load_cfg();
        v.rescan();
        v
    }

    fn load_cfg(&mut self) {
        let Ok(s) = std::fs::read_to_string(self.root.join(".obsidian/app.json")) else { return };
        let Ok(j) = serde_json::from_str::<serde_json::Value>(&s) else { return };
        let st = |k: &str| j.get(k).and_then(|x| x.as_str()).map(|s| s.to_string());
        self.cfg.attachment_folder = st("attachmentFolderPath");
        self.cfg.use_markdown_links = j.get("useMarkdownLinks").and_then(|x| x.as_bool()).unwrap_or(false);
        self.cfg.new_file_location = st("newFileLocation");
        self.cfg.new_file_folder = st("newFileFolderPath");
    }

    pub fn rescan(&mut self) {
        let mut files = vec![];
        let mut dirs = vec![];
        scan_dir(&self.root, "", &mut files, &mut dirs);
        files.sort_by(|a, b| a.rel.to_lowercase().cmp(&b.rel.to_lowercase()));
        dirs.sort_by_key(|d| d.to_lowercase());
        self.files = files;
        self.dirs = dirs;
        self.rebuild_lookup();
        let live: BTreeSet<&str> = self.files.iter().map(|f| f.rel.as_str()).collect();
        self.meta.retain(|k, _| live.contains(k.as_str()));
        self.version += 1;
        self.fs_version += 1;
    }

    fn rebuild_lookup(&mut self) {
        self.by_key.clear();
        self.by_rel.clear();
        self.by_alias.clear();
        for (i, f) in self.files.iter().enumerate() {
            self.by_key.entry(f.stem().to_lowercase()).or_default().push(i);
            self.by_rel.insert(f.rel.to_lowercase(), i);
        }
        for (rel, m) in &self.meta {
            if let Some(&i) = self.by_rel.get(&rel.to_lowercase()) {
                for a in &m.aliases {
                    self.by_alias.entry(a.to_lowercase()).or_default().push(i);
                }
            }
        }
    }

    pub fn set_meta(&mut self, rel: String, m: NoteMeta) {
        if let Some(&i) = self.by_rel.get(&rel.to_lowercase()) {
            for a in &m.aliases {
                let e = self.by_alias.entry(a.to_lowercase()).or_default();
                if !e.contains(&i) {
                    e.push(i);
                }
            }
        }
        self.meta.insert(rel, m);
        self.version += 1;
    }

    pub fn file(&self, rel: &str) -> Option<&VFile> {
        self.by_rel.get(&rel.to_lowercase()).map(|&i| &self.files[i])
    }

    pub fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn rel_of(&self, abs: &Path) -> Option<String> {
        abs.strip_prefix(&self.root).ok().map(|p| p.to_string_lossy().replace('\\', "/"))
    }

    // -----------------------------------------------------------------------------------------
    // Link resolution
    // -----------------------------------------------------------------------------------------

    /// Resolve a link target (as written in `[[...]]` or `](...)`, without `#heading`) to a file.
    pub fn resolve(&self, target: &str, from_rel: &str) -> Option<&VFile> {
        let t = target.trim().trim_start_matches('/');
        if t.is_empty() {
            return self.file(from_rel);
        }
        let from_dir = from_rel.rsplit_once('/').map(|x| x.0).unwrap_or("");
        if t.starts_with("./") || t.starts_with("../") {
            let joined = normalize(&format!("{from_dir}/{t}"));
            if let Some(f) = self.lookup_path(&joined) {
                return Some(f);
            }
        }
        let t = normalize(t);
        let tl = t.to_lowercase();
        let tl = tl.strip_suffix(".md").unwrap_or(&tl).to_string();
        let (dir_part, base) = match tl.rsplit_once('/') {
            Some((d, b)) => (Some(d), b),
            None => (None, tl.as_str()),
        };
        let mut cands: Vec<&VFile> = self
            .by_key
            .get(base)
            .map(|v| v.iter().map(|&i| &self.files[i]).collect())
            .unwrap_or_default();
        if let Some(d) = dir_part {
            let want = format!("{d}/{base}");
            cands.retain(|f| {
                let r = f.rel.to_lowercase();
                let r = r.strip_suffix(".md").unwrap_or(&r);
                r == want || r.ends_with(&format!("/{want}"))
            });
        }
        if !cands.is_empty() {
            return Some(closest(cands, from_dir));
        }
        if dir_part.is_none() {
            if let Some(v) = self.by_alias.get(&tl) {
                if let Some(&i) = v.first() {
                    return Some(&self.files[i]);
                }
            }
        }
        None
    }

    fn lookup_path(&self, p: &str) -> Option<&VFile> {
        let l = p.to_lowercase();
        self.by_rel
            .get(&l)
            .or_else(|| self.by_rel.get(&format!("{l}.md")))
            .map(|&i| &self.files[i])
    }

    /// Shortest unambiguous link text for a file (what Obsidian writes inside `[[ ]]`).
    pub fn link_name(&self, f: &VFile) -> String {
        let stem = f.stem().to_lowercase();
        let unique = self.by_key.get(&stem).map(|v| v.len() <= 1).unwrap_or(true);
        if unique {
            f.stem().to_string()
        } else if f.kind == FileKind::Note {
            f.rel.strip_suffix(".md").unwrap_or(&f.rel).to_string()
        } else {
            f.rel.clone()
        }
    }

    /// Link markup to insert for `target`, written from the note `from_rel`.
    pub fn make_link(&self, target: &VFile, from_rel: &str, embed: bool, alias: Option<&str>) -> String {
        let bang = if embed { "!" } else { "" };
        if self.cfg.use_markdown_links {
            let from_dir = from_rel.rsplit_once('/').map(|x| x.0).unwrap_or("");
            let rel = relative_path(from_dir, &target.rel);
            let label = alias.map(|s| s.to_string()).unwrap_or_else(|| target.stem().to_string());
            format!("{bang}[{label}]({})", encode_path(&rel))
        } else {
            match alias {
                Some(a) => format!("{bang}[[{}|{a}]]", self.link_name(target)),
                None => format!("{bang}[[{}]]", self.link_name(target)),
            }
        }
    }

    pub fn backlinks(&self, rel: &str) -> Vec<(String, Link)> {
        let mut out = vec![];
        for (src, m) in &self.meta {
            if src == rel {
                continue;
            }
            for l in &m.links {
                if self.resolve(&l.target, src).map(|f| f.rel.as_str()) == Some(rel) {
                    out.push((src.clone(), l.clone()));
                }
            }
        }
        out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then(a.1.line.cmp(&b.1.line)));
        out
    }

    pub fn tags(&self) -> Vec<(String, Vec<String>)> {
        let mut map: HashMap<String, BTreeSet<String>> = HashMap::new();
        for (rel, m) in &self.meta {
            for t in &m.tags {
                map.entry(t.clone()).or_default().insert(rel.clone());
            }
        }
        let mut v: Vec<_> = map.into_iter().map(|(k, s)| (k, s.into_iter().collect())).collect();
        v.sort_by(|a: &(String, Vec<String>), b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        v
    }

    // -----------------------------------------------------------------------------------------
    // Attachments
    // -----------------------------------------------------------------------------------------

    /// Folder (vault-relative) where files pasted into `note_rel` are stored.
    /// Follows Obsidian's setting; defaults to an `attachments` folder beside the note.
    pub fn attachment_dir(&self, note_rel: &str) -> String {
        let note_dir = note_rel.rsplit_once('/').map(|x| x.0).unwrap_or("");
        let join = |a: &str, b: &str| if a.is_empty() { b.to_string() } else { format!("{a}/{b}") };
        match self.cfg.attachment_folder.as_deref() {
            None | Some("") => join(note_dir, "attachments"),
            Some("/") => String::new(),
            Some("./") | Some(".") => note_dir.to_string(),
            Some(p) if p.starts_with("./") => join(note_dir, p[2..].trim_matches('/')),
            Some(p) => p.trim_matches('/').to_string(),
        }
    }

    /// Writes `bytes` into the attachment folder for `note_rel`, never overwriting. Returns the new file's rel path.
    pub fn save_attachment(&mut self, note_rel: &str, name: &str, bytes: &[u8]) -> std::io::Result<String> {
        let dir = self.attachment_dir(note_rel);
        std::fs::create_dir_all(self.root.join(&dir))?;
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (name.to_string(), String::new()),
        };
        let mut n = 0;
        let rel = loop {
            let cand = if n == 0 { format!("{stem}{ext}") } else { format!("{stem} {n}{ext}") };
            let rel = if dir.is_empty() { cand } else { format!("{dir}/{cand}") };
            if !self.root.join(&rel).exists() {
                break rel;
            }
            n += 1;
        };
        std::fs::write(self.root.join(&rel), bytes)?;
        self.rescan();
        Ok(rel)
    }

    // -----------------------------------------------------------------------------------------
    // File operations
    // -----------------------------------------------------------------------------------------

    pub fn unique_rel(&self, dir: &str, stem: &str, ext: &str) -> String {
        let mut n = 0;
        loop {
            let name = if n == 0 { format!("{stem}{ext}") } else { format!("{stem} {n}{ext}") };
            let rel = if dir.is_empty() { name } else { format!("{dir}/{name}") };
            if !self.root.join(&rel).exists() {
                return rel;
            }
            n += 1;
        }
    }

    pub fn create_note(&mut self, dir: &str, stem: &str, content: &str) -> std::io::Result<String> {
        let rel = self.unique_rel(dir, stem, ".md");
        if let Some((d, _)) = rel.rsplit_once('/') {
            std::fs::create_dir_all(self.root.join(d))?;
        }
        std::fs::write(self.root.join(&rel), content)?;
        self.rescan();
        Ok(rel)
    }

    pub fn create_folder(&mut self, dir: &str, name: &str) -> std::io::Result<String> {
        let rel = self.unique_rel(dir, name, "");
        std::fs::create_dir_all(self.root.join(&rel))?;
        self.rescan();
        Ok(rel)
    }

    pub fn delete(&mut self, rel: &str) -> Result<(), String> {
        let p = self.root.join(rel);
        trash::delete(&p).or_else(|_| if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) }.map_err(|e| trash::Error::Unknown { description: e.to_string() })).map_err(|e| e.to_string())?;
        self.rescan();
        Ok(())
    }

    /// Rename or move a file or folder, rewriting links in other notes. Returns the notes that were edited.
    pub fn move_path(&mut self, old: &str, new: &str) -> Result<Vec<String>, String> {
        if old == new {
            return Ok(vec![]);
        }
        if self.root.join(new).exists() {
            return Err(format!("'{new}' already exists"));
        }
        // Build old -> new mapping for everything affected (a folder moves all its children).
        let prefix = format!("{old}/");
        let mapping: Vec<(String, String)> = self
            .files
            .iter()
            .filter_map(|f| {
                if f.rel == old {
                    Some((f.rel.clone(), new.to_string()))
                } else {
                    f.rel.strip_prefix(&prefix).map(|r| (f.rel.clone(), format!("{new}/{r}")))
                }
            })
            .collect();

        // Decide rewrites while the old index is still valid.
        let mut edits: Vec<(String, String)> = vec![]; // (note rel, new text)
        let map: HashMap<&str, &str> = mapping.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let new_keys: BTreeSet<String> = mapping
            .iter()
            .map(|(_, n)| n.rsplit('/').next().unwrap_or(n).trim_end_matches(".md").to_lowercase())
            .collect();
        for (src, m) in &self.meta {
            let hits = m.links.iter().any(|l| self.resolve(&l.target, src).is_some_and(|f| map.contains_key(f.rel.as_str())));
            if !hits {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(self.root.join(src)) else { continue };
            let out = rewrite_links(&text, |target, _embed| {
                let f = self.resolve(target, src)?;
                let nr = map.get(f.rel.as_str())?;
                let base = nr.rsplit('/').next().unwrap_or(nr).trim_end_matches(".md").to_lowercase();
                let ambiguous = self.by_key.get(&base).map(|v| v.iter().any(|&i| !map.contains_key(self.files[i].rel.as_str()))).unwrap_or(false)
                    || new_keys.iter().filter(|k| **k == base).count() > 1;
                let had_path = target.contains('/');
                let md = nr.ends_with(".md");
                let strip = |s: &str| if md { s.trim_end_matches(".md").to_string() } else { s.to_string() };
                let keep_ext = target.to_lowercase().ends_with(".md");
                Some(if had_path || ambiguous {
                    let p = strip(nr);
                    if keep_ext && md { format!("{p}.md") } else { p }
                } else {
                    let b = nr.rsplit('/').next().unwrap_or(nr);
                    let b = strip(b);
                    if keep_ext && md { format!("{b}.md") } else { b }
                })
            });
            if out != text {
                edits.push((src.clone(), out));
            }
        }

        if let Some((d, _)) = new.rsplit_once('/') {
            std::fs::create_dir_all(self.root.join(d)).map_err(|e| e.to_string())?;
        }
        std::fs::rename(self.root.join(old), self.root.join(new)).map_err(|e| e.to_string())?;
        let mut changed = vec![];
        for (src, text) in edits {
            let dst = map.get(src.as_str()).map(|s| s.to_string()).unwrap_or(src.clone());
            if std::fs::write(self.root.join(&dst), text).is_ok() {
                changed.push(dst);
            }
        }
        // Carry the cached index across the move.
        for (a, b) in &mapping {
            if let Some(m) = self.meta.remove(a) {
                self.meta.insert(b.clone(), m);
            }
        }
        self.rescan();
        Ok(changed)
    }
}

fn closest<'a>(mut c: Vec<&'a VFile>, from_dir: &str) -> &'a VFile {
    let score = |f: &VFile| {
        let d = f.rel.rsplit_once('/').map(|x| x.0).unwrap_or("");
        let common = d.split('/').zip(from_dir.split('/')).take_while(|(a, b)| a.eq_ignore_ascii_case(b)).count();
        (std::cmp::Reverse(common), f.rel.len())
    };
    c.sort_by_key(|f| score(f));
    c[0]
}

fn normalize(p: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

pub fn relative_path(from_dir: &str, to: &str) -> String {
    let a: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = to.split('/').collect();
    let common = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count().min(b.len() - 1);
    let mut parts: Vec<String> = vec!["..".into(); a.len() - common];
    parts.extend(b[common..].iter().map(|s| s.to_string()));
    parts.join("/")
}

pub fn encode_path(p: &str) -> String {
    p.replace('%', "%25").replace(' ', "%20").replace('(', "%28").replace(')', "%29")
}

pub fn decode_path(p: &str) -> String {
    let b = p.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------------------------
// Note parsing (links, tags, headings, frontmatter)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum PropValue {
    Text(String),
    List(Vec<String>),
}

/// Splits a leading `---` YAML block. Returns (yaml text, byte offset where the body starts).
pub fn split_frontmatter(text: &str) -> Option<(&str, usize)> {
    let rest = text.strip_prefix("---")?;
    let first_nl = rest.find('\n')?;
    if !rest[..first_nl].trim().is_empty() {
        return None;
    }
    let body = &rest[first_nl + 1..];
    let mut off = 0;
    for line in body.split_inclusive('\n') {
        let l = line.trim_end();
        if l == "---" || l == "..." {
            let yaml_end = off;
            let consumed = 3 + first_nl + 1 + off + line.len();
            return Some((&body[..yaml_end], consumed));
        }
        off += line.len();
    }
    None
}

pub fn parse_properties(yaml: &str) -> Vec<(String, PropValue)> {
    let mut out: Vec<(String, PropValue)> = vec![];
    let mut cur_list: Option<usize> = None;
    for line in yaml.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix('-').filter(|r| r.is_empty())) {
            if let Some(i) = cur_list {
                if let PropValue::List(l) = &mut out[i].1 {
                    l.push(unquote(item));
                }
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            if line.starts_with(' ') {
                continue; // nested mapping: unsupported, skip
            }
            let v = v.trim();
            if v.is_empty() {
                out.push((k.trim().to_string(), PropValue::List(vec![])));
                cur_list = Some(out.len() - 1);
            } else if v.starts_with('[') && v.ends_with(']') {
                let items = v[1..v.len() - 1].split(',').map(unquote).filter(|s| !s.is_empty()).collect();
                out.push((k.trim().to_string(), PropValue::List(items)));
                cur_list = None;
            } else {
                out.push((k.trim().to_string(), PropValue::Text(unquote(v))));
                cur_list = None;
            }
        }
    }
    out.retain(|(_, v)| !matches!(v, PropValue::List(l) if l.is_empty()));
    out
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim_matches('\'').to_string()
}

pub fn parse_note(text: &str) -> NoteMeta {
    let mut meta = NoteMeta::default();
    let mut skip_lines = 0;
    if let Some((yaml, end)) = split_frontmatter(text) {
        skip_lines = text[..end].matches('\n').count();
        for (k, v) in parse_properties(yaml) {
            let items: Vec<String> = match v {
                PropValue::Text(t) => t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                PropValue::List(l) => l,
            };
            match k.to_lowercase().as_str() {
                "tags" | "tag" => meta.tags.extend(items.into_iter().map(|t| t.trim_start_matches('#').to_string())),
                "aliases" | "alias" => meta.aliases.extend(items),
                _ => {}
            }
        }
    }
    let mut fence: Option<String> = None;
    for (ln, line) in text.lines().enumerate().skip(skip_lines) {
        let t = line.trim_start();
        if let Some(f) = &fence {
            if t.starts_with(f.as_str()) {
                fence = None;
            }
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(t[..3].to_string());
            continue;
        }
        let hashes = t.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
            let txt = t[hashes..].trim().trim_end_matches('#').trim().to_string();
            meta.headings.push(Heading { level: hashes, text: txt, line: ln });
        }
        scan_line(line, ln, &mut meta);
    }
    meta.tags.sort();
    meta.tags.dedup();
    meta
}

fn scan_line(line: &str, ln: usize, meta: &mut NoteMeta) {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'`' => {
                let run = b[i..].iter().take_while(|&&c| c == b'`').count();
                let needle = "`".repeat(run);
                match line[i + run..].find(&needle) {
                    Some(p) => i += run + p + run,
                    None => i += run,
                }
                continue;
            }
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' if b.get(i + 1) == Some(&b'[') => {
                if let Some(end) = line[i + 2..].find("]]") {
                    let inner = &line[i + 2..i + 2 + end];
                    let embed = i > 0 && b[i - 1] == b'!';
                    let target_part = inner.split('|').next().unwrap_or("");
                    let (page, heading) = match target_part.split_once('#') {
                        Some((p, h)) => (p, Some(h.trim().to_string())),
                        None => (target_part, None),
                    };
                    meta.links.push(Link { target: page.trim().to_string(), heading, line: ln, embed });
                    i += 2 + end + 2;
                    continue;
                }
            }
            b']' if b.get(i + 1) == Some(&b'(') => {
                if let Some(end) = line[i + 2..].find(')') {
                    let raw = line[i + 2..i + 2 + end].trim();
                    let raw = raw.strip_prefix('<').and_then(|r| r.split_once('>')).map(|x| x.0).unwrap_or_else(|| raw.split(" \"").next().unwrap_or(raw));
                    let is_ext = raw.contains("://") || raw.starts_with("mailto:") || raw.starts_with("tel:");
                    if !is_ext && !raw.is_empty() {
                        let raw = decode_path(raw);
                        let (page, heading) = match raw.split_once('#') {
                            Some((p, h)) => (p.to_string(), Some(h.to_string())),
                            None => (raw, None),
                        };
                        meta.links.push(Link { target: page, heading, line: ln, embed: false });
                    }
                    i += 2 + end + 1;
                    continue;
                }
            }
            b'#' => {
                let prev_ok = i == 0 || b[i - 1].is_ascii_whitespace();
                if prev_ok {
                    let tag: String = line[i + 1..].chars().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/')).collect();
                    if !tag.is_empty() && tag.chars().any(|c| !c.is_ascii_digit()) {
                        meta.tags.push(tag.clone());
                        i += 1 + tag.len();
                        continue;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Rewrites every internal link whose target `f` maps to a replacement. `f(target, is_embed)`
/// returns the new target text (without alias/heading) or None to leave it alone.
pub fn rewrite_links(text: &str, f: impl Fn(&str, bool) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut fence: Option<String> = None;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if let Some(fc) = &fence {
            if t.starts_with(fc.as_str()) {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(t[..3].to_string());
            out.push_str(line);
            continue;
        }
        rewrite_line(line, &f, &mut out);
    }
    out
}

fn rewrite_line(line: &str, f: &impl Fn(&str, bool) -> Option<String>, out: &mut String) {
    let b = line.as_bytes();
    let mut i = 0;
    let mut last = 0;
    while i < b.len() {
        match b[i] {
            b'`' => {
                let run = b[i..].iter().take_while(|&&c| c == b'`').count();
                let needle = "`".repeat(run);
                i += match line[i + run..].find(&needle) {
                    Some(p) => run + p + run,
                    None => run,
                };
                continue;
            }
            b'[' if b.get(i + 1) == Some(&b'[') => {
                if let Some(end) = line[i + 2..].find("]]") {
                    let inner = &line[i + 2..i + 2 + end];
                    let embed = i > 0 && b[i - 1] == b'!';
                    let (target_alias, alias) = match inner.split_once('|') {
                        Some((t, a)) => (t, Some(a)),
                        None => (inner, None),
                    };
                    let (page, heading) = match target_alias.split_once('#') {
                        Some((p, h)) => (p, Some(h)),
                        None => (target_alias, None),
                    };
                    if let Some(newt) = f(page.trim(), embed) {
                        out.push_str(&line[last..i + 2]);
                        out.push_str(&newt);
                        if let Some(h) = heading {
                            out.push('#');
                            out.push_str(h);
                        }
                        if let Some(a) = alias {
                            out.push('|');
                            out.push_str(a);
                        }
                        out.push_str("]]");
                        last = i + 2 + end + 2;
                    }
                    i += 2 + end + 2;
                    continue;
                }
            }
            b']' if b.get(i + 1) == Some(&b'(') => {
                if let Some(end) = line[i + 2..].find(')') {
                    let raw = &line[i + 2..i + 2 + end];
                    if !raw.contains("://") && !raw.starts_with('<') {
                        let dec = decode_path(raw.split(" \"").next().unwrap_or(raw));
                        let (page, heading) = match dec.split_once('#') {
                            Some((p, h)) => (p.to_string(), Some(h.to_string())),
                            None => (dec.clone(), None),
                        };
                        if let Some(newt) = f(&page, false) {
                            let mut np = encode_path(&newt);
                            if let Some(h) = heading {
                                np.push('#');
                                np.push_str(&h);
                            }
                            out.push_str(&line[last..i + 2]);
                            out.push_str(&np);
                            last = i + 2 + end;
                        }
                    }
                    i += 2 + end + 1;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out.push_str(&line[last..]);
}

/// Index every note on background threads; results stream back over the channel.
pub fn spawn_index(root: PathBuf, notes: Vec<String>) -> std::sync::mpsc::Receiver<(String, NoteMeta)> {
    let (tx, rx) = std::sync::mpsc::channel();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4);
    std::thread::spawn(move || {
        let chunk = notes.len().div_ceil(threads).max(1);
        std::thread::scope(|s| {
            for part in notes.chunks(chunk) {
                let tx = tx.clone();
                let root = &root;
                s.spawn(move || {
                    for rel in part {
                        if let Ok(text) = std::fs::read_to_string(root.join(rel)) {
                            let _ = tx.send((rel.clone(), parse_note(&text)));
                        }
                    }
                });
            }
        });
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_with(files: &[&str]) -> Vault {
        let mut v = Vault {
            root: PathBuf::from("/nonexistent"),
            name: "t".into(),
            cfg: VaultCfg::default(),
            files: vec![],
            dirs: vec![],
            by_key: HashMap::new(),
            by_rel: HashMap::new(),
            by_alias: HashMap::new(),
            meta: HashMap::new(),
            version: 0,
            fs_version: 0,
        };
        for f in files {
            let name = f.rsplit('/').next().unwrap().to_string();
            v.files.push(VFile { rel: f.to_string(), kind: kind_of(&name).unwrap(), name });
        }
        v.rebuild_lookup();
        v
    }

    #[test]
    fn parses_links_tags_headings() {
        let m = parse_note("---\ntags: [a, b]\naliases:\n  - Home\n---\n# Title\nSee [[Note#Head|alias]] and ![[img.png]] and [x](sub/other%20note.md) #t1 #2024\n```\n[[ignored]] #nope\n```\n`[[inline]]` #t2/child\n");
        let targets: Vec<_> = m.links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(targets, ["Note", "img.png", "sub/other note.md"]);
        assert_eq!(m.links[0].heading.as_deref(), Some("Head"));
        assert!(m.links[1].embed);
        assert_eq!(m.tags, ["a", "b", "t1", "t2/child"]);
        assert_eq!(m.aliases, ["Home"]);
        assert_eq!(m.headings.len(), 1);
        assert_eq!(m.headings[0].text, "Title");
    }

    #[test]
    fn resolves_shortest_and_closest() {
        let v = vault_with(&["Note.md", "A/Dup.md", "B/Dup.md", "B/sub/Deep.md", "assets/pic.png"]);
        assert_eq!(v.resolve("note", "x.md").unwrap().rel, "Note.md");
        assert_eq!(v.resolve("Dup", "B/x.md").unwrap().rel, "B/Dup.md");
        assert_eq!(v.resolve("Dup", "A/x.md").unwrap().rel, "A/Dup.md");
        assert_eq!(v.resolve("B/Dup", "A/x.md").unwrap().rel, "B/Dup.md");
        assert_eq!(v.resolve("sub/Deep.md", "x.md").unwrap().rel, "B/sub/Deep.md");
        assert_eq!(v.resolve("pic.png", "x.md").unwrap().rel, "assets/pic.png");
        assert_eq!(v.resolve("../Note", "A/x.md").unwrap().rel, "Note.md");
        assert!(v.resolve("missing", "x.md").is_none());
        assert_eq!(v.link_name(v.file("Note.md").unwrap()), "Note");
        assert_eq!(v.link_name(v.file("A/Dup.md").unwrap()), "A/Dup");
    }

    #[test]
    fn attachment_dir_follows_note_or_vault_setting() {
        let mut v = vault_with(&["Note.md"]);
        assert_eq!(v.attachment_dir("Note.md"), "attachments");
        assert_eq!(v.attachment_dir("Projects/Deep/Note.md"), "Projects/Deep/attachments");
        v.cfg.attachment_folder = Some("/".into());
        assert_eq!(v.attachment_dir("Projects/Note.md"), "");
        v.cfg.attachment_folder = Some("./assets".into());
        assert_eq!(v.attachment_dir("Projects/Note.md"), "Projects/assets");
        v.cfg.attachment_folder = Some("Files".into());
        assert_eq!(v.attachment_dir("Projects/Note.md"), "Files");
    }

    #[test]
    fn rewrites_links_but_not_code() {
        let t = "[[Old]] and [[Old|alias]] and [[Old#H]] ![[Old]] [t](Old.md) `[[Old]]`\n```\n[[Old]]\n```\n[[Other]]\n";
        let out = rewrite_links(t, |target, _| if target == "Old" || target == "Old.md" { Some("New".into()) } else { None });
        assert_eq!(out, "[[New]] and [[New|alias]] and [[New#H]] ![[New]] [t](New) `[[Old]]`\n```\n[[Old]]\n```\n[[Other]]\n");
    }

    #[test]
    fn frontmatter_properties() {
        let (y, off) = split_frontmatter("---\na: 1\nb: [x, y]\n---\nbody").unwrap();
        assert_eq!(&"---\na: 1\nb: [x, y]\n---\nbody"[off..], "body");
        let p = parse_properties(y);
        assert_eq!(p.len(), 2);
        assert!(split_frontmatter("no front").is_none());
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative_path("A/B", "A/attachments/x.png"), "../attachments/x.png");
        assert_eq!(relative_path("", "x/y.png"), "x/y.png");
        assert_eq!(relative_path("A", "A/y.png"), "y.png");
    }
}

#[cfg(test)]
mod fs_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp").join(name);
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn rename_updates_links_everywhere() {
        let root = scratch("rename");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("Old.md"), "# old").unwrap();
        std::fs::write(root.join("a.md"), "see [[Old]] and [[Old|x]] and [md](Old.md)\n").unwrap();
        std::fs::write(root.join("sub/b.md"), "![[Old#H]]").unwrap();
        std::fs::write(root.join("c.md"), "unrelated [[a]]").unwrap();
        let mut v = Vault::open(root.clone());
        for rel in ["a.md", "sub/b.md", "c.md", "Old.md"] {
            let m = parse_note(&std::fs::read_to_string(root.join(rel)).unwrap());
            v.set_meta(rel.to_string(), m);
        }
        let changed = v.move_path("Old.md", "New.md").unwrap();
        assert_eq!(changed.len(), 2);
        assert_eq!(std::fs::read_to_string(root.join("a.md")).unwrap(), "see [[New]] and [[New|x]] and [md](New.md)\n");
        assert_eq!(std::fs::read_to_string(root.join("sub/b.md")).unwrap(), "![[New#H]]");
        assert_eq!(std::fs::read_to_string(root.join("c.md")).unwrap(), "unrelated [[a]]");
        assert!(root.join("New.md").exists() && !root.join("Old.md").exists());
        assert!(v.file("New.md").is_some());
    }

    #[test]
    fn moving_folder_keeps_name_links_intact() {
        let root = scratch("movedir");
        std::fs::create_dir_all(root.join("A")).unwrap();
        std::fs::write(root.join("A/Note.md"), "x").unwrap();
        std::fs::write(root.join("r.md"), "[[Note]] [[A/Note]]").unwrap();
        let mut v = Vault::open(root.clone());
        let m = parse_note("[[Note]] [[A/Note]]");
        v.set_meta("r.md".into(), m);
        v.move_path("A", "B").unwrap();
        // bare-name link is untouched, path-style link follows the folder
        assert_eq!(std::fs::read_to_string(root.join("r.md")).unwrap(), "[[Note]] [[B/Note]]");
    }

    #[test]
    fn saves_attachments_without_overwriting() {
        let root = scratch("attach");
        std::fs::create_dir_all(root.join("P")).unwrap();
        std::fs::write(root.join("P/n.md"), "").unwrap();
        let mut v = Vault::open(root.clone());
        let a = v.save_attachment("P/n.md", "Pasted image 1.png", b"1").unwrap();
        let b = v.save_attachment("P/n.md", "Pasted image 1.png", b"2").unwrap();
        assert_eq!(a, "P/attachments/Pasted image 1.png");
        assert_eq!(b, "P/attachments/Pasted image 1 1.png");
        assert_eq!(std::fs::read(root.join(&a)).unwrap(), b"1");
        let n = v.file("P/n.md").unwrap().clone();
        let img = v.file(&a).unwrap();
        assert_eq!(v.make_link(img, &n.rel, true, None), "![[Pasted image 1.png]]");
    }
}
