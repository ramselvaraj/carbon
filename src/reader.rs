//! Reading view: markdown → block tree (pulldown-cmark + Obsidian extras) → egui widgets.

use crate::editor::{heading_scale, LinkRef};
use crate::fonts;
use crate::icons::{self, Icon};
use crate::theme::{mix, with_alpha, Theme};
use crate::vault::{FileKind, PropValue, Vault};
use eframe::egui::{
    self, text::LayoutJob, text::TextFormat, text_selection::LabelSelectionState, Align, Color32, FontFamily, FontId, Label, Rect, RichText,
    Sense, Stroke, Ui, Vec2,
};
use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub mark: bool,
}

#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub st: Style,
    pub link: Option<LinkRef>,
}

#[derive(Clone, Debug)]
pub enum Inline {
    Text(Span),
    Image { target: String, alt: String, width: Option<f32>, wiki: bool },
}

#[derive(Clone, Debug)]
pub struct Item {
    pub task: Option<(bool, usize)>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Heading { level: usize, spans: Vec<Span> },
    Para(Vec<Inline>),
    Code { lang: String, text: String },
    Quote(Vec<Block>),
    Callout { kind: String, title: Vec<Span>, fold: Option<bool>, body: Vec<Block> },
    List { start: Option<u64>, items: Vec<Item> },
    Table { head: Vec<Vec<Span>>, rows: Vec<Vec<Vec<Span>>> },
    Rule,
    Footnote { label: String, blocks: Vec<Block> },
}

#[derive(Default, Debug)]
pub struct Doc {
    pub blocks: Vec<Block>,
    pub props: Vec<(String, PropValue)>,
}

// ---------------------------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------------------------

type Ev<'a> = (Event<'a>, Range<usize>);

struct P<'a> {
    ev: Vec<Ev<'a>>,
    p: usize,
}

pub fn parse(src: &str) -> Doc {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_MATH
        | Options::ENABLE_WIKILINKS;
    let ev: Vec<Ev> = Parser::new_ext(src, opts).into_offset_iter().collect();
    let mut p = P { ev, p: 0 };
    let mut doc = Doc::default();
    doc.blocks = p.blocks(&mut doc.props, false);
    doc
}

fn block_start(t: &Tag) -> bool {
    matches!(
        t,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::HtmlBlock
            | Tag::List(_)
            | Tag::Item
            | Tag::FootnoteDefinition(_)
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::MetadataBlock(_)
    )
}

