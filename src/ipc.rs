//! Interprocess communication for only having one ui with multiple windows.

use ::core::{
    fmt::{Debug, Display},
    ops::ControlFlow,
    str::Utf8Error,
    time::Duration,
};
use ::std::path::Path;

use ::clap::ValueEnum;
use ::color_eyre::eyre::eyre;
use ::flume::Sender;
use ::iceoryx2::{
    node::{Node, NodeBuilder, NodeState},
    port::{
        ReceiveError,
        listener::ListenerCreateError,
        subscriber::{Subscriber, SubscriberCreateError},
    },
    prelude::{CallbackProgression, EventId, NodeName, ZeroCopySend},
    service::ipc_threadsafe,
};
use ::iceoryx2_bb_container::vector::StaticVec;
use ::katalog_lib::ThemeValueEnum;
use ::tap::TryConv;

use crate::ui::Message;

/// Error raised when converting a [Path] to a [StaticPath].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, ::thiserror::Error)]
pub enum FromPathError {
    /// Error returned when trying to create StaticPath from a
    /// path that is too long.
    #[error("cannot create StaticPath<{at_most}> from a path of length{len}")]
    TooLong {
        /// Longest length that would have been possible.
        at_most: usize,
        /// Length that was attempted.
        len: usize,
    },
    /// `Path` was not utf-8 on a windows platform.
    #[error("path is required to be utf-8 on windows")]
    NotUtf8,
}

/// Error raised when converting a [SaticPath] to a [Path].
#[derive(Debug, Clone, Copy, PartialEq, Eq, ::thiserror::Error)]
pub enum IntoPathError {
    /// `StaticPath` was not utf-8 on a windows platform.
    #[error("path is required to be utf-8 on windows, {err}")]
    NotUtf8 {
        /// Wrapped utf8 error.
        #[from]
        err: Utf8Error,
    },
}

/// A static path with a lenght of at most N.
#[derive(Clone, ZeroCopySend)]
#[repr(C)]
struct StaticPath<const N: usize> {
    /// Byte data of path.
    data: StaticVec<u8, N>,
}

impl<const N: usize> StaticPath<N> {
    /// Attempt ot get a path reference from a static path.
    #[cfg_attr(
        target_family = "unix",
        expect(clippy::unnecessary_fallible_conversions)
    )]
    pub fn try_into_path(&self) -> Result<&Path, <&Path as TryFrom<&Self>>::Error> {
        self.try_into()
    }
}

#[cfg(target_family = "windows")]
impl<'a, const N: usize> TryFrom<&'a StaticPath<N>> for &'a Path {
    type Error = IntoPathError;

    fn try_from(value: &'a StaticPath<N>) -> Result<Self, Self::Error> {
        str::from_utf8(&value.data)
            .map_err(From::from)
            .map(Path::new)
    }
}

#[cfg(target_family = "unix")]
impl<'a, const N: usize> From<&'a StaticPath<N>> for &'a Path {
    fn from(value: &'a StaticPath<N>) -> Self {
        use ::std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        Path::new(OsStr::from_bytes(&value.data))
    }
}

impl<const N: usize> TryFrom<&Path> for StaticPath<N> {
    type Error = FromPathError;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        #[cfg(target_family = "windows")]
        let bytes = value.to_str().ok_or(FromPathError::NotUtf8)?.as_bytes();

        #[cfg(target_family = "unix")]
        let bytes = {
            use ::std::os::unix::ffi::OsStrExt;
            value.as_os_str().as_bytes()
        };

        StaticVec::try_from(bytes)
            .map(|data| Self { data })
            .map_err(|_| FromPathError::TooLong {
                at_most: N,
                len: bytes.len(),
            })
    }
}

impl<const N: usize> Debug for StaticPath<N> {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.write_str("\"")?;

        for chunk in self.data.utf8_chunks() {
            f.write_str(chunk.valid())?;

            for byte in chunk.invalid() {
                write!(f, "\\x{byte:02X}")?;
            }
        }

