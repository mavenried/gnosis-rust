use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use serde::Deserialize;
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
    /// Shared with the `gnosis-reader:` scheme handler: the path it streams
    /// bytes from for `gnosis-reader:///book/current`. Set before asking the
    /// page to open a book.
    pub current_book: Rc<RefCell<Option<PathBuf>>>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ReaderMessage {
    Ready {
        title: Option<String>,
        #[serde(default)]
        toc: Vec<TocEntry>,
    },
    Relocate {
        cfi: Option<String>,
        fraction: Option<f64>,
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

/// Builds the reading view once: one `WebView` + its own `AdwNavigationPage`
/// (tag `"reader"`), reused for every book that gets opened afterward rather
/// than rebuilt (a WebView spins up its own web process, which isn't cheap).
pub fn build(parent: &GnosisWindow) -> ReaderWidgets {
    let current_book: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

    let context = webkit6::WebContext::new();
    reader_scheme::register(&context, current_book.clone());

    let content_manager = webkit6::UserContentManager::new();
    content_manager.register_script_message_handler("gnosis", None);

    let web_view = webkit6::WebView::builder()
        .web_context(&context)
        .user_content_manager(&content_manager)
        .vexpand(true)
        .hexpand(true)
        .build();

    // Restore the last-used theme/font/size (see build_display_settings())
    // once the shell page has actually finished loading, so the very first
    // book opened this session already renders with them rather than
    // defaults. Injecting this in the same tick as `load_uri` below (as a
    // "poll until window.gnosisSetStyle exists" script, like every other
    // evaluate_javascript call in this file) doesn't work here specifically:
    // unlike those other calls — always made long after the shell has
    // already loaded — this one runs before navigation to reader.html even
    // starts, so it targets the pre-navigation context; that context (and
    // its pending poll) is torn down when the real page loads, and the
    // restore silently never happens even though it looks like it should.
    let saved_prefs = library::reader_prefs::load();
    let web_view_for_restore = web_view.clone();
    let restore_script = initial_style_script(&saved_prefs);
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

    let display_button = build_display_settings(&web_view, &saved_prefs);

    // foliate-js's paginator has no built-in page-turn triggers of its own
    // (confirmed by reading paginator.js — it only tracks touch/pointer
    // selection) — the app embedding it is expected to supply navigation,
    // same as the real Foliate app does. These buttons and the keyboard
    // handler below call the `gnosisPrev`/`gnosisNext` functions exposed by
    // assets/reader.js.
    let prev_button = gtk::Button::from_icon_name("go-previous-symbolic");
    prev_button.set_tooltip_text(Some("Previous Page"));
    let next_button = gtk::Button::from_icon_name("go-next-symbolic");
    next_button.set_tooltip_text(Some("Next Page"));

    let web_view_for_prev = web_view.clone();
    prev_button.connect_clicked(move |_| {
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
    key_controller.connect_key_pressed(move |_, keyval, _keycode, _state| match keyval {
        gtk::gdk::Key::Left | gtk::gdk::Key::Page_Up => {
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
    });
    web_view.add_controller(key_controller);

    let header_bar = adw::HeaderBar::new();
    header_bar.set_title_widget(Some(&title_widget));
    header_bar.pack_start(&prev_button);
    header_bar.pack_start(&next_button);
    header_bar.pack_end(&toc_button);
    header_bar.pack_end(&display_button);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.set_content(Some(&web_view));

    let page = adw::NavigationPage::with_tag(&toolbar_view, "Reader", "reader");

    let parent_weak = parent.downgrade();
    let title_widget_for_msg = title_widget.clone();
    let toc_menu_for_msg = toc_menu.clone();
    content_manager.connect_script_message_received(Some("gnosis"), move |_, value| {
        let Some(parent) = parent_weak.upgrade() else {
            return;
        };
        let Ok(message) = serde_json::from_str::<ReaderMessage>(value.to_str().as_str()) else {
            return;
        };
        match message {
            ReaderMessage::Ready { title, toc } => {
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
                parent.save_reader_position(cfi, fraction.unwrap_or(0.0));
            }
            ReaderMessage::Error { message } => {
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
    }
}

const THEME_NAMES: [&str; 4] = ["light", "sepia", "gray", "dark"];

/// Builds the "Reading Preferences" menu button: a theme picker, a system
/// font picker (family only — size is handled separately below, since
/// point sizes don't map cleanly onto a reflowable page), and a font-size
/// percentage. Selecting any of them pushes the combined choice to the
/// reader shell via `window.gnosisSetStyle` and saves it as the new default
/// for next time (see `library::reader_prefs`).
fn build_display_settings(web_view: &webkit6::WebView, prefs: &ReaderPrefs) -> gtk::MenuButton {
    let theme_index = THEME_NAMES
        .iter()
        .position(|name| *name == prefs.theme)
        .unwrap_or(0) as u32;
    let theme_dropdown = gtk::DropDown::from_strings(&["Light", "Sepia", "Gray", "Dark"]);
    theme_dropdown.set_selected(theme_index);

    let font_dialog = gtk::FontDialog::new();
    let font_button = gtk::FontDialogButton::new(Some(font_dialog));
    font_button.set_level(gtk::FontLevel::Family);

    // GtkFontDialogButton has no "unset" once a font_desc is set, so "use
    // the publisher's own embedded font" (font_family: null) needs its own
    // explicit control rather than relying on the button's default empty
    // state — otherwise there'd be no way back to it after picking a font.
    let publisher_font_check = gtk::CheckButton::builder()
        .label("Publisher Font")
        .active(prefs.font_family.is_none())
        .build();
    font_button.set_sensitive(prefs.font_family.is_some());
    if let Some(family) = &prefs.font_family {
        font_button.set_font_desc(&gtk::pango::FontDescription::from_string(family));
    }

    let font_size_adjustment =
        gtk::Adjustment::new(prefs.font_size as f64, 50.0, 300.0, 10.0, 10.0, 0.0);
    let font_size_spin = gtk::SpinButton::new(Some(&font_size_adjustment), 1.0, 0);

    // Where a book with no saved position opens: at its true first page
    // (off, the default — the print-book convention) or skipping straight
    // to the body text past any cover/title/copyright pages (on). Only
    // affects future opens; a saved position always takes precedence. This
    // one is session-only (not saved to ReaderPrefs) since it's about
    // navigation behavior, not display.
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

    let web_view_for_theme = web_view.clone();
    let publisher_font_for_theme = publisher_font_check.clone();
    let font_button_for_theme = font_button.clone();
    let font_size_for_theme = font_size_spin.clone();
    theme_dropdown.connect_selected_notify(move |dropdown| {
        push_reader_style(
            &web_view_for_theme,
            dropdown,
            &publisher_font_for_theme,
            &font_button_for_theme,
            &font_size_for_theme,
        );
    });

    let web_view_for_publisher = web_view.clone();
    let theme_for_publisher = theme_dropdown.clone();
    let font_button_for_publisher = font_button.clone();
    let font_size_for_publisher = font_size_spin.clone();
    publisher_font_check.connect_toggled(move |check| {
        font_button_for_publisher.set_sensitive(!check.is_active());
        push_reader_style(
            &web_view_for_publisher,
            &theme_for_publisher,
            check,
            &font_button_for_publisher,
            &font_size_for_publisher,
        );
    });

    let web_view_for_font = web_view.clone();
    let theme_for_font = theme_dropdown.clone();
    let publisher_font_for_font = publisher_font_check.clone();
    let font_size_for_font = font_size_spin.clone();
    font_button.connect_font_desc_notify(move |button| {
        // Picking a font implies "not the publisher's font" — but only act
        // on this if the button is actually the one the user can interact
        // with (it's insensitive, and notify won't normally fire, while
        // "Publisher Font" is checked; this is just a safety net).
        publisher_font_for_font.set_active(false);
        push_reader_style(
            &web_view_for_font,
            &theme_for_font,
            &publisher_font_for_font,
            button,
            &font_size_for_font,
        );
    });

    let web_view_for_size = web_view.clone();
    let theme_for_size = theme_dropdown.clone();
    let publisher_font_for_size = publisher_font_check.clone();
    let font_button_for_size = font_button.clone();
    font_size_spin.connect_value_changed(move |spin| {
        push_reader_style(
            &web_view_for_size,
            &theme_for_size,
            &publisher_font_for_size,
            &font_button_for_size,
            spin,
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

    display_button
}

/// Reads the four reading-preference widgets, pushes the combined style to
/// the reader shell, and persists it as the new default for next time.
fn push_reader_style(
    web_view: &webkit6::WebView,
    theme_dropdown: &gtk::DropDown,
    publisher_font_check: &gtk::CheckButton,
    font_button: &gtk::FontDialogButton,
    font_size_spin: &gtk::SpinButton,
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

    library::reader_prefs::save(&ReaderPrefs {
        theme: theme.to_string(),
        font_family: font_family.clone(),
        font_size,
    });

    let style = serde_json::json!({
        "theme": theme,
        "fontFamily": font_family,
        "fontSize": font_size,
    });
    let script = format!("window.gnosisSetStyle({style});");
    web_view.evaluate_javascript(&script, None, None, gio::Cancellable::NONE, |_| {});
}

/// Same JSON shape `push_reader_style` sends, but for restoring the saved
/// preferences right after the reader shell first loads — waits for
/// `window.gnosisSetStyle` to exist, same as `open_book_script` waits for
/// `gnosisOpenBook`.
fn initial_style_script(prefs: &ReaderPrefs) -> String {
    let style = serde_json::json!({
        "theme": prefs.theme,
        "fontFamily": prefs.font_family,
        "fontSize": prefs.font_size,
    });
    format!(
        "(function poll() {{ \
            if (window.gnosisSetStyle) window.gnosisSetStyle({style}); \
            else setTimeout(poll, 20); \
        }})();"
    )
}

/// A script that waits for the reader shell's module script to finish
/// loading (`window.gnosisOpenBook` only exists once it has) before opening
/// the book — avoids racing the shell's own load against the first book a
/// user opens.
pub fn open_book_script(uri: &str, resume_cfi: Option<&str>) -> String {
    let uri_json = serde_json::to_string(uri).unwrap_or_else(|_| "\"\"".to_string());
    let cfi_json = resume_cfi
        .map(|cfi| serde_json::to_string(cfi).unwrap_or_else(|_| "null".to_string()))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "(function poll() {{ \
            if (window.gnosisOpenBook) window.gnosisOpenBook({uri_json}, {cfi_json}); \
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


