//! Markdown editing: styled layout (Live / Source), inline Obsidian syntax scanning,
//! and the text-editing commands (list continuation, auto-pairing, formatting toggles).

use crate::fonts;
use crate::theme::{mix, with_alpha, Theme};
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange, LayoutJob, TextFormat},
    text_edit::TextEditState,
    Color32, FontFamily, FontId, Id, Key, Modifiers, Stroke,
};
use std::ops::Range;

// ---------------------------------------------------------------------------------------------
// Inline scanning
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum LinkRef {
    Wiki { target: String, heading: Option<String>, embed: bool },
    Url(String),
    Tag(String),
}

#[derive(Clone, Default, Debug)]
pub struct Fmt {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub mark: bool,
    pub code: bool,
    pub link: bool,
    pub tag: bool,
    /// Syntax characters (`**`, `[[`, …): dimmed in Source mode, hidden in Live mode.
    pub marker: bool,
    pub found: bool,
}

#[derive(Clone, Debug)]
pub struct Seg {
    pub range: Range<usize>,
    pub fmt: Fmt,
    pub link: Option<LinkRef>,
}

fn is_word(c: Option<char>) -> bool {
    c.map(|c| c.is_alphanumeric()).unwrap_or(false)
}

fn prev_char(s: &str, i: usize) -> Option<char> {
    s[..i].chars().next_back()
}

