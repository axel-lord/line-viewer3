//! [Cli] impl.

use ::std::{
    env::current_exe,
    io::{BufWriter, Write, stdin},
    path::PathBuf,
};

use ::clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use ::clap_complete::Shell;
use ::color_eyre::eyre::eyre;
use ::derive_more::{From, IsVariant};
use ::katalog_lib::ThemeValueEnum;
use ::patharg::{InputArg, OutputArg};

use crate::line_view::{self, LineView};

pub use Feature::{Disabled, Enabled};

/// Get default shell to use.
fn default_shell() -> Shell {
    Shell::from_env().unwrap_or(Shell::Bash)
}

/// View lines and execute actions on clicking them.
#[derive(Debug, Parser, Clone)]
#[command(author, version, args_conflicts_with_subcommands = true)]
pub struct Cli {
    /// What action to take.
    #[command(subcommand)]
    pub command: Option<Action>,
}

impl From<Action> for Cli {
    fn from(value: Action) -> Self {
        Self {
            command: Some(value),
        }
    }
}

impl From<Cli> for Action {
    fn from(value: Cli) -> Self {
        value.command.unwrap_or_default()
    }
}

/// Flag for an application feature.
#[derive(Debug, Clone, Copy, IsVariant, PartialEq, Eq, PartialOrd, Ord, Hash, ValueEnum)]
pub enum Feature {
    /// Feature is enabled.
    #[value(alias = "enable")]
    Enabled,
    /// Feature is disabled.
    #[value(alias = "disable")]
    Disabled,
}

/// What action to take.
#[derive(Debug, Clone, Subcommand, From)]
pub enum Action {
    /// Generate completions.
    Completions(Completions),
    /// Write xdg mimetype xml.
    MimeType(MimeType),
    /// Write xdg .desktop file.
    Application(Application),
    /// Open line-viewer file [default command].
    Open(Open),
    /// Print line-viewer file.
    Print(Print),
}

impl Default for Action {
    fn default() -> Self {
        Self::Open(Open::default())
    }
}

/// Write xdg .desktop file.
#[derive(Debug, Clone, Args)]
pub struct Application {
    /// Override executable used for 'Exec'.
    #[arg(long, short)]
    pub exec: Option<PathBuf>,

    /// File to write .desktop content to.
    #[arg(default_value_t)]
    pub file: OutputArg,
}

impl Application {
    /// Generate and write application file.
    ///
    /// # Errors
    /// If the current exe cannot be queried.
    /// Or if the desktop file cannot be written.
    pub fn generate(self) -> ::color_eyre::Result<()> {
        const CONTENT: &[u8] = include_bytes!("../assets/line-view.desktop");
        let Self { exec, file } = self;
        let exec = exec
            .map_or_else(current_exe, Ok)
            .map_err(|err| eyre!(err))?
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .flat_map(|byte| match byte {
                b' ' => b"\\s",
                b'\n' => b"\\n",
                b'\t' => b"\\t",
                b'\r' => b"\\r",
                b'\\' => b"\\\\",
                other => ::core::slice::from_ref(other),
            })
            .copied()
            .collect::<Vec<u8>>();

        let mut builder = Vec::<u8>::from(CONTENT);
        builder.extend_from_slice(b"Exec=");
        builder.extend_from_slice(&exec);
        builder.extend_from_slice(b" open %f\n");

        file.write(builder).map_err(|err| eyre!(err))
    }
}

/// Write xdg mime type xml.
#[derive(Debug, Clone, Args)]
pub struct MimeType {
    /// File to write mimetype to.
    #[arg(default_value_t)]
    pub file: OutputArg,
}

impl MimeType {
    /// Write mimetype.
    ///
    /// # Errors
    /// If the mimetype cannot be written
    pub fn write(self) -> ::color_eyre::Result<()> {
        let Self { file } = self;
        file.write(include_bytes!("../assets/application-x-lineview.xml"))
            .map_err(|err| eyre!(err))
    }
}

/// Generate completions.
#[derive(Debug, Clone, Args)]
pub struct Completions {
    /// Shell to generate for.
    #[arg(long, short, default_value_t = default_shell())]
    pub shell: Shell,

    /// File to write completions to.
    #[arg(default_value_t)]
    pub file: OutputArg,
}

impl Completions {
    /// Generate completions.
    ///
    /// # Errors
    /// If the completions cannot be written or generated.
    pub fn generate(self) -> ::color_eyre::Result<()> {
        let Self { shell, file } = self;
        ::clap_complete::generate(
            shell,
            &mut Cli::command(),
            current_exe()
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| String::from(env!("CARGO_PKG_NAME"))),
            &mut file.create().map_err(|err| eyre!(err))?,
        );
        Ok(())
    }
}

/// Print line-viewer file.
#[derive(Debug, Clone, Args)]
pub struct Print {
    /// File to print.
    #[arg(default_value_t)]
    pub file: InputArg,

    /// Use specified path as user home.
    #[arg(long)]
    pub home: Option<PathBuf>,

    /// Where to print file.
    #[arg(default_value_t)]
    pub destination: OutputArg,
}

impl Print {
    /// Print line view.
    ///
    /// # Errors
    /// If the lines cannot be read/parsed.
    /// Or if they cannot be written.
    pub fn print(self) -> ::color_eyre::Result<()> {
        let Self {
            file,
            home,
            destination,
        } = self;

        let view = match file {
            InputArg::Stdin => LineView::read_buf(
                stdin().lock(),
                line_view::provide::PathReadProvider,
                home.as_deref(),
            ),
            InputArg::Path(path_buf) => LineView::read_path(
                path_buf
                    .to_str()
                    .ok_or_else(|| eyre!("destination path {destination:?} is not valid utf-8"))?
                    .into(),
                line_view::provide::PathReadProvider,
                home.as_deref(),
            ),
        };
        let view = view.map_err(|err| eyre!(err))?;

        let mut destination = destination
            .create()
            .map_err(|err| eyre!(err))?
            .map_right(BufWriter::new);

        for line in &view {
            if line.is_title() {
                destination.write_all(b"-- ").map_err(|err| eyre!(err))?;
            }
            if line.is_warning() {
                destination
                    .write_all(b"[warning] ")
                    .map_err(|err| eyre!(err))?;
            }
            destination
                .write_all(line.text().as_bytes())
                .map_err(|err| eyre!(err))?;
            destination.write_all(b"\n").map_err(|err| eyre!(err))?;
        }

        Ok(())
    }
}

/// Open line-viewer file.
#[derive(Debug, Clone, Parser)]
#[command(author, version)]
pub struct Open {
    /// Theme to use for application.
    #[arg(long, short, value_enum, default_value_t)]
    pub theme: ThemeValueEnum,

    /// Use specified path as user home.
    #[arg(long)]
    pub home: Option<PathBuf>,

    /// Should ipc be used.
    #[arg(long, value_enum, default_value_t = Enabled)]
    pub ipc: Feature,

    /// File to open.
    pub file: Option<PathBuf>,
}

impl Default for Open {
    fn default() -> Self {
        Self {
            theme: Default::default(),
            home: None,
            ipc: Enabled,
            file: None,
        }
    }
}
