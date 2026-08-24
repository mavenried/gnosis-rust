use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

use crate::library::{log, settings};

use super::window::GnosisWindow;

type Rows = Rc<RefCell<Vec<adw::ActionRow>>>;

/// Widgets from the Settings page that the window needs to reach into
/// directly: to refresh the log view when the page is shown, and to drive
/// the refresh button's icon/spinner while a refresh is running.
pub struct SettingsWidgets {
    pub page: adw::NavigationPage,
    pub log_view: gtk::TextView,
    pub refresh_button: gtk::Button,
    pub refresh_stack: gtk::Stack,
    pub refresh_spinner: gtk::Spinner,
    pub rescan_button: gtk::Button,
    pub rescan_stack: gtk::Stack,
    pub rescan_spinner: gtk::Spinner,
}

/// Builds the Settings page (library folders, maintenance actions, and an
/// activity log) as a page pushed onto the main window's navigation stack.
pub fn build_page(parent: &GnosisWindow) -> SettingsWidgets {
    let folders_group = adw::PreferencesGroup::builder()
        .title("Library Folders")
        .description("Gnosis scans these folders, including subfolders, for EPUB books.")
        .build();

    let add_button = gtk::Button::from_icon_name("list-add-symbolic");
    add_button.add_css_class("flat");
    add_button.set_valign(gtk::Align::Center);
    add_button.set_tooltip_text(Some("Add Folder"));
    folders_group.set_header_suffix(Some(&add_button));

    let maintenance_group = adw::PreferencesGroup::builder()
        .title("Library Maintenance")
        .build();
    let refresh_row = adw::ActionRow::builder()
        .title("Refresh All Metadata")
        .subtitle(
            "Re-reads title, author, and cover art from every book's file; removes books whose \
             file no longer exists and cleans up duplicates.",
        )
        .build();
    let refresh_icon = gtk::Image::from_icon_name("view-refresh-symbolic");
    let refresh_spinner = gtk::Spinner::new();
    let refresh_stack = gtk::Stack::new();
    refresh_stack.add_named(&refresh_icon, Some("icon"));
    refresh_stack.add_named(&refresh_spinner, Some("spinner"));
    refresh_stack.set_visible_child_name("icon");

    let refresh_button = gtk::Button::builder().child(&refresh_stack).build();
    refresh_button.add_css_class("flat");
    refresh_button.set_valign(gtk::Align::Center);
    refresh_button.set_tooltip_text(Some("Refresh All Metadata"));
    let parent_for_refresh = parent.clone();
    refresh_button.connect_clicked(move |_| {
        parent_for_refresh.refresh_all_metadata();
    });
    refresh_row.add_suffix(&refresh_button);
    refresh_row.set_activatable_widget(Some(&refresh_button));
    maintenance_group.add(&refresh_row);

    // Distinct from "Refresh All Metadata" above: that intentionally leaves
    // series/book-number untouched (so it never clobbers a manual edit) —
    // this exists specifically to backfill series/series_index for books
    // added before series parsing worked, without touching reading
    // progress, title, author, or covers.
    let rescan_row = adw::ActionRow::builder()
        .title("Rescan Series &amp; Book Numbers")
        .subtitle(
            "Re-reads series name and book number from every book's file. Leaves reading \
             progress, title, author, and covers untouched.",
        )
        .build();
    let rescan_icon = gtk::Image::from_icon_name("view-refresh-symbolic");
    let rescan_spinner = gtk::Spinner::new();
    let rescan_stack = gtk::Stack::new();
    rescan_stack.add_named(&rescan_icon, Some("icon"));
    rescan_stack.add_named(&rescan_spinner, Some("spinner"));
    rescan_stack.set_visible_child_name("icon");

    let rescan_button = gtk::Button::builder().child(&rescan_stack).build();
    rescan_button.add_css_class("flat");
    rescan_button.set_valign(gtk::Align::Center);
    rescan_button.set_tooltip_text(Some("Rescan Series & Book Numbers"));
    let parent_for_rescan = parent.clone();
    rescan_button.connect_clicked(move |_| {
        parent_for_rescan.rescan_series();
    });
    rescan_row.add_suffix(&rescan_button);
    rescan_row.set_activatable_widget(Some(&rescan_button));
    maintenance_group.add(&rescan_row);

    let log_group = adw::PreferencesGroup::builder()
        .title("Activity Log")
        .description("What Gnosis has done in the background: scans, refreshes, and removals.")
        .build();

    let log_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .build();
    log_view.buffer().set_text(&log::read());

    let log_scroll = gtk::ScrolledWindow::builder()
        .height_request(240)
        .child(&log_view)
        .build();
    log_scroll.add_css_class("card");

    let log_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let reload_log_button = gtk::Button::from_icon_name("view-refresh-symbolic");
    reload_log_button.add_css_class("flat");
    reload_log_button.set_tooltip_text(Some("Reload Log"));
    let clear_log_button = gtk::Button::from_icon_name("edit-clear-all-symbolic");
    clear_log_button.add_css_class("flat");
    clear_log_button.set_tooltip_text(Some("Clear Log"));
    log_buttons.append(&reload_log_button);
    log_buttons.append(&clear_log_button);
    log_group.set_header_suffix(Some(&log_buttons));

    let log_view_for_reload = log_view.clone();
    reload_log_button.connect_clicked(move |_| {
        log_view_for_reload.buffer().set_text(&log::read());
    });
    let log_view_for_clear = log_view.clone();
    clear_log_button.connect_clicked(move |_| {
        log::clear();
        log_view_for_clear.buffer().set_text("");
    });

    log_group.add(&log_scroll);

    let page = adw::PreferencesPage::new();
    page.add(&folders_group);
    page.add(&maintenance_group);
    page.add(&log_group);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&adw::HeaderBar::new());
    toolbar_view.set_content(Some(&page));

    let nav_page = adw::NavigationPage::with_tag(&toolbar_view, "Settings", "settings");

    let rows: Rows = Rc::new(RefCell::new(Vec::new()));
    rebuild_rows(&folders_group, &rows, parent);

    let parent_for_add = parent.clone();
    let folders_group_for_add = folders_group.clone();
    let rows_for_add = rows.clone();
    add_button.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::builder()
            .title("Add Library Folder")
            .build();

        let group = folders_group_for_add.clone();
        let rows = rows_for_add.clone();
        let parent = parent_for_add.clone();
        let parent_for_callback = parent.clone();
        dialog.select_folder(Some(&parent), gio::Cancellable::NONE, move |result| {
            let parent = parent_for_callback;
            let Ok(folder) = result else {
                return;
            };
            let Some(path) = folder.path() else {
                return;
            };
            settings::add_folder(&path);
            rebuild_rows(&group, &rows, &parent);
            parent.scan_library_folders();
        });
    });

    SettingsWidgets {
        page: nav_page,
        log_view,
        refresh_button,
        refresh_stack,
        refresh_spinner,
        rescan_button,
        rescan_stack,
        rescan_spinner,
    }
}

fn rebuild_rows(group: &adw::PreferencesGroup, rows: &Rows, parent: &GnosisWindow) {
    for row in rows.borrow_mut().drain(..) {
        group.remove(&row);
    }

    for folder in settings::list_folders() {
        let row = adw::ActionRow::builder()
            .title(folder.to_string_lossy().to_string())
            .build();

        let remove_button = gtk::Button::from_icon_name("user-trash-symbolic");
        remove_button.add_css_class("flat");
        remove_button.set_valign(gtk::Align::Center);
        remove_button.set_tooltip_text(Some("Remove Folder"));

        let group_for_remove = group.clone();
        let rows_for_remove = rows.clone();
        let parent_for_remove = parent.clone();
        remove_button.connect_clicked(move |_| {
            settings::remove_folder(&folder);
            rebuild_rows(&group_for_remove, &rows_for_remove, &parent_for_remove);
            parent_for_remove.update_empty_state();
        });

        row.add_suffix(&remove_button);
        group.add(&row);
        rows.borrow_mut().push(row);
    }
}
