use std::{borrow::Cow, char};

use crate::Import;

#[derive(Debug, Clone, Default)]
pub enum Directive<'line> {
    #[default]
    Noop,
    Empty,
    Close,
    Clean,
    DisplayWarnings,
    IgnoreWarnings,
    IgnoreText,
    Watch,
    Then,
    Else,
    Debug,
    EndMap {
        automatic: bool,
    },
    Exe(Cow<'line, str>),
    Arg(Cow<'line, str>),
    Warning(Cow<'line, str>),
    Title(Cow<'line, str>),
    Subtitle(Cow<'line, str>),
    Text(Cow<'line, str>),
    Comment(Cow<'line, str>),
    Import(Import<'line>),
    Multiple(Vec<Directive<'static>>),
}

impl<'line> Directive<'line> {
    fn parse_directive_result(text: &'line str) -> Result<Self, Cow<'line, str>> {
        let mut split = text.trim_start().splitn(2, char::is_whitespace);

        let Some(directive) = split.next() else {
            return Err(format!("could not parse directive \"{text}\"").into());
        };
        let payload = split.next();

        let require_payload = move |directive| {
            payload
                .map(|payload| {
                    let payload = payload.trim();
                    payload
                        .strip_prefix('"')
                        .and_then(|payload| payload.strip_suffix('"'))
                        .unwrap_or(payload)
                })
                .ok_or_else(|| Cow::Owned(format!("directive {directive} requires an argument")))
        };

        Ok(match directive {
            "arg" => Self::Arg(require_payload("arg")?.into()),

            "exe" => Self::Exe(require_payload("exe")?.into()),

            "clean" => Self::Clean,

            "title" => Self::Title(require_payload("title")?.into()),

            "subtitle" => Self::Subtitle(require_payload("subtitle")?.into()),

            "import" => Self::Import(Import::new_import(require_payload("import")?)),

            "lines" => Self::Import(Import::new_lines(require_payload("lines")?)),

            "source" => Self::Import(Import::new_source(require_payload("source")?)),

            "warning" => Self::Warning(require_payload("warning")?.into()),

            "text" => Self::Text(require_payload("text")?.into()),

            "empty" => Self::Empty,

            "comment" => Self::Comment(require_payload("comment")?.into()),

            "close" => Self::Close,

            "end" => Self::EndMap { automatic: false },

            "ignore-warnings" => Self::IgnoreWarnings,

            "display-warnings" => Self::DisplayWarnings,

            "ignore-text" => Self::IgnoreText,

            "then" => Self::Then,

            "else" => Self::Else,

            "watch" => Self::Watch,

            "debug" => Self::Debug,

            other => {
                return Err(format!("{other} is not a directive").into());
            }
        })
    }
    pub fn parse_directive(text: &'line str) -> Self {
        match Self::parse_directive_result(text) {
            Err(warn) => Self::Warning(warn),
            Ok(directive) => directive,
        }
    }

    pub fn parse_line(text: &'line str) -> Self {
        let text = text.trim_end();
        if text.is_empty() {
            Self::Empty
        } else if let Some(directive) = text.strip_prefix("#-") {
            Directive::parse_directive(directive.trim_end())
        } else if text.starts_with("##") {
            Self::Text(Cow::Borrowed(&text[1..]))
        } else if let Some(text) = text.strip_prefix('#') {
            Self::Comment(text.trim_start().into())
        } else {
            Self::Text(text.into())
        }
    }
}
