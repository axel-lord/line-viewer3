//! Ui implementation.

use ::core::{cell::RefCell, fmt::Debug, ops::ControlFlow, time::Duration};
use ::std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};

use ::clap::ValueEnum;
use ::color_eyre::eyre::eyre;
use ::derive_more::{Deref, DerefMut};
use ::iced::{
    Element, Font, Length::Fill, Padding, Subscription, Task, Theme, font, widget, window,
};
use ::katalog_lib::ThemeValueEnum;
use ::katalog_lib_ipc::{StaticPath, ZeroCopySend, single_process::SubscriberHandle};
use ::notify::{
    EventKind, RecommendedWatcher, Watcher,
    event::{CreateKind, ModifyKind},
    recommended_watcher,
};
use ::tap::Pipe;

use crate::{
    cli::{Daemon, Open},
    line_view::{
        self, LineView,
        provide::{self, PathReadProvider},
    },
};

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

/// Create receiver for ipc.
fn ipc_receiver(
    tx: ::flume::Sender<Message>,
) -> impl for<'m> Fn(&'m OpenRequest) -> ::color_eyre::Result<()> {
    move |message| {
        let path = message.path.try_into_path()?.to_path_buf();
        let open_at = message.open_at;
        let home = message
            .home
            .as_ref()
            .map(|home| home.try_into_path())
            .transpose()?
            .map(|path| path.to_path_buf());
        let theme = ThemeValueEnum::value_variants()
            .get(message.themeidx)
            .copied()
            .unwrap_or_default();

        tx.send(if open_at {
            Message::DialogAt { path, home, theme }
        } else {
            Message::OpenFile { path, home, theme }
        })?;
        Ok(())
    }
}

