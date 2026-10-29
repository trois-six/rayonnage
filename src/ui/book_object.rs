//! GObject wrapper around a `Book` for the GTK list models.

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

use crate::model::Book;

mod imp {
    use std::cell::Cell;

    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::BookObject)]
    pub struct BookObject {
        /// Bumped every time the book changes, so bound cells refresh.
        #[property(get, set)]
        pub revision: Cell<u64>,
        pub book: RefCell<Book>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BookObject {
        const NAME: &'static str = "BoBookObject";
        type Type = super::BookObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for BookObject {}
}

glib::wrapper! {
    pub struct BookObject(ObjectSubclass<imp::BookObject>);
}

impl BookObject {
    pub fn new(book: Book) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().book.replace(book);
        obj
    }

    pub fn id(&self) -> u64 {
        self.imp().book.borrow().id
    }

    pub fn book(&self) -> std::cell::Ref<'_, Book> {
        self.imp().book.borrow()
    }

    pub fn set_book(&self, book: Book) {
        self.imp().book.replace(book);
        self.set_revision(self.revision() + 1);
    }
}
