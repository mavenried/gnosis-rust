use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::ObjectSubclassIsExt;
use gtk::{gio, glib};
use rusqlite::Connection;

use uuid::Uuid;

use crate::application::GnosisApplication;
use crate::library;

use super::book_card;
use super::book_object::BookObject;

/// How an "Edit Metadata" save should affect the underlying EPUB file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteBackMode {
    /// Only update Gnosis's own library database.
    None,
    /// Write a new EPUB file alongside the original, which is left untouched.
    Copy,
    /// Overwrite the original EPUB file in place.
    InPlace,
}

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::rc::Rc;

    use adw::subclass::prelude::*;
    use gtk::glib;
    use rusqlite::Connection;

    #[derive(Default)]
    pub struct GnosisWindow {
        pub store: OnceCell<gtk::gio::ListStore>,
        pub stack: OnceCell<gtk::Stack>,
        pub toast_overlay: OnceCell<adw::ToastOverlay>,
        pub nav_view: OnceCell<adw::NavigationView>,
        pub settings_page: OnceCell<adw::NavigationPage>,
        pub log_view: OnceCell<gtk::TextView>,
        pub refresh_button: OnceCell<gtk::Button>,
        pub refresh_stack: OnceCell<gtk::Stack>,
        pub refresh_spinner: OnceCell<gtk::Spinner>,
        pub progress_box: OnceCell<gtk::Box>,
        pub progress_label: OnceCell<gtk::Label>,
        pub progress_spinner: OnceCell<gtk::Spinner>,
        pub refreshing: Cell<bool>,
        pub search_query: Rc<RefCell<String>>,
        pub sort_key: Rc<RefCell<String>>,
        pub db: OnceCell<Rc<RefCell<Connection>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GnosisWindow {
        const NAME: &'static str = "GnosisWindow";
        type Type = super::GnosisWindow;
        type ParentType = adw::ApplicationWindow;
    }

    impl ObjectImpl for GnosisWindow {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_ui();
        }
    }
    impl WidgetImpl for GnosisWindow {}
    impl WindowImpl for GnosisWindow {}
    impl ApplicationWindowImpl for GnosisWindow {}
    impl AdwApplicationWindowImpl for GnosisWindow {}
}