/// Splits one line (or the content part of it) into styled segments. `base` is the byte offset
/// of `line` within the real line so ranges stay line-relative.
pub fn inline_segments(line: &str, base: usize) -> Vec<Seg> {
    let b = line.as_bytes();
    let mut segs: Vec<Seg> = vec![];
    let mut cur = Fmt::default();
    let mut i = 0;
    let mut start = 0;

    macro_rules! flush {
        () => {
            if start < i {
                segs.push(Seg { range: base + start..base + i, fmt: cur.clone(), link: None });
            }
        };
    }
    macro_rules! push {
        ($r:expr, $f:expr, $l:expr) => {
            segs.push(Seg { range: base + $r.start..base + $r.end, fmt: $f, link: $l })
        };
    }
    let marker = |c: &Fmt| Fmt { marker: true, ..c.clone() };

    while i < b.len() {
        let c = b[i];
        match c {
            b'\\' if i + 1 < b.len() && b[i + 1].is_ascii_punctuation() => {
                i += 2;
                continue;
            }
            b'`' => {
                let run = b[i..].iter().take_while(|&&x| x == b'`').count();
                let needle = "`".repeat(run);
                if let Some(p) = line[i + run..].find(&needle) {
                    flush!();
                    let (a, z) = (i, i + run + p + run);
                    push!(a..a + run, Fmt { marker: true, code: true, ..cur.clone() }, None);
                    push!(a + run..z - run, Fmt { code: true, ..cur.clone() }, None);
                    push!(z - run..z, Fmt { marker: true, code: true, ..cur.clone() }, None);
                    i = z;
                    start = i;
                    continue;
                }
                i += run;
                continue;
            }
            b'!' | b'[' if line[i..].starts_with("[[") || line[i..].starts_with("![[") => {
                let embed = c == b'!';
                let open = i + if embed { 3 } else { 2 };
                if let Some(p) = line[open..].find("]]") {
                    flush!();
                    let inner = &line[open..open + p];
                    let (target_part, alias) = match inner.split_once('|') {
                        Some((t, a)) => (t, Some(a)),
                        None => (inner, None),
                    };
                    let (page, heading) = match target_part.split_once('#') {
                        Some((pg, h)) => (pg.trim().to_string(), Some(h.trim().to_string())),
                        None => (target_part.trim().to_string(), None),
                    };
                    let link = Some(LinkRef::Wiki { target: page, heading, embed });
                    let lf = Fmt { link: true, ..cur.clone() };
                    push!(i..open, marker(&cur), None);
                    match alias {
                        Some(a) => {
                            let tl = target_part.len() + 1;
                            push!(open..open + tl, marker(&cur), link.clone());
                            push!(open + tl..open + tl + a.len(), lf, link);
                        }
                        None => push!(open..open + inner.len(), lf, link),
                    }
                    push!(open + p..open + p + 2, marker(&cur), None);
                    i = open + p + 2;
                    start = i;
                    continue;
                }
            }
            b'[' => {
                // [text](url)
                if let Some(close) = line[i + 1..].find("](") {
                    let text = &line[i + 1..i + 1 + close];
                    if !text.contains('[') {
                        let url_start = i + 1 + close + 2;
                        if let Some(e) = line[url_start..].find(')') {
                            flush!();
                            let url = line[url_start..url_start + e].trim().to_string();
                            let link = Some(if url.contains("://") || url.starts_with("mailto:") {
                                LinkRef::Url(url)
                            } else {
                                let u = crate::vault::decode_path(url.trim_matches(|c| c == '<' || c == '>'));
                                let (pg, h) = match u.split_once('#') {
                                    Some((p, h)) => (p.to_string(), Some(h.to_string())),
                                    None => (u, None),
                                };
                                LinkRef::Wiki { target: pg, heading: h, embed: false }
                            });
                            push!(i..i + 1, marker(&cur), None);
                            push!(i + 1..i + 1 + close, Fmt { link: true, ..cur.clone() }, link.clone());
                            push!(i + 1 + close..url_start + e + 1, marker(&cur), link.clone());
                            i = url_start + e + 1;
                            start = i;
                            continue;
                        }
                    }
                }
            }
            b'*' | b'_' | b'~' | b'=' => {
                let double = b.get(i + 1) == Some(&c);
                let ch = c as char;
                let (len, kind) = if double { (2, c) } else if c == b'*' || c == b'_' { (1, c) } else { (0, c) };
                if len > 0 {
                    let marker_str: String = std::iter::repeat(ch).take(len).collect();
                    let is_on = match (kind, len) {
                        (b'~', _) => cur.strike,
                        (b'=', _) => cur.mark,
                        (_, 2) => cur.bold,
                        _ => cur.italic,
                    };
                    let after = line[i + len..].chars().next();
                    let before = prev_char(line, i);
                    let intraword = c == b'_' && (if is_on { is_word(after) } else { is_word(before) });
                    let can_open = !is_on
                        && !intraword
                        && after.map(|a| !a.is_whitespace()).unwrap_or(false)
                        && line[i + len..].find(&marker_str).is_some();
                    let can_close = is_on && !intraword && before.map(|p| !p.is_whitespace()).unwrap_or(false);
                    if can_open || can_close {
                        flush!();
                        push!(i..i + len, marker(&cur), None);
                        let on = !is_on;
                        match (kind, len) {
                            (b'~', _) => cur.strike = on,
                            (b'=', _) => cur.mark = on,
                            (_, 2) => cur.bold = on,
                            _ => cur.italic = on,
                        }
                        i += len;
                        start = i;
                        continue;
                    }
                }
            }
            b'#' => {
                let prev_ok = i == 0 || b[i - 1].is_ascii_whitespace();
                if prev_ok {
                    let tag: String = line[i + 1..].chars().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/')).collect();
                    if !tag.is_empty() && tag.chars().any(|c| !c.is_ascii_digit()) {
                        flush!();
                        let end = i + 1 + tag.len();
                        push!(i..end, Fmt { tag: true, ..cur.clone() }, None);
                        i = end;
                        start = i;
                        continue;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    flush!();
    segs
}

/// The link under a byte offset within a line, if any.
pub fn link_at(line: &str, byte: usize) -> Option<LinkRef> {
    inline_segments(line, 0).into_iter().find(|s| s.link.is_some() && s.range.contains(&byte)).and_then(|s| s.link)
}

/// Tag under a byte offset within a line.
pub fn tag_at(line: &str, byte: usize) -> Option<String> {
    inline_segments(line, 0)
        .into_iter()
        .find(|s| s.fmt.tag && s.range.contains(&byte))
        .map(|s| line[s.range.start + 1..s.range.end].to_string())
}

// ---------------------------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------------------------

pub struct LiveStyle<'a> {
    pub theme: &'a Theme,
    pub size: f32,
    pub live: bool,
    /// Inclusive line range that contains the caret/selection (shown raw in Live mode).
    pub active: (usize, usize),
    pub find: &'a str,
    /// Height reserved for an image embed on its own line (None = not loaded → show raw).
    pub image_height: &'a dyn Fn(&str, Option<f32>) -> Option<f32>,
}

pub fn heading_scale(level: usize) -> f32 {
    match level {
        1 => 1.9,
        2 => 1.55,
        3 => 1.3,
        4 => 1.15,
        _ => 1.05,
    }
}

const LINE_H: f32 = 1.55;

/// A line that is only an image embed: returns (target, width hint).
pub fn embed_only(line: &str) -> Option<(String, Option<f32>)> {
    let t = line.trim();
    if let Some(inner) = t.strip_prefix("![[").and_then(|r| r.strip_suffix("]]")) {
        if inner.contains("]]") || inner.contains("[[") {
            return None;
        }
        let (target, opt) = match inner.split_once('|') {
            Some((a, b)) => (a, Some(b)),
            None => (inner, None),
        };
        let w = opt.and_then(|o| o.split('x').next()).and_then(|w| w.trim().parse::<f32>().ok());
        return Some((target.split('#').next().unwrap_or(target).trim().to_string(), w));
    }
    if t.starts_with("![") && t.ends_with(')') {
        if let Some(p) = t.find("](") {
            let url = &t[p + 2..t.len() - 1];
            if !url.contains(')') && !url.contains("://") {
                let alt = &t[2..p];
                let w = alt.rsplit('|').next().and_then(|w| w.trim().parse::<f32>().ok());
                return Some((crate::vault::decode_path(url.split(" \"").next().unwrap_or(url)), w));
            }
        }
    }
    None
}

pub fn build_job(text: &str, st: &LiveStyle, wrap: f32) -> LayoutJob {
    let th = st.theme;
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    job.break_on_newline = true;
    let mut fence: Option<String> = None;
    let mut in_front = false;
    let find = st.find.to_ascii_lowercase();
    let mut in_callout: Option<Color32> = None;
    let plan = if st.live { plan_blocks(text, st.size) } else { vec![] };
    let code_blocks: Vec<(usize, usize, String)> = if st.live {
        decorations(text).into_iter().filter_map(|d| if let DecoKind::Code(l) = d.kind { Some((d.first, d.last, l)) } else { None }).collect()
    } else {
        vec![]
    };

    let mut lines = text.split_inclusive('\n').enumerate().peekable();
    while let Some((ln, raw)) = lines.next() {
        let (line, nl) = match raw.strip_suffix('\n') {
            Some(l) => (l, "\n"),
            None => (raw, ""),
        };
        let active = ln >= st.active.0 && ln <= st.active.1;
        let hide = st.live && !active;

        // ---- rendered blocks: tables / properties / rules (space reserved, painted later) ----
        if st.live {
            if let Some(b) = plan.iter().find(|b| b.first <= ln && ln <= b.last) {
                if !(st.active.0 <= b.last && st.active.1 >= b.first) {
                    let f = TextFormat {
                        font_id: FontId::new(1.0, FontFamily::Proportional),
                        color: Color32::TRANSPARENT,
                        line_height: Some(if ln == b.first { b.height } else { 1.0 }),
                        ..Default::default()
                    };
                    job.append(line, 0.0, f.clone());
                    job.append(nl, 0.0, f);
                    continue;
                }
            }
            // ---- fenced code: hide the fences, colour the code, when the caret is elsewhere ----
            if let Some((f, l, lang)) = code_blocks.iter().find(|b| b.0 <= ln && ln <= b.1) {
                if !(st.active.0 <= *l && st.active.1 >= *f) {
                    let t = line.trim_start();
                    if t.starts_with("```") || t.starts_with("~~~") {
                        let hf = TextFormat {
                            font_id: FontId::new(1.0, FontFamily::Proportional),
                            color: Color32::TRANSPARENT,
                            line_height: Some(if ln == *f { 18.0 } else { 8.0 }),
                            ..Default::default()
                        };
                        job.append(line, 0.0, hf.clone());
                        job.append(nl, 0.0, hf);
                    } else {
                        crate::reader::highlight_code(&mut job, lang, line, th, st.size * 0.92);
                        job.append(nl, 0.0, TextFormat { font_id: FontId::new(st.size * 0.92, FontFamily::Monospace), color: th.fg, line_height: Some(st.size * 0.92 * 1.5), ..Default::default() });
                    }
                    continue;
                }
            }
        }

        // ---- frontmatter / fences -----------------------------------------------------
        let trimmed = line.trim_start();
        let mut kind_code = false;
        let mut kind_dim_mono = false;
        if ln == 0 && line.trim_end() == "---" {
            in_front = true;
            kind_dim_mono = true;
        } else if in_front {
            kind_dim_mono = true;
            if line.trim_end() == "---" || line.trim_end() == "..." {
                in_front = false;
            }
        }
        if !kind_dim_mono {
            if let Some(f) = &fence {
                kind_code = true;
                if trimmed.starts_with(f.as_str()) {
                    fence = None;
                    kind_dim_mono = true;
                }
            } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fence = Some(trimmed[..3].to_string());
                kind_dim_mono = true;
            }
        }

        let body_font = |bold: bool, italic: bool, mono: bool, scale: f32| -> FontId {
            if mono {
                FontId::new(st.size * 0.92 * scale, FontFamily::Monospace)
            } else {
                FontId::new(st.size * scale, fonts::family(bold, italic))
            }
        };
        let plain = |color: Color32, mono: bool, bg: Color32| TextFormat {
            font_id: body_font(false, false, mono, 1.0),
            color,
            background: bg,
            line_height: Some(st.size * LINE_H),
            ..Default::default()
        };

        if kind_dim_mono || kind_code {
            let bg = Color32::TRANSPARENT;
            let color = if kind_dim_mono { th.fg_dim } else { mix(th.fg, th.green, 0.25) };
            job.append(line, 0.0, plain(color, true, bg));
            job.append(nl, 0.0, plain(color, true, bg));
            continue;
        }

        // A table being edited stays raw, in monospace so the pipes line up.
        if st.live && plan.iter().any(|b| matches!(b.kind, BlockKind::Table(_)) && b.first <= ln && ln <= b.last) {
            job.append(line, 0.0, plain(th.fg, true, Color32::TRANSPARENT));
            job.append(nl, 0.0, plain(th.fg, true, Color32::TRANSPARENT));
            continue;
        }

        // ---- image embed on its own line (Live mode, inactive) ---------------------------
        if hide {
            if let Some((target, w)) = embed_only(line) {
                if let Some(h) = (st.image_height)(&target, w) {
                    let f = TextFormat {
                        font_id: FontId::new(1.0, FontFamily::Proportional),
                        color: Color32::TRANSPARENT,
                        line_height: Some(h),
                        ..Default::default()
                    };
                    job.append(line, 0.0, f.clone());
                    job.append(nl, 0.0, f);
                    continue;
                }
            }
        }

        // ---- block prefix --------------------------------------------------------------
        let indent = line.len() - trimmed.len();
        let mut scale = 1.0;
        let mut bold = false;
        let mut color = th.fg;
        let mut prefix_len = 0usize;
        let mut prefix_color = th.fg_dim;
        let mut dim_text = false;
        let mut strike_all = false;
        let mut quote_line = false;
        let mut list_ordered = false;
        let mut callout_title: Option<(usize, Color32)> = None;
        let mut callout_marker_col: Option<Color32> = None;

        let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            scale = heading_scale(hashes);
            bold = true;
            color = th.fg_bright;
            prefix_len = indent + hashes + 1;
        } else if trimmed.starts_with('>') {
            let mut p = indent;
            let rest = &line[indent..];
            let mut it = rest.chars();
            while let Some(c) = it.next() {
                if c == '>' || c == ' ' {
                    p += 1;
                } else {
                    break;
                }
            }
            prefix_len = p;
            color = mix(th.fg, th.fg_dim, 0.25);
            prefix_color = th.accent;
            quote_line = true;
            let content = &line[p..];
            let mut new_callout = None;
            if let Some(rest) = content.strip_prefix("[!") {
                if let Some(e) = rest.find(']') {
                    let kind = rest[..e].to_lowercase();
                    let after = &rest[e + 1..];
                    let fold = after.len() - after.trim_start_matches(['+', '-']).len();
                    let title_off = p + 2 + e + 1 + fold;
                    let ws = line[title_off..].len() - line[title_off..].trim_start().len();
                    let c = crate::reader::callout_style(&kind, th).0;
                    new_callout = Some(c);
                    if !line[title_off..].trim().is_empty() {
                        callout_title = Some((title_off + ws, c));
                    } else {
                        callout_marker_col = Some(c);
                    }
                }
            }
            if new_callout.is_some() {
                in_callout = new_callout;
            } else if in_callout.is_some() {
                color = th.fg;
            }
        } else if let Some(m) = list_marker(trimmed) {
            list_ordered = m.ordered.is_some();
            prefix_len = indent + m.len;
            prefix_color = th.accent;
            if let Some(done) = m.task {
                if done {
                    dim_text = true;
                    strike_all = true;
                }
            }
        } else if is_hr(trimmed) {
            job.append(line, 0.0, plain(th.fg_dim, false, Color32::TRANSPARENT));
            job.append(nl, 0.0, plain(th.fg_dim, false, Color32::TRANSPARENT));
            continue;
        }

        if !trimmed.starts_with('>') {
            in_callout = None;
        }
        if hide {
            if let Some((ts, c)) = callout_title {
                prefix_len = ts;
                bold = true;
                color = c;
            }
        }
        if let Some(c) = callout_marker_col {
            color = c;
        }
        let lh = st.size * scale * LINE_H;
        let hidden_fmt = TextFormat {
            font_id: FontId::new(0.5, FontFamily::Proportional),
            color: Color32::TRANSPARENT,
            line_height: Some(lh),
            ..Default::default()
        };

        // prefix
        if prefix_len > 0 {
            let pre = &line[..prefix_len];
            let bullet_overlay = hide && !list_ordered && list_marker(trimmed).is_some();
            if hide && (hashes > 0 || quote_line) {
                job.append(pre, 0.0, hidden_fmt.clone());
            } else if bullet_overlay {
                // Keep the marker's width but draw a real bullet / checkbox over it afterwards.
                let nf = |c: Color32| TextFormat { font_id: body_font(false, false, false, scale), color: c, line_height: Some(lh), ..Default::default() };
                if indent > 0 {
                    job.append(&pre[..indent], 0.0, nf(th.fg));
                }
                job.append(&pre[indent..], 0.0, nf(Color32::TRANSPARENT));
            } else {
                let mut f = TextFormat {
                    font_id: body_font(bold, false, false, scale),
                    color: prefix_color,
                    line_height: Some(lh),
                    ..Default::default()
                };
                if hashes > 0 {
                    f.color = th.fg_dim;
                }
                job.append(pre, 0.0, f);
            }
        }

        // inline content
        let content = &line[prefix_len..];
        let mut segs = inline_segments(content, prefix_len);
        if !find.is_empty() {
            segs = split_found(segs, &line.to_ascii_lowercase(), &find);
        }
        for s in &segs {
            let f = &s.fmt;
            let txt = &line[s.range.clone()];
            let mut tf = TextFormat {
                font_id: body_font(bold || f.bold, f.italic, f.code, scale),
                color,
                line_height: Some(lh),
                ..Default::default()
            };
            if f.marker {
                if hide {
                    job.append(txt, 0.0, hidden_fmt.clone());
                    continue;
                }
                tf.color = th.fg_dim;
                if f.code {
                    tf.font_id = body_font(false, false, true, scale);
                }
                job.append(txt, 0.0, tf);
                continue;
            }
            if bold || f.bold {
                tf.color = th.fg_bright;
            }
            if f.code {
                tf.color = mix(th.fg, th.orange, 0.55);
                tf.background = with_alpha(th.bg_darker, 0.65);
            }
            if f.strike || strike_all {
                tf.strikethrough = Stroke::new(1.0, th.fg_dim);
                tf.color = th.fg_dim;
            }
            if dim_text {
                tf.color = th.fg_dim;
            }
            if f.mark {
                tf.background = with_alpha(th.yellow, 0.35);
                tf.color = th.fg_bright;
            }
            if f.link {
                tf.color = th.accent;
                tf.underline = Stroke::new(1.0, with_alpha(th.accent, 0.7));
            }
            if f.tag {
                tf.color = th.accent;
                tf.background = with_alpha(th.accent, 0.16);
            }
            if f.found {
                tf.background = with_alpha(th.yellow, 0.55);
                tf.color = th.fg_bright;
            }
            job.append(txt, 0.0, tf);
        }
        job.append(
            nl,
            0.0,
            TextFormat { font_id: body_font(false, false, false, scale), color, line_height: Some(lh), ..Default::default() },
        );
    }
    job
}

fn split_found(segs: Vec<Seg>, lower_line: &str, needle: &str) -> Vec<Seg> {
    let mut out = vec![];
    for s in segs {
        let hay = &lower_line[s.range.clone()];
        let mut pos = 0;
        while let Some(p) = hay[pos..].find(needle) {
            let a = pos + p;
            if a > pos {
                out.push(Seg { range: s.range.start + pos..s.range.start + a, ..s.clone() });
            }
            out.push(Seg {
                range: s.range.start + a..s.range.start + a + needle.len(),
                fmt: Fmt { found: !s.fmt.marker, ..s.fmt.clone() },
                link: s.link.clone(),
            });
            pos = a + needle.len();
        }
        if pos < hay.len() {
            out.push(Seg { range: s.range.start + pos..s.range.end, ..s }.clone());
        }
    }
    out
}

pub struct ListMarker {
    pub len: usize,
    pub task: Option<bool>,
    pub ordered: Option<usize>,
    pub bullet: String,
}

/// Parses a list marker at the start of `s` (already left-trimmed): `- `, `1. `, `- [ ] `.
pub fn list_marker(s: &str) -> Option<ListMarker> {
    let b = s.as_bytes();
    let (mut len, ordered, bullet) = if matches!(b.first(), Some(b'-' | b'*' | b'+')) && b.get(1) == Some(&b' ') {
        (2, None, (b[0] as char).to_string())
    } else {
        let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && digits < 10 && matches!(b.get(digits), Some(b'.' | b')')) && b.get(digits + 1) == Some(&b' ') {
            (digits + 2, s[..digits].parse().ok(), (b[digits] as char).to_string())
        } else {
            return None;
        }
    };
    let mut task = None;
    if s[len..].len() >= 3 && s[len..].starts_with('[') && s.as_bytes()[len + 2] == b']' && matches!(s.as_bytes()[len + 1], b' ' | b'x' | b'X') {
        let after = &s[len + 3..];
        if after.is_empty() || after.starts_with(' ') {
            task = Some(s.as_bytes()[len + 1] != b' ');
            len += 3 + after.starts_with(' ') as usize;
        }
    }
    Some(ListMarker { len, task, ordered, bullet })
}

fn is_hr(t: &str) -> bool {
    let t = t.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

// ---------------------------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------------------------

pub fn byte_of(text: &str, char_idx: usize) -> usize {
    text.char_indices().nth(char_idx).map(|x| x.0).unwrap_or(text.len())
}

pub fn char_of(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

/// (start, end) byte range of the line containing `byte`, excluding the newline.
pub fn line_range(text: &str, byte: usize) -> (usize, usize) {
    let s = text[..byte].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let e = text[byte..].find('\n').map(|p| byte + p).unwrap_or(text.len());
    (s, e)
}

pub fn line_of(text: &str, byte: usize) -> usize {
    text.as_bytes()[..byte.min(text.len())].iter().filter(|&&b| b == b'\n').count()
}

pub fn line_start_byte(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut n = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            n += 1;
            if n == line {
                return i + 1;
            }
        }
    }
    text.len()
}

pub fn count_words(text: &str) -> usize {
    text.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count()
}

// ---------------------------------------------------------------------------------------------
// Editing commands. Selections are (anchor, head) in *char* indices.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Sel {
    pub anchor: usize,
    pub head: usize,
}

impl Sel {
    pub fn lo(&self) -> usize {
        self.anchor.min(self.head)
    }
    pub fn hi(&self) -> usize {
        self.anchor.max(self.head)
    }
    pub fn caret(c: usize) -> Self {
        Sel { anchor: c, head: c }
    }
    pub fn empty(&self) -> bool {
        self.anchor == self.head
    }
}

pub fn load_sel(ctx: &egui::Context, id: Id) -> Option<Sel> {
    let st = TextEditState::load(ctx, id)?;
    let r = st.cursor.char_range()?;
    Some(Sel { anchor: r.secondary.index.0, head: r.primary.index.0 })
}

pub fn store_sel(ctx: &egui::Context, id: Id, sel: Sel) {
    let mut st = TextEditState::load(ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange {
        primary: CCursor::new(sel.head),
        secondary: CCursor::new(sel.anchor),
        h_pos: None,
    }));
    st.store(ctx, id);
}