impl<'a> P<'a> {
    fn blocks(&mut self, props: &mut Vec<(String, PropValue)>, nested: bool) -> Vec<Block> {
        let mut out = vec![];
        let mut pending_task: Option<(bool, usize)> = None;
        let _ = &mut pending_task;
        while self.p < self.ev.len() {
            let (e, range) = self.ev[self.p].clone();
            match e {
                Event::End(_) => {
                    if nested {
                        self.p += 1;
                        return out;
                    }
                    self.p += 1;
                }
                Event::Start(Tag::Paragraph) => {
                    self.p += 1;
                    let inl = self.inlines(Style::default(), None, false);
                    out.extend(split_para(inl));
                }
                Event::Start(Tag::Heading { level, .. }) => {
                    self.p += 1;
                    let inl = self.inlines(Style::default(), None, false);
                    out.push(Block::Heading { level: level as usize, spans: only_spans(inl) });
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    self.p += 1;
                    let inner = self.blocks(props, true);
                    out.push(make_callout(inner));
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    self.p += 1;
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => l.split_whitespace().next().unwrap_or("").to_string(),
                        _ => String::new(),
                    };
                    let mut text = String::new();
                    while self.p < self.ev.len() {
                        match &self.ev[self.p].0 {
                            Event::Text(t) => text.push_str(t),
                            Event::End(TagEnd::CodeBlock) => {
                                self.p += 1;
                                break;
                            }
                            _ => {}
                        }
                        self.p += 1;
                    }
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    out.push(Block::Code { lang, text });
                }
                Event::Start(Tag::List(start)) => {
                    self.p += 1;
                    let mut items = vec![];
                    while self.p < self.ev.len() {
                        match &self.ev[self.p].0 {
                            Event::Start(Tag::Item) => {
                                self.p += 1;
                                let mut task = None;
                                if let Event::TaskListMarker(c) = self.ev[self.p].0 {
                                    task = Some((c, self.ev[self.p].1.start + 1));
                                    self.p += 1;
                                }
                                let blocks = self.blocks(props, true);
                                items.push(Item { task, blocks });
                            }
                            Event::End(TagEnd::List(_)) => {
                                self.p += 1;
                                break;
                            }
                            _ => self.p += 1,
                        }
                    }
                    out.push(Block::List { start, items });
                }
                Event::Start(Tag::Table(_)) => {
                    self.p += 1;
                    let (mut head, mut rows) = (vec![], vec![]);
                    let mut in_head = false;
                    let mut cur: Vec<Vec<Span>> = vec![];
                    while self.p < self.ev.len() {
                        match self.ev[self.p].0.clone() {
                            Event::Start(Tag::TableHead) => {
                                in_head = true;
                                cur = vec![];
                                self.p += 1;
                            }
                            Event::End(TagEnd::TableHead) => {
                                head = std::mem::take(&mut cur);
                                in_head = false;
                                self.p += 1;
                            }
                            Event::Start(Tag::TableRow) => {
                                cur = vec![];
                                self.p += 1;
                            }
                            Event::End(TagEnd::TableRow) => {
                                if !in_head {
                                    rows.push(std::mem::take(&mut cur));
                                }
                                self.p += 1;
                            }
                            Event::Start(Tag::TableCell) => {
                                self.p += 1;
                                let inl = self.inlines(Style::default(), None, false);
                                cur.push(only_spans(inl));
                            }
                            Event::End(TagEnd::Table) => {
                                self.p += 1;
                                break;
                            }
                            _ => self.p += 1,
                        }
                    }
                    out.push(Block::Table { head, rows });
                }
                Event::Start(Tag::MetadataBlock(_)) => {
                    self.p += 1;
                    let mut text = String::new();
                    while self.p < self.ev.len() {
                        match &self.ev[self.p].0 {
                            Event::Text(t) => text.push_str(t),
                            Event::End(TagEnd::MetadataBlock(_)) => {
                                self.p += 1;
                                break;
                            }
                            _ => {}
                        }
                        self.p += 1;
                    }
                    *props = crate::vault::parse_properties(&text);
                }
                Event::Start(Tag::HtmlBlock) => {
                    self.p += 1;
                    while self.p < self.ev.len() {
                        let end = matches!(self.ev[self.p].0, Event::End(TagEnd::HtmlBlock));
                        self.p += 1;
                        if end {
                            break;
                        }
                    }
                }
                Event::Start(Tag::FootnoteDefinition(label)) => {
                    self.p += 1;
                    let inner = self.blocks(props, true);
                    out.push(Block::Footnote { label: label.to_string(), blocks: inner });
                }
                Event::Rule => {
                    self.p += 1;
                    out.push(Block::Rule);
                }
                Event::Start(ref t) if block_start(t) => {
                    self.p += 1; // unknown container: flatten
                }
                Event::DisplayMath(m) => {
                    self.p += 1;
                    out.push(Block::Code { lang: "math".into(), text: m.trim().to_string() });
                }
                _ => {
                    // Tight list items etc: bare inline content.
                    let inl = self.inlines(Style::default(), None, true);
                    if inl.is_empty() {
                        self.p += 1;
                    } else {
                        out.extend(split_para(inl));
                    }
                }
            }
            let _ = range;
        }
        out
    }

    /// Collect inline content. `implicit` = we're not inside an explicit container tag, so stop
    /// (without consuming) at the first End or block start.
    fn inlines(&mut self, st: Style, link: Option<LinkRef>, implicit: bool) -> Vec<Inline> {
        let mut out: Vec<Inline> = vec![];
        while self.p < self.ev.len() {
            let e = self.ev[self.p].0.clone();
            match e {
                Event::End(_) => {
                    if !implicit {
                        self.p += 1;
                    }
                    return out;
                }
                Event::Start(ref t) if block_start(t) => {
                    if implicit {
                        return out;
                    }
                    self.p += 1;
                }
                Event::Text(t) => {
                    self.p += 1;
                    push_text(&mut out, &t, &st, &link);
                }
                Event::Code(c) => {
                    self.p += 1;
                    push_span(&mut out, &c, Style { code: true, ..st.clone() }, link.clone());
                }
                Event::InlineMath(m) | Event::DisplayMath(m) => {
                    self.p += 1;
                    push_span(&mut out, &m, Style { code: true, ..st.clone() }, link.clone());
                }
                Event::SoftBreak | Event::HardBreak => {
                    self.p += 1;
                    push_span(&mut out, "\n", st.clone(), link.clone());
                }
                Event::FootnoteReference(l) => {
                    self.p += 1;
                    push_span(&mut out, &format!("[{l}]"), st.clone(), None);
                }
                Event::InlineHtml(h) | Event::Html(h) => {
                    self.p += 1;
                    let t = h.trim().to_ascii_lowercase();
                    if t.starts_with("<br") {
                        push_span(&mut out, "\n", st.clone(), None);
                    }
                }
                Event::Start(Tag::Emphasis) => {
                    self.p += 1;
                    out.extend(self.inlines(Style { italic: true, ..st.clone() }, link.clone(), false));
                }
                Event::Start(Tag::Strong) => {
                    self.p += 1;
                    out.extend(self.inlines(Style { bold: true, ..st.clone() }, link.clone(), false));
                }
                Event::Start(Tag::Strikethrough) => {
                    self.p += 1;
                    out.extend(self.inlines(Style { strike: true, ..st.clone() }, link.clone(), false));
                }
                Event::Start(Tag::Link { link_type, dest_url, .. }) => {
                    self.p += 1;
                    let l = link_ref(&dest_url, link_type);
                    let mut inner = self.inlines(st.clone(), Some(l), false);
                    if matches!(link_type, LinkType::WikiLink { has_pothole: false }) {
                        for i in &mut inner {
                            if let Inline::Text(s) = i {
                                s.text = s.text.replace('#', " > ");
                            }
                        }
                    }
                    out.extend(inner);
                }
                Event::Start(Tag::Image { link_type, dest_url, .. }) => {
                    self.p += 1;
                    let alt_inl = self.inlines(Style::default(), None, false);
                    let alt: String = only_spans(alt_inl).iter().map(|s| s.text.as_str()).collect();
                    let wiki = link_type == LinkType::WikiLink { has_pothole: true } || link_type == LinkType::WikiLink { has_pothole: false };
                    let mut target = if wiki { dest_url.to_string() } else { crate::vault::decode_path(&dest_url) };
                    let mut width = None;
                    let mut alt_out = alt.clone();
                    if wiki {
                        if let Some((w, _)) = alt.split_once('x').or(Some((alt.as_str(), ""))) {
                            if let Ok(v) = w.trim().parse::<f32>() {
                                width = Some(v);
                                alt_out = String::new();
                            }
                        }
                    } else if let Some((a, w)) = alt.rsplit_once('|') {
                        if let Ok(v) = w.trim().parse::<f32>() {
                            width = Some(v);
                            alt_out = a.to_string();
                        }
                    }
                    if target.contains("://") {
                        // keep as-is: external
                    } else {
                        target = target.trim().to_string();
                    }
                    out.push(Inline::Image { target, alt: alt_out, width, wiki });
                }
                _ => self.p += 1,
            }
        }
        out
    }
}

