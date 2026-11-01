//! Two-way sync of the open tab strip with Obsidian's `.obsidian/workspace.json`.
//!
//! Obsidian stores the open tabs (and which one is focused) inside the vault, so syncing the vault
//! (Syncthing, Obsidian Sync, git…) syncs the tabs. Carbon reads that file on open, follows external
//! changes, and writes its own tab changes back. Only the first tab group of the main area is touched,
//! and everything else in the file (sidebars, ribbon, foreign tabs like graph/canvas) is preserved.

use crate::config::ViewMode;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct WsTab {
    pub path: String,
    pub view: ViewMode,
    pub pinned: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ws {
    pub tabs: Vec<WsTab>,
    pub active: usize,
}

pub const REL: &str = ".obsidian/workspace.json";

pub fn file(root: &Path) -> PathBuf {
    root.join(REL)
}

pub fn read(root: &Path) -> Option<Ws> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(file(root)).ok()?).ok()?;
    parse(&v)
}

fn first_tabs_group(n: &Value) -> Option<&Value> {
    if n.get("type").and_then(Value::as_str) == Some("tabs") {
        return Some(n);
    }
    n.get("children")?.as_array()?.iter().find_map(first_tabs_group)
}

fn first_tabs_group_mut(n: &mut Value) -> Option<&mut Value> {
    if n.get("type").and_then(Value::as_str) == Some("tabs") {
        return Some(n);
    }
    n.get_mut("children")?.as_array_mut()?.iter_mut().find_map(first_tabs_group_mut)
}

fn leaf_file(leaf: &Value) -> Option<&str> {
    let t = leaf.pointer("/state/type")?.as_str()?;
    if t != "markdown" && t != "image" {
        return None;
    }
    leaf.pointer("/state/state/file")?.as_str()
}

fn leaf_view(leaf: &Value) -> ViewMode {
    let st = leaf.pointer("/state/state");
    match st.and_then(|s| s.get("mode")).and_then(Value::as_str) {
        Some("preview") => ViewMode::Reading,
        _ if st.and_then(|s| s.get("source")).and_then(Value::as_bool) == Some(true) => ViewMode::Source,
        _ => ViewMode::Live,
    }
}

fn parse(v: &Value) -> Option<Ws> {
    let group = first_tabs_group(v.get("main")?)?;
    let children = group.get("children")?.as_array()?;
    let mut tabs = vec![];
    let mut active = 0;
    let cur = group.get("currentTab").and_then(Value::as_u64).unwrap_or(0) as usize;
    for (i, leaf) in children.iter().enumerate() {
        if let Some(p) = leaf_file(leaf) {
            if i == cur {
                active = tabs.len();
            }
            tabs.push(WsTab { path: p.to_string(), view: leaf_view(leaf), pinned: leaf.get("pinned").and_then(Value::as_bool).unwrap_or(false) });
        }
    }
    if tabs.is_empty() {
        return None;
    }
    Some(Ws { tabs, active })
}

fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) as u64;
    let mut h = n ^ 0x9e3779b97f4a7c15;
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94d049bb133111eb);
    format!("{:016x}", h ^ (h >> 31))
}

fn apply_view(state: &mut Map<String, Value>, view: ViewMode) {
    let (mode, source) = match view {
        ViewMode::Reading => ("preview", false),
        ViewMode::Source => ("source", true),
        ViewMode::Live => ("source", false),
    };
    state.insert("mode".into(), json!(mode));
    state.insert("source".into(), json!(source));
}

