use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::library::ReadingStatus;

use super::book_object::BookObject;

const COVER_WIDTH: i32 = 120;
const COVER_SLOT_HEIGHT: i32 = 216;
const PROGRESS_PIE_SIZE: i32 = 18;
const MARQUEE_TICK: Duration = Duration::from_millis(16);
const MARQUEE_STEP: f64 = 1.0;
const MARQUEE_PAUSE_TICKS: u32 = 45;

#[derive(Clone, Copy, Debug, PartialEq)]
enum MarqueePhase {
    Forward,
    PauseAtEnd(u32),
    Backward,
    PauseAtStart(u32),
}

fn marquee_tick(phase: MarqueePhase, value: f64, max: f64, step: f64, pause_ticks: u32) -> (MarqueePhase, f64) {
    match phase {
        MarqueePhase::Forward => {
            let next = value + step;
            if next >= max {
                (MarqueePhase::PauseAtEnd(pause_ticks), max)
            } else {
                (MarqueePhase::Forward, next)
            }
        }
        MarqueePhase::PauseAtEnd(1) => (MarqueePhase::Backward, value),
        MarqueePhase::PauseAtEnd(remaining) => (MarqueePhase::PauseAtEnd(remaining - 1), value),
        MarqueePhase::Backward => {
            let next = value - step;
            if next <= 0.0 {
                (MarqueePhase::PauseAtStart(pause_ticks), 0.0)
            } else {
                (MarqueePhase::Backward, next)
            }
        }
        MarqueePhase::PauseAtStart(1) => (MarqueePhase::Forward, value),
        MarqueePhase::PauseAtStart(remaining) => (MarqueePhase::PauseAtStart(remaining - 1), value),
    }
}

pub fn setup(list_item: &gtk::ListItem) {
    let picture = gtk::Picture::builder()
        .width_request(COVER_WIDTH)
        .content_fit(gtk::ContentFit::Contain)
        .valign(gtk::Align::End)
        .build();
    picture.add_css_class("card");

    let progress_pie = gtk::DrawingArea::builder()
        .width_request(PROGRESS_PIE_SIZE)
        .height_request(PROGRESS_PIE_SIZE)
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .margin_end(6)
        .margin_bottom(6)
        .visible(false)
        .build();

    let cover_overlay = gtk::Overlay::new();
    cover_overlay.set_halign(gtk::Align::Center);
    cover_overlay.set_height_request(COVER_SLOT_HEIGHT);
    cover_overlay.set_child(Some(&picture));
    cover_overlay.add_overlay(&progress_pie);

    let title_label = gtk::Label::builder()
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .xalign(0.0)
        .css_classes(["heading", "gnosis-card-title"])
        .build();

    let title_scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .min_content_width(COVER_WIDTH)
        .max_content_width(COVER_WIDTH)
        .propagate_natural_height(true)
        .halign(gtk::Align::Center)
        .child(&title_label)
        .build();

    let marquee_timer: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));

    let motion = gtk::EventControllerMotion::new();
    let label_for_enter = title_label.clone();
    let scroller_for_enter = title_scroller.clone();
    let timer_for_enter = marquee_timer.clone();
    motion.connect_enter(move |_, _, _| {
        if !label_for_enter.layout().is_ellipsized() {
            return;
        }
        label_for_enter.set_ellipsize(gtk::pango::EllipsizeMode::None);

        let scroller = scroller_for_enter.clone();
        let timer = timer_for_enter.clone();
        glib::idle_add_local_once(move || {
            if let Some(id) = timer.take() {
                id.remove();
            }
            let adj = scroller.hadjustment();
            let max = (adj.upper() - adj.page_size()).max(0.0);
            if max <= 0.0 {
                return;
            }
            adj.set_value(0.0);
            let adj_tick = adj.clone();
            let phase = Cell::new(MarqueePhase::Forward);
            let id = glib::timeout_add_local(MARQUEE_TICK, move || {
                let max = (adj_tick.upper() - adj_tick.page_size()).max(0.0);
                let (next_phase, next_value) = marquee_tick(
                    phase.get(),
                    adj_tick.value(),
                    max,
                    MARQUEE_STEP,
                    MARQUEE_PAUSE_TICKS,
                );
                phase.set(next_phase);
                adj_tick.set_value(next_value);
                glib::ControlFlow::Continue
            });
            timer.set(Some(id));
        });
    });
    let label_for_leave = title_label.clone();
    let scroller_for_leave = title_scroller.clone();
    let timer_for_leave = marquee_timer.clone();
    motion.connect_leave(move |_| {
        if let Some(id) = timer_for_leave.take() {
            id.remove();
        }
        label_for_leave.set_ellipsize(gtk::pango::EllipsizeMode::End);
        scroller_for_leave.hadjustment().set_value(0.0);
    });
    title_scroller.add_controller(motion);

    let author_label = gtk::Label::builder()
        .css_classes(["caption", "dim-label"])
        .width_request(COVER_WIDTH)
        .halign(gtk::Align::Center)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();

    let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
    card.set_margin_top(6);
    card.set_margin_bottom(6);
    card.set_margin_start(6);
    card.set_margin_end(6);
    card.append(&cover_overlay);
    card.append(&title_scroller);
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

    let cover_overlay = card
        .first_child()
        .and_downcast::<gtk::Overlay>()
        .expect("cover overlay is first child");
    let title_scroller = cover_overlay
        .next_sibling()
        .and_downcast::<gtk::ScrolledWindow>()
        .expect("title scroller follows cover overlay");
    let title_label = title_scroller
        .child()
        .and_downcast::<gtk::Viewport>()
        .and_then(|viewport| viewport.child())
        .and_downcast::<gtk::Label>()
        .expect("title label is the scroller's viewport's child");
    let author_label = title_scroller
        .next_sibling()
        .and_downcast::<gtk::Label>()
        .expect("author label follows title scroller");

    let picture = cover_overlay
        .first_child()
        .and_downcast::<gtk::Picture>()
        .expect("picture is the overlay's main child");
    let progress_pie = picture
        .next_sibling()
        .and_downcast::<gtk::DrawingArea>()
        .expect("progress pie follows picture as an overlay child");

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

    title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title_scroller.hadjustment().set_value(0.0);

    title_label.set_label(&book.title);
    author_label.set_label(book.author.as_deref().unwrap_or(""));
    author_label.set_visible(book.author.is_some());

    let is_reading = book.reading_status() == ReadingStatus::Reading;
    progress_pie.set_visible(is_reading);
    if is_reading {
        let fraction = book.progress.clamp(0.0, 1.0);
        progress_pie.set_draw_func(move |_, cr, width, height| {
            draw_progress_pie(cr, width, height, fraction);
        });
    }
}

fn draw_progress_pie(cr: &gtk::cairo::Context, width: i32, height: i32, fraction: f64) {
    let (w, h) = (f64::from(width), f64::from(height));
    let (cx, cy) = (w / 2.0, h / 2.0);
    let radius = w.min(h) / 2.0;

    cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
    cr.set_source_rgba(0.35, 0.35, 0.37, 0.92);
    let _ = cr.fill();

    let start = -std::f64::consts::FRAC_PI_2;
    let end = start + fraction * std::f64::consts::TAU;
    cr.move_to(cx, cy);
    cr.arc(cx, cy, radius - 2.5, start, end);
    cr.line_to(cx, cy);
    cr.close_path();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
    let _ = cr.fill();
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