glib::wrapper! {
    pub struct GnosisWindow(ObjectSubclass<imp::GnosisWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionMap, gio::ActionGroup, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl GnosisWindow {
    pub fn new(app: &GnosisApplication) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    /// Attaches the library database and populates the shelf from it.
    /// Must be called once, right after construction.
    pub fn set_database(&self, db: Rc<RefCell<Connection>>) {
        let books = match library::db::list_books(&db.borrow()) {
            Ok(books) => books,
            Err(err) => {
                eprintln!("failed to load library: {err}");
                Vec::new()
            }
        };

        let imp = self.imp();
        let store = imp.store.get().expect("store built in constructed()");
        for book in books {
            store.append(&BookObject::new(book));
        }
        imp.db.set(db).ok();

        self.update_empty_state();
    }

    fn setup_ui(&self) {
        let imp = self.imp();

        self.set_title(Some("Gnosis"));
        self.set_default_size(900, 640);

        let store = gio::ListStore::new::<BookObject>();

        let search_query = imp.search_query.clone();
        let filter = gtk::CustomFilter::new(move |obj| {
            let query = search_query.borrow();
            if query.is_empty() {
                return true;
            }
            let Some(book_object) = obj.downcast_ref::<BookObject>() else {
                return false;
            };
            let book = book_object.book();
            let query = query.to_lowercase();
            book.title.to_lowercase().contains(&query)
                || book
                    .author
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&query)
        });

        let filter_model = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));

        *imp.sort_key.borrow_mut() = "title".to_string();
        let sort_key = imp.sort_key.clone();
        let sorter = gtk::CustomSorter::new(move |a, b| {
            let (Some(a), Some(b)) = (
                a.downcast_ref::<BookObject>().map(BookObject::book),
                b.downcast_ref::<BookObject>().map(BookObject::book),
            ) else {
                return gtk::Ordering::Equal;
            };

            let ordering = match sort_key.borrow().as_str() {
                "author" => a
                    .author
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .cmp(&b.author.as_deref().unwrap_or_default().to_lowercase())
                    .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
                "added" => b.added_at.cmp(&a.added_at),
                _ => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            };

            match ordering {
                std::cmp::Ordering::Less => gtk::Ordering::Smaller,
                std::cmp::Ordering::Equal => gtk::Ordering::Equal,
                std::cmp::Ordering::Greater => gtk::Ordering::Larger,
            }
        });

        let sorted_model = gtk::SortListModel::new(Some(filter_model), Some(sorter.clone()));
        let selection_model = gtk::NoSelection::new(Some(sorted_model));

        let grid_view = gtk::GridView::new(Some(selection_model), Some(book_card::factory()));
        grid_view.set_single_click_activate(false);
        // GtkGridView caps at 7 columns by default and stretches them to fill
        // the width, which balloons cover cards on wide windows. Raise the
        // cap so it keeps adding columns near the cards' natural size instead.
        grid_view.set_min_columns(2);
        grid_view.set_max_columns(64);

        let window_weak = self.downgrade();
        grid_view.connect_activate(move |grid_view, position| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Some(model) = grid_view.model() else {
                return;
            };
            let Some(book_object) = model.item(position).and_downcast::<BookObject>() else {
                return;
            };
            window.show_reader_stub(&book_object.book());
        });

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&grid_view)
            .build();

        let empty_page = adw::StatusPage::builder()
            .title("Your library is empty")
            .description("Add EPUB books to start building your shelf.")
            .vexpand(true)
            .build();
        let empty_add_button = gtk::Button::builder()
            .label("Add Book")
            .css_classes(["suggested-action", "pill"])
            .halign(gtk::Align::Center)
            .build();
        empty_add_button.connect_clicked(|button| {
            button.activate_action("win.add-book", None).ok();
        });
        empty_page.set_child(Some(&empty_add_button));

        let stack = gtk::Stack::new();
        stack.add_named(&empty_page, Some("empty"));
        stack.add_named(&scrolled, Some("library"));

        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text("Search your library")
            .build();
        let query_slot = imp.search_query.clone();
        search_entry.connect_search_changed(move |entry| {
            *query_slot.borrow_mut() = entry.text().to_string();
            filter.changed(gtk::FilterChange::Different);
        });

        let add_button = gtk::Button::from_icon_name("list-add-symbolic");
        add_button.set_tooltip_text(Some("Add Book"));
        add_button.connect_clicked(|button| {
            button.activate_action("win.add-book", None).ok();
        });

        let settings_button = gtk::Button::from_icon_name("preferences-system-symbolic");
        settings_button.set_tooltip_text(Some("Settings"));
        settings_button.connect_clicked(|button| {
            button.activate_action("win.show-settings", None).ok();
        });

        let sort_menu = gio::Menu::new();
        sort_menu.append(Some("Title"), Some("win.sort-by('title')"));
        sort_menu.append(Some("Author"), Some("win.sort-by('author')"));
        sort_menu.append(Some("Date Added"), Some("win.sort-by('added')"));
        let sort_button = gtk::MenuButton::builder()
            .icon_name("view-sort-descending-symbolic")
            .tooltip_text("Sort")
            .menu_model(&sort_menu)
            .build();

        let sort_action = gio::SimpleAction::new_stateful(
            "sort-by",
            Some(glib::VariantTy::STRING),
            &"title".to_variant(),
        );
        let sort_key_slot = imp.sort_key.clone();
        sort_action.connect_activate(move |action, parameter| {
            let Some(key) = parameter.and_then(glib::Variant::str) else {
                return;
            };
            *sort_key_slot.borrow_mut() = key.to_string();
            action.set_state(&key.to_variant());
            sorter.changed(gtk::SorterChange::Different);
        });
        self.add_action(&sort_action);

        let header_bar = adw::HeaderBar::new();
        header_bar.set_title_widget(Some(&adw::WindowTitle::new("Gnosis", "")));
        header_bar.pack_start(&search_entry);
        header_bar.pack_end(&add_button);
        header_bar.pack_end(&settings_button);
        header_bar.pack_end(&sort_button);

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header_bar);
        toolbar_view.set_content(Some(&stack));
        let library_page = adw::NavigationPage::with_tag(&toolbar_view, "Gnosis", "library");

        let settings = super::preferences::build_page(self);

        let nav_view = adw::NavigationView::new();
        nav_view.push(&library_page);

        // A small corner notification showing background-refresh progress,
        // separate from the (bottom-center, fire-and-forget) toast overlay.
        let progress_spinner = gtk::Spinner::new();
        let progress_label = gtk::Label::new(None);
        let progress_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        progress_box.append(&progress_spinner);
        progress_box.append(&progress_label);
        progress_box.add_css_class("card");
        progress_box.set_margin_top(10);
        progress_box.set_margin_bottom(18);
        progress_box.set_margin_start(12);
        progress_box.set_margin_end(18);
        progress_box.set_halign(gtk::Align::End);
        progress_box.set_valign(gtk::Align::End);
        progress_box.set_visible(false);
        progress_box.set_can_target(false);

        let content_overlay = gtk::Overlay::new();
        content_overlay.set_child(Some(&nav_view));
        content_overlay.add_overlay(&progress_box);

        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_child(Some(&content_overlay));
        self.set_content(Some(&toast_overlay));

        imp.store.set(store).ok();
        imp.stack.set(stack).ok();
        imp.toast_overlay.set(toast_overlay).ok();
        imp.nav_view.set(nav_view).ok();
        imp.settings_page.set(settings.page).ok();
        imp.log_view.set(settings.log_view).ok();
        imp.refresh_button.set(settings.refresh_button).ok();
        imp.refresh_stack.set(settings.refresh_stack).ok();
        imp.refresh_spinner.set(settings.refresh_spinner).ok();
        imp.progress_box.set(progress_box).ok();
        imp.progress_label.set(progress_label).ok();
        imp.progress_spinner.set(progress_spinner).ok();

        self.setup_actions();

        // `/` or Ctrl+F focuses search; Ctrl+R refreshes the library. Neither
        // fires while a text field already has focus (typing "/" should type
        // a slash, not jump focus), and grid items no longer grab focus on
        // startup the way search used to, so nothing is focused by default.
        let key_controller = gtk::EventControllerKey::new();
        let window_weak = self.downgrade();
        let search_entry_weak = search_entry.downgrade();
        key_controller.connect_key_pressed(move |_, keyval, _keycode, state| {
            let Some(window) = window_weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let ctrl = state.intersects(gtk::gdk::ModifierType::CONTROL_MASK);

            if ctrl && (keyval == gtk::gdk::Key::r || keyval == gtk::gdk::Key::R) {
                window.refresh_all_metadata();
                return glib::Propagation::Stop;
            }

            let wants_search = (keyval == gtk::gdk::Key::slash && !ctrl)
                || (ctrl && (keyval == gtk::gdk::Key::f || keyval == gtk::gdk::Key::F));
            if !wants_search {
                return glib::Propagation::Proceed;
            }
            if let Some(focus) = gtk::prelude::GtkWindowExt::focus(&window)
                && focus.is::<gtk::Editable>()
            {
                return glib::Propagation::Proceed;
            }
            let Some(search_entry) = search_entry_weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            search_entry.grab_focus();
            glib::Propagation::Stop
        });
        self.add_controller(key_controller);
    }

    fn setup_actions(&self) {
        let add_book_action = gio::SimpleAction::new("add-book", None);
        let window_weak = self.downgrade();
        add_book_action.connect_activate(move |_, _| {
            if let Some(window) = window_weak.upgrade() {
                window.pick_and_add_book();
            }
        });
        self.add_action(&add_book_action);

        let show_settings_action = gio::SimpleAction::new("show-settings", None);
        let window_weak = self.downgrade();
        show_settings_action.connect_activate(move |_, _| {
            if let Some(window) = window_weak.upgrade() {
                window.show_settings();
            }
        });
        self.add_action(&show_settings_action);

        let edit_action = gio::SimpleAction::new("edit-book", Some(glib::VariantTy::STRING));
        let window_weak = self.downgrade();
        edit_action.connect_activate(move |_, parameter| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Some(id) = parameter
                .and_then(glib::Variant::str)
                .and_then(|s| Uuid::parse_str(s).ok())
            else {
                return;
            };
            window.edit_book(id);
        });
        self.add_action(&edit_action);

        let remove_action = gio::SimpleAction::new("remove-book", Some(glib::VariantTy::STRING));
        let window_weak = self.downgrade();
        remove_action.connect_activate(move |_, parameter| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Some(id) = parameter
                .and_then(glib::Variant::str)
                .and_then(|s| Uuid::parse_str(s).ok())
            else {
                return;
            };
            window.remove_book(id);
        });
        self.add_action(&remove_action);
    }

    fn show_settings(&self) {
        let imp = self.imp();
        if let Some(log_view) = imp.log_view.get() {
            log_view.buffer().set_text(&library::log::read());
        }
        if let (Some(nav_view), Some(settings_page)) = (imp.nav_view.get(), imp.settings_page.get())
        {
            nav_view.push(settings_page);
        }
    }

    fn find_book(&self, id: Uuid) -> Option<(u32, BookObject)> {
        let store = self.imp().store.get()?;
        for i in 0..store.n_items() {
            let book_object = store.item(i).and_downcast::<BookObject>()?;
            if book_object.book().id == id {
                return Some((i, book_object));
            }
        }
        None
    }

    fn edit_book(&self, id: Uuid) {
        if let Some((_, book_object)) = self.find_book(id) {
            super::edit_dialog::present(self, book_object.book());
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_book_metadata(
        &self,
        id: Uuid,
        title: String,
        author: Option<String>,
        series: Option<String>,
        series_index: Option<f64>,
        write_back: WriteBackMode,
    ) {
        let Some((index, book_object)) = self.find_book(id) else {
            return;
        };
        let Some(db) = self.imp().db.get() else {
            return;
        };

        let mut book = book_object.book();
        book.title = title;
        book.author = author;
        book.series = series;
        book.series_index = series_index;

        match write_back {
            WriteBackMode::None => {}
            WriteBackMode::Copy => {
                let update = library::epub_writer::MetadataUpdate {
                    title: &book.title,
                    author: book.author.as_deref(),
                    series: book.series.as_deref(),
                    series_index: book.series_index,
                };
                match library::epub_writer::write_copy_with_metadata(&book.path, &update) {
                    Ok(new_path) => {
                        let name = new_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        self.show_toast(&format!("Saved a copy as \u{201c}{name}\u{201d}"));
                        library::log::log(&format!(
                            "Edited \u{201c}{}\u{201d} \u{2014} saved a copy as {name}",
                            book.title
                        ));
                        book.path = new_path;
                    }
                    Err(err) => {
                        self.show_toast(&format!("Couldn't write updated EPUB: {err}"));
                        library::log::log(&format!(
                            "Couldn't write updated copy of \u{201c}{}\u{201d}: {err}",
                            book.title
                        ));
                    }
                }
            }
            WriteBackMode::InPlace => {
                let update = library::epub_writer::MetadataUpdate {
                    title: &book.title,
                    author: book.author.as_deref(),
                    series: book.series.as_deref(),
                    series_index: book.series_index,
                };
                match library::epub_writer::write_metadata_in_place(&book.path, &update) {
                    Ok(()) => library::log::log(&format!(
                        "Edited \u{201c}{}\u{201d} \u{2014} overwrote the EPUB file in place",
                        book.title
                    )),
                    Err(err) => {
                        self.show_toast(&format!("Couldn't write updated EPUB: {err}"));
                        library::log::log(&format!(
                            "Couldn't overwrite \u{201c}{}\u{201d}: {err}",
                            book.title
                        ));
                    }
                }
            }
        }

        if let Err(err) = library::db::insert_book(&db.borrow(), &book) {
            self.show_toast(&format!("Couldn't save changes: {err}"));
            return;
        }
        if write_back == WriteBackMode::None {
            library::log::log(&format!("Edited \u{201c}{}\u{201d}", book.title));
        }

        book_object.set_book(book);
        if let Some(store) = self.imp().store.get() {
            store.items_changed(index, 1, 1);
        }
    }

    fn remove_book(&self, id: Uuid) {
        let Some((index, book_object)) = self.find_book(id) else {
            return;
        };
        let Some(db) = self.imp().db.get() else {
            return;
        };

        if let Err(err) = library::db::delete_book(&db.borrow(), id) {
            self.show_toast(&format!("Couldn't remove book: {err}"));
            return;
        }

        if let Some(store) = self.imp().store.get() {
            store.remove(index);
        }
        self.update_empty_state();
        library::log::log(&format!(
            "Removed \u{201c}{}\u{201d}",
            book_object.book().title
        ));
        self.show_toast(&format!(
            "Removed \u{201c}{}\u{201d}",
            book_object.book().title
        ));
    }

    fn pick_and_add_book(&self) {
        let filter = gtk::FileFilter::new();
        filter.add_suffix("epub");
        filter.set_name(Some("EPUB books"));
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);

        let dialog = gtk::FileDialog::builder()
            .title("Add Book")
            .modal(true)
            .filters(&filters)
            .build();

        let window_weak = self.downgrade();
        dialog.open(Some(self), gio::Cancellable::NONE, move |result| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Ok(file) = result else {
                return;
            };
            let Some(path) = file.path() else {
                return;
            };
            window.add_book_from_path(&path);
        });
    }

    fn add_book_from_path(&self, path: &std::path::Path) {
        let imp = self.imp();
        let Some(db) = imp.db.get() else {
            return;
        };

        let path = library::scanner::canonicalize_path(path);

        let already_present = library::db::book_exists_at(&db.borrow(), &path).unwrap_or(false);
        if already_present {
            self.show_toast("That book is already in your library.");
            return;
        }

        match library::scanner::scan_epub(&path) {
            Ok(book) => {
                if let Err(err) = library::db::insert_book(&db.borrow(), &book) {
                    self.show_toast(&format!("Couldn't save book: {err}"));
                    return;
                }
                library::log::log(&format!("Added \u{201c}{}\u{201d}", book.title));
                let store = imp.store.get().expect("store built in constructed()");
                store.append(&BookObject::new(book));
                self.update_empty_state();
            }
            Err(err) => {
                self.show_toast(&format!("Couldn't read EPUB: {err}"));
            }
        }
    }

    /// Scans every folder configured in Preferences for EPUB files not yet
    /// in the library, and adds them.
    pub fn scan_library_folders(&self) {
        let imp = self.imp();
        let Some(db) = imp.db.get() else {
            return;
        };
        let Some(store) = imp.store.get() else {
            return;
        };

        let mut added = 0usize;
        for folder in library::settings::list_folders() {
            for path in library::scanner::find_epubs(&folder) {
                let path = library::scanner::canonicalize_path(&path);
                let already_present =
                    library::db::book_exists_at(&db.borrow(), &path).unwrap_or(true);
                if already_present {
                    continue;
                }

                match library::scanner::scan_epub(&path) {
                    Ok(book) => {
                        if library::db::insert_book(&db.borrow(), &book).is_err() {
                            continue;
                        }
                        library::log::log(&format!(
                            "Added \u{201c}{}\u{201d} from {}",
                            book.title,
                            folder.display()
                        ));
                        store.append(&BookObject::new(book));
                        added += 1;
                    }
                    Err(err) => {
                        library::log::log(&format!("Couldn't scan {}: {err}", path.display()));
                    }
                }
            }
        }

        if added > 0 {
            self.update_empty_state();
            let noun = if added == 1 { "book" } else { "books" };
            let message = format!("Added {added} {noun} from your library folders");
            library::log::log(&message);
            self.show_toast(&message);
        }
    }

    /// Re-reads title, author, and cover art for every book in the library
    /// from its EPUB file (keeping the library's own data — series, reading
    /// progress — untouched), removes entries whose file no longer exists,
    /// and collapses duplicate entries that point at the same on-disk file
    /// (comparing canonicalized paths, so a symlink or a `..` segment can't
    /// hide a duplicate).
    ///
    /// The (fast) removal/dedup pass runs synchronously; the (potentially
    /// slow, one-EPUB-at-a-time) re-scan pass runs one book per main-loop
    /// idle tick, so the UI stays responsive and repaints/input keep working
    /// throughout — this is what the corner progress notification tracks.
    pub fn refresh_all_metadata(&self) {
        enum RemovalReason {
            Missing(std::path::PathBuf),
            Duplicate(std::path::PathBuf),
        }

        let imp = self.imp();
        if imp.refreshing.get() {
            return;
        }
        let Some(db) = imp.db.get() else {
            return;
        };
        let Some(store) = imp.store.get() else {
            return;
        };

        // Pass 1 (synchronous — just path stats and hashmap lookups, cheap
        // even for a large library): find books whose file is gone, and
        // books that share a canonical path with an earlier (kept) entry.
        let mut best_by_path: std::collections::HashMap<std::path::PathBuf, (u32, i64)> =
            std::collections::HashMap::new();
        let mut to_remove: Vec<(u32, String, RemovalReason)> = Vec::new();

        for i in 0..store.n_items() {
            let Some(book_object) = store.item(i).and_downcast::<BookObject>() else {
                continue;
            };
            let book = book_object.book();
            let canonical = library::scanner::canonicalize_path(&book.path);

            if !canonical.exists() {
                to_remove.push((i, book.title, RemovalReason::Missing(book.path)));
                continue;
            }

            match best_by_path.get(&canonical) {
                None => {
                    best_by_path.insert(canonical, (i, book.added_at));
                }
                Some(&(kept_index, kept_added_at)) if book.added_at < kept_added_at => {
                    if let Some(kept_object) = store.item(kept_index).and_downcast::<BookObject>() {
                        to_remove.push((
                            kept_index,
                            kept_object.book().title,
                            RemovalReason::Duplicate(canonical.clone()),
                        ));
                    }
                    best_by_path.insert(canonical, (i, book.added_at));
                }
                Some(_) => {
                    to_remove.push((i, book.title, RemovalReason::Duplicate(canonical)));
                }
            }
        }

        to_remove.sort_by(|a, b| b.0.cmp(&a.0));
        to_remove.dedup_by_key(|(index, ..)| *index);

        let mut removed = 0usize;
        for (index, title, reason) in &to_remove {
            let Some(book_object) = store.item(*index).and_downcast::<BookObject>() else {
                continue;
            };
            if library::db::delete_book(&db.borrow(), book_object.book().id).is_err() {
                continue;
            }
            store.remove(*index);
            removed += 1;
            let message = match reason {
                RemovalReason::Missing(path) => format!(
                    "Removed \u{201c}{title}\u{201d} \u{2014} file no longer exists ({})",
                    path.display()
                ),
                RemovalReason::Duplicate(path) => format!(
                    "Removed duplicate \u{201c}{title}\u{201d} \u{2014} same file as another entry ({})",
                    path.display()
                ),
            };
            library::log::log(&message);
        }
        if removed > 0 {
            self.update_empty_state();
        }

        // Pass 2 (potentially slow — one EPUB open + XML parse + cover
        // decode per book): queue survivors by id (not index, since the
        // user can still add/remove books while this runs in the
        // background) and process one per idle tick.
        let ids: Vec<Uuid> = (0..store.n_items())
            .filter_map(|i| store.item(i).and_downcast::<BookObject>())
            .map(|object| object.book().id)
            .collect();
        let total = ids.len();

        if total == 0 {
            self.finish_refresh(RefreshTally {
                removed,
                ..Default::default()
            });
            return;
        }

        imp.refreshing.set(true);
        self.set_refresh_active(true);
        self.update_progress_text(0, total);

        let state = Rc::new(RefCell::new(RefreshQueueState {
            queue: ids.into(),
            total,
            done: 0,
            tally: RefreshTally {
                removed,
                ..Default::default()
            },
        }));

        let window_weak = self.downgrade();
        glib::idle_add_local(move || {
            let Some(window) = window_weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            window.process_refresh_tick(&state)
        });
    }

    fn process_refresh_tick(&self, state: &Rc<RefCell<RefreshQueueState>>) -> glib::ControlFlow {
        let Some(id) = state.borrow_mut().queue.pop_front() else {
            self.finish_refresh(state.borrow().tally.clone());
            return glib::ControlFlow::Break;
        };

        let Some(db) = self.imp().db.get() else {
            return glib::ControlFlow::Break;
        };

        // If the user removed this book while the refresh was in flight,
        // there's nothing left to do for it — just move on.
        if let Some((_, book_object)) = self.find_book(id) {
            let old_book = book_object.book();
            let canonical = library::scanner::canonicalize_path(&old_book.path);

            match library::scanner::scan_epub(&canonical) {
                Ok(scanned) => {
                    let mut updated = old_book.clone();
                    updated.path = canonical;
                    updated.title = scanned.title;
                    updated.author = scanned.author;

                    if covers_differ(
                        old_book.cover_path.as_deref(),
                        scanned.cover_path.as_deref(),
                    ) {
                        if let Some(old_cover) = &old_book.cover_path {
                            std::fs::remove_file(old_cover).ok();
                        }
                        updated.cover_path = scanned.cover_path;
                        state.borrow_mut().tally.covers_updated += 1;
                        library::log::log(&format!(
                            "Updated cover for \u{201c}{}\u{201d}",
                            updated.title
                        ));
                    } else {
                        if let Some(new_cover) = &scanned.cover_path {
                            std::fs::remove_file(new_cover).ok();
                        }
                        updated.cover_path = old_book.cover_path.clone();
                    }

                    if library::db::insert_book(&db.borrow(), &updated).is_ok() {
                        book_object.set_book(updated);
                        state.borrow_mut().tally.refreshed += 1;
                    } else {
                        state.borrow_mut().tally.failed += 1;
                    }
                }
                Err(err) => {
                    library::log::log(&format!(
                        "Couldn't refresh \u{201c}{}\u{201d}: {err}",
                        old_book.title
                    ));
                    state.borrow_mut().tally.failed += 1;
                }
            }
        }

        let mut s = state.borrow_mut();
        s.done += 1;
        let (done, total) = (s.done, s.total);
        drop(s);
        self.update_progress_text(done, total);

        glib::ControlFlow::Continue
    }

    fn finish_refresh(&self, tally: RefreshTally) {
        let imp = self.imp();
        imp.refreshing.set(false);
        self.set_refresh_active(false);

        if tally.refreshed > 0 {
            if let Some(store) = imp.store.get() {
                let count = store.n_items();
                store.items_changed(0, count, count);
            }
        }

        let mut parts = Vec::new();
        if tally.refreshed > 0 {
            parts.push(format!("refreshed {}", tally.refreshed));
        }
        if tally.covers_updated > 0 {
            parts.push(format!("{} cover(s) updated", tally.covers_updated));
        }
        if tally.removed > 0 {
            parts.push(format!("removed {}", tally.removed));
        }
        if tally.failed > 0 {
            parts.push(format!("{} failed", tally.failed));
        }
        let message = if parts.is_empty() {
            "Library is already up to date".to_string()
        } else {
            format!("Refresh complete: {}", parts.join(", "))
        };
        library::log::log(&message);
        self.show_toast(&message);
    }

    fn set_refresh_active(&self, active: bool) {
        let imp = self.imp();
        if let Some(button) = imp.refresh_button.get() {
            button.set_sensitive(!active);
        }
        if let Some(stack) = imp.refresh_stack.get() {
            stack.set_visible_child_name(if active { "spinner" } else { "icon" });
        }
        if let Some(spinner) = imp.refresh_spinner.get() {
            spinner.set_spinning(active);
        }
        if let Some(spinner) = imp.progress_spinner.get() {
            spinner.set_spinning(active);
        }
        if let Some(progress_box) = imp.progress_box.get() {
            progress_box.set_visible(active);
        }
    }

    fn update_progress_text(&self, done: usize, total: usize) {
        if let Some(label) = self.imp().progress_label.get() {
            label.set_text(&format!("Refreshing library\u{2026} {done}/{total}"));
        }
    }

    pub fn update_empty_state(&self) {
        let imp = self.imp();
        let Some(store) = imp.store.get() else {
            return;
        };
        let Some(stack) = imp.stack.get() else {
            return;
        };
        let page = if store.n_items() == 0 {
            "empty"
        } else {
            "library"
        };
        stack.set_visible_child_name(page);
    }

    fn show_reader_stub(&self, book: &library::Book) {
        self.show_toast(&format!("Reader not implemented yet — {}", book.title));
    }

    fn show_toast(&self, message: &str) {
        if let Some(overlay) = self.imp().toast_overlay.get() {
            overlay.add_toast(adw::Toast::new(message));
        }
    }
}

/// Compares two cached cover images by content, so a re-scan that produces
/// byte-identical art isn't treated as a change.
fn covers_differ(a: Option<&std::path::Path>, b: Option<&std::path::Path>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(a), Some(b)) => std::fs::read(a).ok() != std::fs::read(b).ok(),
        _ => true,
    }
}

/// The queue driving [`GnosisWindow::process_refresh_tick`]: one book id per
/// main-loop idle tick, so a big library doesn't freeze the UI while
/// refreshing.
struct RefreshQueueState {
    queue: std::collections::VecDeque<Uuid>,
    total: usize,
    done: usize,
    tally: RefreshTally,
}

#[derive(Clone, Default)]
struct RefreshTally {
    refreshed: usize,
    covers_updated: usize,
    removed: usize,
    failed: usize,
}
