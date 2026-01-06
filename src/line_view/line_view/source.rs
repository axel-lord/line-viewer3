use ::core::cell::RefCell;
use ::std::{
    borrow::Cow,
    io::BufReader,
    path::Path,
    rc::Rc,
    sync::{Arc, RwLock},
};

use tap::Pipe;

use crate::line_view::{
    Directive, PathSet, Result, cmd, escape_path,
    line_view::{
        Cmd,
        directive_reader::DirectiveReader,
        directive_source::{DirectiveSource, DirectiveStream},
        line_map::DirectiveMapperChain,
    },
    path_ext::PathExt as _,
    provide,
};

type ParseResult<T> = core::result::Result<T, Cow<'static, str>>;

#[derive(Debug, Default)]
pub enum Watch {
    Watching {
        occured: Vec<String>,
    },
    #[default]
    Sleeping,
}

impl Watch {
    pub fn watch(&mut self) {
        if self.is_sleeping() {
            *self = Self::Watching {
                occured: Vec::new(),
            }
        }
    }
    pub fn sleep(&mut self) {
        if self.is_watching() {
            *self = Self::Sleeping
        }
    }
    pub const fn is_sleeping(&self) -> bool {
        matches!(self, Watch::Sleeping)
    }
    pub const fn is_watching(&self) -> bool {
        matches!(self, Watch::Watching { .. })
    }
}

#[derive(Debug)]
pub struct Source {
    pub read: DirectiveStream,
    pub path: Arc<str>,
    pub cmd: cmd::Handle,
    pub sourced: Arc<RwLock<PathSet>>,
    pub dir: Arc<str>,
    pub warning_watcher: Rc<RefCell<Watch>>,
    pub line_map: Option<DirectiveMapperChain>,
}

impl Source {
    pub fn new(path: Arc<str>, cmd_directory: &mut cmd::Directory<Cmd>) -> Self {
        Self {
            read: DirectiveStream::new(NullReader),
            dir: {
                let mut dir = AsRef::<Path>::as_ref(path.as_ref()).to_path_buf();
                dir.pop();
                dir.to_string_lossy().into()
            },
            path,
            sourced: Default::default(),
            cmd: cmd_directory.new_handle(),
            warning_watcher: Default::default(),
            line_map: None,
        }
    }

    pub fn shallow(&self) -> Self {
        Self {
            read: DirectiveStream::new(NullReader),
            path: self.path.clone(),
            cmd: self.cmd,
            sourced: self.sourced.clone(),
            dir: self.dir.clone(),
            warning_watcher: self.warning_watcher.clone(),
            line_map: self.line_map.clone(),
        }
    }

    pub fn open(
        path: Arc<str>,
        cmd_directory: &mut cmd::Directory<Cmd>,
        provider: impl provide::Read,
    ) -> Result<Self> {
        Ok(Source {
            read: provider
                .provide(path.as_ref())?
                .pipe(BufReader::new)
                .pipe(DirectiveReader::new)
                .pipe(DirectiveStream::new),
            ..Source::new(path, cmd_directory)
        })
    }

    pub fn parse(
        line: &str,
        dir: &str,
        cmd_directory: &mut cmd::Directory<Cmd>,
        provider: impl provide::Read,
    ) -> ParseResult<Self> {
        let line = escape_path(line)?;

        let path = line.canonicalize_at(dir.as_ref()).map_err(|err| {
            Cow::Owned(format!(
                "could not canonicalize path, {}, {err}",
                line.display()
            ))
        })?;

        if !path.exists() {
            // non canonicalized is uded when printing
            return Err(Cow::from(format!("could not find {}", line.display())));
        }

        Source::open(path.to_string_lossy().into(), cmd_directory, provider)
            .map_err(|err| Cow::from(format!("could not create source, {err}")))
    }
}

#[derive(Clone, Copy, Debug)]
struct NullReader;

impl DirectiveSource for NullReader {
    fn read(&mut self) -> Result<(usize, Directive<'_>)> {
        Ok((0, Directive::Noop))
    }
}
