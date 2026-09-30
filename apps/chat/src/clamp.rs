//! A container that centres its child and caps its width, so text stays a
//! comfortable line length on wide windows.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::Cell;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Clamp {
        pub max: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Clamp {
        const NAME: &'static str = "NexusChatClamp";
        type Type = super::Clamp;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Clamp {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Clamp {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = self.obj().first_child() else { return (0, 0, -1, -1) };
            let max = self.max.get();
            if orientation == gtk::Orientation::Horizontal {
                let (min, nat, _, _) = child.measure(orientation, for_size);
                (min, nat.min(max.max(min)), -1, -1)
            } else {
                let (child_min, _, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
                let w = if for_size < 0 { -1 } else { for_size.min(max).max(child_min) };
                let (min, nat, _, _) = child.measure(orientation, w);
                (min, nat, -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(child) = self.obj().first_child() else { return };
            let (min, _, _, _) = child.measure(gtk::Orientation::Horizontal, -1);
            let w = width.min(self.max.get()).max(min.min(width));
            let x = (width - w) / 2;
            child.size_allocate(&gtk::Allocation::new(x, 0, w, height), baseline);
        }
    }
}

glib::wrapper! {
    pub struct Clamp(ObjectSubclass<imp::Clamp>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Clamp {
    pub fn new(child: &impl IsA<gtk::Widget>, max: i32) -> Clamp {
        let c: Clamp = glib::Object::new();
        c.imp().max.set(max);
        child.set_parent(&c);
        c.set_hexpand(true);
        c
    }
}
