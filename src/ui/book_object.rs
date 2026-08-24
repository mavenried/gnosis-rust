use gtk::glib;
use gtk::subclass::prelude::*;

use crate::library::Book;

mod imp {
    use std::cell::RefCell;

    use gtk::glib;
    use gtk::subclass::prelude::*;

    use crate::library::Book;

    #[derive(Default)]
    pub struct BookObject {
        pub book: RefCell<Option<Book>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BookObject {
        const NAME: &'static str = "GnosisBookObject";
        type Type = super::BookObject;
    }

    impl ObjectImpl for BookObject {}
}

glib::wrapper! {
    pub struct BookObject(ObjectSubclass<imp::BookObject>);
}

impl BookObject {
    pub fn new(book: Book) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().book.replace(Some(book));
        obj
    }

    pub fn book(&self) -> Book {
        self.imp()
            .book
            .borrow()
            .clone()
            .expect("BookObject constructed without a Book")
    }

    pub fn set_book(&self, book: Book) {
        self.imp().book.replace(Some(book));
    }
}
