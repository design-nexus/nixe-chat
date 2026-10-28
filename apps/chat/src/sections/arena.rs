//! Arena: two anonymous models answer the same prompt; the user votes, and
//! an Elo leaderboard keeps score.

use gtk::glib;
use gtk::prelude::*;
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::markdown::Markdown;
use crate::ollama::api::Conn;
use crate::ollama::chat::{self, Event, Msg, Request};
use crate::{hub, models, prefs, rt, store, tools, util, widgets, window};

const RANDOM: &str = "Random";

/// Elo ratings from votes, best first, with wins, losses and ties. A tie
/// counts half a win each; "both bad" is a tie that also costs each a few points.
pub fn elo(votes: &[(String, String, String)]) -> Vec<(String, f64, u32, u32, u32)> {
    const K: f64 = 32.0;
    let mut r: HashMap<String, (f64, u32, u32, u32)> = HashMap::new();
    for (a, b, winner) in votes {
        let ra = r.entry(a.clone()).or_insert((1000.0, 0, 0, 0)).0;
        let rb = r.entry(b.clone()).or_insert((1000.0, 0, 0, 0)).0;
        let ea = 1.0 / (1.0 + 10f64.powf((rb - ra) / 400.0));
        let sa = match winner.as_str() {
            "a" => 1.0,
            "b" => 0.0,
            _ => 0.5,
        };
        let penalty = if winner == "bad" { 8.0 } else { 0.0 };
        let na = ra + K * (sa - ea) - penalty;
        let nb = rb + K * ((1.0 - sa) - (1.0 - ea)) - penalty;
        let ea_ = r.get_mut(a).expect("inserted");
        ea_.0 = na;
        match winner.as_str() {
            "a" => ea_.1 += 1,
            "b" => ea_.2 += 1,
            _ => ea_.3 += 1,
        }
        let eb = r.get_mut(b).expect("inserted");
        eb.0 = nb;
        match winner.as_str() {
            "b" => eb.1 += 1,
            "a" => eb.2 += 1,
            _ => eb.3 += 1,
        }
    }
    let mut out: Vec<(String, f64, u32, u32, u32)> = r.into_iter().map(|(m, (e, w, l, t))| (m, e, w, l, t)).collect();
    out.sort_by(|x, y| y.1.total_cmp(&x.1));
    out
}

struct Round {
    prompt: String,
    a: String,
    b: String,
    done: Cell<u8>,
    voted: Cell<bool>,
}

