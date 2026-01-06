use crate::line_view::Result;

pub trait Read {
    type BufRead: ::std::io::BufRead + ::core::fmt::Debug + 'static;

    fn provide(&self, from: &str) -> Result<Self::BufRead>;
}

impl<P> self::Read for &P
where
    P: self::Read,
{
    type BufRead = P::BufRead;

    fn provide(&self, from: &str) -> Result<Self::BufRead> {
        (*self).provide(from)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PathReadProvider;
impl self::Read for PathReadProvider {
    type BufRead = ::std::io::BufReader<std::fs::File>;

    fn provide(&self, from: &str) -> Result<Self::BufRead> {
        Ok(std::io::BufReader::new(std::fs::File::open(from)?))
    }
}
