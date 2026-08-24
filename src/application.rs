use gtk::{gio, glib};

mod imp {
    use std::cell::OnceCell;
    use std::cell::RefCell;
    use std::rc::Rc;

    use adw::prelude::*;
    use adw::subclass::prelude::*;
    use gtk::glib;
    use rusqlite::Connection;

    use crate::library;
    use crate::ui::GnosisWindow;

    #[derive(Default)]
    pub struct GnosisApplication {
        pub db: OnceCell<Rc<RefCell<Connection>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GnosisApplication {
        const NAME: &'static str = "GnosisApplication";
        type Type = super::GnosisApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for GnosisApplication {}

    impl ApplicationImpl for GnosisApplication {
        fn startup(&self) {
            self.parent_startup();

            match library::db::init_db() {
                Ok(conn) => {
                    self.db.set(Rc::new(RefCell::new(conn))).ok();
                }
                Err(err) => eprintln!("failed to open library database: {err}"),
            }
        }

        fn activate(&self) {
            self.parent_activate();

            let app = self.obj();
            if let Some(window) = app.active_window() {
                window.present();
                return;
            }

            let window = GnosisWindow::new(&app);
            if let Some(db) = self.db.get() {
                window.set_database(db.clone());
                window.scan_library_folders();
            }
            window.present();
        }
    }

    impl GtkApplicationImpl for GnosisApplication {}
    impl AdwApplicationImpl for GnosisApplication {}
}

glib::wrapper! {
    pub struct GnosisApplication(ObjectSubclass<imp::GnosisApplication>)
        @extends adw::Application, gtk::Application, gio::Application,
        @implements gio::ActionMap, gio::ActionGroup;
}

impl GnosisApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", "me.mavenried.Gnosis")
            .property("flags", gio::ApplicationFlags::empty())
            .build()
    }
}

impl Default for GnosisApplication {
    fn default() -> Self {
        Self::new()
    }
}
