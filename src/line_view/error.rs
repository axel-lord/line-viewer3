use std::fmt::Display;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to spawn {} with args |{}|, {}", program, ArgProxy(args), err)]
    Spawn {
        err: std::io::Error,
        program: String,
        args: Vec<String>,
    },
}

struct ArgProxy<'a>(&'a Vec<String>);

impl Display for ArgProxy<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut i = self.0.iter();

        if let Some(arg) = i.next() {
            write!(f, "{arg}")?;
        }

        for arg in i {
            write!(f, ", {arg}")?;
        }

        Ok(())
    }
}
