use gtk::gio;
use gtk::prelude::*;

use super::collection_object::CollectionObject;

const TILE_SIZE: i32 = 160;

pub fn setup(list_item: &gtk::ListItem) {
    let picture = gtk::Picture::builder()
        .width_request(TILE_SIZE)
        .height_request(TILE_SIZE)
        .content_fit(gtk::ContentFit::Cover)
        .build();
    picture.add_css_class("card");

    let name_label = gtk::Label::builder()
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .justify(gtk::Justification::Center)
        .css_classes(["heading"])
        .width_request(TILE_SIZE)
        .build();

    let count_label = gtk::Label::builder()
        .css_classes(["caption", "dim-label"])
        .width_request(TILE_SIZE)
        .build();

    let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
    card.set_margin_top(6);
    card.set_margin_bottom(6);
    card.set_margin_start(6);
    card.set_margin_end(6);
    card.append(&picture);
    card.append(&name_label);
    card.append(&count_label);

    let popover = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
    popover.set_parent(&card);
    popover.set_has_arrow(true);

    let right_click = gtk::GestureClick::new();
    right_click.set_button(gtk::gdk::BUTTON_SECONDARY);
    let list_item_for_click = list_item.clone();
    let popover_for_click = popover.clone();
    right_click.connect_released(move |gesture, _n_press, x, y| {
        let Some(collection_object) = list_item_for_click.item().and_downcast::<CollectionObject>()
        else {
            return;
        };
        let data = collection_object.data();
        let target = (data.kind.as_str().to_string(), data.name.clone()).to_variant();

        let menu = gio::Menu::new();
        let set_cover_item = gio::MenuItem::new(Some("Set Cover Image\u{2026}"), None);
        set_cover_item
            .set_action_and_target_value(Some("win.set-collection-cover"), Some(&target));
        menu.append_item(&set_cover_item);

        if data.cover_path.is_some() {
            let remove_cover_item = gio::MenuItem::new(Some("Remove Custom Cover"), None);
            remove_cover_item
                .set_action_and_target_value(Some("win.remove-collection-cover"), Some(&target));
            menu.append_item(&remove_cover_item);
        }

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
    let Some(collection_object) = list_item.item().and_downcast::<CollectionObject>() else {
        return;
    };
    let Some(card) = list_item.child().and_downcast::<gtk::Box>() else {
        return;
    };

    let data = collection_object.data();

    let picture = card
        .first_child()
        .and_downcast::<gtk::Picture>()
        .expect("picture is first child");
    let name_label = picture
        .next_sibling()
        .and_downcast::<gtk::Label>()
        .expect("name label follows picture");
    let count_label = name_label
        .next_sibling()
        .and_downcast::<gtk::Label>()
        .expect("count label follows name");

    match &data.cover_path {
        Some(path) => {
            picture.set_file(Some(&gio::File::for_path(path)));
            picture.remove_css_class("no-cover");
        }
        None => {
            picture.set_file(None::<&gio::File>);
            picture.add_css_class("no-cover");
        }
    }

    name_label.set_label(&data.name);
    let noun = if data.book_count == 1 { "book" } else { "books" };
    count_label.set_label(&format!("{} {noun}", data.book_count));
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

