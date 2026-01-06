#![allow(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::module_inception,
    clippy::missing_errors_doc
)]

mod cmd;
mod directive;
mod error;
mod import;
mod line_view;
mod path_ext;

pub mod provide;

use ::std::path::Path;

pub use self::{cmd::Cmd, directive::Directive, error::Error, import::Import, line_view::LineView};

type PathSet = rustc_hash::FxHashSet<std::sync::Arc<str>>;
fn escape_path(
    line: &str,
    home: Option<&Path>,
) -> ::core::result::Result<std::path::PathBuf, &'static str> {
    const HOME_PREFIX: &str = "~/";

    Ok(match line.strip_prefix(HOME_PREFIX) {
        Some(line) if line.starts_with(HOME_PREFIX) => ::std::path::PathBuf::from(line),
        Some(line) => {
            let Some(home_dir) = home else {
                return Err("could not find user home");
            };
            home_dir.join(line)
        }
        None => ::std::path::PathBuf::from(line),
    })
}

pub type Result<T = ()> = ::core::result::Result<T, Error>;
