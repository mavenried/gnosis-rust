use adw::prelude::*;

use crate::library::Book;

use super::window::{GnosisWindow, WriteBackMode};

pub fn present(parent: &GnosisWindow, book: Book) {
    let window = adw::Window::builder()
        .transient_for(parent)
        .modal(true)
        .default_width(420)
        .title("Edit Metadata")
        .build();

    let title_row = adw::EntryRow::builder()
        .title("Title")
        .text(book.title.as_str())
        .build();
    let author_row = adw::EntryRow::builder()
        .title("Author")
        .text(book.author.as_deref().unwrap_or(""))
        .build();
    let series_row = adw::EntryRow::builder()
        .title("Series")
        .text(book.series.as_deref().unwrap_or(""))
        .build();
    let series_index_row = adw::EntryRow::builder()
        .title("Series Number")
        .text(book.series_index.map(|n| n.to_string()).unwrap_or_default())
        .build();

    let group = adw::PreferencesGroup::new();
    group.add(&title_row);
    group.add(&author_row);
    group.add(&series_row);
    group.add(&series_index_row);

    let write_back_options = gtk::StringList::new(&[
        "Don't touch the EPUB file",
        "Save a copy with updated metadata",
        "Overwrite the original EPUB file",
    ]);
    let write_back_row = adw::ComboRow::builder()
        .title("Write Back to File")
        .subtitle("A copy is saved alongside the original; overwriting replaces it in place.")
        .model(&write_back_options)
        .selected(0)
        .build();
    let write_back_group = adw::PreferencesGroup::new();
    write_back_group.add(&write_back_row);

    let page = adw::PreferencesPage::new();
    page.add(&group);
    page.add(&write_back_group);

    let cancel_button = gtk::Button::with_label("Cancel");
    let save_button = gtk::Button::with_label("Save");
    save_button.add_css_class("suggested-action");

    let header_bar = adw::HeaderBar::new();
    header_bar.set_show_start_title_buttons(false);
    header_bar.set_show_end_title_buttons(false);
    header_bar.set_title_widget(Some(&adw::WindowTitle::new("Edit Metadata", "")));
    header_bar.pack_start(&cancel_button);
    header_bar.pack_end(&save_button);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.set_content(Some(&page));
    window.set_content(Some(&toolbar_view));

    let window_weak = window.downgrade();
    cancel_button.connect_clicked(move |_| {
        if let Some(window) = window_weak.upgrade() {
            window.close();
        }
    });

    let window_weak = window.downgrade();
    let parent = parent.clone();
    let book_id = book.id;
    save_button.connect_clicked(move |_| {
        let title = title_row.text().trim().to_string();
        if title.is_empty() {
            return;
        }
        let author = non_empty(author_row.text().as_str());
        let series = non_empty(series_row.text().as_str());
        let series_index = series_index_row.text().trim().parse::<f64>().ok();
        let write_back = match write_back_row.selected() {
            1 => WriteBackMode::Copy,
            2 => WriteBackMode::InPlace,
            _ => WriteBackMode::None,
        };

        parent.update_book_metadata(book_id, title, author, series, series_index, write_back);

        if let Some(window) = window_weak.upgrade() {
            window.close();
        }
    });

    window.present();
}

fn non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
