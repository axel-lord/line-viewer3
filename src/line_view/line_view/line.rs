use ::std::{sync::Arc};
use ::core::fmt::Display;

use crate::line_view::{cmd, Cmd, Result};

#[derive(Debug, Clone, Copy, Default)]
enum Kind {
    #[default]
    Default,
    Title,
    Warning,
}

#[derive(Debug, Clone)]
pub enum Source {
    File(Arc<str>),
}

impl Display for Source {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        match self {
            Source::File(src) => write!(f, "FILE:{}", src),
        }
    }
}

impl From<Arc<str>> for Source {
    fn from(value: Arc<str>) -> Self {
        Self::File(value)
    }
}

impl From<&Arc<str>> for Source {
    fn from(value: &Arc<str>) -> Self {
        Self::File(Arc::clone(value))
    }
}

#[derive(Debug, Clone)]
pub struct Builder<T, P> {
    source: T,
    position: P,
    text: String,
    cmd: Option<cmd::Handle>,
    kind: Kind,
}

impl Builder<(), ()> {
    pub fn new() -> Self {
        Builder {
            source: (),
            position: (),
            text: String::new(),
            cmd: None,
            kind: Kind::default(),
        }
    }
}

impl<T, P> Builder<T, P> {
    pub fn source(self, source: Source) -> Builder<Source, P> {
        let Self {
            position,
            text,
            cmd,
            kind,
            ..
        } = self;
        Builder {
            source,
            position,
            text,
            cmd,
            kind,
        }
    }

    pub fn position(self, position: usize) -> Builder<T, usize> {
        let Self {
            source,
            text,
            cmd,
            kind,
            ..
        } = self;
        Builder {
            source,
            position,
            text,
            cmd,
            kind,
        }
    }

    pub fn text(self, text: String) -> Self {
        Self { text, ..self }
    }

    pub fn title(self) -> Self {
        Self {
            kind: Kind::Title,
            ..self
        }
    }

    pub fn warning(self) -> Self {
        Self {
            kind: Kind::Warning,
            ..self
        }
    }

    pub fn cmd(self, cmd: cmd::Handle) -> Self {
        Self {
            cmd: Some(cmd),
            ..self
        }
    }
}

impl Builder<Source, usize> {
    pub fn build(self, cmd_directory: &mut cmd::Directory<Cmd>) -> Line<cmd::Handle> {
        let Self {
            source,
            position,
            text,
            cmd,
            kind,
        } = self;
        Line {
            text,
            source,
            position,
            cmd: cmd.unwrap_or_else(|| cmd_directory.new_handle()),
            kind,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Line<C> {
    text: String,
    source: Source,
    position: usize,
    cmd: C,
    kind: Kind,
}

impl<C> Line<C> {
    pub const fn source(&self) -> &Source {
        &self.source
    }

    pub const fn line(&self) -> usize {
        self.position
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub const fn is_title(&self) -> bool {
        matches!(self.kind, Kind::Title)
    }

    pub const fn is_warning(&self) -> bool {
        matches!(self.kind, Kind::Warning)
    }
}

impl Line<cmd::Handle> {
    pub fn map_to_arc_cmd(self, cmd_directory: &cmd::Directory<Arc<Cmd>>) -> Line<Arc<Cmd>> {
        let Self {
            text,
            source,
            position,
            cmd,
            kind,
        } = self;
        Line::<Arc<Cmd>> {
            text,
            source,
            position,
            kind,
            cmd: cmd_directory[cmd].clone(),
        }
    }
}

impl Line<Arc<Cmd>> {
    pub fn has_command(&self) -> bool {
        !self.cmd.is_empty()
    }

    pub fn execute(&self) -> Result {
        self.cmd
            .execute(self.position, self.source.clone(), [self.text()])
    }
}