fn link_ref(dest: &str, lt: LinkType) -> LinkRef {
    if matches!(lt, LinkType::WikiLink { .. }) {
        let (t, h) = match dest.split_once('#') {
            Some((a, b)) => (a.trim().to_string(), Some(b.trim().to_string())),
            None => (dest.trim().to_string(), None),
        };
        return LinkRef::Wiki { target: t, heading: h, embed: false };
    }
    if dest.contains("://") || dest.starts_with("mailto:") {
        return LinkRef::Url(dest.to_string());
    }
    let d = crate::vault::decode_path(dest);
    let (t, h) = match d.split_once('#') {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (d, None),
    };
    LinkRef::Wiki { target: t, heading: h, embed: false }
}

fn push_span(out: &mut Vec<Inline>, text: &str, st: Style, link: Option<LinkRef>) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text(last)) = out.last_mut() {
        if last.st == st && last.link == link {
            last.text.push_str(text);
            return;
        }
    }
    out.push(Inline::Text(Span { text: text.to_string(), st, link }));
}

/// Plain text with Obsidian extras: `#tags`, `==highlight==`, `%%comments%%`.
fn push_text(out: &mut Vec<Inline>, t: &str, st: &Style, link: &Option<LinkRef>) {
    if link.is_some() || !(t.contains('#') || t.contains("==") || t.contains("%%")) {
        push_span(out, t, st.clone(), link.clone());
        return;
    }
    let mut rest = t;
    let mut mark = st.mark;
    let mut prev: Option<char> = out.last().and_then(|i| match i {
        Inline::Text(s) => s.text.chars().next_back(),
        _ => None,
    });
    while !rest.is_empty() {
        // next special position
        let mut idx = None;
        for (i, c) in rest.char_indices() {
            let pc = if i == 0 { prev } else { rest[..i].chars().next_back() };
            if (c == '#' && pc.map(|p| p.is_whitespace()).unwrap_or(true)) || rest[i..].starts_with("==") || rest[i..].starts_with("%%") {
                idx = Some(i);
                break;
            }
        }
        let Some(i) = idx else {
            push_span(out, rest, Style { mark, ..st.clone() }, None);
            break;
        };
        if i > 0 {
            push_span(out, &rest[..i], Style { mark, ..st.clone() }, None);
        }
        let r = &rest[i..];
        if r.starts_with("%%") {
            match r[2..].find("%%") {
                Some(e) => rest = &r[2 + e + 2..],
                None => rest = "",
            }
        } else if r.starts_with("==") {
            if mark || r[2..].contains("==") {
                mark = !mark;
                rest = &r[2..];
            } else {
                push_span(out, "==", Style { mark, ..st.clone() }, None);
                rest = &r[2..];
            }
        } else {
            let tag: String = r[1..].chars().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/')).collect();
            if !tag.is_empty() && tag.chars().any(|c| !c.is_ascii_digit()) {
                let full = format!("#{tag}");
                out.push(Inline::Text(Span { text: full.clone(), st: Style { mark, ..st.clone() }, link: Some(LinkRef::Tag(tag.clone())) }));
                rest = &r[1 + tag.len()..];
            } else {
                push_span(out, "#", Style { mark, ..st.clone() }, None);
                rest = &r[1..];
            }
        }
        prev = out.last().and_then(|i| match i {
            Inline::Text(s) => s.text.chars().next_back(),
            _ => None,
        });
    }
}

fn only_spans(inl: Vec<Inline>) -> Vec<Span> {
    inl.into_iter()
        .filter_map(|i| match i {
            Inline::Text(s) => Some(s),
            Inline::Image { alt, .. } => Some(Span { text: alt, st: Style::default(), link: None }),
        })
        .collect()
}

/// Paragraph → one or more blocks (block ids stripped).
fn split_para(mut inl: Vec<Inline>) -> Vec<Block> {
    if let Some(Inline::Text(last)) = inl.last_mut() {
        let t = last.text.trim_end();
        if let Some(p) = t.rfind(" ^") {
            if t[p + 2..].chars().all(|c| c.is_alphanumeric() || c == '-') && p + 2 < t.len() {
                last.text.truncate(p);
            }
        } else if t.starts_with('^') && t[1..].chars().all(|c| c.is_alphanumeric() || c == '-') && t.len() > 1 {
            last.text.clear();
        }
    }
    inl.retain(|i| !matches!(i, Inline::Text(s) if s.text.is_empty()));
    if inl.is_empty() {
        return vec![];
    }
    vec![Block::Para(inl)]
}

