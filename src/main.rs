mod application;
mod library;
mod ui;

use gtk::glib::ExitCode;
use gtk::prelude::*;

use application::GnosisApplication;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("gnosis=info")),
        )
        .init();

    GnosisApplication::new().run()
}
