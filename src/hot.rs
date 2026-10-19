//! inotify-driven hot reload for the Omarchy theme, Carbon's config and the custom cursor shader.
//! No polling: the app sleeps until the kernel reports a change.

use crate::config::config_dir;
use crate::theme::omarchy_current_dir;
use eframe::egui;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::{channel, Receiver};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotEvent {
    Theme,
    Config,
    Shader,
}

pub struct Hot {
    rx: Receiver<HotEvent>,
    _watchers: Vec<RecommendedWatcher>,
}

impl Hot {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        let mut watchers = Vec::new();

        let mut watch = |dir: std::path::PathBuf, classify: fn(&std::path::Path) -> Option<HotEvent>| {
            if !dir.exists() {
                let _ = std::fs::create_dir_all(&dir);
            }
            let tx = tx.clone();
            let ctx = ctx.clone();
            let w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(ev) = res {
                    if ev.kind.is_access() {
                        return;
                    }
                    for p in &ev.paths {
                        if let Some(e) = classify(p) {
                            let _ = tx.send(e);
                            ctx.request_repaint();
                        }
                    }
                }
            });
            if let Ok(mut w) = w {
                if w.watch(&dir, RecursiveMode::NonRecursive).is_ok() {
                    watchers.push(w);
                }
            }
        };

        // `omarchy theme set` rewrites `theme.name` and swaps the `theme` directory in here.
        watch(omarchy_current_dir(), |_| Some(HotEvent::Theme));
        watch(config_dir(), |p| match p.file_name()?.to_str()? {
            "config.json" => Some(HotEvent::Config),
            "cursor.wgsl" => Some(HotEvent::Shader),
            _ => None,
        });

        Self { rx, _watchers: watchers }
    }

    /// Drain pending events (deduplicated).
    pub fn poll(&self) -> Vec<HotEvent> {
        let mut out = Vec::new();
        while let Ok(e) = self.rx.try_recv() {
            if !out.contains(&e) {
                out.push(e);
            }
        }
        out
    }
}