/// Turn a blockquote into an Obsidian callout when it starts with `[!type]`.
fn make_callout(mut inner: Vec<Block>) -> Block {
    if let Some(Block::Para(inl)) = inner.first() {
        if let Some(Inline::Text(first)) = inl.first() {
            let t = &first.text;
            if t.starts_with("[!") {
                if let Some(end) = t.find(']') {
                    let kind = t[2..end].to_lowercase();
                    let after = &t[end + 1..];
                    let (fold, after) = match after.chars().next() {
                        Some('+') => (Some(false), &after[1..]),
                        Some('-') => (Some(true), &after[1..]),
                        _ => (None, after),
                    };
                    // Title is the rest of the first line.
                    let mut title: Vec<Span> = vec![];
                    let mut body_inl: Vec<Inline> = vec![];
                    let mut in_title = true;
                    let mut first_piece = Some(after.to_string());
                    for (i, it) in inl.iter().enumerate() {
                        let Inline::Text(s) = it else {
                            if !in_title {
                                body_inl.push(it.clone());
                            }
                            continue;
                        };
                        let text = if i == 0 { first_piece.take().unwrap() } else { s.text.clone() };
                        if in_title {
                            if let Some(nl) = text.find('\n') {
                                title.push(Span { text: text[..nl].trim_start().to_string(), ..s.clone() });
                                in_title = false;
                                let rest = &text[nl + 1..];
                                if !rest.is_empty() {
                                    body_inl.push(Inline::Text(Span { text: rest.to_string(), ..s.clone() }));
                                }
                            } else {
                                let tt = if title.is_empty() { text.trim_start().to_string() } else { text };
                                title.push(Span { text: tt, ..s.clone() });
                            }
                        } else {
                            body_inl.push(Inline::Text(s.clone()));
                        }
                    }
                    let mut body: Vec<Block> = vec![];
                    if !body_inl.is_empty() {
                        body.push(Block::Para(body_inl));
                    }
                    body.extend(inner.drain(1..));
                    title.retain(|s| !s.text.is_empty());
                    return Block::Callout { kind, title, fold, body };
                }
            }
        }
    }
    Block::Quote(inner)
}

// ---------------------------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------------------------

pub enum Action {
    Open { target: String, heading: Option<String>, new_tab: bool },
    Url(String),
    Tag(String),
    ToggleTask(usize),
    OpenFile(String),
}

#[derive(Default)]
pub struct DocCache {
    map: HashMap<String, (std::time::SystemTime, Arc<Doc>)>,
}

impl DocCache {
    pub fn get(&mut self, vault: &Vault, rel: &str) -> Option<Arc<Doc>> {
        let path = vault.abs(rel);
        let mtime = std::fs::metadata(&path).ok()?.modified().ok()?;
        if let Some((m, d)) = self.map.get(rel) {
            if *m == mtime {
                return Some(d.clone());
            }
        }
        let text = std::fs::read_to_string(&path).ok()?;
        let doc = Arc::new(parse(&text));
        self.map.insert(rel.to_string(), (mtime, doc.clone()));
        Some(doc)
    }
}

pub struct ReadCtx<'a> {
    pub vault: &'a Vault,
    pub theme: &'a Theme,
    pub size: f32,
    pub note_rel: &'a str,
    pub actions: &'a mut Vec<Action>,
    pub docs: &'a mut DocCache,
    pub scroll_to: &'a mut Option<String>,
    pub depth: usize,
    pub width: f32,
}

pub fn render_doc(ui: &mut Ui, doc: &Doc, cx: &mut ReadCtx) {
    if !doc.props.is_empty() && cx.depth == 0 {
        render_props(ui, &doc.props, cx);
        ui.add_space(cx.size * 0.8);
    }
    render_blocks(ui, &doc.blocks, cx, 0);
}

fn render_props(ui: &mut Ui, props: &[(String, PropValue)], cx: &mut ReadCtx) {
    let th = cx.theme;
    egui::Frame::new()
        .fill(with_alpha(th.bg_darker, 0.35))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("props").num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                for (k, v) in props {
                    ui.label(RichText::new(k).color(th.fg_dim).size(cx.size * 0.9));
                    match v {
                        PropValue::Text(t) => {
                            ui.label(RichText::new(t).size(cx.size * 0.92));
                        }
                        PropValue::List(items) => {
                            ui.horizontal_wrapped(|ui| {
                                for it in items {
                                    let is_tag = k.eq_ignore_ascii_case("tags") || k.eq_ignore_ascii_case("tag");
                                    let txt = if is_tag { format!("#{}", it.trim_start_matches('#')) } else { it.clone() };
                                    let r = ui.add(
                                        egui::Button::new(RichText::new(&txt).color(th.accent).size(cx.size * 0.85))
                                            .fill(with_alpha(th.accent, 0.14))
                                            .corner_radius(10),
                                    );
                                    if is_tag && r.clicked() {
                                        cx.actions.push(Action::Tag(it.trim_start_matches('#').to_string()));
                                    }
                                }
                            });
                        }
                    }
                    ui.end_row();
                }
            });
        });
}

fn render_blocks(ui: &mut Ui, blocks: &[Block], cx: &mut ReadCtx, list_depth: usize) {
    let gap = cx.size * 0.65;
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            ui.add_space(if matches!(b, Block::Heading { .. }) { gap * 1.4 } else { gap });
        }
        render_block(ui, b, cx, list_depth);
    }
}

