use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::ObjectSubclassIsExt;
use gtk::{gio, glib};
use rusqlite::Connection;
use webkit6::prelude::*;

use uuid::Uuid;

use crate::application::GnosisApplication;
use crate::library;

use super::book_card;
use super::book_object::BookObject;
use super::collection_card;
use super::collection_object::{CollectionKind, CollectionObject};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MaintenanceOp {
    Refresh,
    RescanSeries,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteBackMode {
    None,
    Copy,
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
        pub rescan_button: OnceCell<gtk::Button>,
        pub rescan_stack: OnceCell<gtk::Stack>,
        pub rescan_spinner: OnceCell<gtk::Spinner>,
        pub progress_box: OnceCell<gtk::Box>,
        pub progress_label: OnceCell<gtk::Label>,
        pub progress_spinner: OnceCell<gtk::Spinner>,
        pub refreshing: Cell<bool>,
        pub reader: OnceCell<crate::ui::reader::ReaderWidgets>,
        pub reader_book_id: RefCell<Option<uuid::Uuid>>,
        pub search_query: Rc<RefCell<String>>,
        pub sort_key: Rc<RefCell<String>>,
        pub db: OnceCell<Rc<RefCell<Connection>>>,
        pub authors_store: OnceCell<gtk::gio::ListStore>,
        pub series_store: OnceCell<gtk::gio::ListStore>,
        pub collection_detail: OnceCell<crate::ui::collection_detail::CollectionDetailWidgets>,
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

    pub fn set_database(&self, db: Rc<RefCell<Connection>>) {
        let books = match library::db::list_books(&db.borrow()) {
            Ok(books) => books,
            Err(err) => {
                eprintln!("failed to load library: {err}");
                Vec::new()
            }
        };

        let imp = self.imp();
        imp.db.set(db).ok();
        let store = imp.store.get().expect("store built in constructed()");
        let book_objects: Vec<BookObject> = books.into_iter().map(BookObject::new).collect();
        store.splice(0, 0, &book_objects);

        self.update_empty_state();
    }

    fn setup_ui(&self) {
        let imp = self.imp();

        self.set_title(Some("Gnosis"));
        self.set_default_size(900, 640);

        let store = gio::ListStore::new::<BookObject>();

        let search_query = imp.search_query.clone();
        let search_filter = gtk::CustomFilter::new(move |obj| {
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

        let every_filter = gtk::EveryFilter::new();
        every_filter.append(search_filter.clone());
        let library_view_stack = gtk::Stack::new();
        let sidebar_widget = super::library_sidebar::build(&every_filter, &library_view_stack);

        let filter_model = gtk::FilterListModel::new(Some(store.clone()), Some(every_filter));

        let initial_sort_key = library::library_prefs::load().sort_key;
        *imp.sort_key.borrow_mut() = initial_sort_key.clone();
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
                "series" => {
                    let a_has_series = a.series.is_some();
                    let b_has_series = b.series.is_some();
                    a_has_series
                        .cmp(&b_has_series)
                        .reverse()
                        .then_with(|| {
                            a.series
                                .as_deref()
                                .unwrap_or_default()
                                .to_lowercase()
                                .cmp(&b.series.as_deref().unwrap_or_default().to_lowercase())
                        })
                        .then_with(|| {
                            a.series_index
                                .partial_cmp(&b.series_index)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
                }
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
            window.open_reader(&book_object.book());
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

        let authors_store = gio::ListStore::new::<CollectionObject>();
        let series_store = gio::ListStore::new::<CollectionObject>();

        let authors_grid = gtk::GridView::new(
            Some(gtk::NoSelection::new(Some(authors_store.clone()))),
            Some(collection_card::factory()),
        );
        authors_grid.set_single_click_activate(false);
        authors_grid.set_min_columns(2);
        authors_grid.set_max_columns(64);
        let window_weak = self.downgrade();
        authors_grid.connect_activate(move |grid_view, position| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Some(model) = grid_view.model() else {
                return;
            };
            let Some(collection_object) = model.item(position).and_downcast::<CollectionObject>()
            else {
                return;
            };
            let data = collection_object.data();
            window.open_collection(data.kind, &data.name);
        });
        let authors_scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&authors_grid)
            .build();

        let series_grid = gtk::GridView::new(
            Some(gtk::NoSelection::new(Some(series_store.clone()))),
            Some(collection_card::factory()),
        );
        series_grid.set_single_click_activate(false);
        series_grid.set_min_columns(2);
        series_grid.set_max_columns(64);
        let window_weak = self.downgrade();
        series_grid.connect_activate(move |grid_view, position| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Some(model) = grid_view.model() else {
                return;
            };
            let Some(collection_object) = model.item(position).and_downcast::<CollectionObject>()
            else {
                return;
            };
            let data = collection_object.data();
            window.open_collection(data.kind, &data.name);
        });
        let series_scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&series_grid)
            .build();

        library_view_stack.add_named(&stack, Some("books"));
        library_view_stack.add_named(&authors_scrolled, Some("authors"));
        library_view_stack.add_named(&series_scrolled, Some("series"));
        library_view_stack.set_visible_child_name("books");

        let window_weak = self.downgrade();
        store.connect_items_changed(move |_, _, _, _| {
            if let Some(window) = window_weak.upgrade() {
                window.rebuild_collections();
            }
        });

        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text("Search your library")
            .build();
        let query_slot = imp.search_query.clone();
        search_entry.connect_search_changed(move |entry| {
            *query_slot.borrow_mut() = entry.text().to_string();
            search_filter.changed(gtk::FilterChange::Different);
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
        sort_menu.append(Some("Series"), Some("win.sort-by('series')"));
        let sort_button = gtk::MenuButton::builder()
            .icon_name("view-sort-descending-symbolic")
            .tooltip_text("Sort")
            .menu_model(&sort_menu)
            .build();

        let sort_action = gio::SimpleAction::new_stateful(
            "sort-by",
            Some(glib::VariantTy::STRING),
            &initial_sort_key.to_variant(),
        );
        let sort_key_slot = imp.sort_key.clone();
        sort_action.connect_activate(move |action, parameter| {
            let Some(key) = parameter.and_then(glib::Variant::str) else {
                return;
            };
            *sort_key_slot.borrow_mut() = key.to_string();
            action.set_state(&key.to_variant());
            sorter.changed(gtk::SorterChange::Different);
            library::library_prefs::save(&library::library_prefs::LibraryPrefs {
                sort_key: key.to_string(),
            });
        });
        self.add_action(&sort_action);

        let split_view = adw::OverlaySplitView::builder()
            .sidebar(&sidebar_widget)
            .content(&library_view_stack)
            .build();

        let sidebar_toggle = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-symbolic")
            .tooltip_text("Toggle Sidebar")
            .active(true)
            .build();
        split_view
            .bind_property("show-sidebar", &sidebar_toggle, "active")
            .bidirectional()
            .sync_create()
            .build();

        let header_bar = adw::HeaderBar::new();
        header_bar.set_title_widget(Some(&adw::WindowTitle::new("Gnosis", "")));
        header_bar.pack_start(&sidebar_toggle);
        header_bar.pack_start(&search_entry);
        header_bar.pack_end(&add_button);
        header_bar.pack_end(&settings_button);
        header_bar.pack_end(&sort_button);

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header_bar);
        toolbar_view.set_content(Some(&split_view));
        let library_page = adw::NavigationPage::with_tag(&toolbar_view, "Gnosis", "library");

        let settings = super::preferences::build_page(self);
        let reader = super::reader::build(self);
        let collection_detail = super::collection_detail::build(&store, self);

        let nav_view = adw::NavigationView::new();
        nav_view.push(&library_page);

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
        imp.rescan_button.set(settings.rescan_button).ok();
        imp.rescan_stack.set(settings.rescan_stack).ok();
        imp.rescan_spinner.set(settings.rescan_spinner).ok();
        imp.progress_box.set(progress_box).ok();
        imp.progress_label.set(progress_label).ok();
        imp.progress_spinner.set(progress_spinner).ok();
        imp.reader.set(reader).ok();
        imp.authors_store.set(authors_store).ok();
        imp.series_store.set(series_store).ok();
        imp.collection_detail.set(collection_detail).ok();

        self.setup_actions();

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

        let mark_read_action = gio::SimpleAction::new("mark-book-read", Some(glib::VariantTy::STRING));
        let window_weak = self.downgrade();
        mark_read_action.connect_activate(move |_, parameter| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Some(id) = parameter
                .and_then(glib::Variant::str)
                .and_then(|s| Uuid::parse_str(s).ok())
            else {
                return;
            };
            window.set_book_read(id, true);
        });
        self.add_action(&mark_read_action);

        let mark_unread_action = gio::SimpleAction::new("mark-book-unread", Some(glib::VariantTy::STRING));
        let window_weak = self.downgrade();
        mark_unread_action.connect_activate(move |_, parameter| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Some(id) = parameter
                .and_then(glib::Variant::str)
                .and_then(|s| Uuid::parse_str(s).ok())
            else {
                return;
            };
            window.set_book_read(id, false);
        });
        self.add_action(&mark_unread_action);

        let collection_target_type = glib::VariantTy::new("(ss)").expect("valid variant type");

        let open_collection_action =
            gio::SimpleAction::new("open-collection", Some(collection_target_type));
        let window_weak = self.downgrade();
        open_collection_action.connect_activate(move |_, parameter| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Some((kind, name)) = collection_kind_and_name(parameter) else {
                return;
            };
            window.open_collection(kind, &name);
        });
        self.add_action(&open_collection_action);

        let set_cover_action =
            gio::SimpleAction::new("set-collection-cover", Some(collection_target_type));
        let window_weak = self.downgrade();
        set_cover_action.connect_activate(move |_, parameter| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Some((kind, name)) = collection_kind_and_name(parameter) else {
                return;
            };
            window.pick_collection_cover(kind, name);
        });
        self.add_action(&set_cover_action);

        let remove_cover_action =
            gio::SimpleAction::new("remove-collection-cover", Some(collection_target_type));
        let window_weak = self.downgrade();
        remove_cover_action.connect_activate(move |_, parameter| {
            let Some(window) = window_weak.upgrade() else {
                return;
            };
            let Some((kind, name)) = collection_kind_and_name(parameter) else {
                return;
            };
            window.remove_collection_cover(kind, &name);
        });
        self.add_action(&remove_cover_action);
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

    fn set_book_read(&self, id: Uuid, read: bool) {
        let Some((index, book_object)) = self.find_book(id) else {
            return;
        };
        let Some(db) = self.imp().db.get() else {
            return;
        };

        let mut book = book_object.book();
        if read {
            book.progress = 1.0;
        } else {
            book.progress = 0.0;
            book.locator = None;
        }

        if library::db::update_reader_position(
            &db.borrow(),
            id,
            book.locator.as_deref(),
            book.progress,
        )
        .is_err()
        {
            return;
        }
        library::log::log(&format!(
            "Marked \u{201c}{}\u{201d} as {}",
            book.title,
            if read { "read" } else { "unread" }
        ));

        if let Some(store) = self.imp().store.get() {
            store.splice(index, 1, &[BookObject::new(book)]);
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

        if let Some(store) = self.imp().store.get() {
            store.splice(index, 1, &[BookObject::new(book)]);
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

        let _ = std::fs::remove_dir_all(library::db::book_cache_dir().join(id.to_string()));
        let _ = std::fs::remove_file(
            library::db::book_cache_dir().join(format!("{id}.json")),
        );

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

    pub fn scan_library_folders(&self) {
        let folders = library::settings::list_folders();
        if folders.is_empty() {
            return;
        }
        tracing::info!(folders = folders.len(), "starting library folder scan");

        let state = Rc::new(RefCell::new(FolderScanState {
            pending_dirs: folders.into(),
            pending_files: std::collections::VecDeque::new(),
            added: 0,
            started: std::time::Instant::now(),
            dirs_visited: 0,
        }));

        let window_weak = self.downgrade();
        glib::idle_add_local(move || {
            let Some(window) = window_weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            window.process_folder_scan_tick(&state)
        });
    }

    fn process_folder_scan_tick(&self, state: &Rc<RefCell<FolderScanState>>) -> glib::ControlFlow {
        let next_file = state.borrow_mut().pending_files.pop_front();
        if let Some(path) = next_file {
            let (Some(db), Some(store)) = (self.imp().db.get(), self.imp().store.get()) else {
                return glib::ControlFlow::Break;
            };

            let path = library::scanner::canonicalize_path(&path);
            let already_present =
                library::db::book_exists_at(&db.borrow(), &path).unwrap_or(true);
            if !already_present {
                match library::scanner::scan_epub(&path) {
                    Ok(book) => {
                        if library::db::insert_book(&db.borrow(), &book).is_ok() {
                            library::log::log(&format!("Added \u{201c}{}\u{201d}", book.title));
                            store.append(&BookObject::new(book));
                            state.borrow_mut().added += 1;
                        }
                    }
                    Err(err) => {
                        library::log::log(&format!("Couldn't scan {}: {err}", path.display()));
                    }
                }
            }
            return glib::ControlFlow::Continue;
        }

        let next_dir = state.borrow_mut().pending_dirs.pop_front();
        let Some(dir) = next_dir else {
            let s = state.borrow();
            tracing::info!(
                added = s.added,
                dirs_visited = s.dirs_visited,
                elapsed = ?s.started.elapsed(),
                "library folder scan finished"
            );
            let added = s.added;
            drop(s);
            if added > 0 {
                self.update_empty_state();
                let noun = if added == 1 { "book" } else { "books" };
                let message = format!("Added {added} {noun} from your library folders");
                library::log::log(&message);
                self.show_toast(&message);
            }
            return glib::ControlFlow::Break;
        };

        let (epubs, subdirs) = library::scanner::read_dir_epubs_and_subdirs(&dir);
        let mut s = state.borrow_mut();
        s.dirs_visited += 1;
        s.pending_files.extend(epubs);
        s.pending_dirs.extend(subdirs);
        glib::ControlFlow::Continue
    }

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
            let removed_id = book_object.book().id;
            if library::db::delete_book(&db.borrow(), removed_id).is_err() {
                continue;
            }
            let _ = std::fs::remove_dir_all(library::db::book_cache_dir().join(removed_id.to_string()));
            let _ = std::fs::remove_file(
                library::db::book_cache_dir().join(format!("{removed_id}.json")),
            );
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
        self.set_maintenance_active(MaintenanceOp::Refresh, true);
        self.update_progress_text("Refreshing library", 0, total);

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

        if let Some((index, book_object)) = self.find_book(id) {
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
                        if let Some(store) = self.imp().store.get() {
                            store.splice(index, 1, &[BookObject::new(updated)]);
                        }
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
        self.update_progress_text("Refreshing library", done, total);

        glib::ControlFlow::Continue
    }

    fn finish_refresh(&self, tally: RefreshTally) {
        let imp = self.imp();
        imp.refreshing.set(false);
        self.set_maintenance_active(MaintenanceOp::Refresh, false);

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

    pub fn rescan_series(&self) {
        let imp = self.imp();
        if imp.refreshing.get() {
            return;
        }
        let Some(store) = imp.store.get() else {
            return;
        };

        let ids: Vec<Uuid> = (0..store.n_items())
            .filter_map(|i| store.item(i).and_downcast::<BookObject>())
            .map(|object| object.book().id)
            .collect();
        let total = ids.len();

        if total == 0 {
            self.finish_series_rescan(SeriesRescanTally::default());
            return;
        }

        imp.refreshing.set(true);
        self.set_maintenance_active(MaintenanceOp::RescanSeries, true);
        self.update_progress_text("Rescanning series", 0, total);

        let state = Rc::new(RefCell::new(SeriesRescanState {
            queue: ids.into(),
            total,
            done: 0,
            tally: SeriesRescanTally::default(),
        }));

        let window_weak = self.downgrade();
        glib::idle_add_local(move || {
            let Some(window) = window_weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            window.process_series_rescan_tick(&state)
        });
    }

    fn process_series_rescan_tick(
        &self,
        state: &Rc<RefCell<SeriesRescanState>>,
    ) -> glib::ControlFlow {
        let Some(id) = state.borrow_mut().queue.pop_front() else {
            self.finish_series_rescan(state.borrow().tally.clone());
            return glib::ControlFlow::Break;
        };

        let Some(db) = self.imp().db.get() else {
            return glib::ControlFlow::Break;
        };

        if let Some((index, book_object)) = self.find_book(id) {
            let old_book = book_object.book();
            let canonical = library::scanner::canonicalize_path(&old_book.path);

            match library::scanner::scan_epub(&canonical) {
                Ok(scanned) => {
                    if scanned.series != old_book.series
                        || scanned.series_index != old_book.series_index
                    {
                        let mut updated = old_book.clone();
                        updated.series = scanned.series;
                        updated.series_index = scanned.series_index;

                        if library::db::insert_book(&db.borrow(), &updated).is_ok() {
                            if let Some(store) = self.imp().store.get() {
                                store.splice(index, 1, &[BookObject::new(updated)]);
                            }
                            state.borrow_mut().tally.updated += 1;
                        } else {
                            state.borrow_mut().tally.failed += 1;
                        }
                    }
                }
                Err(err) => {
                    library::log::log(&format!(
                        "Couldn't rescan series for \u{201c}{}\u{201d}: {err}",
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
        self.update_progress_text("Rescanning series", done, total);

        glib::ControlFlow::Continue
    }

    fn finish_series_rescan(&self, tally: SeriesRescanTally) {
        let imp = self.imp();
        imp.refreshing.set(false);
        self.set_maintenance_active(MaintenanceOp::RescanSeries, false);

        let mut parts = Vec::new();
        if tally.updated > 0 {
            parts.push(format!("updated {}", tally.updated));
        }
        if tally.failed > 0 {
            parts.push(format!("{} failed", tally.failed));
        }
        let message = if parts.is_empty() {
            "No series changes found".to_string()
        } else {
            format!("Series rescan complete: {}", parts.join(", "))
        };
        library::log::log(&message);
        self.show_toast(&message);
    }

    fn set_maintenance_active(&self, op: MaintenanceOp, active: bool) {
        let imp = self.imp();
        if let Some(button) = imp.refresh_button.get() {
            button.set_sensitive(!active);
        }
        if let Some(button) = imp.rescan_button.get() {
            button.set_sensitive(!active);
        }

        let refresh_on = active && op == MaintenanceOp::Refresh;
        let rescan_on = active && op == MaintenanceOp::RescanSeries;

        if let Some(stack) = imp.refresh_stack.get() {
            stack.set_visible_child_name(if refresh_on { "spinner" } else { "icon" });
        }
        if let Some(spinner) = imp.refresh_spinner.get() {
            spinner.set_spinning(refresh_on);
        }
        if let Some(stack) = imp.rescan_stack.get() {
            stack.set_visible_child_name(if rescan_on { "spinner" } else { "icon" });
        }
        if let Some(spinner) = imp.rescan_spinner.get() {
            spinner.set_spinning(rescan_on);
        }

        if let Some(spinner) = imp.progress_spinner.get() {
            spinner.set_spinning(active);
        }
        if let Some(progress_box) = imp.progress_box.get() {
            progress_box.set_visible(active);
        }
    }

    fn update_progress_text(&self, label: &str, done: usize, total: usize) {
        if let Some(widget) = self.imp().progress_label.get() {
            widget.set_text(&format!("{label}\u{2026} {done}/{total}"));
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

    pub(crate) fn open_reader(&self, book: &library::Book) {
        let imp = self.imp();
        let Some(reader) = imp.reader.get() else {
            return;
        };

        *reader.current_book_id.borrow_mut() = Some(book.id);
        *imp.reader_book_id.borrow_mut() = Some(book.id);
        reader.title_widget.set_title(&book.title);

        let cache_dir = library::db::book_cache_dir().join(book.id.to_string());
        if !cache_dir.exists()
            && let Err(err) = library::scanner::unpack_book(book.id, &book.path)
        {
            self.show_reader_error(&format!("Couldn't open book: {err}"));
            return;
        }

        let prefs = library::reader_prefs::load_for(book.id);
        (reader.apply_prefs)(&prefs);

        reader.spinner.show_soon();
        let script = super::reader::open_book_script(book.id, book.locator.as_deref(), &prefs);
        reader
            .web_view
            .evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| {});

        if let Some(nav_view) = imp.nav_view.get() {
            nav_view.push(&reader.page);
        }
    }

    pub fn save_reader_position(&self, cfi: Option<String>, fraction: f64) {
        let imp = self.imp();
        let Some(id) = *imp.reader_book_id.borrow() else {
            return;
        };
        let Some(db) = imp.db.get() else {
            return;
        };
        if library::db::update_reader_position(&db.borrow(), id, cfi.as_deref(), fraction).is_err()
        {
            return;
        }
        if let Some((index, book_object)) = self.find_book(id) {
            let mut book = book_object.book();
            book.locator = cfi;
            book.progress = fraction;
            if let Some(store) = self.imp().store.get() {
                store.splice(index, 1, &[BookObject::new(book)]);
            }
        }
    }

    pub fn show_reader_error(&self, message: &str) {
        self.show_toast(&format!("Couldn't open book: {message}"));
    }

    fn open_collection(&self, kind: CollectionKind, name: &str) {
        let imp = self.imp();
        let Some(detail) = imp.collection_detail.get() else {
            return;
        };
        let Some(db) = imp.db.get() else {
            return;
        };

        let cover_path = library::db::all_collection_covers(&db.borrow(), kind.as_str())
            .ok()
            .and_then(|covers| covers.get(name).cloned())
            .or_else(|| self.first_cover_for(kind, name));

        detail.configure(kind, name, cover_path.as_deref());

        if let Some(nav_view) = imp.nav_view.get() {
            nav_view.push(&detail.page);
        }
    }

    fn first_cover_for(&self, kind: CollectionKind, name: &str) -> Option<std::path::PathBuf> {
        let store = self.imp().store.get()?;
        for i in 0..store.n_items() {
            let Some(book_object) = store.item(i).and_downcast::<BookObject>() else {
                continue;
            };
            let book = book_object.book();
            let matches = match kind {
                CollectionKind::Author => book.author.as_deref() == Some(name),
                CollectionKind::Series => book.series.as_deref() == Some(name),
            };
            if matches && book.cover_path.is_some() {
                return book.cover_path;
            }
        }
        None
    }

    fn pick_collection_cover(&self, kind: CollectionKind, name: String) {
        let filter = gtk::FileFilter::new();
        filter.add_pixbuf_formats();
        filter.set_name(Some("Images"));
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);

        let dialog = gtk::FileDialog::builder()
            .title("Set Cover Image")
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
            window.apply_collection_cover(kind, &name, &path);
        });
    }

    fn apply_collection_cover(&self, kind: CollectionKind, name: &str, source: &std::path::Path) {
        let Some(db) = self.imp().db.get() else {
            return;
        };

        let dest_dir = library::db::collection_covers_dir();
        if std::fs::create_dir_all(&dest_dir).is_err() {
            self.show_toast("Couldn't save cover image");
            return;
        }
        let extension = source.extension().and_then(|e| e.to_str()).unwrap_or("img");
        let dest = dest_dir.join(format!("{}-{}.{extension}", kind.as_str(), Uuid::new_v4()));
        if std::fs::copy(source, &dest).is_err() {
            self.show_toast("Couldn't save cover image");
            return;
        }
        if library::db::set_collection_cover(&db.borrow(), kind.as_str(), name, &dest).is_err() {
            self.show_toast("Couldn't save cover image");
            return;
        }

        self.rebuild_collections();
        self.show_toast(&format!("Updated cover for \u{201c}{name}\u{201d}"));
    }

    fn remove_collection_cover(&self, kind: CollectionKind, name: &str) {
        let Some(db) = self.imp().db.get() else {
            return;
        };
        if library::db::remove_collection_cover(&db.borrow(), kind.as_str(), name).is_err() {
            return;
        }
        self.rebuild_collections();
        self.show_toast(&format!("Removed custom cover for \u{201c}{name}\u{201d}"));
    }

    fn rebuild_collections(&self) {
        let t0 = std::time::Instant::now();
        let imp = self.imp();
        let (Some(store), Some(db)) = (imp.store.get(), imp.db.get()) else {
            return;
        };

        let books: Vec<library::Book> = (0..store.n_items())
            .filter_map(|i| store.item(i).and_downcast::<BookObject>())
            .map(|object| object.book())
            .collect();

        for (kind, target_store) in [
            (CollectionKind::Author, imp.authors_store.get()),
            (CollectionKind::Series, imp.series_store.get()),
        ] {
            let Some(target_store) = target_store else {
                continue;
            };
            let custom_covers =
                library::db::all_collection_covers(&db.borrow(), kind.as_str()).unwrap_or_default();

            let objects: Vec<CollectionObject> =
                super::collection_object::group_books(&books, kind, &custom_covers)
                    .into_iter()
                    .map(CollectionObject::new)
                    .collect();
            let old_count = target_store.n_items();
            target_store.splice(0, old_count, &objects);
        }

        tracing::debug!(books = books.len(), elapsed = ?t0.elapsed(), "rebuilt author/series collections");
    }

    fn show_toast(&self, message: &str) {
        if let Some(overlay) = self.imp().toast_overlay.get() {
            overlay.add_toast(adw::Toast::new(message));
        }
    }
}

fn collection_kind_and_name(parameter: Option<&glib::Variant>) -> Option<(CollectionKind, String)> {
    let (kind, name) = parameter?.get::<(String, String)>()?;
    let kind = match kind.as_str() {
        "author" => CollectionKind::Author,
        "series" => CollectionKind::Series,
        _ => return None,
    };
    Some((kind, name))
}

fn covers_differ(a: Option<&std::path::Path>, b: Option<&std::path::Path>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(a), Some(b)) => std::fs::read(a).ok() != std::fs::read(b).ok(),
        _ => true,
    }
}

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

struct SeriesRescanState {
    queue: std::collections::VecDeque<Uuid>,
    total: usize,
    done: usize,
    tally: SeriesRescanTally,
}

#[derive(Clone, Default)]
struct SeriesRescanTally {
    updated: usize,
    failed: usize,
}

struct FolderScanState {
    pending_dirs: std::collections::VecDeque<std::path::PathBuf>,
    pending_files: std::collections::VecDeque<std::path::PathBuf>,
    added: usize,
    started: std::time::Instant,
    dirs_visited: usize,
}
