//! Vault-wide full-text search on a background thread. Supports quoted phrases and the
//! `file:`, `path:` and `tag:` operators; every other term must appear in the note.

use crate::vault::{FileKind, NoteMeta};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Hit {
    pub rel: String,
    /// (0-based line, line text)
    pub lines: Vec<(usize, String)>,
    #[allow(dead_code)]
    pub name_match: bool,
}

#[derive(Default, Clone, Debug)]
pub struct Query {
    pub terms: Vec<String>,
    pub files: Vec<String>,
    pub paths: Vec<String>,
    pub tags: Vec<String>,
    pub case: bool,
}

pub fn parse_query(q: &str, case: bool) -> Query {
    let mut out = Query { case, ..Default::default() };
    let mut cur = String::new();
    let mut quoted = false;
    let mut toks: Vec<String> = vec![];
    for c in q.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    toks.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    for t in toks {
        let norm = |s: &str| {
            let s = s.trim_matches('"');
            if case { s.to_string() } else { s.to_lowercase() }
        };
        if let Some(v) = t.strip_prefix("file:") {
            out.files.push(v.trim_matches('"').to_lowercase());
        } else if let Some(v) = t.strip_prefix("path:") {
            out.paths.push(v.trim_matches('"').to_lowercase());
        } else if let Some(v) = t.strip_prefix("tag:") {
            out.tags.push(v.trim_start_matches('#').to_lowercase());
        } else {
            let n = norm(&t);
            if !n.is_empty() {
                out.terms.push(n);
            }
        }
    }
    out
}

impl Query {
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.files.is_empty() && self.paths.is_empty() && self.tags.is_empty()
    }
}

pub struct SearchHandle {
    pub rx: Receiver<(u64, Hit)>,
    pub id: u64,
}

pub fn spawn(root: PathBuf, notes: Vec<(String, FileKind)>, meta: HashMap<String, NoteMeta>, q: Query, generation: Arc<AtomicU64>) -> SearchHandle {
    let id = generation.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, rx) = channel();
    let gen2 = generation.clone();
    std::thread::spawn(move || {
        let mut sent = 0;
        for (rel, kind) in notes {
            if gen2.load(Ordering::SeqCst) != id || sent >= 400 {
                return;
            }
            let lower = rel.to_lowercase();
            let name = lower.rsplit('/').next().unwrap_or(&lower);
            if !q.files.iter().all(|f| name.contains(f.as_str())) || !q.paths.iter().all(|p| lower.contains(p.as_str())) {
                continue;
            }
            if !q.tags.is_empty() {
                let Some(m) = meta.get(&rel) else { continue };
                let ok = q.tags.iter().all(|t| {
                    m.tags.iter().any(|x| {
                        let x = x.to_lowercase();
                        x == *t || x.starts_with(&format!("{t}/"))
                    })
                });
                if !ok {
                    continue;
                }
            }
            let name_match = !q.terms.is_empty() && q.terms.iter().all(|t| name.contains(&t.to_lowercase()));
            let mut lines = vec![];
            if kind == FileKind::Note && !q.terms.is_empty() {
                let Ok(text) = std::fs::read_to_string(root.join(&rel)) else { continue };
                let hay = if q.case { text.clone() } else { text.to_lowercase() };
                if !q.terms.iter().all(|t| hay.contains(t.as_str()) || name.contains(&t.to_lowercase())) {
                    continue;
                }
                let orig: Vec<&str> = text.lines().collect();
                let low: Vec<&str> = hay.lines().collect();
                for (n, l) in low.iter().enumerate() {
                    if q.terms.iter().any(|t| l.contains(t.as_str())) {
                        lines.push((n, orig.get(n).copied().unwrap_or("").trim().to_string()));
                        if lines.len() >= 6 {
                            break;
                        }
                    }
                }
                if lines.is_empty() && !name_match {
                    continue;
                }
            } else if !q.terms.is_empty() {
                if !name_match {
                    continue;
                }
            }
            sent += 1;
            if tx.send((id, Hit { rel, lines, name_match })).is_err() {
                return;
            }
        }
    });
    SearchHandle { rx, id }
}