fn replace(text: &mut String, char_lo: usize, char_hi: usize, with: &str) {
    let (a, b) = (byte_of(text, char_lo), byte_of(text, char_hi));
    text.replace_range(a..b, with);
}

pub fn wrap_selection(text: &mut String, sel: Sel, open: &str, close: &str) -> Sel {
    let (lo, hi) = (sel.lo(), sel.hi());
    let (blo, bhi) = (byte_of(text, lo), byte_of(text, hi));
    let ol = open.chars().count();
    let cl = close.chars().count();
    // Unwrap if already wrapped.
    if text[..blo].ends_with(open) && text[bhi..].starts_with(close) && !(open == "*" && text[..blo].ends_with("**")) {
        replace(text, hi, hi + cl, "");
        replace(text, lo - ol, lo, "");
        return Sel { anchor: lo - ol, head: hi - ol };
    }
    if sel.empty() {
        replace(text, lo, lo, &format!("{open}{close}"));
        return Sel::caret(lo + ol);
    }
    replace(text, hi, hi, close);
    replace(text, lo, lo, open);
    Sel { anchor: lo + ol, head: hi + ol }
}

#[derive(Clone, Copy, PartialEq)]
pub enum LinePrefix {
    Bullet,
    Number,
    Task,
    Quote,
    Heading(usize),
}

