use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::library::ReadingStatus;

use super::book_object::BookObject;

pub fn build(filters: &gtk::EveryFilter, view_stack: &gtk::Stack) -> gtk::Widget {
    let status_state: Rc<RefCell<Option<ReadingStatus>>> = Rc::new(RefCell::new(None));
    let status_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Browse)
        .css_classes(["boxed-list"])
        .build();
    for name in ["All", "Unread", "Reading", "Read"] {
        status_list.append(&row_label(name));
    }
    status_list.select_row(status_list.row_at_index(0).as_ref());

    let status_filter = gtk::CustomFilter::new({
        let status_state = status_state.clone();
        move |obj| {
            let Some(status) = *status_state.borrow() else {
                return true;
            };
            obj.downcast_ref::<BookObject>()
                .is_some_and(|book_object| book_object.book().reading_status() == status)
        }
    });
    filters.append(status_filter.clone());

    let status_filter_for_signal = status_filter.clone();
    let view_stack_for_status = view_stack.clone();
    status_list.connect_row_selected(move |_, row| {
        let index = row.map(|r| r.index()).unwrap_or(0);
        *status_state.borrow_mut() = match index {
            1 => Some(ReadingStatus::Unread),
            2 => Some(ReadingStatus::Reading),
            3 => Some(ReadingStatus::Read),
            _ => None,
        };
        status_filter_for_signal.changed(gtk::FilterChange::Different);
        view_stack_for_status.set_visible_child_name("books");
    });

    let browse_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    browse_list.append(&row_label("Authors"));
    browse_list.append(&row_label("Series"));

    let view_stack_for_browse = view_stack.clone();
    browse_list.connect_row_activated(move |_, row| {
        let name = match row.index() {
            0 => "authors",
            1 => "series",
            _ => return,
        };
        view_stack_for_browse.set_visible_child_name(name);
    });

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&section("Status", &status_list));
    content.append(&section("Browse", &browse_list));

    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .width_request(200)
        .child(&content)
        .build()
        .upcast()
}

fn row_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build()
}

fn section(title: &str, list: &gtk::ListBox) -> gtk::Box {
    let heading = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .css_classes(["heading"])
        .build();
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 6);
    box_.append(&heading);
    box_.append(list);
    box_
}
