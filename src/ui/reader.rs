use std::cell::{Cell, RefCell};
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

pub fn build(parent: &GnosisWindow) -> ReaderWidgets {
    let current_book_id: Rc<RefCell<Option<Uuid>>> = Rc::new(RefCell::new(None));

    let context = webkit6::WebContext::default().unwrap_or_else(webkit6::WebContext::new);
    reader_scheme::register(&context);

    let content_manager = webkit6::UserContentManager::new();
    content_manager.register_script_message_handler("gnosis", None);

    let settings = webkit6::Settings::new();
    settings.set_enable_write_console_messages_to_stdout(true);
    settings.set_enable_html5_database(false);
    settings.set_enable_html5_local_storage(false);
    settings.set_enable_back_forward_navigation_gestures(false);
    settings.set_enable_smooth_scrolling(false);
    settings.set_hardware_acceleration_policy(webkit6::HardwareAccelerationPolicy::Always);
    settings.set_enable_developer_extras(true);

    let web_view = webkit6::WebView::builder()
        .web_context(&context)
        .user_content_manager(&content_manager)
        .settings(&settings)
        .vexpand(true)
        .hexpand(true)
        .build();

    let spinner_widget = gtk::Spinner::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .width_request(32)
        .height_request(32)
        .visible(false)
        .build();
    let spinner = SpinnerHandle::new(spinner_widget.clone());

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
    let toc_button = gtk::Button::from_icon_name("view-list-symbolic");
    toc_button.set_tooltip_text(Some("Table of Contents"));
    let web_view_for_toc = web_view.clone();
    toc_button.connect_clicked(move |_| {
        web_view_for_toc.evaluate_javascript(
            "window.gnosisToggleToc && window.gnosisToggleToc();",
            None,
            None,
            gio::Cancellable::NONE,
            |_| {},
        );
    });

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

    // The spinner for next/prev is driven entirely by JS's `loading` message
    // (sent only when the turn will cross a chapter boundary and may need to
    // load new content) rather than triggered here unconditionally, since a
    // same-chapter turn is just foliate's own animation, not a real wait.
    let web_view_for_prev = web_view.clone();
    prev_button.connect_clicked(move |_| {
        tracing::info!("dispatching prev_script (button)");
        web_view_for_prev.evaluate_javascript(
            &prev_script(),
            None,
            None,
            gio::Cancellable::NONE,
            |_| {},
        );
    });
    let web_view_for_next = web_view.clone();
    next_button.connect_clicked(move |_| {
        tracing::info!("dispatching next_script (button)");
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
                tracing::info!("dispatching prev_script (key)");
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
                tracing::info!("dispatching next_script (key)");
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
            ReaderMessage::Ready { title } => {
                spinner_for_msg.hide();
                if let Some(title) = title {
                    title_widget_for_msg.set_title(&title);
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

    let line_height_adjustment =
        gtk::Adjustment::new(prefs.line_height as f64, 100.0, 250.0, 10.0, 10.0, 0.0);
    let line_height_spin = gtk::SpinButton::new(Some(&line_height_adjustment), 1.0, 0);

    let paragraph_spacing_adjustment =
        gtk::Adjustment::new(prefs.paragraph_spacing as f64, 0.0, 300.0, 10.0, 10.0, 0.0);
    let paragraph_spacing_spin =
        gtk::SpinButton::new(Some(&paragraph_spacing_adjustment), 1.0, 0);

    let margin_adjustment = gtk::Adjustment::new(prefs.margin as f64, 0.0, 160.0, 8.0, 8.0, 0.0);
    let margin_spin = gtk::SpinButton::new(Some(&margin_adjustment), 1.0, 0);

    let justify_switch = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Start)
        .active(prefs.justify)
        .tooltip_text("Justify body text")
        .build();

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
    grid.attach(&label("Line Spacing"), 0, 3, 1, 1);
    grid.attach(&line_height_spin, 1, 3, 1, 1);
    grid.attach(&label("Paragraph Spacing"), 0, 4, 1, 1);
    grid.attach(&paragraph_spacing_spin, 1, 4, 1, 1);
    grid.attach(&label("Margins"), 0, 5, 1, 1);
    grid.attach(&margin_spin, 1, 5, 1, 1);
    grid.attach(&label("Justify Text"), 0, 6, 1, 1);
    grid.attach(&justify_switch, 1, 6, 1, 1);
    grid.attach(&label("Skip to First Chapter"), 0, 7, 1, 1);
    grid.attach(&skip_front_matter_switch, 1, 7, 1, 1);

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
            // foliate's own paginator caps a live drag to one page width in
            // either direction, so a high multiplier here just means a light
            // brush of the trackpad is enough to hit that cap and flip the
            // page. Keep this low so a full page turn takes a deliberate swipe.
            const SCROLL_PIXELS_PER_UNIT: f64 = 10.0;
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

    let push_style: Rc<dyn Fn()> = {
        let web_view = web_view.clone();
        let theme_dropdown = theme_dropdown.clone();
        let publisher_font_check = publisher_font_check.clone();
        let font_button = font_button.clone();
        let font_size_spin = font_size_spin.clone();
        let line_height_spin = line_height_spin.clone();
        let paragraph_spacing_spin = paragraph_spacing_spin.clone();
        let margin_spin = margin_spin.clone();
        let justify_switch = justify_switch.clone();
        let current_book_id = current_book_id.clone();
        Rc::new(move || {
            push_reader_style(
                &web_view,
                &theme_dropdown,
                &publisher_font_check,
                &font_button,
                &font_size_spin,
                &line_height_spin,
                &paragraph_spacing_spin,
                &margin_spin,
                &justify_switch,
                &current_book_id,
            );
        })
    };

    let push_style_for_theme = push_style.clone();
    theme_dropdown.connect_selected_notify(move |_| push_style_for_theme());

    let font_button_for_publisher = font_button.clone();
    let push_style_for_publisher = push_style.clone();
    publisher_font_check.connect_toggled(move |check| {
        font_button_for_publisher.set_sensitive(!check.is_active());
        push_style_for_publisher();
    });

    let publisher_font_for_font = publisher_font_check.clone();
    let push_style_for_font = push_style.clone();
    font_button.connect_font_desc_notify(move |_| {
        publisher_font_for_font.set_active(false);
        push_style_for_font();
    });

    let push_style_for_size = push_style.clone();
    font_size_spin.connect_value_changed(move |_| push_style_for_size());

    let push_style_for_line_height = push_style.clone();
    line_height_spin.connect_value_changed(move |_| push_style_for_line_height());

    let push_style_for_paragraph_spacing = push_style.clone();
    paragraph_spacing_spin.connect_value_changed(move |_| push_style_for_paragraph_spacing());

    let push_style_for_margin = push_style.clone();
    margin_spin.connect_value_changed(move |_| push_style_for_margin());

    let push_style_for_justify = push_style.clone();
    justify_switch.connect_active_notify(move |_| push_style_for_justify());

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
        line_height_spin.set_value(prefs.line_height as f64);
        paragraph_spacing_spin.set_value(prefs.paragraph_spacing as f64);
        margin_spin.set_value(prefs.margin as f64);
        justify_switch.set_active(prefs.justify);
    });

    (display_button, apply_prefs, popover)
}

#[allow(clippy::too_many_arguments)]
fn push_reader_style(
    web_view: &webkit6::WebView,
    theme_dropdown: &gtk::DropDown,
    publisher_font_check: &gtk::CheckButton,
    font_button: &gtk::FontDialogButton,
    font_size_spin: &gtk::SpinButton,
    line_height_spin: &gtk::SpinButton,
    paragraph_spacing_spin: &gtk::SpinButton,
    margin_spin: &gtk::SpinButton,
    justify_switch: &gtk::Switch,
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
    let line_height = line_height_spin.value_as_int();
    let paragraph_spacing = paragraph_spacing_spin.value_as_int();
    let margin = margin_spin.value_as_int();
    let justify = justify_switch.is_active();

    if let Some(id) = *current_book_id.borrow() {
        let rsvp_wpm = library::reader_prefs::load_for(id).rsvp_wpm;
        library::reader_prefs::save_for(
            id,
            &ReaderPrefs {
                theme: theme.to_string(),
                font_family: font_family.clone(),
                font_size,
                rsvp_wpm,
                line_height,
                paragraph_spacing,
                margin,
                justify,
            },
        );
    }

    let style = serde_json::json!({
        "theme": theme,
        "fontFamily": font_family,
        "fontSize": font_size,
        "lineHeight": line_height,
        "paragraphSpacing": paragraph_spacing,
        "margin": margin,
        "justify": justify,
    });
    let script = format!("window.gnosisSetStyle({style});");
    web_view.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| {});
}