pub fn build() -> gtk::Widget {
    let page = widgets::page("arena");

    let g = page.group("New round");
    let pickers = widgets::hbox(10);
    let pick_a = gtk::DropDown::from_strings(&[]);
    let pick_b = gtk::DropDown::from_strings(&[]);
    widgets::ellipsize(&pick_a);
    widgets::ellipsize(&pick_b);
    for (label, dd) in [("Model A", &pick_a), ("Model B", &pick_b)] {
        let v = widgets::vbox(4);
        v.set_hexpand(true);
        v.append(&widgets::label(label, "field-label"));
        v.append(dd);
        pickers.append(&v);
    }
    let fill = {
        let (a, b) = (pick_a.clone(), pick_b.clone());
        move || {
            let mut names = vec![RANDOM.to_string()];
            names.extend(models::chat_models());
            // Short names: two pickers sit side by side.
            let labels: Vec<String> = names
                .iter()
                .map(|m| if m == RANDOM { m.clone() } else { util::clip_middle(&util::model_label(m), 26) })
                .collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            a.set_model(Some(&gtk::StringList::new(&refs)));
            b.set_model(Some(&gtk::StringList::new(&refs)));
        }
    };
    fill();
    hub::on(&hub::MODELS, &pickers, fill);
    g.add(&pickers);
    let note = widgets::label("Leave both on Random to keep it blind. Names are shown after you vote.", "dim");
    note.add_css_class("small-text");
    note.set_wrap(true);
    g.add(&note);
    let (sw, prompt) = widgets::text_area("", false, 80);
    let prompt_buf = prompt.buffer();
    g.add(&sw);
    let start = gtk::Button::with_label("Start round");
    start.add_css_class("suggested-action");
    start.set_halign(gtk::Align::End);
    g.add(&start);

    let arena = widgets::hbox(14);
    arena.set_homogeneous(true);
    arena.add_css_class("arena-row");
    page.body.append(&arena);
    let votes = widgets::hbox(8);
    votes.set_halign(gtk::Align::Center);
    votes.add_css_class("arena-votes");
    votes.set_visible(false);
    page.body.append(&votes);
    crate::window::on_narrow({
        let arena = arena.clone();
        move |n| arena.set_orientation(if n { gtk::Orientation::Vertical } else { gtk::Orientation::Horizontal })
    });

    let board = page.group("Leaderboard");
    let board_list = widgets::vbox(6);
    board.add(&board_list);
    let fill_board = {
        let bl = board_list.clone();
        Rc::new(move || draw_board(&bl))
    };
    fill_board();

    let round: Rc<RefCell<Option<Rc<Round>>>> = Rc::new(RefCell::new(None));
    let (pa, pb, arena2, votes2, round2, fb) =
        (pick_a.clone(), pick_b.clone(), arena.clone(), votes.clone(), round.clone(), fill_board.clone());
    start.connect_clicked(move |_| {
        let text = widgets::buffer_text(&prompt).trim().to_string();
        if text.is_empty() {
            prompt.grab_focus();
            return;
        }
        let names = models::chat_models();
        if names.len() < 2 {
            window::toast("The arena needs at least two chat models");
            return;
        }
        let chosen = |dd: &gtk::DropDown| -> Option<String> {
            let i = dd.selected() as usize;
            if i == 0 { None } else { names.get(i - 1).cloned() }
        };
        let (mut a, mut b) = (chosen(&pa), chosen(&pb));
        let seed = util::now() as usize ^ (text.len() * 7919);
        if a.is_none() {
            let pool: Vec<&String> = names.iter().filter(|n| Some(*n) != b.as_ref()).collect();
            a = pool.get(seed % pool.len()).map(|s| s.to_string());
        }
        if b.is_none() {
            let pool: Vec<&String> = names.iter().filter(|n| Some(*n) != a.as_ref()).collect();
            b = pool.get((seed / 3 + 1) % pool.len()).map(|s| s.to_string());
        }
        let (Some(a), Some(b)) = (a, b) else { return };
        if a == b {
            window::toast("Pick two different models");
            return;
        }
        let r = Rc::new(Round { prompt: text.clone(), a: a.clone(), b: b.clone(), done: Cell::new(0), voted: Cell::new(false) });
        *round2.borrow_mut() = Some(r.clone());
        widgets::clear(&arena2);
        widgets::clear(&votes2);
        votes2.set_visible(false);
        let cols = [("Model A", a), ("Model B", b)].map(|(label, model)| {
            let col = widgets::vbox(8);
            col.add_css_class("assistant-msg");
            col.add_css_class("arena-answer");
            let name = widgets::short_label(label, "model-name", gtk::pango::EllipsizeMode::End);
            col.append(&name);
            let md = Markdown::new();
            col.append(&md.root);
            arena2.append(&col);
            (model, name, md)
        });
        let names_labels: Vec<gtk::Label> = cols.iter().map(|c| c.1.clone()).collect();
        if prefs::get().host().backend == prefs::Backend::LlamaCpp {
            let [(a, _, a_md), (b, _, b_md)] = cols;
            let (r2, votes3, fb2, labels, prompt) = (r.clone(), votes2.clone(), fb.clone(), names_labels.clone(), text.clone());
            run_one(&a, &text, a_md, move || {
                r2.done.set(1);
                let (r3, votes4, fb3, labels2) = (r2.clone(), votes3.clone(), fb2.clone(), labels.clone());
                run_one(&b, &prompt, b_md.clone(), move || {
                    r3.done.set(2);
                    show_votes(&votes4, &r3, &labels2, fb3.clone());
                });
            });
        } else {
            for (model, _, md) in cols {
                let (r2, votes3, fb2, labels) = (r.clone(), votes2.clone(), fb.clone(), names_labels.clone());
                run_one(&model, &text, md, move || {
                    r2.done.set(r2.done.get() + 1);
                    if r2.done.get() == 2 { show_votes(&votes3, &r2, &labels, fb2.clone()); }
                });
            }
        }
    });
    // Developer aid: CHAT_SNAPSHOT_ARENA="prompt" plays a round on its own.
    if let Ok(text) = std::env::var("CHAT_SNAPSHOT_ARENA") {
        let s2 = start.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
            prompt_buf.set_text(&text);
            s2.emit_clicked();
        });
    }
    let fb = fill_board.clone();
    let clear = widgets::two_click("Reset leaderboard", "Click again to clear every vote", move || {
        let _ = store::clear_votes();
        fb();
    });
    clear.set_halign(gtk::Align::Start);
    board.add(&clear);
    page.root.upcast()
}

