//! Change notifications on the GTK thread. A subscriber is tied to a widget
//! and dropped once that widget is gone.

use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

type Sub = (glib::WeakRef<gtk::Widget>, Rc<dyn Fn()>);

#[derive(Default)]
pub struct Hub {
    subs: RefCell<Vec<Sub>>,
}

impl Hub {
    pub fn subscribe(&self, owner: &impl IsA<gtk::Widget>, f: impl Fn() + 'static) {
        self.subs.borrow_mut().push((owner.upcast_ref::<gtk::Widget>().downgrade(), Rc::new(f)));
    }

    pub fn emit(&self) {
        // Copy out first: a subscriber may subscribe or emit again.
        let live: Vec<Rc<dyn Fn()>> = {
            let mut subs = self.subs.borrow_mut();
            subs.retain(|(w, _)| w.upgrade().is_some());
            subs.iter().map(|(_, f)| f.clone()).collect()
        };
        for f in live {
            f();
        }
    }
}

thread_local! {
    /// The palette changed.
    pub static THEME: Hub = Hub::default();
    /// Installed models, capabilities or the server's reachability changed.
    pub static MODELS: Hub = Hub::default();
    /// A chat was created, renamed, moved, pinned, archived or deleted.
    pub static CHATS: Hub = Hub::default();
    /// A download started, progressed or finished.
    pub static PULLS: Hub = Hub::default();
    /// Knowledge bases, prompts or memories changed.
    pub static LIBRARY: Hub = Hub::default();
    /// The active server changed.
    pub static HOST: Hub = Hub::default();
}

pub fn on(hub: &'static std::thread::LocalKey<Hub>, owner: &impl IsA<gtk::Widget>, f: impl Fn() + 'static) {
    hub.with(|h| h.subscribe(owner, f));
}

pub fn emit(hub: &'static std::thread::LocalKey<Hub>) {
    hub.with(|h| h.emit());
}
