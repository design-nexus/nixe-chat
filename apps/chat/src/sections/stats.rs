//! Stats: how much you chat, with which models, and how fast they are.

use gtk::cairo;
use gtk::prelude::*;
use std::rc::Rc;

use crate::theme::{self, Rgb};
use crate::{hub, models, store, util, widgets};

pub fn build() -> gtk::Widget {
    let page = widgets::page("stats");
    let body = widgets::vbox(0);
    page.body.append(&body);
    let fill = {
        let body = body.clone();
        move || fill(&body)
    };
    fill();
    let f2 = fill.clone();
    hub::on(&hub::CHATS, &body, f2);
    hub::on(&hub::MODELS, &body, fill);
    page.root.upcast()
}

fn tile(title: &str, value: &str, sub: &str) -> gtk::Box {
    let b = widgets::vbox(2);
    b.add_css_class("stat-tile");
    b.append(&widgets::label(&title.to_uppercase(), "stat-title"));
    let v = widgets::label(value, "stat-value");
    v.add_css_class("mono");
    b.append(&v);
    if !sub.is_empty() {
        b.append(&widgets::label(sub, "stat-sub"));
    }
    b
}

fn fill(body: &gtk::Box) {
    widgets::clear(body);
    let totals = store::totals().unwrap_or(store::Totals { chats: 0, messages: 0, tokens: 0 });
    let installed = models::installed();
    let disk: u64 = installed.iter().map(|m| m.size).sum();

    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_min_children_per_line(2);
    flow.set_max_children_per_line(4);
    flow.set_row_spacing(8);
    flow.set_column_spacing(8);
    flow.add_css_class("stat-flow");
    flow.insert(&tile("Chats", &util::count(totals.chats as u64), ""), -1);
    flow.insert(&tile("Messages", &util::count(totals.messages as u64), ""), -1);
    flow.insert(&tile("Tokens written", &util::count(totals.tokens as u64), "by models"), -1);
    flow.insert(&tile("Models", &installed.len().to_string(), &util::size(disk)), -1);
    let mut c = flow.first_child();
    while let Some(w) = c {
        w.set_focusable(false);
        c = w.next_sibling();
    }
    body.append(&flow);

    let days = store::daily(30).unwrap_or_default();
    let tokens: Vec<(i64, f64)> = days.iter().map(|(d, t, _)| (*d, *t as f64)).collect();
    let chats: Vec<(i64, f64)> = days.iter().map(|(d, _, c)| (*d, *c as f64)).collect();
    body.append(&graph_card("Tokens per day", "last 30 days", tokens, true));
    body.append(&graph_card("New chats per day", "last 30 days", chats, false));

    let per = store::per_model().unwrap_or_default();
    let g = widgets::vbox(6);
    g.add_css_class("settings-group");
    g.append(&widgets::label("BY MODEL", "group-title"));
    if per.is_empty() {
        g.append(&widgets::empty("No replies yet."));
    }
    let most = per.iter().map(|m| m.replies).max().unwrap_or(1).max(1);
    for m in per {
        let row = widgets::vbox(6);
        row.add_css_class("settings-option");
        row.add_css_class("tall");
        let head = widgets::hbox(10);
        let name = widgets::label(&m.model, "settings-option-title");
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        head.append(&name);
        let speed = widgets::label(&format!("{:.1} tok/s", m.avg_tps), "value-readout");
        speed.add_css_class("mono");
        speed.set_xalign(1.0);
        head.append(&speed);
        row.append(&head);
        // A share, not a load: it never turns to the warning colour.
        let bar = gtk::LevelBar::for_interval(0.0, 1.0);
        for name in ["low", "high", "full"] {
            bar.remove_offset_value(Some(name));
        }
        bar.set_value(m.replies as f64 / most as f64);
        row.append(&bar);
        let d = widgets::label(
            &format!("{} repl{} · {} tokens", m.replies, if m.replies == 1 { "y" } else { "ies" }, util::count(m.tokens as u64)),
            "settings-option-description",
        );
        d.add_css_class("mono");
        row.append(&d);
        g.append(&row);
    }
    body.append(&g);
}

