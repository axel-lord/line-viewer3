//! Application to view and execute commands using lines.

use ::clap::Parser;
use ::line_viewer3::cli::{Action, Cli};
use ::log::LevelFilter;

fn main() -> ::color_eyre::Result<()> {
    let cli = Cli::parse();
    ::color_eyre::install()?;
    ::env_logger::builder()
        .filter_level(LevelFilter::Info)
        .init();

    match Action::from(cli) {
        Action::Completions(completions) => completions.generate(),
        Action::MimeType(mime_type) => mime_type.write(),
        Action::Application(application) => application.generate(),
        Action::Open(open) => Ok(()),
        Action::Print(print) => Ok(()),
    }
}
