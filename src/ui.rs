//! Ui implementation.

use ::core::{fmt::Debug, ops::ControlFlow};
use ::std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use ::color_eyre::eyre::eyre;
use ::derive_more::{Deref, DerefMut};
use ::iced::{
    Element, Font, Length::Fill, Padding, Subscription, Task, Theme, font, widget, window,
};
use ::katalog_lib::ThemeValueEnum;
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
    let Open {
        theme,
        home,
        file,
        ipc,
    } = open;

    let (tx, rx) = ::flume::bounded::<Message>(16);

    if ipc.is_enabled() {
        match crate::ipc::ipc_setup(tx, file.as_deref(), home.as_deref(), theme) {
            // On error we continue without ipc.
            Err(err) => {
                ::log::error!("ipc setup failed\n{err:?}");
            }
            // I we are the subscriber we continue.
            Ok(ControlFlow::Continue(..)) => {}
            // If the inputs were sent to another instance we return.
            Ok(ControlFlow::Break(..)) => return Ok(()),
        }
    }

    let home = home.or_else(::std::env::home_dir);
    let cwd = ::std::env::current_dir()?;
    ::iced::daemon(
        move || {
            let open_path = Task::done(if let Some(path) = file.clone() {
                Message::OpenFile {
                    path,
                    home: home.clone(),
                    theme,
                }
            } else {
                Message::DialogAt {
                    path: cwd.clone(),
                    home: home.clone(),
                    theme,
                }
            });
            let receive_message = Task::stream(rx.clone().into_stream());
            (State::default(), Task::batch([open_path, receive_message]))
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
pub enum Message {
    /// Add window to state.
    AddWindow {
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
    /// Open a file dialog at location.
    DialogAt {
        /// Path to open dialog at.
        path: PathBuf,
        /// Home directory to use.
        home: Option<PathBuf>,
        /// Theme to use.
        theme: ThemeValueEnum,
    },
    /// Open a line-viewer file.
    OpenFile {
        /// Path of file.
        path: PathBuf,
        /// Home directory to use.
        home: Option<PathBuf>,
        /// Theme to use.
        theme: ThemeValueEnum,
    },
}

/// Window state.
#[derive(Debug)]
pub struct Window {
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
            Message::AddWindow { id, window } => {
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
            Message::DialogAt { path, home, theme } => Task::future(async move {
                let path = ::rfd::AsyncFileDialog::new()
                    .set_title("Open Line View File")
                    .set_directory(path)
                    .pick_file()
                    .await?
                    .path()
                    .to_path_buf();

                Some(Message::OpenFile { path, home, theme })
            })
            .then(|message| message.map_or_else(Task::none, Task::done)),
            Message::OpenFile { path, home, theme } => {
                let Some(file) = path.to_str() else {
                    ::log::error!("path {path:?} is not valid utf-8");
                    return Task::none();
                };
                let file = file.to_owned();
                Task::future(::smol::unblock(move || {
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
                        Message::AddWindow { id, window }
                    })
                })
            }
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
                            .on_press_maybe(
                                (!line.text().is_empty())
                                    .then_some(Message::ExecLine { id, line: idx }),
                            )
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
