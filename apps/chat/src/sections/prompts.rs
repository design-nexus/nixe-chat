//! Saved prompts, dropped into any chat by typing / and their command.

use gtk::prelude::*;

use crate::store::{self, Prompt};
use crate::{hub, util, widgets, window};

pub fn build() -> gtk::Widget {
    let page = widgets::page(
        "Prompts",
        "Text you use often. Type / and the command in any chat to insert it; variables such as {{CLIPBOARD}} are filled in.",
        &[],
    );
    let g = page.group("");
    let (row, _) = widgets::button_row("New prompt", "Give it a short command, like <tt>summarize</tt>.", "Add…", |_| {
        edit(Prompt::default())
    });
    g.add(&row);

    let list = widgets::vbox(6);
    let lg = page.group("Saved");
    lg.add(&list);
    let fill = {
        let list = list.clone();
        move || {
            widgets::clear(&list);
            let prompts = store::prompts().unwrap_or_default();
            if prompts.is_empty() {
                list.append(&widgets::empty("No prompts yet."));
            }
            for p in prompts {
                let controls = widgets::hbox(6);
                let e = gtk::Button::with_label("Edit");
                let p2 = p.clone();
                e.connect_clicked(move |_| edit(p2.clone()));
                controls.append(&e);
                let id = p.id;
                controls.append(&widgets::two_click("Delete", "Click again", move || {
                    let _ = store::delete_prompt(id);
                    hub::emit(&hub::LIBRARY);
                }));
                let desc = if p.title.is_empty() {
                    util::esc(&util::clip(&p.content, 140))
                } else {
                    format!("{} · {}", util::esc(&p.title), util::esc(&util::clip(&p.content, 100)))
                };
                let row = widgets::row(&format!("/{}", p.command), &desc, Some(controls.upcast_ref()));
                if let Some(t) = row.first_child().and_then(|b| b.first_child()) {
                    t.add_css_class("mono");
                }
                list.append(&row);
            }
        }
    };
    fill();
    hub::on(&hub::LIBRARY, &list, fill);
    page.root.upcast()
}

fn edit(p: Prompt) {
    let new = p.id == 0;
    let (dialog, card) = widgets::dialog(if new { "New prompt" } else { "Edit prompt" }, 560);
    card.append(&widgets::label("Command", "field-label"));
    let cmd = gtk::Entry::new();
    cmd.add_css_class("mono");
    cmd.set_text(&p.command);
    cmd.set_placeholder_text(Some("summarize"));
    card.append(&cmd);
    card.append(&widgets::label("Title", "field-label"));
    let title = gtk::Entry::new();
    title.set_text(&p.title);
    title.set_placeholder_text(Some("Summarize this"));
    card.append(&title);
    card.append(&widgets::label("Text", "field-label"));
    let (sw, content) = widgets::text_area(&p.content, false, 140);
    card.append(&sw);
    let vars = widgets::label("{{CLIPBOARD}} {{CURRENT_DATE}} {{CURRENT_TIME}} {{USER_NAME}}", "dim");
    vars.add_css_class("mono");
    vars.add_css_class("small-text");
    card.append(&vars);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&save);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let d = dialog.clone();
    save.connect_clicked(move |_| {
        let command: String = cmd.text().trim().trim_start_matches('/').chars().filter(|c| !c.is_whitespace()).collect();
        let text = widgets::buffer_text(&content);
        if command.is_empty() || text.trim().is_empty() {
            window::toast("A prompt needs a command and some text");
            return;
        }
        let taken = store::prompts().unwrap_or_default().iter().any(|x| x.command == command && x.id != p.id);
        if taken {
            window::toast(&format!("/{command} is already used"));
            return;
        }
        let _ = store::save_prompt(&Prompt { id: p.id, command, title: title.text().trim().to_string(), content: text });
        hub::emit(&hub::LIBRARY);
        d.close();
    });
    dialog.present();
}