fn run_one(model: &str, prompt: &str, md: Rc<Markdown>, done: impl Fn() + 'static) {
    let pr = prefs::get();
    let settings = prefs::for_model(model);
    let local_llama = crate::llama::api::local_service();
    let think = models::has(model, "thinking").then_some(false);
    let req = Request {
        conn: Conn::current(),
        model: model.to_string(),
        messages: vec![Msg::new("user", prompt)],
        tools: vec![],
        think,
        format: None,
        options: if local_llama { json!({ "temperature": settings.temperature, "top_p": settings.top_p,
            "top_k": settings.top_k, "min_p": settings.min_p, "repeat_penalty": settings.repeat_penalty,
            "num_ctx": settings.num_ctx }) } else { json!({ "temperature": pr.temperature, "num_ctx": pr.num_ctx }) },
        keep_alive: models::keep_alive_for(model),
        max_rounds: 1,
        tool_ctx: tools::Ctx { prefs: pr },
    };
    let text = Rc::new(RefCell::new(String::new()));
    let cancel = Arc::new(AtomicBool::new(false));
    rt::stream(
        move |tx| chat::run(req, tx, cancel),
        move |ev| match ev {
            Event::Delta { content, .. } => {
                text.borrow_mut().push_str(&content);
                md.set(&text.borrow());
            }
            Event::Error(e) => md.set(&format!("{}\n\n*Error: {e}*", text.borrow())),
            Event::Done => done(),
            _ => {}
        },
    );
}

fn show_votes(votes: &gtk::Box, r: &Rc<Round>, labels: &[gtk::Label], board: Rc<dyn Fn()>) {
    votes.set_visible(true);
    for (id, text) in [("a", "A is better"), ("tie", "It's a tie"), ("b", "B is better"), ("bad", "Both are bad")] {
        let b = gtk::Button::with_label(text);
        if id == "a" || id == "b" {
            b.add_css_class("suggested-action");
        }
        let (r2, labels2, votes2, board2) = (r.clone(), labels.to_vec(), votes.clone(), board.clone());
        b.connect_clicked(move |_| {
            if r2.voted.replace(true) {
                return;
            }
            let _ = store::add_vote(&r2.prompt, &r2.a, &r2.b, id);
            for (l, (side, m)) in labels2.iter().zip([("A", &r2.a), ("B", &r2.b)]) {
                l.set_text(&format!("{side} · {}", util::model_label(m)));
                l.set_tooltip_text(Some(m));
            }
            widgets::clear(&votes2);
            votes2.append(&widgets::label("Thanks. The leaderboard is updated.", "dim"));
            board2();
        });
        votes.append(&b);
    }
}

fn draw_board(b: &gtk::Box) {
    widgets::clear(b);
    let votes = store::votes().unwrap_or_default();
    if votes.is_empty() {
        b.append(&widgets::empty("No votes yet. Play a round above."));
        return;
    }
    for (i, (model, rating, w, l, t)) in elo(&votes).into_iter().enumerate() {
        let row = widgets::hbox(12);
        row.add_css_class("settings-option");
        let rank = widgets::label(&format!("{}", i + 1), "board-rank");
        rank.add_css_class("tnum");
        rank.set_width_chars(3);
        row.append(&rank);
        let name = widgets::label(&model, "settings-option-title");
        name.set_hexpand(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        row.append(&name);
        let rec = widgets::label(&format!("{w}–{l}–{t}"), "dim");
        rec.add_css_class("tnum");
        rec.set_tooltip_text(Some("Wins, losses, ties"));
        row.append(&rec);
        let r = widgets::label(&format!("{rating:.0}"), "value-readout");
        r.add_css_class("tnum");
        r.set_xalign(1.0);
        row.append(&r);
        b.append(&row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &str, b: &str, w: &str) -> (String, String, String) {
        (a.into(), b.into(), w.into())
    }

    #[test]
    fn winners_rise() {
        let board = elo(&[v("x", "y", "a"), v("x", "y", "a"), v("y", "z", "tie")]);
        assert_eq!(board[0].0, "x");
        assert!(board[0].1 > 1000.0);
        let x = board.iter().find(|r| r.0 == "x").unwrap();
        assert_eq!((x.2, x.3, x.4), (2, 0, 0));
        let y = board.iter().find(|r| r.0 == "y").unwrap();
        assert_eq!((y.2, y.3, y.4), (0, 2, 1));
    }

    #[test]
    fn ties_between_equals_change_nothing() {
        let board = elo(&[v("x", "y", "tie")]);
        assert!(board.iter().all(|r| (r.1 - 1000.0).abs() < 1e-9));
    }
}