fn strip_block_prefix(line: &str) -> (usize, &str) {
    let ind = line.len() - line.trim_start().len();
    let t = &line[ind..];
    let hashes = t.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
        return (ind, &t[hashes + 1..]);
    }
    if let Some(r) = t.strip_prefix("> ") {
        return (ind, r);
    }
    if let Some(m) = list_marker(t) {
        return (ind, &t[m.len..]);
    }
    (ind, t)
}

pub fn toggle_line_prefix(text: &mut String, sel: Sel, kind: LinePrefix) -> Sel {
    let (blo, bhi) = (byte_of(text, sel.lo()), byte_of(text, sel.hi()));
    let start = line_range(text, blo).0;
    let end = line_range(text, bhi).1;
    let block = text[start..end].to_string();
    let prefix_of = |n: usize| -> String {
        match kind {
            LinePrefix::Bullet => "- ".into(),
            LinePrefix::Number => format!("{}. ", n + 1),
            LinePrefix::Task => "- [ ] ".into(),
            LinePrefix::Quote => "> ".into(),
            LinePrefix::Heading(l) => format!("{} ", "#".repeat(l)),
        }
    };
    let has = |line: &str| {
        let t = line.trim_start();
        match kind {
            LinePrefix::Bullet => list_marker(t).is_some_and(|m| m.ordered.is_none() && m.task.is_none()),
            LinePrefix::Number => list_marker(t).is_some_and(|m| m.ordered.is_some()),
            LinePrefix::Task => list_marker(t).is_some_and(|m| m.task.is_some()),
            LinePrefix::Quote => t.starts_with("> "),
            LinePrefix::Heading(l) => t.starts_with(&format!("{} ", "#".repeat(l))),
        }
    };
    let all = block.lines().filter(|l| !l.trim().is_empty()).all(has);
    let mut out = String::new();
    for (n, line) in block.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let (ind, rest) = strip_block_prefix(line);
        if all {
            out.push_str(&line[..ind]);
            out.push_str(rest);
        } else if line.trim().is_empty() {
            out.push_str(line);
        } else {
            out.push_str(&line[..ind]);
            out.push_str(&prefix_of(n));
            out.push_str(rest);
        }
    }
    text.replace_range(start..end, &out);
    let cs = char_of(text, start);
    Sel { anchor: cs, head: cs + out.chars().count() }
}