fn render_block(ui: &mut Ui, b: &Block, cx: &mut ReadCtx, list_depth: usize) {
    let th = cx.theme;
    match b {
        Block::Heading { level, spans } => {
            let scale = heading_scale(*level);
            let r = rich(ui, &spans_to_inlines(spans), cx, scale, true, th.fg_bright);
            let title: String = spans.iter().map(|s| s.text.as_str()).collect();
            if let Some(want) = cx.scroll_to.as_ref() {
                if want.eq_ignore_ascii_case(title.trim()) {
                    ui.scroll_to_rect(r.rect, Some(Align::TOP));
                    *cx.scroll_to = None;
                }
            }
            if *level <= 2 {
                let y = r.rect.bottom() + 3.0;
                ui.painter().hline(r.rect.left()..=r.rect.left() + ui.available_width(), y, Stroke::new(1.0, with_alpha(th.fg, 0.1)));
            }
        }
        Block::Para(inl) => {
            para(ui, inl, cx, 1.0, th.fg);
        }
        Block::Code { lang, text } => code_block(ui, lang, text, cx),
        Block::Quote(inner) => {
            let f = egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 0, top: 0, bottom: 0 });
            let r = f.show(ui, |ui| {
                ui.set_width(ui.available_width());
                render_blocks(ui, inner, cx, list_depth);
            });
            let x = r.response.rect.left() + 3.0;
            ui.painter().vline(x, r.response.rect.y_range(), Stroke::new(3.0, with_alpha(th.accent, 0.6)));
        }
        Block::Callout { kind, title, fold, body } => callout(ui, kind, title, *fold, body, cx, list_depth),
        Block::List { start, items } => {
            for (n, it) in items.iter().enumerate() {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.add_space(cx.size * 0.4 + list_depth as f32 * 2.0);
                    if let Some((done, off)) = it.task {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(cx.size * 0.95), Sense::click());
                        let r = rect.shrink(1.5).translate(Vec2::new(0.0, cx.size * 0.2));
                        let col = if done { th.accent } else { with_alpha(th.fg, 0.5) };
                        if done {
                            ui.painter().rect_filled(r, 3.0, col);
                            let c = r.center();
                            let s = r.width() * 0.25;
                            ui.painter().line(vec![c + Vec2::new(-s * 1.2, 0.0), c + Vec2::new(-s * 0.3, s), c + Vec2::new(s * 1.3, -s)], Stroke::new(1.8, th.bg));
                        } else {
                            ui.painter().rect_stroke(r, 3.0, Stroke::new(1.3, col), egui::StrokeKind::Inside);
                        }
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            cx.actions.push(Action::ToggleTask(off));
                        }
                    } else {
                        let marker = match start {
                            Some(s) => format!("{}.", s + n as u64),
                            None => ["•", "◦", "▪"][list_depth % 3].to_string(),
                        };
                        ui.label(RichText::new(marker).color(th.fg_dim).size(cx.size));
                    }
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        let done = it.task.map(|t| t.0).unwrap_or(false);
                        if done {
                            // Completed tasks read dimmed + struck.
                            render_blocks_dim(ui, &it.blocks, cx, list_depth + 1);
                        } else {
                            render_blocks(ui, &it.blocks, cx, list_depth + 1);
                        }
                    });
                });
                ui.add_space(cx.size * 0.12);
            }
        }
        Block::Table { head, rows } => table(ui, head, rows, cx),
        Block::Rule => {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 10.0), Sense::hover());
            ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, with_alpha(th.fg, 0.2)));
        }
        Block::Footnote { label, blocks } => {
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(format!("[{label}]")).color(th.fg_dim).size(cx.size * 0.9));
                ui.vertical(|ui| render_blocks(ui, blocks, cx, list_depth));
            });
        }
    }
}

fn render_blocks_dim(ui: &mut Ui, blocks: &[Block], cx: &mut ReadCtx, depth: usize) {
    for b in blocks {
        match b {
            Block::Para(inl) => {
                let dimmed: Vec<Inline> = inl
                    .iter()
                    .map(|i| match i {
                        Inline::Text(s) => Inline::Text(Span { st: Style { strike: true, ..s.st.clone() }, ..s.clone() }),
                        o => o.clone(),
                    })
                    .collect();
                para(ui, &dimmed, cx, 1.0, cx.theme.fg_dim);
            }
            other => render_block(ui, other, cx, depth),
        }
    }
}

fn spans_to_inlines(spans: &[Span]) -> Vec<Inline> {
    spans.iter().cloned().map(Inline::Text).collect()
}

/// A paragraph: text runs as one wrapped label, images as separate widgets.
fn para(ui: &mut Ui, inl: &[Inline], cx: &mut ReadCtx, scale: f32, color: Color32) {
    let mut run: Vec<Inline> = vec![];
    for it in inl {
        match it {
            Inline::Text(_) => run.push(it.clone()),
            Inline::Image { target, alt, width, wiki } => {
                if !run.is_empty() {
                    rich(ui, &run, cx, scale, false, color);
                    run.clear();
                }
                embed(ui, target, alt, *width, *wiki, cx);
            }
        }
    }
    if !run.is_empty() {
        rich(ui, &run, cx, scale, false, color);
    }
}

