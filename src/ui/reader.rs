use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};
use serde::Deserialize;
use uuid::Uuid;
use webkit6::prelude::*;

use crate::library;
use crate::library::reader_prefs::ReaderPrefs;

use super::reader_scheme;
use super::window::GnosisWindow;

pub struct ReaderWidgets {
    pub page: adw::NavigationPage,
    pub web_view: webkit6::WebView,
    pub title_widget: adw::WindowTitle,
    pub toc_menu: gio::Menu,
    pub current_book: Rc<RefCell<Option<PathBuf>>>,
    pub current_book_id: Rc<RefCell<Option<Uuid>>>,
    pub apply_prefs: Rc<dyn Fn(&ReaderPrefs)>,
    pub spinner: SpinnerHandle,
}

#[derive(Clone)]
pub struct SpinnerHandle {
    spinner: gtk::Spinner,
    pending: Rc<Cell<Option<glib::SourceId>>>,
}

impl SpinnerHandle {
    fn new(spinner: gtk::Spinner) -> Self {
        Self {
            spinner,
            pending: Rc::new(Cell::new(None)),
        }
    }

    fn cancel_pending(&self) {
        if let Some(id) = self.pending.take() {
            id.remove();
        }
    }

    pub fn show_soon(&self) {
        self.cancel_pending();
        if self.spinner.is_visible() {
            return;
        }
        let spinner = self.spinner.clone();
        let pending = self.pending.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(150), move || {
            pending.set(None);
            spinner.set_visible(true);
            spinner.start();
        });
        self.pending.set(Some(id));
    }

    pub fn hide(&self) {
        self.cancel_pending();
        self.spinner.stop();
        self.spinner.set_visible(false);
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ReaderMessage {
    Loading,
    Ready {
        title: Option<String>,
        #[serde(default)]
        toc: Vec<TocEntry>,
    },
    Relocate {
        cfi: Option<String>,
        fraction: Option<f64>,
    },
    RsvpWpm {
        wpm: u32,
    },
    Error {
        message: String,
    },
}

#[derive(Deserialize)]
struct TocEntry {
    label: String,
    href: String,
    depth: u32,
}

pub fn build(parent: &GnosisWindow) -> ReaderWidgets {
    let current_book: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let current_book_id: Rc<RefCell<Option<Uuid>>> = Rc::new(RefCell::new(None));

    let context = webkit6::WebContext::default().unwrap_or_else(webkit6::WebContext::new);
    reader_scheme::register(&context);

    let content_manager = webkit6::UserContentManager::new();
    content_manager.register_script_message_handler("gnosis", None);

    let settings = webkit6::Settings::new();
    settings.set_enable_write_console_messages_to_stdout(true);
    // matches the actual Foliate app's WebKit settings; none of these showed
    // a measurable effect on chapter-crossing time in isolation, but no
    // evidence of harm either (unlike CacheModel::WebBrowser, which was a
    // clean, repeatable regression and is deliberately not set)
    settings.set_enable_html5_database(false);
    settings.set_enable_html5_local_storage(false);
    settings.set_enable_back_forward_navigation_gestures(false);
    settings.set_enable_smooth_scrolling(false);
    // forcing this to Never was tried to test the DMA-BUF handoff theory and
    // made things actively worse (intermittent missing spinner, rendering
    // corruption) rather than clarifying anything — reverted to Always,
    // which was at least stable and neutral
    settings.set_hardware_acceleration_policy(webkit6::HardwareAccelerationPolicy::Always);
    // lets us open WebKit's own inspector (right-click -> Inspect Element)
    // to profile the content process directly instead of guessing settings
    settings.set_enable_developer_extras(true);

    let web_view = webkit6::WebView::builder()
        .web_context(&context)
        .user_content_manager(&content_manager)
        .settings(&settings)
        .vexpand(true)
        .hexpand(true)
        .build();

    // a page-content spinner would be animated by the WebView's own content
    // process, which is exactly the thread that's busy during a slow chapter
    // crossing — so it just freezes when it's needed most. A GTK-native
    // spinner is driven by GTK's own compositor and keeps spinning
    // regardless of what the WebView's main thread is doing.
    let spinner_widget = gtk::Spinner::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .width_request(32)
        .height_request(32)
        .visible(false)
        .build();
    let spinner = SpinnerHandle::new(spinner_widget.clone());

    let current_book_for_chooser = current_book.clone();
    web_view.connect_run_file_chooser(move |_, request| {
        match current_book_for_chooser.borrow().as_ref().and_then(|p| p.to_str()) {
            Some(path) => {
                request.select_files(&[path]);
                true
            }
            None => false,
        }
    });

    let web_view_for_restore = web_view.clone();
    let restore_script = initial_style_script(&ReaderPrefs::default());
    web_view.connect_load_changed(move |_, event| {
        if event == webkit6::LoadEvent::Finished {
            web_view_for_restore.evaluate_javascript(
                &restore_script,
                None,
                None,
                gio::Cancellable::NONE,
                |_| {},
            );
        }
    });
    web_view.load_uri(&format!("{}:///shell/reader.html", reader_scheme::SCHEME));

    let title_widget = adw::WindowTitle::new("", "");
    let toc_menu = gio::Menu::new();
    let toc_button = gtk::MenuButton::builder()
        .icon_name("view-list-symbolic")
        .tooltip_text("Table of Contents")
        .menu_model(&toc_menu)
        .build();

    let (display_button, apply_prefs, display_popover) =
        build_display_settings(&web_view, current_book_id.clone());

    let clock_label = gtk::Label::builder()
        .css_classes(["caption", "dim-label"])
        .build();
    update_clock(&clock_label);
    let clock_label_for_timer = clock_label.clone();
    glib::timeout_add_seconds_local(30, move || {
        update_clock(&clock_label_for_timer);
        glib::ControlFlow::Continue
    });

    let prev_button = gtk::Button::from_icon_name("go-previous-symbolic");
    prev_button.set_tooltip_text(Some("Previous Page"));
    let next_button = gtk::Button::from_icon_name("go-next-symbolic");
    next_button.set_tooltip_text(Some("Next Page"));

    let rsvp_button = gtk::Button::from_icon_name("media-playback-start-symbolic");
    rsvp_button.set_tooltip_text(Some("Speed Read"));
    let web_view_for_rsvp = web_view.clone();
    rsvp_button.connect_clicked(move |_| {
        web_view_for_rsvp.evaluate_javascript(
            "window.gnosisToggleRsvp && window.gnosisToggleRsvp();",
            None,
            None,
            gio::Cancellable::NONE,
            |_| {},
        );
    });

    let search_button = gtk::Button::from_icon_name("system-search-symbolic");
    search_button.set_tooltip_text(Some("Search"));
    let web_view_for_search = web_view.clone();
    search_button.connect_clicked(move |_| {
        web_view_for_search.evaluate_javascript(
            "window.gnosisToggleSearch && window.gnosisToggleSearch();",
            None,
            None,
            gio::Cancellable::NONE,
            |_| {},
        );
    });

    let web_view_for_prev = web_view.clone();
    let spinner_for_prev = spinner.clone();
    prev_button.connect_clicked(move |_| {
        spinner_for_prev.show_soon();
        web_view_for_prev.evaluate_javascript(
            &prev_script(),
            None,
            None,
            gio::Cancellable::NONE,
            |_| {},
        );
    });
    let web_view_for_next = web_view.clone();
    let spinner_for_next = spinner.clone();
    next_button.connect_clicked(move |_| {
        spinner_for_next.show_soon();
        web_view_for_next.evaluate_javascript(
            &next_script(),
            None,
            None,
            gio::Cancellable::NONE,
            |_| {},
        );
    });

    let key_controller = gtk::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let web_view_for_keys = web_view.clone();
    let spinner_for_keys = spinner.clone();
    key_controller.connect_key_pressed(move |_, keyval, _keycode, state| {
        if keyval == gtk::gdk::Key::f && state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
            web_view_for_keys.evaluate_javascript(
                "window.gnosisToggleSearch && window.gnosisToggleSearch();",
                None,
                None,
                gio::Cancellable::NONE,
                |_| {},
            );
            return glib::Propagation::Stop;
        }
        match keyval {
            gtk::gdk::Key::Left | gtk::gdk::Key::Page_Up => {
                spinner_for_keys.show_soon();
                web_view_for_keys.evaluate_javascript(
                    &prev_script(),
                    None,
                    None,
                    gio::Cancellable::NONE,
                    |_| {},
                );
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Right | gtk::gdk::Key::Page_Down | gtk::gdk::Key::space => {
                spinner_for_keys.show_soon();
                web_view_for_keys.evaluate_javascript(
                    &next_script(),
                    None,
                    None,
                    gio::Cancellable::NONE,
                    |_| {},
                );
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    web_view.add_controller(key_controller);

    let header_bar = adw::HeaderBar::new();
    header_bar.set_title_widget(Some(&title_widget));
    header_bar.pack_start(&prev_button);
    header_bar.pack_start(&next_button);
    header_bar.pack_end(&toc_button);
    header_bar.pack_end(&search_button);
    header_bar.pack_end(&rsvp_button);
    header_bar.pack_end(&display_button);
    header_bar.pack_end(&clock_label);

    let header_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .valign(gtk::Align::Start)
        .reveal_child(true)
        .child(&header_bar)
        .build();

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&web_view));
    overlay.add_overlay(&header_revealer);
    overlay.add_overlay(&spinner_widget);

    let header_motion = gtk::EventControllerMotion::new();
    let header_revealer_for_enter = header_revealer.clone();
    header_motion.connect_enter(move |_, _, _| header_revealer_for_enter.set_reveal_child(true));
    let header_revealer_for_leave = header_revealer.clone();
    let popover_for_leave = display_popover.clone();
    header_motion.connect_leave(move |_| {
        header_revealer_for_leave.set_reveal_child(popover_for_leave.get_visible());
    });
    header_revealer.add_controller(header_motion);

    let hide_chrome = gtk::GestureClick::new();
    hide_chrome.set_propagation_phase(gtk::PropagationPhase::Capture);
    let header_revealer_for_hide = header_revealer.clone();
    hide_chrome.connect_pressed(move |_, _, _, _| {
        header_revealer_for_hide.set_reveal_child(false);
    });
    web_view.add_controller(hide_chrome);

    let page = adw::NavigationPage::with_tag(&overlay, "Reader", "reader");

    let web_view_for_shown = web_view.clone();
    page.connect_shown(move |_| {
        web_view_for_shown.grab_focus();
    });

    let parent_weak = parent.downgrade();
    let title_widget_for_msg = title_widget.clone();
    let toc_menu_for_msg = toc_menu.clone();
    let current_book_id_for_msg = current_book_id.clone();
    let spinner_for_msg = spinner.clone();
    content_manager.connect_script_message_received(Some("gnosis"), move |_, value| {
        let Some(parent) = parent_weak.upgrade() else {
            return;
        };
        let Ok(message) = serde_json::from_str::<ReaderMessage>(value.to_str().as_str()) else {
            return;
        };
        match message {
            ReaderMessage::Loading => {
                spinner_for_msg.show_soon();
            }
            ReaderMessage::Ready { title, toc } => {
                spinner_for_msg.hide();
                if let Some(title) = title {
                    title_widget_for_msg.set_title(&title);
                }
                toc_menu_for_msg.remove_all();
                for entry in toc {
                    let indent = "\u{2003}".repeat(entry.depth as usize);
                    let target = glib::Variant::from(entry.href.as_str()).print(false);
                    toc_menu_for_msg.append(
                        Some(&format!("{indent}{}", entry.label)),
                        Some(&format!("win.reader-goto({target})")),
                    );
                }
            }
            ReaderMessage::Relocate { cfi, fraction } => {
                spinner_for_msg.hide();
                parent.save_reader_position(cfi, fraction.unwrap_or(0.0));
            }
            ReaderMessage::RsvpWpm { wpm } => {
                if let Some(id) = *current_book_id_for_msg.borrow() {
                    let mut prefs = library::reader_prefs::load_for(id);
                    prefs.rsvp_wpm = wpm;
                    library::reader_prefs::save_for(id, &prefs);
                }
            }
            ReaderMessage::Error { message } => {
                spinner_for_msg.hide();
                parent.show_reader_error(&message);
            }
        }
    });

    ReaderWidgets {
        page,
        web_view,
        title_widget,
        toc_menu,
        current_book,
        current_book_id,
        apply_prefs,
        spinner,
    }
}

const THEME_NAMES: [&str; 4] = ["light", "sepia", "gray", "dark"];

fn build_display_settings(
    web_view: &webkit6::WebView,
    current_book_id: Rc<RefCell<Option<Uuid>>>,
) -> (gtk::MenuButton, Rc<dyn Fn(&ReaderPrefs)>, gtk::Popover) {
    let prefs = ReaderPrefs::default();
    let theme_index = THEME_NAMES
        .iter()
        .position(|name| *name == prefs.theme)
        .unwrap_or(0) as u32;
    let theme_dropdown = gtk::DropDown::from_strings(&["Light", "Sepia", "Gray", "Dark"]);
    theme_dropdown.set_selected(theme_index);

    let font_dialog = gtk::FontDialog::new();
    let font_button = gtk::FontDialogButton::new(Some(font_dialog));
    font_button.set_level(gtk::FontLevel::Family);

    let publisher_font_check = gtk::CheckButton::builder()
        .label("Publisher Font")
        .active(prefs.font_family.is_none())
        .build();
    font_button.set_sensitive(prefs.font_family.is_some());

    let font_size_adjustment =
        gtk::Adjustment::new(prefs.font_size as f64, 50.0, 300.0, 10.0, 10.0, 0.0);
    let font_size_spin = gtk::SpinButton::new(Some(&font_size_adjustment), 1.0, 0);

    let skip_front_matter_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Start)
        .tooltip_text("Skip cover/title pages when opening a book for the first time")
        .build();

    let grid = gtk::Grid::builder()
        .row_spacing(8)
        .column_spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    let label = |text| {
        gtk::Label::builder()
            .label(text)
            .halign(gtk::Align::Start)
            .build()
    };
    let font_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    font_row.append(&publisher_font_check);
    font_row.append(&font_button);

    grid.attach(&label("Theme"), 0, 0, 1, 1);
    grid.attach(&theme_dropdown, 1, 0, 1, 1);
    grid.attach(&label("Font"), 0, 1, 1, 1);
    grid.attach(&font_row, 1, 1, 1, 1);
    grid.attach(&label("Font Size"), 0, 2, 1, 1);
    grid.attach(&font_size_spin, 1, 2, 1, 1);
    grid.attach(&label("Skip to First Chapter"), 0, 3, 1, 1);
    grid.attach(&skip_front_matter_switch, 1, 3, 1, 1);

    let popover = gtk::Popover::builder().child(&grid).build();
    let display_button = gtk::MenuButton::builder()
        .icon_name("font-x-generic-symbolic")
        .tooltip_text("Reading Preferences")
        .popover(&popover)
        .build();

    let popover_for_dismiss = popover.clone();
    let dismiss_on_webview_click = gtk::GestureClick::new();
    dismiss_on_webview_click.set_propagation_phase(gtk::PropagationPhase::Capture);
    dismiss_on_webview_click.connect_pressed(move |_, _, _, _| {
        if popover_for_dismiss.get_visible() {
            popover_for_dismiss.popdown();
        }
    });
    web_view.add_controller(dismiss_on_webview_click);

    let swipe_active: Rc<std::cell::Cell<bool>> = Rc::new(std::cell::Cell::new(false));
    let swipe_snapped: Rc<std::cell::Cell<bool>> = Rc::new(std::cell::Cell::new(false));

    let scroll_controller = gtk::EventControllerScroll::new(
        gtk::EventControllerScrollFlags::BOTH_AXES | gtk::EventControllerScrollFlags::KINETIC,
    );
    scroll_controller.set_propagation_phase(gtk::PropagationPhase::Capture);

    let swipe_active_for_begin = swipe_active.clone();
    let swipe_snapped_for_begin = swipe_snapped.clone();
    scroll_controller.connect_scroll_begin(move |_| {
        swipe_active_for_begin.set(false);
        swipe_snapped_for_begin.set(false);
    });

    let font_size_for_scroll = font_size_spin.clone();
    let web_view_for_swipe = web_view.clone();
    let swipe_active_for_scroll = swipe_active.clone();
    scroll_controller.connect_scroll(move |controller, dx, dy| {
        if controller
            .current_event_state()
            .contains(gtk::gdk::ModifierType::CONTROL_MASK)
        {
            let step = if dy < 0.0 { 10.0 } else { -10.0 };
            let new_value = (font_size_for_scroll.value() + step).clamp(50.0, 300.0);
            font_size_for_scroll.set_value(new_value);
            return glib::Propagation::Stop;
        }

        if dx.abs() > dy.abs() {
            swipe_active_for_scroll.set(true);
            const SCROLL_PIXELS_PER_UNIT: f64 = 40.0;
            let (px, py) = (dx * SCROLL_PIXELS_PER_UNIT, dy * SCROLL_PIXELS_PER_UNIT);
            let script = format!("window.gnosisScrollBy && window.gnosisScrollBy({px}, {py});");
            web_view_for_swipe.evaluate_javascript(
                &script,
                None,
                None,
                gio::Cancellable::NONE,
                |_| {},
            );
            return glib::Propagation::Stop;
        }

        glib::Propagation::Proceed
    });

    let web_view_for_decelerate = web_view.clone();
    let swipe_active_for_decelerate = swipe_active.clone();
    let swipe_snapped_for_decelerate = swipe_snapped.clone();
    scroll_controller.connect_decelerate(move |_, vel_x, vel_y| {
        if !swipe_active_for_decelerate.get() {
            return;
        }
        swipe_snapped_for_decelerate.set(true);
        let script = format!(
            "window.gnosisSnap && window.gnosisSnap({}, {});",
            vel_x / 1000.0,
            vel_y / 1000.0
        );
        web_view_for_decelerate.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| {});
    });

    let web_view_for_end = web_view.clone();
    let swipe_active_for_end = swipe_active.clone();
    let swipe_snapped_for_end = swipe_snapped.clone();
    scroll_controller.connect_scroll_end(move |_| {
        if swipe_active_for_end.get() && !swipe_snapped_for_end.get() {
            web_view_for_end.evaluate_javascript(
                "window.gnosisSnap && window.gnosisSnap(0, 0);",
                None,
                None,
                gio::Cancellable::NONE,
                |_| {},
            );
        }
        swipe_active_for_end.set(false);
        swipe_snapped_for_end.set(false);
    });
    web_view.add_controller(scroll_controller);

    let web_view_for_theme = web_view.clone();
    let publisher_font_for_theme = publisher_font_check.clone();
    let font_button_for_theme = font_button.clone();
    let font_size_for_theme = font_size_spin.clone();
    let current_book_id_for_theme = current_book_id.clone();
    theme_dropdown.connect_selected_notify(move |dropdown| {
        push_reader_style(
            &web_view_for_theme,
            dropdown,
            &publisher_font_for_theme,
            &font_button_for_theme,
            &font_size_for_theme,
            &current_book_id_for_theme,
        );
    });

    let web_view_for_publisher = web_view.clone();
    let theme_for_publisher = theme_dropdown.clone();
    let font_button_for_publisher = font_button.clone();
    let font_size_for_publisher = font_size_spin.clone();
    let current_book_id_for_publisher = current_book_id.clone();
    publisher_font_check.connect_toggled(move |check| {
        font_button_for_publisher.set_sensitive(!check.is_active());
        push_reader_style(
            &web_view_for_publisher,
            &theme_for_publisher,
            check,
            &font_button_for_publisher,
            &font_size_for_publisher,
            &current_book_id_for_publisher,
        );
    });

    let web_view_for_font = web_view.clone();
    let theme_for_font = theme_dropdown.clone();
    let publisher_font_for_font = publisher_font_check.clone();
    let font_size_for_font = font_size_spin.clone();
    let current_book_id_for_font = current_book_id.clone();
    font_button.connect_font_desc_notify(move |button| {
        publisher_font_for_font.set_active(false);
        push_reader_style(
            &web_view_for_font,
            &theme_for_font,
            &publisher_font_for_font,
            button,
            &font_size_for_font,
            &current_book_id_for_font,
        );
    });

    let web_view_for_size = web_view.clone();
    let theme_for_size = theme_dropdown.clone();
    let publisher_font_for_size = publisher_font_check.clone();
    let font_button_for_size = font_button.clone();
    let current_book_id_for_size = current_book_id.clone();
    font_size_spin.connect_value_changed(move |spin| {
        push_reader_style(
            &web_view_for_size,
            &theme_for_size,
            &publisher_font_for_size,
            &font_button_for_size,
            spin,
            &current_book_id_for_size,
        );
    });

    let web_view_for_start_mode = web_view.clone();
    skip_front_matter_switch.connect_active_notify(move |switch| {
        let script = format!("window.gnosisSetStartMode({});", switch.is_active());
        web_view_for_start_mode.evaluate_javascript(
            &script,
            None,
            None,
            gio::Cancellable::NONE,
            |_| {},
        );
    });

    let apply_prefs: Rc<dyn Fn(&ReaderPrefs)> = Rc::new(move |prefs: &ReaderPrefs| {
        let theme_index = THEME_NAMES
            .iter()
            .position(|name| *name == prefs.theme)
            .unwrap_or(0) as u32;
        theme_dropdown.set_selected(theme_index);
        match &prefs.font_family {
            Some(family) => {
                font_button.set_font_desc(&gtk::pango::FontDescription::from_string(family));
            }
            None => {
                publisher_font_check.set_active(true);
                font_button.set_sensitive(false);
            }
        }
        font_size_spin.set_value(prefs.font_size as f64);
    });

    (display_button, apply_prefs, popover)
}

fn push_reader_style(
    web_view: &webkit6::WebView,
    theme_dropdown: &gtk::DropDown,
    publisher_font_check: &gtk::CheckButton,
    font_button: &gtk::FontDialogButton,
    font_size_spin: &gtk::SpinButton,
    current_book_id: &Rc<RefCell<Option<Uuid>>>,
) {
    let theme = THEME_NAMES
        .get(theme_dropdown.selected() as usize)
        .copied()
        .unwrap_or("light");
    let font_family = if publisher_font_check.is_active() {
        None
    } else {
        font_button
            .font_desc()
            .and_then(|desc| desc.family().map(|f| f.to_string()))
    };
    let font_size = font_size_spin.value_as_int();

    if let Some(id) = *current_book_id.borrow() {
        let rsvp_wpm = library::reader_prefs::load_for(id).rsvp_wpm;
        library::reader_prefs::save_for(
            id,
            &ReaderPrefs {
                theme: theme.to_string(),
                font_family: font_family.clone(),
                font_size,
                rsvp_wpm,
            },
        );
    }

    let style = serde_json::json!({
        "theme": theme,
        "fontFamily": font_family,
        "fontSize": font_size,
    });
    let script = format!("window.gnosisSetStyle({style});");
    web_view.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| {});
}

