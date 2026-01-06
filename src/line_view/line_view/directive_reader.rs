use std::{fmt::Debug, io::BufRead};

use crate::line_view::directive_source::DirectiveSource;
use crate::{Directive, Result};

#[derive(Debug)]
pub struct DirectiveReader<R>(R, usize, String);

impl<R> DirectiveReader<R>
where
    R: BufRead,
{
    pub fn new(read: R) -> Self {
        Self(read, 0, String::new())
    }
}

impl<R> DirectiveSource for DirectiveReader<R>
where
    R: Debug + BufRead,
{
    fn read(&mut self) -> Result<(usize, Directive<'_>)> {
        let Self(read, pos, buf) = self;

        let pos = {
            *pos += 1;
            *pos - 1
        };

        buf.clear();
        if read.read_line(buf)? == 0 {
            return Ok((pos, Directive::Close));
        }

        Ok((pos, Directive::parse_line(buf)))
    }
}