/// Enter pressed: continue lists/quotes. Returns the new selection if handled.
pub fn smart_enter(text: &mut String, sel: Sel) -> Option<Sel> {
    if !sel.empty() {
        return None;
    }
    let caret_b = byte_of(text, sel.head);
    let (ls, le) = line_range(text, caret_b);
    let line = text[ls..le].to_string();
    let ind = line.len() - line.trim_start().len();
    let t = &line[ind..];
    let (pre_len, next_prefix, content) = if let Some(m) = list_marker(t) {
        let mut next = String::new();
        next.push_str(&line[..ind]);
        match m.ordered {
            Some(n) => next.push_str(&format!("{}{} ", n + 1, m.bullet)),
            None => next.push_str(&format!("{} ", m.bullet)),
        }
        if m.task.is_some() {
            next.push_str("[ ] ");
        }
        (ind + m.len, next, t[m.len..].to_string())
    } else if t.starts_with('>') {
        let q = t.len() - t.trim_start_matches(|c| c == '>' || c == ' ').len();
        (ind + q, format!("{}{}", &line[..ind], &t[..q]), t[q..].to_string())
    } else {
        return None;
    };
    // Caret must be after the marker.
    if caret_b < ls + pre_len {
        return None;
    }
    if content.trim().is_empty() {
        // Empty item: clear the marker (leave the list).
        text.replace_range(ls..le, "");
        return Some(Sel::caret(char_of(text, ls)));
    }
    let ins = format!("\n{next_prefix}");
    text.insert_str(caret_b, &ins);
    Some(Sel::caret(sel.head + ins.chars().count()))
}