fn initial_style_script(prefs: &ReaderPrefs) -> String {
    let style = serde_json::json!({
        "theme": prefs.theme,
        "fontFamily": prefs.font_family,
        "fontSize": prefs.font_size,
        "rsvpWpm": prefs.rsvp_wpm,
    });
    format!(
        "(function poll() {{ \
            if (window.gnosisSetStyle) window.gnosisSetStyle({style}); \
            else setTimeout(poll, 20); \
        }})();"
    )
}

pub fn open_book_script(resume_cfi: Option<&str>, prefs: &ReaderPrefs) -> String {
    let cfi_json = resume_cfi
        .map(|cfi| serde_json::to_string(cfi).unwrap_or_else(|_| "null".to_string()))
        .unwrap_or_else(|| "null".to_string());
    let style_json = serde_json::json!({
        "theme": prefs.theme,
        "fontFamily": prefs.font_family,
        "fontSize": prefs.font_size,
        "rsvpWpm": prefs.rsvp_wpm,
    });
    format!(
        "(function poll() {{ \
            if (window.gnosisOpenBook) window.gnosisOpenBook({cfi_json}, {style_json}); \
            else setTimeout(poll, 20); \
        }})();"
    )
}

pub fn goto_script(href: &str) -> String {
    let href_json = serde_json::to_string(href).unwrap_or_else(|_| "\"\"".to_string());
    format!("window.gnosisGoTo({href_json});")
}

fn prev_script() -> &'static str {
    "if (window.gnosisPrev) window.gnosisPrev();"
}

fn next_script() -> &'static str {
    "if (window.gnosisNext) window.gnosisNext();"
}

fn update_clock(label: &gtk::Label) {
    let text = glib::DateTime::now_local()
        .and_then(|now| now.format("%H:%M"))
        .map(|s| s.to_string())
        .unwrap_or_default();
    label.set_label(&text);
}