        f.write_str("\"")?;
        Ok(())
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

/// Alias for publish subscribe service.
type PublishSubscribeService = ::iceoryx2::service::port_factory::publish_subscribe::PortFactory<
    ipc_threadsafe::Service,
    OpenRequest,
    (),
>;

/// Alias for event service.
type EventService = ::iceoryx2::service::port_factory::event::PortFactory<ipc_threadsafe::Service>;

/// Event used for notifying subscriber.
const NOTIFY_EVENT: EventId = EventId::new(11);

/// Create ipc node.
fn build_node() -> ::color_eyre::Result<Node<ipc_threadsafe::Service>> {
    NodeBuilder::new()
        .name(
            &"line_viewer"
                .try_conv::<NodeName>()
                .map_err(|err| eyre!(err))?,
        )
        .create::<ipc_threadsafe::Service>()
        .map_err(|err| eyre!(err))
}

/// Create publish subscribe service.
fn build_serice_(
    node: &Node<ipc_threadsafe::Service>,
) -> ::color_eyre::Result<PublishSubscribeService> {
    node.service_builder(&"open_path".try_into()?)
        .publish_subscribe::<OpenRequest>()
        .max_subscribers(1)
        .open_or_create()
        .map_err(|err| eyre!(err))
}

/// Create publish subscribe service.
fn build_serice(
    node: &Node<ipc_threadsafe::Service>,
) -> ::color_eyre::Result<PublishSubscribeService> {
    build_serice_(node).or_else(|_| {
        Node::<ipc_threadsafe::Service>::list(
            ::iceoryx2::config::Config::global_config(),
            |node_state| {
                if let NodeState::<ipc_threadsafe::Service>::Dead(view) = node_state {
                    ::log::info!("cleanup of dead node {view:?}");
                    if let Err(err) = view.remove_stale_resources() {
                        ::log::warn!("could nod clean up stale resources, {err:?}");
                    }
                }
                CallbackProgression::Continue
            },
        )?;

        build_serice_(node)
    })
}

/// Create event service.
fn build_event_service(node: &Node<ipc_threadsafe::Service>) -> ::color_eyre::Result<EventService> {
    node.service_builder(&"open_path".try_into()?)
        .event()
        .open_or_create()
        .map_err(|err| eyre!(err))
}

/// Create subscriber thread.
fn create_subscriber_thread_<M, E, S>(
    subscriber: Subscriber<ipc_threadsafe::Service, M, ()>,
    event_service: EventService,
    thread_name: String,
    mut send: S,
) -> Result<(), E>
where
    M: Debug + ZeroCopySend,
    E: 'static
        + Send
        + Sync
        + Display
        + From<::std::io::Error>
        + From<ListenerCreateError>
        + From<ReceiveError>,
    S: 'static + Send + FnMut(&M) -> Result<(), E>,
{
    ::std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let mut receive_messages = move || -> Result<(), E> {
                let listener = event_service.listener_builder().create()?;
                while listener
                    .timed_wait_all(|_| {}, Duration::from_millis(200))
                    .is_ok()
                {
                    while let Some(message) = subscriber.receive()? {
                        ::log::info!("received ipc message");
                        send(&message)?;
                    }
                }
                Ok(())
            };

            if let Err(err) = receive_messages() {
                ::log::error!("error receiving ipc messages\n{err}");
            }

            ::log::info!("closing ipc thread");
        })?;
    Ok(())
}

/// Create subscriber thread.
fn create_subscriber_thread(
    subscriber: Subscriber<ipc_threadsafe::Service, OpenRequest, ()>,
    event_service: EventService,
    tx: Sender<Message>,
) -> ::color_eyre::Result<()> {
    create_subscriber_thread_::<OpenRequest, _, _>(
        subscriber,
        event_service,
        "line-viewer-ipc".to_owned(),
        move |message: &OpenRequest| -> ::color_eyre::Result<()> {
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
        },
    )
}

/// Publish input to eventual subscribers.
fn publish_input(
    node: Node<ipc_threadsafe::Service>,
    service: PublishSubscribeService,
    event_service: EventService,
    file: Option<&Path>,
    home: Option<&Path>,
    theme: ThemeValueEnum,
) -> ::color_eyre::Result<()> {
    let (path, open_at) = if let Some(path) = file {
        (path.to_path_buf(), false)
    } else {
        (::std::env::current_dir().map_err(|err| eyre!(err))?, true)
    };

    let publisher = service.publisher_builder().create()?;
    let notifier = event_service
        .notifier_builder()
        .default_event_id(NOTIFY_EVENT)
        .create()?;

    let message = publisher.loan_uninit()?;
    let message = message.write_payload(OpenRequest {
        open_at,
        path: path.as_path().try_into()?,
        home: home.map(|home| home.try_into()).transpose()?,
        themeidx: ThemeValueEnum::value_variants()
            .iter()
            .position(|variant| variant == &theme)
            .unwrap_or(usize::MAX),
    });
    message.send()?;
    ::log::info!("sent ipc message");
    let wait_result = if let Err(err) = notifier.notify() {
        ::log::error!("could not send notification event, {err}");
        node.wait(Duration::from_millis(200))
    } else {
        node.wait(Duration::from_millis(50))
    };
    if let Err(err) = wait_result {
        ::log::warn!("after-publish wait interrupted, {err}");
    }
    Ok(())
}

/// Setup ipc functionality.
///
/// # Errors
/// If ipc cannot be established.
#[inline(never)]
pub fn ipc_setup(
    tx: Sender<Message>,
    file: Option<&Path>,
    home: Option<&Path>,
    theme: ThemeValueEnum,
) -> ::color_eyre::Result<ControlFlow<()>> {
    let node = build_node()?;
    let service = build_serice(&node)?;
    let event_service = build_event_service(&node)?;

    match service.subscriber_builder().create() {
        Ok(subscriber) => {
            create_subscriber_thread(subscriber, event_service, tx)?;
            Ok(ControlFlow::Continue(()))
        }
        Err(SubscriberCreateError::ExceedsMaxSupportedSubscribers) => {
            publish_input(node, service, event_service, file, home, theme)?;
            Ok(ControlFlow::Break(()))
        }
        Err(err) => Err(eyre!(err)),
    }
}
