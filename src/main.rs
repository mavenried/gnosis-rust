mod application;
mod library;
mod ui;

use gtk::glib::ExitCode;
use gtk::prelude::*;

use application::GnosisApplication;

fn main() -> ExitCode {
    GnosisApplication::new().run()
}