pub fn indent_lines(text: &mut String, sel: Sel, outdent: bool) -> Sel {
    let (blo, bhi) = (byte_of(text, sel.lo()), byte_of(text, sel.hi()));
    let start = line_range(text, blo).0;
    let end = line_range(text, bhi).1;
    let block = text[start..end].to_string();
    let mut out = String::new();
    let mut first_delta: isize = 0;
    let mut total: isize = 0;
    for (n, line) in block.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let delta: isize = if outdent {
            let rm = if line.starts_with('\t') { 1 } else { line.bytes().take(4).take_while(|&b| b == b' ').count() };
            out.push_str(&line[rm..]);
            -(rm as isize)
        } else {
            out.push('\t');
            out.push_str(line);
            1
        };
        if n == 0 {
            first_delta = delta;
        }
        total += delta;
    }
    text.replace_range(start..end, &out);
    let adj = |c: usize, d: isize| (c as isize + d).max(0) as usize;
    if sel.empty() {
        Sel::caret(adj(sel.head, first_delta))
    } else {
        let (lo, hi) = (adj(sel.lo(), first_delta), adj(sel.hi(), total));
        if sel.anchor <= sel.head { Sel { anchor: lo, head: hi } } else { Sel { anchor: hi, head: lo } }
    }
}

/// `[[` autocomplete context: (char index where the query starts, query text).
pub fn wiki_context(text: &str, caret_char: usize) -> Option<(usize, String)> {
    let cb = byte_of(text, caret_char);
    let (ls, _) = line_range(text, cb);
    let before = &text[ls..cb];
    let open = before.rfind("[[")?;
    let q = &before[open + 2..];
    if q.contains("]]") || q.contains('|') || q.contains('#') || q.contains('[') {
        return None;
    }
    Some((char_of(text, ls + open + 2), q.to_string()))
}

pub struct EditFlags {
    pub focused: bool,
    /// A popup (autocomplete) owns Enter/Tab/arrows this frame.
    pub popup_open: bool,
}

/// Runs before the TextEdit: Enter/Tab/Backspace handling, auto-pairing, URL paste over selection.
/// Returns true if the text was changed by us.
pub fn preprocess(ctx: &egui::Context, id: Id, text: &mut String, flags: &EditFlags) -> bool {
    if !flags.focused {
        return false;
    }
    let Some(mut sel) = load_sel(ctx, id) else { return false };
    let mut changed = false;

    if !flags.popup_open {
        if ctx.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.any()) {
            if let Some(ns) = smart_enter(text, sel) {
                ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                sel = ns;
                changed = true;
            }
        }
        for outdent in [false, true] {
            let mods = if outdent { Modifiers::SHIFT } else { Modifiers::NONE };
            if ctx.input_mut(|i| i.consume_key(mods, Key::Tab)) {
                sel = indent_lines(text, sel, outdent);
                changed = true;
            }
        }
    }

    // Backspace inside an empty auto-pair: remove both halves.
    if sel.empty() && ctx.input(|i| i.key_pressed(Key::Backspace) && !i.modifiers.any()) {
        let b = byte_of(text, sel.head);
        let (p, n) = (text[..b].chars().next_back(), text[b..].chars().next());
        if matches!((p, n), (Some('['), Some(']')) | (Some('('), Some(')')) | (Some('{'), Some('}')) | (Some('`'), Some('`'))) {
            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Backspace));
            replace(text, sel.head - 1, sel.head + 1, "");
            sel = Sel::caret(sel.head - 1);
            changed = true;
        }
    }

    // Typed characters.
    let events = ctx.input_mut(|i| std::mem::take(&mut i.events));
    let mut kept = Vec::with_capacity(events.len());
    for ev in events {
        match &ev {
            egui::Event::Text(s) if s.chars().count() == 1 => {
                let c = s.chars().next().unwrap();
                let b = byte_of(text, sel.head);
                let next = text[b..].chars().next();
                let prev = text[..b].chars().next_back();
                let close_of = |c: char| match c {
                    '[' => Some(']'),
                    '(' => Some(')'),
                    '{' => Some('}'),
                    '`' => Some('`'),
                    _ => None,
                };
                if matches!(c, ']' | ')' | '}' | '`') && next == Some(c) && sel.empty() {
                    sel = Sel::caret(sel.head + 1);
                    changed = true;
                } else if let Some(cl) = close_of(c) {
                    if c == '`' && (prev == Some('`') || is_word(prev)) && sel.empty() {
                        kept.push(ev);
                    } else if sel.empty() && next.map(|n| n.is_alphanumeric()).unwrap_or(false) {
                        kept.push(ev);
                    } else {
                        sel = wrap_selection_force(text, sel, c, cl);
                        changed = true;
                    }
                } else if matches!(c, '*' | '_' | '~' | '=') && !sel.empty() {
                    let m = c.to_string();
                    let mm = if c == '*' || c == '_' { m.clone() } else { format!("{c}{c}") };
                    sel = wrap_selection(text, sel, &mm, &mm);
                    changed = true;
                } else {
                    kept.push(ev);
                }
            }
            egui::Event::Paste(s) if !sel.empty() && is_url(s) => {
                let sel_text: String = text.chars().skip(sel.lo()).take(sel.hi() - sel.lo()).collect();
                if !sel_text.contains('\n') {
                    let link = format!("[{}]({})", sel_text, s.trim());
                    replace(text, sel.lo(), sel.hi(), &link);
                    sel = Sel::caret(sel.lo() + link.chars().count());
                    changed = true;
                } else {
                    kept.push(ev);
                }
            }
            _ => kept.push(ev),
        }
    }
    ctx.input_mut(|i| i.events = kept);

    if changed {
        store_sel(ctx, id, sel);
    }
    changed
}

