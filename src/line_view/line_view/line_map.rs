use ::core::{any, fmt::Debug, ops::Deref};
use ::std::rc::Rc;

use crate::line_view::Directive;

pub trait DirectiveMapper {
    fn map<'l>(&self, line: Directive<'l>, depth: usize) -> Directive<'l>;
    fn name(&self) -> &str;
}

impl<F> DirectiveMapper for F
where
    F: Fn(Directive) -> Directive,
{
    fn map<'line>(&self, line: Directive<'line>, _: usize) -> Directive<'line> {
        self(line)
    }
    fn name(&self) -> &str {
        any::type_name::<F>()
    }
}

struct Inner<LM: ?Sized> {
    pub prev: Option<Rc<Inner<dyn DirectiveMapper>>>,
    pub automatic: bool,
    pub line_map: LM,
}

#[derive(Clone)]
pub struct DirectiveMapperChain {
    this: Rc<Inner<dyn DirectiveMapper>>,
}

impl DirectiveMapperChain {
    pub fn new<LM>(line_map: LM, prev: Option<Self>, automatic: bool) -> Self
    where
        LM: DirectiveMapper + 'static,
    {
        let this = Rc::new(Inner {
            prev: prev.map(|p| p.this),
            automatic,
            line_map,
        });

        Self { this }
    }

    pub fn prev(&self) -> Option<Self> {
        self.this
            .prev
            .as_ref()
            .map(|p| DirectiveMapperChain { this: Rc::clone(p) })
    }

    pub fn automatic(&self) -> bool {
        self.this.automatic
    }

    pub fn apply<'d>(&self, mut directive: Directive<'d>) -> Directive<'d> {
        for (depth, directive_map) in self.into_iter().enumerate() {
            directive = directive_map.map(directive, depth);
        }
        directive
    }
}

#[derive(Debug)]
pub struct Iter(Option<DirectiveMapperChain>);
impl Iterator for Iter {
    type Item = DirectiveMapperChain;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(next) = self.0.take() {
            self.0 = next.prev();
            Some(next)
        } else {
            None
        }
    }
}

impl IntoIterator for DirectiveMapperChain {
    type Item = DirectiveMapperChain;

    type IntoIter = Iter;

    fn into_iter(self) -> Self::IntoIter {
        Iter(Some(self))
    }
}

impl IntoIterator for &DirectiveMapperChain {
    type Item = DirectiveMapperChain;

    type IntoIter = Iter;

    fn into_iter(self) -> Self::IntoIter {
        self.clone().into_iter()
    }
}

impl Deref for DirectiveMapperChain {
    type Target = dyn DirectiveMapper;

    fn deref(&self) -> &Self::Target {
        &self.this.line_map
    }
}

impl AsRef<dyn DirectiveMapper + 'static> for DirectiveMapperChain {
    fn as_ref(&self) -> &(dyn DirectiveMapper + 'static) {
        &self.this.line_map
    }
}

impl Debug for DirectiveMapperChain {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.debug_struct("LineMapNode")
            .field("line_map", &self.name())
            .field("prev", &self.prev())
            .finish()
    }
}