fn span_format(th: &Theme, sp: &Span, size: f32, scale: f32, bold_all: bool, color: Color32) -> TextFormat {
    let st = &sp.st;
    let mut f = TextFormat {
        font_id: if st.code { FontId::new(size * 0.9 * scale, FontFamily::Monospace) } else { FontId::new(size * scale, fonts::family(st.bold || bold_all, st.italic)) },
        color: if st.bold || bold_all { mix(color, th.fg_bright, 0.6) } else { color },
        line_height: Some(size * scale * 1.55),
        ..Default::default()
    };
    if st.code {
        f.color = mix(th.fg, th.orange, 0.55);
        f.background = with_alpha(th.bg_darker, 0.65);
    }
    if st.strike {
        f.strikethrough = Stroke::new(1.0, th.fg_dim);
        f.color = th.fg_dim;
    }
    if st.mark {
        f.background = with_alpha(th.yellow, 0.35);
        f.color = th.fg_bright;
    }
    match &sp.link {
        Some(LinkRef::Tag(_)) => {
            f.color = th.accent;
            f.background = with_alpha(th.accent, 0.16);
        }
        Some(_) => {
            f.color = th.accent;
            f.underline = Stroke::new(1.0, with_alpha(th.accent, 0.7));
        }
        None => {}
    }
    f
}

fn rich(ui: &mut Ui, inl: &[Inline], cx: &mut ReadCtx, scale: f32, bold: bool, color: Color32) -> egui::Response {
    let th = cx.theme;
    let mut job = LayoutJob::default();
    let mut ranges: Vec<(Range<usize>, LinkRef)> = vec![];
    let mut chars = 0;
    for it in inl {
        if let Inline::Text(sp) = it {
            let n = sp.text.chars().count();
            if let Some(l) = &sp.link {
                ranges.push((chars..chars + n, l.clone()));
            }
            chars += n;
            job.append(&sp.text, 0.0, span_format(th, sp, cx.size, scale, bold, color));
        }
    }
    let (pos, galley, resp) = Label::new(job).sense(Sense::click()).layout_in_ui(ui);
    if ui.is_rect_visible(resp.rect) {
        LabelSelectionState::label_text_selection(ui, &resp, pos, galley.clone(), color, Stroke::NONE);
    }
    if !ranges.is_empty() {
        if let Some(hp) = resp.hover_pos() {
            let idx = galley.cursor_from_pos(hp - pos).index.0;
            if let Some((_, l)) = ranges.iter().find(|(r, _)| r.contains(&idx)) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if resp.clicked() || resp.middle_clicked() {
                    let new_tab = resp.middle_clicked() || ui.input(|i| i.modifiers.command);
                    match l {
                        LinkRef::Wiki { target, heading, .. } => cx.actions.push(Action::Open { target: target.clone(), heading: heading.clone(), new_tab }),
                        LinkRef::Url(u) => cx.actions.push(Action::Url(u.clone())),
                        LinkRef::Tag(t) => cx.actions.push(Action::Tag(t.clone())),
                    }
                }
            }
        }
    }
    resp
}

fn embed(ui: &mut Ui, target: &str, alt: &str, width: Option<f32>, wiki: bool, cx: &mut ReadCtx) {
    let th = cx.theme;
    if target.contains("://") {
        let l = format!("image: {}", if alt.is_empty() { target } else { alt });
        if ui.link(l).clicked() {
            cx.actions.push(Action::Url(target.to_string()));
        }
        return;
    }
    let (page, heading) = match target.split_once('#') {
        Some((a, b)) if wiki => (a, Some(b)),
        _ => (target, None),
    };
    let Some(f) = cx.vault.resolve(page, cx.note_rel) else {
        ui.label(RichText::new(format!("{target} not found")).color(th.red).size(cx.size * 0.9));
        return;
    };
    match f.kind {
        FileKind::Image => {
            let uri = format!("file://{}", cx.vault.abs(&f.rel).display());
            let w = width.unwrap_or(f32::INFINITY).min(ui.available_width());
            let img = egui::Image::new(uri).corner_radius(4).max_width(w).fit_to_original_size(1.0);
            let r = ui.add(img.sense(Sense::click()));
            if r.double_clicked() {
                cx.actions.push(Action::OpenFile(f.rel.clone()));
            }
        }
        FileKind::Note => {
            if cx.depth >= 3 {
                ui.label(RichText::new(format!("→ {}", f.stem())).color(th.fg_dim));
                return;
            }
            let rel = f.rel.clone();
            let stem = f.stem().to_string();
            let Some(doc) = cx.docs.get(cx.vault, &rel) else { return };
            let fr = egui::Frame::new()
                .fill(with_alpha(th.bg_darker, 0.3))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8));
            let r = fr.show(ui, |ui| {
                ui.set_width(ui.available_width());
                if ui.add(egui::Label::new(RichText::new(format!("→ {stem}")).color(th.fg_dim).size(cx.size * 0.85)).sense(Sense::click())).clicked() {
                    cx.actions.push(Action::Open { target: rel.clone(), heading: heading.map(|s| s.to_string()), new_tab: false });
                }
                ui.add_space(4.0);
                let blocks: &[Block] = match heading {
                    Some(h) => section(&doc.blocks, h),
                    None => &doc.blocks,
                };
                let note_rel = rel.clone();
                let mut inner = ReadCtx {
                    vault: cx.vault,
                    theme: cx.theme,
                    size: cx.size,
                    note_rel: &note_rel,
                    actions: cx.actions,
                    docs: cx.docs,
                    scroll_to: &mut None,
                    depth: cx.depth + 1,
                    width: cx.width,
                };
                render_blocks(ui, blocks, &mut inner, 0);
            });
            ui.painter().vline(r.response.rect.left(), r.response.rect.y_range(), Stroke::new(3.0, with_alpha(th.accent, 0.7)));
        }
        _ => {
            if ui.link(format!("attachment: {}", f.name)).clicked() {
                cx.actions.push(Action::OpenFile(f.rel.clone()));
            }
        }
    }
}

