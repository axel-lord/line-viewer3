//! Ui implementation.

use ::std::{collections::BTreeMap, sync::Arc};

use ::color_eyre::eyre::eyre;
use ::derive_more::{Deref, DerefMut};
use ::iced::{
    Element, Font, Length::Fill, Padding, Subscription, Task, Theme, font, widget, window,
};
use ::tap::Pipe;

use crate::{
    cli::Open,
    line_view::{self, LineView},
};

/// Run application ui.
///
/// # Errors
/// If ui cannot be created.
pub fn run(open: Open) -> ::color_eyre::Result<()> {
    let Open { theme, home, file } = open;
    let home = home.or_else(::std::env::home_dir);
    let file = file.map_or_else(
        || {
            ::rfd::FileDialog::new()
                .set_title("Open Line View File")
                .pick_file()
                .ok_or_else(|| eyre!("no file selected"))
        },
        Ok,
    )?;
    let file = file
        .to_str()
        .ok_or_else(|| eyre!("path {file:?} is not valid utf-8"))?
        .to_owned();

    ::iced::daemon(
        move || {
            let home = home.clone();
            let file = file.clone();
            let task = Task::future(::smol::unblock(move || {
                let title = format!("Line Viewer: {file}");
                let theme = theme.into_inner();
                let content = LineView::read_path(
                    file.into(),
                    line_view::provide::PathReadProvider,
                    home.as_deref(),
                )
                .map_err(|err| err.to_string());

                Arc::new(Window {
                    title,
                    theme,
                    content,
                })
            }))
            .then(|window| {
                let (_, task) = window::open(window::Settings::default());

                task.map(move |id| {
                    let window = window.clone();
                    Message::Open { id, window }
                })
            });

            (State::default(), task)
        },
        State::update,
        State::view,
    )
    .title(State::title)
    .theme(State::theme)
    .subscription(State::subscription)
    .run()
    .map_err(|err| eyre!(err))
}

/// Ui message type.
#[derive(Debug, Clone)]
enum Message {
    /// Open a new window.
    Open {
        /// Id of window.
        id: window::Id,
        /// Content of window.
        window: Arc<Window>,
    },
    /// Close a window.
    Close(window::Id),
    /// Line is hovered.
    LineHover {
        /// Id of window of line.
        id: window::Id,
        /// Index of line.
        idx: usize,
    },
    /// Line is unhovered.
    LineUnhover {
        /// Id of window of line.
        id: window::Id,
        /// Index of line.
        idx: usize,
    },
    /// Execute line.
    ExecLine {
        /// Id of window of line.
        id: window::Id,
        /// Line number to execute.
        line: usize,
    },
}

/// Window state.
#[derive(Debug)]
struct Window {
    /// Theme in use.
    theme: Theme,
    /// Window Title.
    title: String,
    /// Lines.
    content: Result<LineView, String>,
}

#[derive(Debug, Clone, Deref, DerefMut)]
struct WindowState {
    /// Static window state.
    #[deref_mut]
    #[deref]
    window: Arc<Window>,
    /// Dynamic window state.
    hovered: Option<usize>,
}

/// Ui state.
#[derive(Debug, Default)]
struct State {
    /// Windows of application.
    windows: BTreeMap<window::Id, WindowState>,
}

impl State {
    /// Get window title.
    pub fn title(&self, id: window::Id) -> String {
        self.windows
            .get(&id)
            .map_or_else(|| "Line Viewer".to_owned(), |window| window.title.clone())
    }

    /// Get window theme.
    pub fn theme(&self, id: window::Id) -> Option<Theme> {
        self.windows
            .get(&id)
            .map(|window| window.theme.clone())
            .or(Some(Theme::Dark))
    }

    /// Application subscriptions.
    pub fn subscription(&self) -> Subscription<Message> {
        window::close_events().map(Message::Close)
    }