/// Like `wrap_selection` but never unwraps (used for auto-pairing brackets).
fn wrap_selection_force(text: &mut String, sel: Sel, open: char, close: char) -> Sel {
    let (lo, hi) = (sel.lo(), sel.hi());
    if sel.empty() {
        replace(text, lo, lo, &format!("{open}{close}"));
        return Sel::caret(lo + 1);
    }
    replace(text, hi, hi, &close.to_string());
    replace(text, lo, lo, &open.to_string());
    Sel { anchor: lo + 1, head: hi + 1 }
}

fn is_url(s: &str) -> bool {
    let t = s.trim();
    (t.starts_with("http://") || t.starts_with("https://")) && !t.contains(char::is_whitespace)
}

// ---------------------------------------------------------------------------------------------
// Block decorations (painted behind the text)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum DecoKind {
    Code(String),
    Callout(String),
    Quote,
}

#[derive(Clone, Debug)]
pub struct Deco {
    pub first: usize,
    pub last: usize,
    pub kind: DecoKind,
}

pub fn decorations(text: &str) -> Vec<Deco> {
    let mut out = vec![];
    let mut fence: Option<(String, usize, String)> = None;
    let mut front = false;
    let mut quote: Option<(usize, DecoKind)> = None;
    let mut total = 0;
    for (ln, line) in text.lines().enumerate() {
        total = ln + 1;
        let t = line.trim_start();
        if ln == 0 && line.trim_end() == "---" {
            front = true;
            continue;
        }
        if front {
            if line.trim_end() == "---" || line.trim_end() == "..." {
                front = false;
            }
            continue;
        }
        if let Some((f, start, lang)) = &fence {
            if t.starts_with(f.as_str()) {
                out.push(Deco { first: *start, last: ln, kind: DecoKind::Code(lang.clone()) });
                fence = None;
            }
            continue;
        }
        let is_quote = t.starts_with('>');
        if !is_quote {
            if let Some((s, k)) = quote.take() {
                out.push(Deco { first: s, last: ln - 1, kind: k });
            }
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some((t[..3].to_string(), ln, t[3..].trim().split_whitespace().next().unwrap_or("").to_string()));
            continue;
        }
        if is_quote {
            let content = t.trim_start_matches(['>', ' ']);
            let new_callout = content.strip_prefix("[!").and_then(|r| r.find(']').map(|e| r[..e].to_lowercase()));
            if let Some(kind) = new_callout {
                if let Some((s, k)) = quote.take() {
                    out.push(Deco { first: s, last: ln - 1, kind: k });
                }
                quote = Some((ln, DecoKind::Callout(kind)));
            } else if quote.is_none() {
                quote = Some((ln, DecoKind::Quote));
            }
        }
    }
    if let Some((s, k)) = quote {
        out.push(Deco { first: s, last: total.saturating_sub(1), kind: k });
    }
    if let Some((_, s, lang)) = fence {
        out.push(Deco { first: s, last: total.saturating_sub(1), kind: DecoKind::Code(lang) });
    }
    out
}

/// Byte range of the `[ ]` / `[x]` box if `line` is a task item.
pub fn task_box(line: &str) -> Option<Range<usize>> {
    let ind = line.len() - line.trim_start().len();
    let m = list_marker(&line[ind..])?;
    m.task?;
    let p = line[ind..].find('[')? + ind;
    Some(p..p + 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_continues_lists() {
        let mut t = "- item".to_string();
        let s = smart_enter(&mut t, Sel::caret(6)).unwrap();
        assert_eq!(t, "- item\n- ");
        assert_eq!(s.head, 9);

        let mut t = "3. three".to_string();
        smart_enter(&mut t, Sel::caret(8)).unwrap();
        assert_eq!(t, "3. three\n4. ");

        let mut t = "- [x] done".to_string();
        smart_enter(&mut t, Sel::caret(10)).unwrap();
        assert_eq!(t, "- [x] done\n- [ ] ");

        // Empty item exits the list.
        let mut t = "- a\n- ".to_string();
        let s = smart_enter(&mut t, Sel::caret(6)).unwrap();
        assert_eq!(t, "- a\n");
        assert_eq!(s.head, 4);

        assert!(smart_enter(&mut "plain".to_string(), Sel::caret(5)).is_none());
    }

    #[test]
    fn wrap_and_unwrap() {
        let mut t = "hello world".to_string();
        let s = wrap_selection(&mut t, Sel { anchor: 0, head: 5 }, "**", "**");
        assert_eq!(t, "**hello** world");
        let s = wrap_selection(&mut t, s, "**", "**");
        assert_eq!(t, "hello world");
        assert_eq!((s.anchor, s.head), (0, 5));
        let mut e = "x".to_string();
        let s = wrap_selection(&mut e, Sel::caret(1), "[[", "]]");
        assert_eq!(e, "x[[]]");
        assert_eq!(s.head, 3);
    }

    #[test]
    fn line_prefix_toggle() {
        let mut t = "a\nb".to_string();
        toggle_line_prefix(&mut t, Sel { anchor: 0, head: 3 }, LinePrefix::Bullet);
        assert_eq!(t, "- a\n- b");
        toggle_line_prefix(&mut t, Sel { anchor: 0, head: 7 }, LinePrefix::Bullet);
        assert_eq!(t, "a\nb");
        toggle_line_prefix(&mut t, Sel::caret(0), LinePrefix::Heading(2));
        assert_eq!(t, "## a\nb");
        toggle_line_prefix(&mut t, Sel::caret(0), LinePrefix::Heading(2));
        assert_eq!(t, "a\nb");
    }

    #[test]
    fn indent_outdent() {
        let mut t = "a\nb".to_string();
        indent_lines(&mut t, Sel { anchor: 0, head: 3 }, false);
        assert_eq!(t, "\ta\n\tb");
        indent_lines(&mut t, Sel { anchor: 0, head: 5 }, true);
        assert_eq!(t, "a\nb");
    }

    #[test]
    fn wikilink_context() {
        assert_eq!(wiki_context("see [[Ro", 8), Some((6, "Ro".to_string())));
        assert_eq!(wiki_context("see [[Ro]] x", 12), None);
        assert_eq!(wiki_context("see [[a|b", 9), None);
    }

    #[test]
    fn inline_scan_finds_links_and_tags() {
        let l = "a [[Note|al]] and #tag and [x](http://e.com)";
        assert!(matches!(link_at(l, 7), Some(LinkRef::Wiki { .. })));
        assert_eq!(tag_at(l, 20).as_deref(), Some("tag"));
        assert!(matches!(link_at(l, 35), Some(LinkRef::Url(_))));
        assert!(link_at(l, 0).is_none());
    }

    #[test]
    fn decorations_cover_code_and_callouts() {
        let t = "text\n```rs\nlet x;\n```\n> [!tip] hi\n> body\n\n> plain quote\n";
        let d = decorations(t);
        assert_eq!(d.len(), 3);
        assert_eq!((d[0].first, d[0].last, d[0].kind.clone()), (1, 3, DecoKind::Code("rs".into())));
        assert_eq!((d[1].first, d[1].last), (4, 5));
        assert!(matches!(d[1].kind, DecoKind::Callout(_)));
        assert_eq!(d[2].kind, DecoKind::Quote);
    }

    #[test]
    fn plans_frontmatter_tables_rules() {
        let t = "---\ntags: [a]\n---\n\n| H1 | H2 |\n|:--|--:|\n| **x** | [[n\\|y]] |\n\n---\n\ntext\n---\n";
        let p = plan_blocks(t, 16.0);
        assert_eq!(p.len(), 3);
        assert!(matches!(p[0].kind, BlockKind::Props(_)));
        assert_eq!((p[0].first, p[0].last), (0, 2));
        match &p[1].kind {
            BlockKind::Table(tb) => {
                assert_eq!(tb.head, ["H1", "H2"]);
                assert_eq!(tb.aligns, [-1, 1]);
                assert_eq!(tb.rows, vec![vec!["x".to_string(), "y".to_string()]]);
            }
            _ => panic!("expected table"),
        }
        assert_eq!((p[1].first, p[1].last), (4, 6));
        assert!(matches!(p[2].kind, BlockKind::Rule));
        // `---` directly under text is a setext heading, not a rule
        assert_eq!(p[2].first, 8);
    }

    #[test]
    fn task_box_range() {
        assert_eq!(task_box("  - [ ] x"), Some(4..7));
        assert_eq!(task_box("- plain"), None);
    }
}

// ---------------------------------------------------------------------------------------------
// Rendered blocks (tables, properties, rules) shown in place of their source in Live preview
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub head: Vec<String>,
    /// -1 left, 0 centre, 1 right
    pub aligns: Vec<i8>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Clone, Debug)]
