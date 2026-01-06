//! Ui implementation.

use ::core::{fmt::Debug, ops::Deref, time::Duration};
use ::std::{
    collections::BTreeMap,
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Arc,
};

use ::clap::ValueEnum;
use ::color_eyre::eyre::eyre;
use ::derive_more::{Deref, DerefMut};
use ::iced::{
    Element, Font, Length::Fill, Padding, Subscription, Task, Theme, font, widget, window,
};
use ::iceoryx2::{
    node::NodeBuilder,
    port::subscriber::SubscriberCreateError,
    prelude::{EventId, ZeroCopySend},
    service::ipc_threadsafe,
};
use ::iceoryx2_bb_container::vector::StaticVec;
use ::katalog_lib::ThemeValueEnum;
use ::tap::Pipe;

use crate::{
    cli::Open,
    line_view::{self, LineView},
};

/// A static path with a lenght of at most N.
#[derive(Clone, ZeroCopySend)]
#[repr(C)]
struct StaticPath<const N: usize> {
    /// Byte data of path.
    data: StaticVec<u8, N>,
}

/// Error returned when trying to create StaticPath from a
/// path that is too long.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, ::thiserror::Error)]
#[error("cannot create StaticPath<{at_most}> from a path of length{len}")]
pub struct PathTooLong {
    /// Longest length that would have been possible.
    pub at_most: usize,
    /// Length that was attempted.
    pub len: usize,
}

impl<const N: usize> TryFrom<&Path> for StaticPath<N> {
    type Error = PathTooLong;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        let bytes = value.as_os_str().as_encoded_bytes();
        StaticVec::try_from(bytes)
            .map(|data| Self { data })
            .map_err(|_| PathTooLong {
                at_most: N,
                len: bytes.len(),
            })
    }
}

impl<const N: usize> AsRef<OsStr> for StaticPath<N> {
    #[inline]
    fn as_ref(&self) -> &OsStr {
        OsStr::from_bytes(&self.data)
    }
}

impl<const N: usize> AsRef<Path> for StaticPath<N> {
    #[inline]
    fn as_ref(&self) -> &Path {
        Path::new(AsRef::<OsStr>::as_ref(self))
    }
}

impl<const N: usize> Deref for StaticPath<N> {
    type Target = Path;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<const N: usize> Debug for StaticPath<N> {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        Debug::fmt(AsRef::<Path>::as_ref(self), f)
    }
}

/// Request a path be either opened or used ast the start
/// of a file dialog.
#[derive(Debug, Clone, ZeroCopySend)]
#[repr(C)]
pub struct OpenRequest {
    /// If true a file dialog should be opened at location.
    open_at: bool,
    /// Path used for either opening or file dialog.
    path: StaticPath<4096>,
    /// Path used for home.
    home: Option<StaticPath<4096>>,
    /// Index of theme used.
    themeidx: usize,
}

/// Run application ui.
///
/// # Errors
/// If ui cannot be created.
pub fn run(open: Open) -> ::color_eyre::Result<()> {
    let Open { theme, home, file } = open;

    let node = NodeBuilder::new()
        .name(&"line_viewer".try_into()?)
        .create::<ipc_threadsafe::Service>()?;

    let service = node
        .service_builder(&"open_path".try_into()?)
        .publish_subscribe::<OpenRequest>()
        .max_subscribers(1)
        .open_or_create()?;

    let ping_event = EventId::new(11);
    let event_service = node
        .service_builder(&"open_path".try_into()?)
        .event()
        .open_or_create()?;

    let subscriber = match service.subscriber_builder().create() {
        Ok(subscriber) => subscriber,
        Err(SubscriberCreateError::ExceedsMaxSupportedSubscribers) => {
            let (path, open_at) = if let Some(path) = file {
                (path, false)
            } else {
                (::std::env::current_dir().map_err(|err| eyre!(err))?, true)
            };

            let publisher = service.publisher_builder().create()?;
            let notifier = event_service
                .notifier_builder()
                .default_event_id(ping_event)
                .create()?;

            let message = publisher.loan_uninit()?;
            let message = message.write_payload(OpenRequest {
                open_at,
                path: path.as_path().try_into()?,
                home: home.map(|home| home.as_path().try_into()).transpose()?,
                themeidx: ThemeValueEnum::value_variants()
                    .iter()
                    .position(|variant| variant == &theme)
                    .unwrap_or(usize::MAX),
            });
            message.send()?;
            notifier.notify()?;
            ::log::info!("sent ipc message");
            node.wait(Duration::from_millis(50))?;
            return Ok(());
        }
        Err(err) => return Err(eyre!(err)),
    };

    let (tx, rx) = ::flume::bounded::<Message>(16);

    ::std::thread::Builder::new()
        .name("line-viewer-ipc".to_owned())
        .spawn(move || {
            let receive_messages = move || -> ::color_eyre::Result<()> {
                let listener = event_service.listener_builder().create()?;
                while listener
                    .timed_wait_all(|_| {}, Duration::from_millis(200))
                    .is_ok()
                {
                    while let Some(message) = subscriber.receive()? {
                        ::log::info!("received ipc message");
                        let path = message.path.to_path_buf();
                        let open_at = message.open_at;
                        let home = message.home.as_ref().map(|home| home.to_path_buf());
                        let theme = ThemeValueEnum::value_variants()
                            .get(message.themeidx)
                            .copied()
                            .unwrap_or_default();

                        tx.send(if open_at {
                            Message::DialogAt { path, home, theme }
                        } else {
                            Message::OpenFile { path, home, theme }
                        })?;
                    }
                }
                Ok(())
            };

            if let Err(err) = receive_messages() {
                ::log::error!("error receiving ipc messages\n{err}");
            }

            ::log::info!("closing ipc thread");
        })?;

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
enum Message {
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