/// Run application.
#[bon::builder]
#[builder(finish_fn = run)]
fn application<F>(
    /// If application should be ran as a daemon.
    #[builder(default = false)]
    is_daemon: bool,
    /// Handle of ipc subscriber.
    subscriber: Option<SubscriberHandle>,
    /// Message receiver.
    receiver: ::flume::Receiver<Message>,
    /// Message sender.
    sender: ::flume::Sender<Message>,
    /// Additional tasks.
    task: F,
) -> ::color_eyre::Result<()>
where
    F: 'static + Fn() -> Task<Message>,
{
    ::iced::daemon(
        move || {
            let sender = sender.clone();
            let watcher =
                recommended_watcher(
                    move |event: ::notify::Result<::notify::Event>| match event {
                        Ok(event) => _ = sender.send(Message::Watcher(event)),
                        Err(err) => ::log::error!("notify watcher error\n{err}"),
                    },
                )
                .map_err(|err| ::log::error!("could not create notify watcher\n{err}"))
                .ok();
            let subscriber_handle = subscriber.clone();
            let receive_message = Task::stream(receiver.clone().into_stream());
            (
                State {
                    subscriber_handle: subscriber_handle.unwrap_or_default(),
                    is_daemon,
                    watcher,
                    ..Default::default()
                },
                Task::batch([receive_message, task()]),
            )
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

/// Run application daemon.
///
/// # Errors
/// If ui cannot be created.
pub fn run_daemon(daemon: Daemon) -> ::color_eyre::Result<()> {
    let Daemon { timeout } = daemon;
    let timeout = Duration::from_millis(timeout.into());
    let (tx, rx) = ::flume::bounded::<Message>(16);

    let handle = ::katalog_lib_ipc::single_process::subscribe_only()
        .node_name("line_viewer")
        .service_name("open_path")
        .thread_name(|| "line_viewer_subscriber".to_owned())
        .receive(ipc_receiver(tx.clone()))
        .timeout(timeout)
        .setup()?;

    application()
        .is_daemon(true)
        .subscriber(handle)
        .receiver(rx)
        .sender(tx)
        .task(Task::none)
        .run()
}

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
    let home = home.or_else(::std::env::home_dir);
    let cwd = ::std::env::current_dir()?;

    let subscriber_handle = if ipc.is_enabled() {
        let single_process = ::katalog_lib_ipc::single_process()
            .node_name("line_viewer")
            .service_name("open_path")
            .thread_name(|| "line_viewer_subscriber".to_owned())
            .input(|| {
                Ok(OpenRequest {
                    open_at: file.is_none(),
                    path: file.as_ref().unwrap_or(&cwd).as_path().try_into()?,
                    home: home
                        .as_ref()
                        .map(|home| home.as_path().try_into())
                        .transpose()?,
                    themeidx: ThemeValueEnum::value_variants()
                        .iter()
                        .copied()
                        .position(|variant| variant == theme)
                        .unwrap_or(usize::MAX),
                })
            })
            .receive(ipc_receiver(tx.clone()));
        match single_process.setup() {
            // On error we continue without ipc.
            Err(err) => {
                ::log::error!("ipc setup failed\n{err:?}");
                None
            }
            // I we are the subscriber we continue.
            Ok(ControlFlow::Continue(handle)) => Some(handle),
            // If the inputs were sent to another instance we return.
            Ok(ControlFlow::Break(..)) => return Ok(()),
        }
    } else {
        None
    };

    application()
        .maybe_subscriber(subscriber_handle)
        .receiver(rx)
        .sender(tx)
        .task(move || {
            Task::done(if let Some(path) = file.clone() {
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
            })
        })
        .run()
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
    /// Set content of a window.
    /// Unlike `AddWindow` will not add new entries to windows.
    SetWindow {
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
    /// Notify watcher event.
    Watcher(::notify::Event),
    /// Add a path to be watched.
    Watch(PathBuf, window::Id),
    /// Attempt to exit if no windows are open, or not running as a daemon.
    TryExit,
}

/// Window state.
#[derive(Debug)]
pub struct Window {
    /// Theme in use.
    theme: Theme,
    /// Window Title.
    title: String,
    /// Window home.
    home: Option<PathBuf>,
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

#[derive(Debug, Default, Clone)]
struct PathReadProviderWrapper(PathReadProvider, Rc<RefCell<BTreeSet<PathBuf>>>);

impl PathReadProviderWrapper {
    /// Get created path set.
    fn get_set(self) -> BTreeSet<PathBuf> {
        let Self(_, path_set) = self;
        path_set.borrow().clone()
    }
}

impl provide::Read for PathReadProviderWrapper {
    type BufRead = <PathReadProvider as provide::Read>::BufRead;

    fn provide(&self, from: &str) -> line_view::Result<Self::BufRead> {
        let Self(provider, path_set) = self;
        let reader = provider.provide(from)?;
        path_set.borrow_mut().insert(PathBuf::from(from));
        Ok(reader)
    }
}

/// Ui state.
#[derive(Debug, Default)]
struct State {
    /// Windows of application.
    windows: BTreeMap<window::Id, WindowState>,
    /// Handle to subscriber.
    subscriber_handle: SubscriberHandle,
    /// Set to true if daemon.
    is_daemon: bool,
    /// File update notification watcher.
    watcher: Option<RecommendedWatcher>,
    /// Paths watched by windows.
    watched: BTreeMap<PathBuf, BTreeSet<window::Id>>,
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
        Subscription::batch([
            ::iced::time::every(Duration::from_millis(15))
                .with(self.subscriber_handle.clone())
                .filter_map(|(handle, _)| handle.is_closed().then_some(Message::TryExit)),
            window::close_events().map(Message::Close),
        ])
    }

    /// Exit if not a daemon, and no windows are open.
    fn try_exit(&self) -> Task<Message> {
        if self.subscriber_handle.is_closed() && self.windows.is_empty() {
            ::iced::exit()
        } else {
            if !self.is_daemon && self.windows.is_empty() {
                self.subscriber_handle.close();
            }
            Task::none()
        }
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
            Message::SetWindow { id, window } => {
                if let Some(entry) = self.windows.get_mut(&id) {
                    entry.window = window;
                } else {
                    ::log::warn!("could not set window content for id {id:?}");
                }
                Task::none()
            }
            Message::Close(id) => {
                self.windows.remove(&id);

                let unwatch = self.watched.extract_if(.., |_path, id_set| {
                    id_set.remove(&id);
                    id_set.is_empty()
                });

                for (path, _) in unwatch {
                    if let Some(watcher) = &mut self.watcher
                        && let Err(err) = watcher.unwatch(&path)
                    {
                        ::log::warn!("could nod unwatch {path:?}\n{err}");
                    }
                }

                self.try_exit()
            }
            Message::TryExit => self.try_exit(),
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
                    let provider = PathReadProviderWrapper::default();
                    let title = format!("Line Viewer: {file}");
                    let theme = theme.into_inner();
                    let content =
                        LineView::read_path(file.into(), provider.clone(), home.as_deref())
                            .map_err(|err| err.to_string());

                    (
                        Arc::new(Window {
                            title,
                            home,
                            theme,
                            content,
                        }),
                        provider.get_set(),
                    )
                }))
                .then(move |(window, path_set)| {
                    let (id, task) = window::open(window::Settings::default());

                    task.map(move |id| {
                        let window = window.clone();
                        Message::AddWindow { id, window }
                    })
                    .chain(Task::batch(
                        path_set
                            .into_iter()
                            .map(|path| Task::done(Message::Watch(path, id))),
                    ))
                })
            }
            Message::Watcher(event) => match event.kind {
                EventKind::Create(CreateKind::File) | EventKind::Modify(ModifyKind::Data(..)) => {
                    let mut tasks = Vec::new();
                    for path in event.paths {
                        let Some(file) = path.to_str() else {
                            ::log::error!("path {path:?} is not valid utf-8");
                            continue;
                        };
                        let Some(id_set) = self.watched.get(&path) else {
                            if let Some(watcher) = &mut self.watcher
                                && let Err(err) = watcher.unwatch(&path)
                            {
                                ::log::warn!("could not unwatch {path:?}\n{err}");
                            };
                            continue;
                        };
                        for id in id_set {
                            let Some(window) = self.windows.get(id) else {
                                continue;
                            };
                            let file = file.to_owned();
                            let theme = window.theme.clone();
                            let home = window.home.clone();
                            let id = *id;
                            tasks.push(
                                Task::future(::smol::unblock(move || {
                                    let title = format!("Line Viewer: {file}");
                                    let provider = PathReadProviderWrapper::default();
                                    let theme = theme;
                                    let content = LineView::read_path(
                                        file.into(),
                                        provider.clone(),
                                        home.as_deref(),
                                    )
                                    .map_err(|err| err.to_string());

                                    (
                                        id,
                                        Arc::new(Window {
                                            title,
                                            home,
                                            theme,
                                            content,
                                        }),
                                        provider.get_set(),
                                    )
                                }))
                                .then(
                                    |(id, window, path_set)| {
                                        Task::done(Message::SetWindow { id, window }).chain(
                                            Task::batch(
                                                path_set.into_iter().map(|path| {
                                                    Task::done(Message::Watch(path, id))
                                                }),
                                            ),
                                        )
                                    },
                                ),
                            );
                        }
                    }
                    Task::batch(tasks)
                }
                _ => Task::none(),
            },
            Message::Watch(path, id) => {
                if let Some(id_set) = self.watched.get_mut(&path) {
                    id_set.insert(id);
                } else if let Some(watcher) = &mut self.watcher {
                    if let Err(err) = watcher.watch(&path, ::notify::RecursiveMode::NonRecursive) {
                        ::log::error!("could not watch {path:?}\n{err}");
                    } else {
                        self.watched.insert(path, BTreeSet::from_iter([id]));
                    };
                }

                Task::none()
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