    /// Update ui state.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Open { id, window } => {
                self.windows.insert(
                    id,
                    WindowState {
                        window,
                        hovered: None,
                    },
                );
                Task::none()
            }
            Message::Close(id) => {
                self.windows.remove(&id);
                if self.windows.is_empty() {
                    ::iced::exit()
                } else {
                    Task::none()
                }
            }
            Message::LineHover { id, idx } => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.hovered = Some(idx);
                }
                Task::none()
            }
            Message::LineUnhover { id, idx } => {
                if let Some(window) = self.windows.get_mut(&id)
                    && window.hovered == Some(idx)
                {
                    window.hovered = None;
                }
                Task::none()
            }
            Message::ExecLine { id, line } => self
                .windows
                .get(&id)
                .and_then(|window| window.content.as_ref().ok()?.get(line)?.clone().pipe(Some))
                .map(|line| {
                    Task::future(::smol::unblock(move || {
                        if let Err(err) = line.execute() {
                            ::log::error!("could not execute line\n{err}")
                        }
                    }))
                    .discard()
                })
                .unwrap_or_else(Task::none),
        }
    }

    /// View ui.
    pub fn view<'this>(&'this self, id: window::Id) -> impl Into<Element<'this, Message>> {
        let Some(WindowState { window, hovered }) = self.windows.get(&id) else {
            return widget::container(widget::space().width(Fill).height(Fill));
        };
        let Window { content, .. } = window.as_ref();

        let line_view = match content {
            Ok(line_view) => line_view,
            Err(err) => {
                return widget::text(err)
                    .style(widget::text::danger)
                    .pipe(widget::container)
                    .padding(5)
                    .style(widget::container::bordered_box)
                    .pipe(widget::container)
                    .center(Fill);
            }
        };

        widget::Column::new()
            .height(Fill)
            .spacing(5)
            .push(
                widget::text(line_view.title())
                    .font(Font {
                        weight: font::Weight::Bold,
                        ..Default::default()
                    })
                    .size(18),
            )
            .push(
                line_view
                    .iter()
                    .enumerate()
                    .map(|(idx, line)| {
                        if line.text().is_empty() {
                            widget::space().height(5).pipe(Element::from)
                        } else if line.is_title() {
                            widget::text(line.text())
                                .wrapping(widget::text::Wrapping::None)
                                .size(16)
                                .font(Font {
                                    weight: font::Weight::ExtraBold,
                                    ..Default::default()
                                })
                                .pipe(Element::from)
                        } else if line.is_warning() {
                            widget::text(line.text())
                                .wrapping(widget::text::Wrapping::None)
                                .style(widget::text::warning)
                                .size(12)
                                .pipe(Element::from)
                        } else {
                            if Some(idx) == *hovered {
                                widget::text(line.text())
                                    .wrapping(widget::text::Wrapping::None)
                                    .size(12)
                                    .font(Font {
                                        weight: font::Weight::Bold,
                                        ..Default::default()
                                    })
                            } else {
                                widget::text(line.text())
                                    .wrapping(widget::text::Wrapping::None)
                                    .size(12)
                            }
                            .pipe(widget::button)
                            .on_press_maybe((!line.text().is_empty()).then(|| Message::ExecLine {
                                id,
                                line: line.line(),
                            }))
                            .padding(0)
                            .style(widget::button::text)
                            .pipe(widget::mouse_area)
                            .on_enter(Message::LineHover { id, idx })
                            .on_exit(Message::LineUnhover { id, idx })
                            .pipe(Element::from)
                        }
                    })
                    .fold(
                        widget::Column::new().push(widget::space().height(2)),
                        widget::Column::push,
                    )
                    .padding(Padding {
                        left: 5.0,
                        right: 5.0,
                        ..Padding::new(0.0)
                    })
                    .width(Fill)
                    .spacing(2)
                    .pipe(widget::scrollable)
                    .pipe(widget::container)
                    .height(Fill)
                    .style(widget::container::bordered_box),
            )
            .pipe(widget::container)
            .padding(5)
    }
}
