use ::std::{
    borrow::Cow,
    path::Path,
    sync::{Arc, RwLock},
};

use crate::line_view::{
    Cmd, Directive, PathSet, cmd,
    line_view::{line_map::DirectiveMapperChain, source::Source},
    provide,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportKind {
    Source,
    Import,
    Lines,
}

#[derive(Debug, Clone)]
pub struct Import<'line> {
    file: Cow<'line, str>,
    kind: ImportKind,
}

impl<'line> Import<'line> {
    pub fn new_import(path: &'line str) -> Self {
        Self {
            file: path.into(),
            kind: ImportKind::Import,
        }
    }
    pub fn new_source(path: &'line str) -> Self {
        Self {
            file: path.into(),
            kind: ImportKind::Source,
        }
    }
    pub fn new_lines(path: &'line str) -> Self {
        Self {
            file: path.into(),
            kind: ImportKind::Lines,
        }
    }

    pub fn perform_import(
        self,
        parent: Source,
        imported: &mut PathSet,
        cmd_directory: &mut cmd::Directory<Cmd>,
        provider: impl provide::Read,
        home: Option<&Path>,
    ) -> ::core::result::Result<Source, Directive<'static>> {
        let Self { file, kind } = self;
        match kind {
            ImportKind::Source => source(
                &file,
                parent.dir,
                parent.cmd,
                parent.sourced,
                cmd_directory,
                provider,
                home,
            ),
            ImportKind::Import => {
                import(&file, parent.dir, imported, cmd_directory, provider, home)
            }
            ImportKind::Lines => {
                lines(&file, parent.dir, parent.cmd, cmd_directory, provider, home)
            }
        }
        .ok_or_else(|| Directive::Warning(format!("could not source/import/lines {file}").into()))
    }
}

fn import(
    line: &str,
    dir: Arc<str>,
    imported: &mut PathSet,
    cmd_directory: &mut cmd::Directory<Cmd>,
    provider: impl provide::Read,
    home: Option<&Path>,
) -> Option<Source> {
    let source = match Source::parse(line, &dir, cmd_directory, provider, home) {
        Ok(source) => source,
        Err(err) => {
            ::log::error!("\n{err}");
            return None;
        }
    };

    if let Some(path) = &source.path {
        // prevent cycles
        if imported.contains(path) {
            return None;
        }

        imported.insert(Arc::clone(path));
    }

    Some(source)
}

fn source(
    line: &str,
    dir: Arc<str>,
    cmd: cmd::Handle,
    sourced: Arc<RwLock<PathSet>>,
    cmd_directory: &mut cmd::Directory<Cmd>,
    provider: impl provide::Read,
    home: Option<&Path>,
) -> Option<Source> {
    let source = match Source::parse(line, &dir, cmd_directory, provider, home) {
        Ok(source) => Source {
            // sources gain source context of parent, while imports get their own
            sourced: Arc::clone(&sourced),
            // sourced content keep command of parent
            cmd,
            // all of these are created for the source and not inherited
            read: source.read,
            path: source.path,
            dir: source.dir,
            line_map: source.line_map,
            warning_watcher: source.warning_watcher,
        },
        Err(err) => {
            ::log::error!("\n{err}");
            return None;
        }
    };

    let mut sourced = sourced.write().unwrap();

    if let Some(path) = &source.path {
        // skip if already sourced in this context
        if sourced.contains(path) {
            return None;
        }

        sourced.insert(Arc::clone(path));
    }
    Some(source)
}

fn skip_directives(parsed: Directive<'_>) -> Directive<'_> {
    match parsed {
        directive @ (Directive::Close | Directive::Empty | Directive::Text(..)) => directive,
        _ => Directive::Noop,
    }
}

fn lines(
    line: &str,
    dir: Arc<str>,
    cmd: cmd::Handle,
    cmd_directory: &mut cmd::Directory<Cmd>,
    provider: impl provide::Read,
    home: Option<&Path>,
) -> Option<Source> {
    // lines can be sourced however much is wanted since they cannot create cycles
    match Source::parse(line, &dir, cmd_directory, provider, home) {
        Ok(source) => Some(Source {
            // lines inherit command from parent
            cmd,
            // the special part about lines
            line_map: Some(DirectiveMapperChain::new(skip_directives, None, true)),
            // all of these are newly created and not inherited
            read: source.read,
            path: source.path,
            sourced: source.sourced,
            dir: source.dir,
            warning_watcher: source.warning_watcher,
        }),
        Err(err) => {
            ::log::error!("\n{err}");
            None
        }
    }
}
