//! Vector icons drawn with the painter: crisp at any scale, no icon-font dependency.

use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Folder,
    FolderPlus,
    FilePlus,
    Search,
    Hash,
    Link,
    Outgoing,
    Outline,
    Collapse,
    Book,
    Pencil,
    Gear,
    SidebarLeft,
    SidebarRight,
    Plus,
    Close,
    Back,
    Forward,
    Up,
    Down,
    Pin,
    ChevronDown,
    ChevronRight,
    Vault,
    Replace,
    // callouts
    Info,
    Check,
    Question,
    Warning,
    Bolt,
    Quote,
    Bug,
    Flame,
    List,
}

/// Paint `icon` centred in `r` (the icon occupies a square of `size`).
pub fn paint(p: &Painter, c: Pos2, size: f32, icon: Icon, col: Color32) {
    let u = size / 16.0; // design grid is 16x16
    let s = Stroke::new((1.35 * u).max(1.0), col);
    let pt = |x: f32, y: f32| pos2(c.x + (x - 8.0) * u, c.y + (y - 8.0) * u);
    let line = |a: (f32, f32), b: (f32, f32)| p.line_segment([pt(a.0, a.1), pt(b.0, b.1)], s);
    let poly = |pts: &[(f32, f32)], closed: bool| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| pt(x, y)).collect();
        if closed {
            p.add(Shape::closed_line(v, s));
        } else {
            p.add(Shape::line(v, s));
        }
    };
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| p.rect_stroke(Rect::from_min_max(pt(x0, y0), pt(x1, y1)), CornerRadius::same((r * u) as u8), s, StrokeKind::Middle);
    let circle = |x: f32, y: f32, r: f32| p.circle_stroke(pt(x, y), r * u, s);
    let dot = |x: f32, y: f32, r: f32| p.circle_filled(pt(x, y), r * u, col);

    match icon {
        Icon::Folder => {
            poly(&[(2.0, 4.5), (6.0, 4.5), (7.5, 6.0), (14.0, 6.0), (14.0, 12.5), (2.0, 12.5)], true);
        }
        Icon::FolderPlus => {
            poly(&[(2.0, 4.5), (6.0, 4.5), (7.5, 6.0), (14.0, 6.0), (14.0, 12.5), (2.0, 12.5)], true);
            line((8.0, 7.8), (8.0, 11.2));
            line((6.3, 9.5), (9.7, 9.5));
        }
        Icon::FilePlus => {
            poly(&[(3.5, 2.0), (9.5, 2.0), (12.5, 5.0), (12.5, 14.0), (3.5, 14.0)], true);
            line((8.0, 7.2), (8.0, 11.5));
            line((5.9, 9.35), (10.1, 9.35));
        }
        Icon::Search => {
            circle(7.0, 7.0, 4.2);
            line((10.2, 10.2), (13.6, 13.6));
        }
        Icon::Hash => {
            line((6.0, 2.5), (5.0, 13.5));
            line((11.0, 2.5), (10.0, 13.5));
            line((3.0, 6.0), (13.0, 6.0));
            line((2.5, 10.0), (12.5, 10.0));
        }
        Icon::Link => {
            // two interlocking rounded links
            rect(1.5, 5.5, 8.0, 10.5, 2.5);
            rect(8.0, 5.5, 14.5, 10.5, 2.5);
        }
        Icon::Outgoing => {
            line((4.0, 12.0), (12.0, 4.0));
            poly(&[(6.0, 4.0), (12.0, 4.0), (12.0, 10.0)], false);
        }
        Icon::Outline | Icon::List => {
            dot(3.0, 4.5, 0.9);
            dot(3.0, 8.0, 0.9);
            dot(3.0, 11.5, 0.9);
            line((6.0, 4.5), (13.5, 4.5));
            line((6.0, 8.0), (13.5, 8.0));
            line((6.0, 11.5), (13.5, 11.5));
        }
        Icon::Collapse => {
            line((3.0, 5.0), (13.0, 5.0));
            line((3.0, 8.0), (13.0, 8.0));
            line((3.0, 11.0), (13.0, 11.0));
            poly(&[(6.0, 2.5), (8.0, 4.0), (10.0, 2.5)], false);
            poly(&[(6.0, 13.5), (8.0, 12.0), (10.0, 13.5)], false);
        }
        Icon::Book => {
            poly(&[(8.0, 4.0), (8.0, 13.0)], false);
            poly(&[(8.0, 4.0), (5.0, 3.0), (2.0, 3.5), (2.0, 12.0), (5.0, 11.5), (8.0, 13.0), (11.0, 11.5), (14.0, 12.0), (14.0, 3.5), (11.0, 3.0), (8.0, 4.0)], false);
        }
        Icon::Pencil => {
            poly(&[(3.0, 13.0), (3.6, 10.0), (11.0, 2.6), (13.4, 5.0), (6.0, 12.4), (3.0, 13.0)], false);
            line((9.4, 4.2), (11.8, 6.6));
        }
        Icon::Gear => {
            circle(8.0, 8.0, 2.3);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (sn, cs) = a.sin_cos();
                line((8.0 + cs * 4.6, 8.0 + sn * 4.6), (8.0 + cs * 6.4, 8.0 + sn * 6.4));
            }
            circle(8.0, 8.0, 4.6);
        }
        Icon::SidebarLeft => {
            rect(2.0, 3.0, 14.0, 13.0, 1.5);
            line((6.5, 3.0), (6.5, 13.0));
        }
        Icon::SidebarRight => {
            rect(2.0, 3.0, 14.0, 13.0, 1.5);
            line((9.5, 3.0), (9.5, 13.0));
        }
        Icon::Plus => {
            line((8.0, 3.5), (8.0, 12.5));
            line((3.5, 8.0), (12.5, 8.0));
        }
        Icon::Close => {
            line((4.0, 4.0), (12.0, 12.0));
            line((12.0, 4.0), (4.0, 12.0));
        }
        Icon::Back => {
            line((13.0, 8.0), (3.5, 8.0));
            poly(&[(7.5, 4.0), (3.5, 8.0), (7.5, 12.0)], false);
        }
        Icon::Forward => {
            line((3.0, 8.0), (12.5, 8.0));
            poly(&[(8.5, 4.0), (12.5, 8.0), (8.5, 12.0)], false);
        }
        Icon::Up => poly(&[(4.0, 10.0), (8.0, 5.5), (12.0, 10.0)], false),
        Icon::Down => poly(&[(4.0, 6.0), (8.0, 10.5), (12.0, 6.0)], false),
        Icon::Pin => {
            poly(&[(5.0, 2.5), (11.0, 2.5), (10.0, 7.0), (12.0, 9.0), (4.0, 9.0), (6.0, 7.0)], true);
            line((8.0, 9.0), (8.0, 14.0));
        }
        Icon::ChevronDown => poly(&[(4.5, 6.0), (8.0, 9.8), (11.5, 6.0)], false),
        Icon::ChevronRight => poly(&[(6.0, 4.5), (9.8, 8.0), (6.0, 11.5)], false),
        Icon::Vault => {
            rect(2.5, 3.0, 13.5, 13.0, 2.0);
            circle(8.0, 8.0, 2.3);
            line((8.0, 5.7), (8.0, 4.2));
        }
        Icon::Replace => {
            poly(&[(3.0, 6.0), (13.0, 6.0)], false);
            poly(&[(10.0, 3.5), (13.0, 6.0), (10.0, 8.5)], false);
            poly(&[(13.0, 11.0), (3.0, 11.0)], false);
            poly(&[(6.0, 8.5), (3.0, 11.0), (6.0, 13.5)], false);
        }
        Icon::Info => {
            circle(8.0, 8.0, 5.8);
            line((8.0, 7.2), (8.0, 11.0));
            dot(8.0, 5.1, 0.75);
        }
        Icon::Check => {
            circle(8.0, 8.0, 5.8);
            poly(&[(5.3, 8.2), (7.2, 10.1), (10.8, 5.9)], false);
        }
        Icon::Question => {
            circle(8.0, 8.0, 5.8);
            poly(&[(6.2, 6.4), (6.6, 5.4), (8.0, 4.9), (9.4, 5.5), (9.6, 6.8), (8.0, 8.0), (8.0, 9.0)], false);
            dot(8.0, 10.9, 0.75);
        }
        Icon::Warning => {
            poly(&[(8.0, 2.8), (14.0, 13.0), (2.0, 13.0)], true);
            line((8.0, 6.6), (8.0, 9.8));
            dot(8.0, 11.5, 0.7);
        }
        Icon::Bolt => poly(&[(9.0, 2.0), (4.0, 9.0), (8.0, 9.0), (7.0, 14.0), (12.0, 6.5), (8.0, 6.5), (9.0, 2.0)], true),
        Icon::Quote => {
            poly(&[(3.0, 11.5), (3.0, 7.0), (6.5, 5.0)], false);
            poly(&[(9.0, 11.5), (9.0, 7.0), (12.5, 5.0)], false);
        }
        Icon::Bug => {
            rect(5.0, 5.0, 11.0, 13.0, 3.0);
            line((8.0, 5.0), (8.0, 13.0));
            line((2.5, 7.0), (5.0, 8.0));
            line((13.5, 7.0), (11.0, 8.0));
            line((2.5, 11.5), (5.0, 10.5));
            line((13.5, 11.5), (11.0, 10.5));
        }
        Icon::Flame => poly(&[(8.0, 2.0), (12.0, 7.5), (11.0, 12.5), (8.0, 14.0), (5.0, 12.5), (4.0, 8.5), (6.5, 9.5), (8.0, 2.0)], true),
    }
}

/// A square icon button. Returns the click response.
pub fn button(ui: &mut egui::Ui, icon: Icon, tip: &str, selected: bool, th: &crate::theme::Theme) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), egui::Sense::click());
    let resp = resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand);
    let bg = if selected {
        crate::theme::with_alpha(th.accent, 0.22)
    } else if resp.hovered() {
        crate::theme::with_alpha(th.fg, 0.10)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(5), bg);
    let col = if !ui.is_enabled() {
        crate::theme::with_alpha(th.fg_dim, 0.4)
    } else if selected || resp.hovered() {
        th.fg_bright
    } else {
        th.fg_dim
    };
    paint(ui.painter(), rect.center(), 15.0, icon, col);
    resp
}