pub enum BlockKind {
    Table(Table),
    Props(Vec<(String, crate::vault::PropValue)>),
    Rule,
}

#[derive(Clone, Debug)]
pub struct BlockPlan {
    pub first: usize,
    pub last: usize,
    pub kind: BlockKind,
    /// Vertical space reserved for the rendered widget.
    pub height: f32,
}

pub fn table_row_h(size: f32) -> f32 {
    size * 1.55 + 10.0
}

pub fn prop_row_h(size: f32) -> f32 {
    size * 1.75
}

fn split_row(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut out = vec![];
    let mut cur = String::new();
    let mut esc = false;
    for c in t.chars() {
        if esc {
            if c != '|' {
                cur.push('\\');
            }
            cur.push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '|' {
            out.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    out.push(cur.trim().to_string());
    out
}

fn is_table_sep(line: &str) -> bool {
    let t = line.trim();
    t.contains('-') && t.contains('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// Text of an inline run with Markdown syntax removed (what Live preview shows).
pub fn plain_inline(s: &str) -> String {
    inline_segments(s, 0).into_iter().filter(|g| !g.fmt.marker).map(|g| s[g.range].to_string()).collect()
}

/// Finds frontmatter, tables and horizontal rules so Live preview can render them in place.
pub fn plan_blocks(text: &str, size: f32) -> Vec<BlockPlan> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = vec![];
    let mut i = 0;
    if lines.first().map(|l| l.trim_end() == "---").unwrap_or(false) {
        if let Some(end) = lines.iter().skip(1).position(|l| matches!(l.trim_end(), "---" | "...")) {
            let last = end + 1;
            let props = crate::vault::parse_properties(&lines[1..last].join("\n"));
            let h = props.len().max(1) as f32 * prop_row_h(size) + 22.0;
            out.push(BlockPlan { first: 0, last, kind: BlockKind::Props(props), height: h });
            i = last + 1;
        }
    }
    let mut fence: Option<String> = None;
    while i < lines.len() {
        let l = lines[i];
        let t = l.trim_start();
        if let Some(f) = &fence {
            if t.starts_with(f.as_str()) {
                fence = None;
            }
            i += 1;
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(t[..3].to_string());
            i += 1;
            continue;
        }
        if t.contains('|') && i + 1 < lines.len() && is_table_sep(lines[i + 1]) {
            let head: Vec<String> = split_row(l).iter().map(|c| plain_inline(c)).collect();
            let aligns: Vec<i8> = split_row(lines[i + 1])
                .iter()
                .map(|c| match (c.starts_with(':'), c.ends_with(':')) {
                    (true, true) => 0,
                    (false, true) => 1,
                    _ => -1,
                })
                .collect();
            let mut j = i + 2;
            let mut rows = vec![];
            while j < lines.len() && !lines[j].trim().is_empty() && lines[j].contains('|') {
                rows.push(split_row(lines[j]).iter().map(|c| plain_inline(c)).collect());
                j += 1;
            }
            let h = (rows.len() + 1) as f32 * table_row_h(size) + 8.0;
            out.push(BlockPlan { first: i, last: j - 1, kind: BlockKind::Table(Table { head, aligns, rows }), height: h });
            i = j;
            continue;
        }
        if is_hr(l) && (i == 0 || lines[i - 1].trim().is_empty()) {
            out.push(BlockPlan { first: i, last: i, kind: BlockKind::Rule, height: size * 1.5 });
        }
        i += 1;
    }
    out
}