/// Merge `ws` into the JSON document. Returns false if there is nothing to merge into.
fn merge(doc: &mut Value, ws: &Ws, is_image: &dyn Fn(&str) -> bool) -> bool {
    let Some(group) = doc.get_mut("main").and_then(first_tabs_group_mut) else { return false };
    let old: Vec<Value> = group.get("children").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut pool: Vec<Option<Value>> = old.into_iter().map(Some).collect();
    let mut out = vec![];
    let mut active_id = None;
    for (i, t) in ws.tabs.iter().enumerate() {
        let hit = pool.iter().position(|l| l.as_ref().and_then(leaf_file) == Some(t.path.as_str()));
        let mut leaf = hit.and_then(|h| pool[h].take()).unwrap_or_else(|| {
            let title = Path::new(&t.path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if is_image(&t.path) {
                json!({"id": new_id(), "type": "leaf", "state": {"type": "image", "state": {"file": t.path}, "icon": "lucide-image", "title": t.path.rsplit('/').next().unwrap_or("")}})
            } else {
                json!({"id": new_id(), "type": "leaf", "state": {"type": "markdown", "state": {"file": t.path}, "icon": "lucide-file", "title": title}})
            }
        });
        if leaf.pointer("/state/type").and_then(Value::as_str) == Some("markdown") {
            if let Some(st) = leaf.pointer_mut("/state/state").and_then(Value::as_object_mut) {
                apply_view(st, t.view);
            }
        }
        if let Some(o) = leaf.as_object_mut() {
            if t.pinned {
                o.insert("pinned".into(), json!(true));
            } else {
                o.remove("pinned");
            }
        }
        if i == ws.active {
            active_id = leaf.get("id").cloned();
        }
        out.push(leaf);
    }
    // Keep tabs Carbon can't show (graph, canvas, …) so we don't destroy them.
    out.extend(pool.into_iter().flatten().filter(|l| leaf_file(l).is_none()));
    let cur = ws.active.min(ws.tabs.len().saturating_sub(1));
    let cur_idx = out.iter().position(|l| l.get("id") == active_id.as_ref()).unwrap_or(cur);
    let g = group.as_object_mut().unwrap();
    g.insert("children".into(), Value::Array(out));
    g.insert("currentTab".into(), json!(cur_idx));
    if let (Some(id), Some(o)) = (active_id, doc.as_object_mut()) {
        o.insert("active".into(), id);
    }
    if let Some(a) = ws.tabs.get(ws.active) {
        if let Some(o) = doc.as_object_mut() {
            let mut last: Vec<Value> = o.get("lastOpenFiles").and_then(Value::as_array).cloned().unwrap_or_default();
            last.retain(|v| v.as_str() != Some(a.path.as_str()));
            last.insert(0, json!(a.path));
            last.truncate(20);
            o.insert("lastOpenFiles".into(), Value::Array(last));
        }
    }
    true
}

/// Write `ws` into the vault's workspace.json. Returns true if the file changed.
pub fn write(root: &Path, ws: &Ws, is_image: &dyn Fn(&str) -> bool) -> bool {
    if ws.tabs.is_empty() {
        return false;
    }
    let f = file(root);
    let Some(mut doc) = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else { return false };
    let before = doc.clone();
    if !merge(&mut doc, ws, is_image) || doc == before {
        return false;
    }
    match serde_json::to_string_pretty(&doc) {
        Ok(s) => std::fs::write(&f, s).is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({
            "main": {"id": "m", "type": "split", "children": [{"id": "g", "type": "tabs", "currentTab": 1, "children": [
                {"id": "a", "type": "leaf", "state": {"type": "markdown", "state": {"file": "a.md", "mode": "source", "source": false}}},
                {"id": "b", "type": "leaf", "state": {"type": "markdown", "state": {"file": "b.md", "mode": "preview", "source": false}}},
                {"id": "gr", "type": "leaf", "state": {"type": "graph", "state": {}}}
            ]}]},
            "left": {"id": "l"}, "active": "b", "lastOpenFiles": ["b.md", "a.md"]
        })
    }

    #[test]
    fn parses_tabs_and_active() {
        let ws = parse(&sample()).unwrap();
        assert_eq!(ws.tabs.len(), 2);
        assert_eq!(ws.active, 1);
        assert_eq!(ws.tabs[1].view, ViewMode::Reading);
        assert_eq!(ws.tabs[0].view, ViewMode::Live);
    }

    #[test]
    fn merge_preserves_foreign_and_reorders() {
        let mut d = sample();
        let ws = Ws {
            tabs: vec![
                WsTab { path: "c.png".into(), view: ViewMode::Live, pinned: false },
                WsTab { path: "a.md".into(), view: ViewMode::Source, pinned: true },
            ],
            active: 0,
        };
        assert!(merge(&mut d, &ws, &|p| p.ends_with(".png")));
        let back = parse(&d).unwrap();
        assert_eq!(back.tabs.iter().map(|t| t.path.as_str()).collect::<Vec<_>>(), ["c.png", "a.md"]);
        assert_eq!(back.active, 0);
        assert!(back.tabs[1].pinned);
        assert_eq!(back.tabs[1].view, ViewMode::Source);
        let kids = d.pointer("/main/children/0/children").unwrap().as_array().unwrap();
        assert_eq!(kids.len(), 3); // graph leaf kept
        assert_eq!(d["left"]["id"], "l");
        assert_eq!(d["lastOpenFiles"][0], "c.png");
        assert_eq!(kids[1]["id"], "a"); // existing leaf reused
    }
}