/// Blocks under a heading, up to the next heading of the same or higher level.
fn section<'a>(blocks: &'a [Block], heading: &str) -> &'a [Block] {
    let title = |spans: &Vec<Span>| spans.iter().map(|s| s.text.as_str()).collect::<String>();
    let Some(start) = blocks.iter().position(|b| matches!(b, Block::Heading { spans, .. } if title(spans).trim().eq_ignore_ascii_case(heading.trim()))) else {
        return blocks;
    };
    let lvl = if let Block::Heading { level, .. } = &blocks[start] { *level } else { 1 };
    let end = blocks[start + 1..]
        .iter()
        .position(|b| matches!(b, Block::Heading { level, .. } if *level <= lvl))
        .map(|p| start + 1 + p)
        .unwrap_or(blocks.len());
    &blocks[start..end]
}

pub fn callout_style(kind: &str, th: &Theme) -> (Color32, Icon) {
    match kind {
        "note" => (th.blue, Icon::Pencil),
        "abstract" | "summary" | "tldr" => (th.cyan, Icon::List),
        "info" => (th.blue, Icon::Info),
        "todo" => (th.blue, Icon::Check),
        "tip" | "hint" | "important" => (th.cyan, Icon::Flame),
        "success" | "check" | "done" => (th.green, Icon::Check),
        "question" | "help" | "faq" => (th.yellow, Icon::Question),
        "warning" | "caution" | "attention" => (th.orange, Icon::Warning),
        "failure" | "fail" | "missing" => (th.red, Icon::Close),
        "danger" | "error" => (th.red, Icon::Bolt),
        "bug" => (th.red, Icon::Bug),
        "example" => (th.magenta, Icon::List),
        "quote" | "cite" => (th.fg_dim, Icon::Quote),
        _ => (th.blue, Icon::Pencil),
    }
}

fn callout(ui: &mut Ui, kind: &str, title: &[Span], fold: Option<bool>, body: &[Block], cx: &mut ReadCtx, depth: usize) {
    let th = cx.theme;
    let (color, icon) = callout_style(kind, th);

    let mut title_txt: String = title.iter().map(|s| s.text.as_str()).collect();
    if title_txt.trim().is_empty() {
        let mut k = kind.to_string();
        if let Some(c) = k.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        title_txt = k;
    }
    let id = ui.id().with(("callout", kind, &title_txt, body.len()));
    let mut open = fold.map(|f| !f).unwrap_or(true);
    if fold.is_some() {
        open = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || open));
    }
    let fr = egui::Frame::new().fill(with_alpha(color, 0.10)).corner_radius(6).inner_margin(egui::Margin::symmetric(14, 10));
    fr.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let r = ui.horizontal(|ui| {
            {
                let (r, _) = ui.allocate_exact_size(egui::vec2(cx.size * 1.1, cx.size * 1.3), Sense::hover());
                icons::paint(ui.painter(), r.center(), cx.size * 1.05, icon, color);
            }
            let spans: Vec<Span> = if title.is_empty() { vec![Span { text: title_txt.clone(), st: Style { bold: true, ..Default::default() }, link: None }] } else { title.to_vec() };
            rich(ui, &spans_to_inlines(&spans), cx, 1.0, true, color);
            if fold.is_some() {
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
                    icons::paint(ui.painter(), r.center(), 14.0, if open { Icon::ChevronDown } else { Icon::ChevronRight }, th.fg_dim);
                });
            }
        });
        if fold.is_some() && ui.interact(r.response.rect, id.with("hdr"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_temp(id, open));
        }
        if open && !body.is_empty() {
            ui.add_space(cx.size * 0.4);
            render_blocks(ui, body, cx, depth);
        }
    });
}

fn table(ui: &mut Ui, head: &[Vec<Span>], rows: &[Vec<Vec<Span>>], cx: &mut ReadCtx) {
    let th = cx.theme;
    let ncols = head.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0)).max(1);
    let colw = (ui.available_width() / ncols as f32 - 14.0).max(60.0);
    egui::Frame::new().stroke(Stroke::new(1.0, with_alpha(th.fg, 0.14))).corner_radius(4).show(ui, |ui| {
        egui::Grid::new(ui.next_auto_id())
            .num_columns(ncols)
            .min_col_width(colw)
            .max_col_width(colw)
            .spacing([12.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for c in head {
                    ui.scope(|ui| {
                        rich(ui, &spans_to_inlines(c), cx, 1.0, true, th.fg_bright);
                    });
                }
                ui.end_row();
                for r in rows {
                    for c in r {
                        ui.scope(|ui| {
                            rich(ui, &spans_to_inlines(c), cx, 1.0, false, th.fg);
                        });
                    }
                    ui.end_row();
                }
            });
    });
}

