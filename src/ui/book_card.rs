use gtk::gio;
use gtk::prelude::*;

use super::book_object::BookObject;

const COVER_WIDTH: i32 = 120;
const COVER_HEIGHT: i32 = 180;

pub fn setup(list_item: &gtk::ListItem) {
    // GtkGridView lays its cells out on a uniform grid, so the cover slot
    // needs a fixed size; ContentFit::Cover crops each image to fill that
    // slot edge-to-edge, matching Foliate's shelf.
    let picture = gtk::Picture::builder()
        .width_request(COVER_WIDTH)
        .height_request(COVER_HEIGHT)
        .content_fit(gtk::ContentFit::Cover)
        .build();
    picture.add_css_class("card");

    let title_label = gtk::Label::builder()
        .wrap(true)
        .lines(2)
        .max_width_chars(16)
        .justify(gtk::Justification::Center)
        .css_classes(["heading"])
        .width_request(COVER_WIDTH)
        .build();

    let author_label = gtk::Label::builder()
        .css_classes(["caption", "dim-label"])
        .width_request(COVER_WIDTH)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();

    let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
    card.set_margin_top(6);
    card.set_margin_bottom(6);
    card.set_margin_start(6);
    card.set_margin_end(6);
    card.append(&picture);
    card.append(&title_label);
    card.append(&author_label);

    let popover = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
    popover.set_parent(&card);
    popover.set_has_arrow(true);

    let right_click = gtk::GestureClick::new();
    right_click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let list_item_for_click = list_item.clone();
    let popover_for_click = popover.clone();
    right_click.connect_released(move |gesture, _n_press, x, y| {
        let Some(book_object) = list_item_for_click.item().and_downcast::<BookObject>() else {
            return;
        };
        let book_id = book_object.book().id.to_string();

        let menu = gio::Menu::new();
        menu.append(
            Some("Edit Metadata…"),
            Some(format!("win.edit-book('{book_id}')").as_str()),
        );
        menu.append(
            Some("Remove from Library"),
            Some(format!("win.remove-book('{book_id}')").as_str()),
        );
        popover_for_click.set_menu_model(Some(&menu));

        popover_for_click
            .set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover_for_click.popup();
        gesture.set_state(gtk::EventSequenceState::Claimed);
    });
    card.add_controller(right_click);

    list_item.set_child(Some(&card));
}

pub fn bind(list_item: &gtk::ListItem) {
    let Some(book_object) = list_item.item().and_downcast::<BookObject>() else {
        return;
    };
    let Some(card) = list_item.child().and_downcast::<gtk::Box>() else {
        return;
    };

    let book = book_object.book();

    let picture = card
        .first_child()
        .and_downcast::<gtk::Picture>()
        .expect("picture is first child");
    let title_label = picture
        .next_sibling()
        .and_downcast::<gtk::Label>()
        .expect("title label follows picture");
    let author_label = title_label
        .next_sibling()
        .and_downcast::<gtk::Label>()
        .expect("author label follows title");

    match &book.cover_path {
        Some(path) => {
            picture.set_file(Some(&gio::File::for_path(path)));
            picture.remove_css_class("no-cover");
        }
        None => {
            picture.set_file(None::<&gio::File>);
            picture.add_css_class("no-cover");
        }
    }

    title_label.set_label(&book.title);
    author_label.set_label(book.author.as_deref().unwrap_or(""));
    author_label.set_visible(book.author.is_some());
}

pub fn factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        setup(list_item);
    });
    factory.connect_bind(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        bind(list_item);
    });
    factory
}
