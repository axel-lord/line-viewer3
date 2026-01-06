use ::std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use crate::line_view::{line_view::line::Source as LineSource, Error, Result};

#[derive(Debug, Clone, Default)]
pub struct Cmd {
    exe: Option<PathBuf>,
    arg: Vec<String>,
}

impl Cmd {
    pub fn exe(&mut self, exe: PathBuf) -> &mut Self {
        self.exe = Some(exe);
        self
    }

    pub fn arg(&mut self, arg: String) -> &mut Self {
        self.arg.push(arg);
        self
    }

    pub const fn is_empty(&self) -> bool {
        self.exe.is_none()
    }

    pub fn execute(
        &self,
        line_nr: usize,
        line_src: LineSource,
        params: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result {
        let Some(exe) = &self.exe else { return Ok(()) };

        let args = self
            .arg
            .iter()
            .map(String::from)
            .chain(params.into_iter().map(|param| param.into()))
            .collect::<Vec<String>>();

        ::std::process::Command::new(exe)
            .env("LINE_VIEW_LINE_NR", line_nr.to_string())
            .env("LINE_VIEW_LINE_SRC", line_src.to_string())
            .args(&args)
            .spawn()
            .map_err(|err| Error::Spawn {
                err,
                program: exe.display().to_string(),
                args,
            })?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(usize, usize);

#[derive(Debug, Clone)]
pub struct Directory<T> {
    contents: Vec<BTreeMap<usize, T>>,
}

impl Directory<Cmd> {
    pub const fn new() -> Self {
        Self {
            contents: Vec::new(),
        }
    }

    pub fn map_to_arc(self) -> Directory<Arc<Cmd>> {
        Directory {
            contents: self
                .contents
                .into_iter()
                .map(|sub| {
                    sub.into_iter()
                        .map(|(key, value)| (key, Arc::from(value)))
                        .collect()
                })
                .collect(),
        }
    }

    pub fn new_handle(&mut self) -> Handle {
        self.contents.push({
            let mut btree = BTreeMap::new();
            btree.insert(0, Cmd::default());
            btree
        });
        Handle(self.contents.len() - 1, 0)
    }

    pub fn select_command(&mut self, handle: Handle, index: usize) -> Handle {
        self.contents[handle.0].entry(index).or_default();
        Handle(handle.0, index)
    }
}

impl Default for Directory<Cmd> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Directory<T> {
    pub fn get(&self, handle: Handle) -> Option<&T> {
        self.contents.get(handle.0)?.get(&handle.1)
    }

    pub fn get_mut(&mut self, handle: Handle) -> Option<&mut T> {
        self.contents.get_mut(handle.0)?.get_mut(&handle.1)
    }
}

impl<T> ::core::ops::Index<Handle> for Directory<T> {
    type Output = T;

    fn index(&self, index: Handle) -> &Self::Output {
        self.get(index).unwrap()
    }
}

impl<T> ::core::ops::IndexMut<Handle> for Directory<T> {
    fn index_mut(&mut self, index: Handle) -> &mut Self::Output {
        self.get_mut(index).unwrap()
    }
}