fn code_block(ui: &mut Ui, lang: &str, text: &str, cx: &mut ReadCtx) {
    let th = cx.theme;
    let fr = egui::Frame::new().fill(with_alpha(th.bg_darker, 0.6)).corner_radius(6).inner_margin(egui::Margin::symmetric(14, 10));
    let r = fr.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut job = LayoutJob::default();
        highlight_code(&mut job, lang, text, th, cx.size * 0.9);
        job.wrap.max_width = f32::INFINITY;
        egui::ScrollArea::horizontal().auto_shrink([false, true]).id_salt(ui.next_auto_id()).show(ui, |ui| {
            ui.add(Label::new(job).selectable(true));
        });
    });
    let rect = r.response.rect;
    let hover = ui.rect_contains_pointer(rect);
    if hover {
        let br = Rect::from_min_size(rect.right_top() + Vec2::new(-64.0, 6.0), Vec2::new(58.0, 20.0));
        let label = if lang.is_empty() { "copy".to_string() } else { format!("{lang}  copy") };
        let br = Rect::from_min_size(br.right_top() - Vec2::new(ui.fonts_mut(|f| f.layout_no_wrap(label.clone(), FontId::proportional(11.0), th.fg_dim).size().x) + 12.0, 0.0), Vec2::new(0.0, 20.0));
        let _ = br;
        let resp = ui.put(
            Rect::from_min_size(rect.right_top() + Vec2::new(-98.0, 6.0), Vec2::new(92.0, 20.0)),
            egui::Button::new(RichText::new(label).size(11.0).color(th.fg_dim)).fill(with_alpha(th.bg, 0.5)).frame(true),
        );
        if resp.clicked() {
            ui.ctx().copy_text(text.to_string());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Lightweight, language-agnostic syntax colouring
// ---------------------------------------------------------------------------------------------

const KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod", "match", "if", "else", "elif", "for", "while", "loop", "in", "return", "const", "static",
    "def", "class", "import", "from", "as", "self", "this", "function", "var", "async", "await", "true", "false", "null", "None", "nil", "True", "False", "new", "type",
    "interface", "extends", "implements", "package", "public", "private", "protected", "void", "int", "bool", "string", "str", "char", "float", "double", "try", "catch",
    "except", "finally", "throw", "raise", "with", "yield", "lambda", "pass", "break", "continue", "switch", "case", "default", "do", "end", "then", "local", "export",
    "where", "dyn", "ref", "move", "crate", "super", "select", "insert", "update", "delete", "create", "table", "values", "set", "and", "or", "not", "echo", "fi", "done",
];

pub fn highlight_code(job: &mut LayoutJob, lang: &str, text: &str, th: &Theme, size: f32) {
    let font = FontId::new(size, FontFamily::Monospace);
    let fmt = |c: Color32| TextFormat { font_id: font.clone(), color: c, line_height: Some(size * 1.5), ..Default::default() };
    if lang == "math" || lang == "text" || lang == "txt" || lang == "plaintext" {
        job.append(text, 0.0, fmt(th.fg));
        return;
    }
    let hash_comment = matches!(lang, "py" | "python" | "sh" | "bash" | "zsh" | "fish" | "yaml" | "yml" | "toml" | "rb" | "ruby" | "conf" | "ini" | "dockerfile" | "make" | "nix" | "r");
    let dash_comment = matches!(lang, "sql" | "lua" | "hs" | "haskell");
    let slash_comment = !hash_comment && !dash_comment;
    let b = text.as_bytes();
    let mut i = 0;
    let mut plain_from = 0;
    macro_rules! flush {
        () => {
            if plain_from < i {
                job.append(&text[plain_from..i], 0.0, fmt(th.fg));
            }
        };
    }
    while i < b.len() {
        let rest = &text[i..];
        let c = b[i];
        let (end, color) = if (slash_comment && rest.starts_with("//")) || (hash_comment && c == b'#') || (dash_comment && rest.starts_with("--")) {
            (i + rest.find('\n').unwrap_or(rest.len()), th.fg_dim)
        } else if slash_comment && rest.starts_with("/*") {
            (i + rest.find("*/").map(|p| p + 2).unwrap_or(rest.len()), th.fg_dim)
        } else if c == b'"' || c == b'\'' && !lang.starts_with("rust") && lang != "rs" || c == b'`' {
            let mut j = i + 1;
            while j < b.len() && b[j] != c && b[j] != b'\n' {
                if b[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            ((j + 1).min(b.len()), th.green)
        } else if c.is_ascii_digit() && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) {
            let j = i + b[i..].iter().take_while(|x| x.is_ascii_alphanumeric() || **x == b'.' || **x == b'_').count();
            (j, th.orange)
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let j = i + b[i..].iter().take_while(|x| x.is_ascii_alphanumeric() || **x == b'_').count();
            let w = &text[i..j];
            let col = if KEYWORDS.contains(&w) {
                th.magenta
            } else if w.chars().next().unwrap().is_uppercase() {
                th.yellow
            } else if b.get(j) == Some(&b'(') {
                th.blue
            } else {
                // plain identifier
                i = j;
                continue;
            };
            (j, col)
        } else {
            i += text[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            continue;
        };
        flush!();
        job.append(&text[i..end], 0.0, fmt(color));
        i = end;
        plain_from = i;
    }
    flush!();
}
