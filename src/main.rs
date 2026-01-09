//! Application to view and execute commands using lines.

use ::std::path::PathBuf;

use ::clap::Parser;
use ::line_viewer3::{
    cli::{Action, Cli, Open},
    ui,
};
use ::log::LevelFilter;
use ::mimalloc::MiMalloc;
use ::tap::Conv;

/// Use mimalloc as global allocator
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> ::color_eyre::Result<()> {
    let cli = if let Some(path) = ::std::env::args_os().next().map(PathBuf::from)
        && let Some(name) = path.file_name()
        && let Some(name) = name.to_str()
        && name == "line-viewer"
    {
        Open::parse().conv::<Action>().conv::<Cli>()
    } else {
        Cli::parse()
    };
    ::color_eyre::install()?;
    ::env_logger::builder()
        .filter_module("line_viewer3", LevelFilter::Info)
        .init();

    match Action::from(cli) {
        Action::Completions(completions) => completions.generate(),
        Action::MimeType(mime_type) => mime_type.write(),
        Action::Application(application) => application.generate(),
        Action::Print(print) => print.print(),
        Action::Open(open) => ui::run(open),
        Action::Daemon(daemon) => ui::run_daemon(daemon),
    }
}