fn style_json(prefs: &ReaderPrefs) -> serde_json::Value {
    serde_json::json!({
        "theme": prefs.theme,
        "fontFamily": prefs.font_family,
        "fontSize": prefs.font_size,
        "rsvpWpm": prefs.rsvp_wpm,
        "lineHeight": prefs.line_height,
        "paragraphSpacing": prefs.paragraph_spacing,
        "margin": prefs.margin,
        "justify": prefs.justify,
    })
}

fn initial_style_script(prefs: &ReaderPrefs) -> String {
    let style = style_json(prefs);
    format!(
        "(function poll() {{ \
            if (window.gnosisSetStyle) window.gnosisSetStyle({style}); \
            else setTimeout(poll, 20); \
        }})();"
    )
}

pub fn open_book_script(book_id: Uuid, resume_cfi: Option<&str>, prefs: &ReaderPrefs) -> String {
    let id_json = serde_json::to_string(&book_id.to_string()).unwrap_or_else(|_| "\"\"".to_string());
    let cfi_json = resume_cfi
        .map(|cfi| serde_json::to_string(cfi).unwrap_or_else(|_| "null".to_string()))
        .unwrap_or_else(|| "null".to_string());
    let style_json = style_json(prefs);
    format!(
        "(function poll() {{ \
            if (window.gnosisOpenBook) window.gnosisOpenBook({id_json}, {cfi_json}, {style_json}); \
            else setTimeout(poll, 20); \
        }})();"
    )
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


