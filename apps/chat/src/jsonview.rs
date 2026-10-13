//! A collapsible tree for JSON replies, with a toggle to the raw text.

use gtk::prelude::*;
use serde_json::Value;

use crate::{util, widgets};

pub fn view(v: &Value, raw: &str) -> gtk::Widget {
    let root = widgets::vbox(6);
    root.add_css_class("json-view");
    let head = widgets::hbox(8);
    let t = widgets::label("JSON", "json-badge");
    t.add_css_class("mono");
    t.set_hexpand(true);
    head.append(&t);
    let raw_btn = gtk::ToggleButton::with_label("Raw");
    raw_btn.add_css_class("chip");
    head.append(&raw_btn);
    let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
    copy.add_css_class("flat");
    copy.add_css_class("tiny");
    copy.set_tooltip_text(Some("Copy JSON"));
    let pretty = serde_json::to_string_pretty(v).unwrap_or_else(|_| raw.to_string());
    let p2 = pretty.clone();
    copy.connect_clicked(move |b| {
        b.clipboard().set_text(&p2);
        crate::window::toast("Copied");
    });
    head.append(&copy);
    root.append(&head);

    let stack = gtk::Stack::new();
    let tree = widgets::vbox(2);
    tree.add_css_class("json-tree");
    node(&tree, None, v, "$", 0);
    stack.add_named(&tree, Some("tree"));
    let rawl = widgets::label(&pretty, "code-block");
    rawl.set_selectable(true);
    rawl.set_wrap(true);
    rawl.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    stack.add_named(&rawl, Some("raw"));
    let s2 = stack.clone();
    raw_btn.connect_toggled(move |b| s2.set_visible_child_name(if b.is_active() { "raw" } else { "tree" }));
    root.append(&stack);
    root.upcast()
}

fn scalar(v: &Value) -> (String, &'static str) {
    match v {
        Value::String(s) => (format!("\"{}\"", util::clip(s, 400)), "json-string"),
        Value::Number(n) => (n.to_string(), "json-number"),
        Value::Bool(b) => (b.to_string(), "json-number"),
        Value::Null => ("null".into(), "dim"),
        _ => (String::new(), ""),
    }
}

/// One key/value, recursing into objects and arrays (open to depth 2).
fn node(parent: &gtk::Box, key: Option<&str>, v: &Value, path: &str, depth: usize) {
    let key_label = |k: &str| {
        let l = widgets::label(&format!("{k}:"), "json-key");
        l.add_css_class("mono");
        l
    };
    match v {
        Value::Object(_) | Value::Array(_) => {
            let n = match v {
                Value::Object(o) => o.len(),
                Value::Array(a) => a.len(),
                _ => 0,
            };
            let summary = if v.is_object() { format!("{{{n}}}") } else { format!("[{n}]") };
            let exp = gtk::Expander::new(None);
            let row = widgets::hbox(6);
            if let Some(k) = key {
                row.append(&key_label(k));
            }
            let s = widgets::label(&summary, "dim");
            s.add_css_class("mono");
            row.append(&s);
            exp.set_label_widget(Some(&row));
            exp.set_expanded(depth < 2);
            exp.set_tooltip_text(Some(path));
            let children = widgets::vbox(2);
            children.set_margin_start(16);
            match v {
                Value::Object(o) => {
                    for (k, child) in o {
                        node(&children, Some(k), child, &format!("{path}.{k}"), depth + 1);
                    }
                }
                Value::Array(a) => {
                    for (i, child) in a.iter().enumerate() {
                        node(&children, Some(&i.to_string()), child, &format!("{path}[{i}]"), depth + 1);
                    }
                }
                _ => {}
            }
            exp.set_child(Some(&children));
            parent.append(&exp);
        }
        _ => {
            let row = widgets::hbox(6);
            row.set_margin_start(20);
            if let Some(k) = key {
                row.append(&key_label(k));
            }
            let (text, class) = scalar(v);
            let l = widgets::label(&text, class);
            l.add_css_class("mono");
            l.set_selectable(true);
            l.set_wrap(true);
            l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            l.set_tooltip_text(Some(path));
            row.append(&l);
            parent.append(&row);
        }
    }
}