fn graph_card(title: &str, sub: &str, data: Vec<(i64, f64)>, compact_numbers: bool) -> gtk::Box {
    let card = widgets::vbox(10);
    card.add_css_class("graph-card");
    card.set_margin_top(14);
    let head = widgets::hbox(10);
    let t = widgets::label(title, "graph-card-title");
    t.set_hexpand(true);
    head.append(&t);
    let total: f64 = data.iter().map(|d| d.1).sum();
    let s = widgets::label(
        &format!("{} · {sub}", if compact_numbers { util::count(total as u64) } else { format!("{total:.0}") }),
        "dim",
    );
    s.add_css_class("mono");
    head.append(&s);
    card.append(&head);
    let area = gtk::DrawingArea::new();
    area.set_content_height(150);
    area.add_css_class("graph-plot");
    let data = Rc::new(data);
    let d1 = data.clone();
    area.set_draw_func(move |_, cr, w, h| draw_bars(cr, w as f64, h as f64, &d1, compact_numbers));
    // Hover shows the day and value.
    area.set_has_tooltip(true);
    let d2 = data.clone();
    area.connect_query_tooltip(move |a, x, _, _, tip| {
        let plot_w = (a.width() as f64 - 50.0).max(10.0);
        if d2.is_empty() || (x as f64) < 44.0 {
            return false;
        }
        let i = (((x as f64 - 44.0) / plot_w) * d2.len() as f64) as usize;
        let Some((day, v)) = d2.get(i.min(d2.len() - 1)) else { return false };
        let date = chrono::DateTime::from_timestamp(*day, 0)
            .map(|t| t.with_timezone(&chrono::Local).format("%a %b %-d").to_string())
            .unwrap_or_default();
        tip.set_text(Some(&format!("{date}: {}", if compact_numbers { util::count(*v as u64) } else { format!("{v:.0}") })));
        true
    });
    card.append(&area);
    let redraw = area.clone();
    hub::on(&hub::THEME, &area, move || redraw.queue_draw());
    card
}

fn draw_bars(cr: &cairo::Context, w: f64, h: f64, data: &[(i64, f64)], compact: bool) {
    let p = theme::palette();
    let (accent, border, dim) = (Rgb::hex(&p.accent), Rgb::hex(&p.border), Rgb::hex(&p.dim_text));
    let (left, bottom, top) = (44.0, 20.0, 8.0);
    let plot_h = (h - bottom - top).max(10.0);
    let plot_w = (w - left - 6.0).max(10.0);
    let max = data.iter().map(|d| d.1).fold(0.0, f64::max).max(1.0);
    // Counts get whole-number gridlines (three steps), so no label repeats.
    let nice = if compact { nice_ceiling(max) } else { ((max / 3.0).ceil() * 3.0).max(3.0) };

    cr.set_line_width(1.0);
    cr.set_font_size(11.0);
    for i in 0..=3 {
        let y = top + plot_h * (1.0 - i as f64 / 3.0);
        cr.set_source_rgba(border.0, border.1, border.2, 0.4);
        cr.move_to(left, y.round() + 0.5);
        cr.line_to(left + plot_w, y.round() + 0.5);
        let _ = cr.stroke();
        let v = nice * i as f64 / 3.0;
        let label = if compact { util::count(v as u64) } else { format!("{v:.0}") };
        cr.set_source_rgb(dim.0, dim.1, dim.2);
        if let Ok(ext) = cr.text_extents(&label) {
            cr.move_to(left - 6.0 - ext.width(), y + ext.height() / 2.0);
            let _ = cr.show_text(&label);
        }
    }
    if data.is_empty() {
        return;
    }
    let slot = plot_w / data.len() as f64;
    let bw = (slot * 0.7).max(1.0);
    for (i, (_, v)) in data.iter().enumerate() {
        let bh = plot_h * (v / nice);
        let x = left + slot * i as f64 + (slot - bw) / 2.0;
        let y = top + plot_h - bh;
        cr.set_source_rgba(accent.0, accent.1, accent.2, if i + 1 == data.len() { 1.0 } else { 0.78 });
        rounded_top(cr, x, y, bw, bh, (bw / 2.0).min(3.0));
        let _ = cr.fill();
    }
    // First and last day labels.
    cr.set_source_rgb(dim.0, dim.1, dim.2);
    for (i, align_end) in [(0usize, false), (data.len() - 1, true)] {
        let Some(t) = chrono::DateTime::from_timestamp(data[i].0, 0) else { continue };
        let label = t.with_timezone(&chrono::Local).format("%b %-d").to_string();
        if let Ok(ext) = cr.text_extents(&label) {
            let x = left + slot * i as f64 + slot / 2.0;
            let x = if align_end { (x - ext.width()).min(w - ext.width() - 2.0) } else { x.max(left) };
            cr.move_to(x, h - 5.0);
            let _ = cr.show_text(&label);
        }
    }
}

fn rounded_top(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    if h <= 0.5 {
        return;
    }
    let r = r.min(h);
    cr.new_path();
    cr.move_to(x, y + h);
    cr.line_to(x, y + r);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.line_to(x + w - r, y);
    cr.arc(x + w - r, y + r, r, 1.5 * std::f64::consts::PI, 0.0);
    cr.line_to(x + w, y + h);
    cr.close_path();
}

fn nice_ceiling(v: f64) -> f64 {
    if v <= 0.0 {
        return 1.0;
    }
    let exp = 10f64.powf(v.log10().floor());
    let f = v / exp;
    let n = [1.0, 1.5, 2.0, 3.0, 5.0, 10.0].into_iter().find(|n| f <= *n).unwrap_or(10.0);
    n * exp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_axis_tops() {
        assert_eq!(nice_ceiling(7.0), 10.0);
        assert_eq!(nice_ceiling(120.0), 150.0);
        assert_eq!(nice_ceiling(0.0), 1.0);
    }
}
