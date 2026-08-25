use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk;
use gtk::gio;

use super::book_card;
use super::book_object::BookObject;
use super::collection_object::CollectionKind;
use super::window::GnosisWindow;

pub struct CollectionDetailWidgets {
    pub page: adw::NavigationPage,
    title_widget: adw::WindowTitle,
    cover: gtk::Picture,
    state: Rc<RefCell<(CollectionKind, String)>>,
    filter: gtk::CustomFilter,
    sorter: gtk::CustomSorter,
}

const COVER_SIZE: i32 = 160;

impl CollectionDetailWidgets {
    pub fn configure(&self, kind: CollectionKind, name: &str, cover_path: Option<&Path>) {
        *self.state.borrow_mut() = (kind, name.to_string());
        self.title_widget.set_title(name);
        match cover_path.and_then(|path| square_cover_texture(path, COVER_SIZE)) {
            Some(texture) => {
                self.cover.set_paintable(Some(&texture));
                self.cover.remove_css_class("no-cover");
            }
            None => {
                self.cover.set_paintable(gdk::Paintable::NONE);
                self.cover.add_css_class("no-cover");
            }
        }
        self.filter.changed(gtk::FilterChange::Different);
        self.sorter.changed(gtk::SorterChange::Different);
    }
}

fn square_cover_texture(path: &Path, size: i32) -> Option<gdk::Texture> {
    let pixbuf = gdk_pixbuf::Pixbuf::from_file(path).ok()?;
    let (width, height) = (pixbuf.width(), pixbuf.height());
    if width <= 0 || height <= 0 {
        return None;
    }

    let scale = f64::from(size) / f64::from(width.min(height));
    let scaled_width = ((f64::from(width) * scale).round() as i32).max(size);
    let scaled_height = ((f64::from(height) * scale).round() as i32).max(size);
    let scaled = pixbuf.scale_simple(
        scaled_width,
        scaled_height,
        gdk_pixbuf::InterpType::Bilinear,
    )?;

    let x = (scaled_width - size) / 2;
    let y = (scaled_height - size) / 2;
    let cropped = scaled.new_subpixbuf(x, y, size, size);
    Some(gdk::Texture::for_pixbuf(&cropped))
}

pub fn build(store: &gio::ListStore, parent: &GnosisWindow) -> CollectionDetailWidgets {
    let state: Rc<RefCell<(CollectionKind, String)>> =
        Rc::new(RefCell::new((CollectionKind::Author, String::new())));

    let filter = gtk::CustomFilter::new({
        let state = state.clone();
        move |obj| {
            let Some(book_object) = obj.downcast_ref::<BookObject>() else {
                return false;
            };
            let (kind, name) = &*state.borrow();
            let book = book_object.book();
            match kind {
                CollectionKind::Author => book.author.as_deref() == Some(name.as_str()),
                CollectionKind::Series => book.series.as_deref() == Some(name.as_str()),
            }
        }
    });
    let filter_model = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));

    let sorter = gtk::CustomSorter::new({
        let state = state.clone();
        move |a, b| {
            let (Some(a), Some(b)) = (
                a.downcast_ref::<BookObject>().map(BookObject::book),
                b.downcast_ref::<BookObject>().map(BookObject::book),
            ) else {
                return gtk::Ordering::Equal;
            };
            let (kind, _) = &*state.borrow();
            let ordering = match kind {
                CollectionKind::Series => a
                    .series_index
                    .partial_cmp(&b.series_index)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
                CollectionKind::Author => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            };
            match ordering {
                std::cmp::Ordering::Less => gtk::Ordering::Smaller,
                std::cmp::Ordering::Equal => gtk::Ordering::Equal,
                std::cmp::Ordering::Greater => gtk::Ordering::Larger,
            }
        }
    });

    let sorted_model = gtk::SortListModel::new(Some(filter_model), Some(sorter.clone()));
    let selection_model = gtk::NoSelection::new(Some(sorted_model));

    let grid_view = gtk::GridView::new(Some(selection_model), Some(book_card::factory()));
    grid_view.set_single_click_activate(false);
    grid_view.set_min_columns(2);
    grid_view.set_max_columns(64);

    let parent_weak = parent.downgrade();
    grid_view.connect_activate(move |grid_view, position| {
        let Some(parent) = parent_weak.upgrade() else {
            return;
        };
        let Some(model) = grid_view.model() else {
            return;
        };
        let Some(book_object) = model.item(position).and_downcast::<BookObject>() else {
            return;
        };
        parent.open_reader(&book_object.book());
    });

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&grid_view)
        .build();

    let cover = gtk::Picture::builder()
        .width_request(COVER_SIZE)
        .height_request(COVER_SIZE)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Start)
        .hexpand(false)
        .vexpand(false)
        .margin_top(12)
        .build();
    cover.add_css_class("card");

    let title_widget = adw::WindowTitle::new("", "");

    let header_bar = adw::HeaderBar::new();
    header_bar.set_title_widget(Some(&title_widget));

    let cover_bar = gtk::CenterBox::new();
    cover_bar.set_center_widget(Some(&cover));
    cover_bar.set_margin_top(12);
    cover_bar.set_margin_bottom(12);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.add_top_bar(&cover_bar);
    toolbar_view.set_content(Some(&scrolled));

    let page = adw::NavigationPage::with_tag(&toolbar_view, "Collection", "collection-detail");

    CollectionDetailWidgets {
        page,
        title_widget,
        cover,
        state,
        filter,
        sorter,
    }
}

