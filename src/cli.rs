//! [Cli] impl.

use ::std::{env::current_exe, path::PathBuf};

use ::clap::{Args, CommandFactory, Parser, Subcommand};
use ::clap_complete::{Generator, Shell};
use ::color_eyre::eyre::eyre;
use ::katalog_lib::ThemeValueEnum;
use ::patharg::{InputArg, OutputArg};

/// Get default shell to use.
fn default_shell() -> Shell {
    Shell::from_env().unwrap_or(Shell::Bash)
}

/// View lines and execute actions on clicking them.
#[derive(Debug, Parser, Clone)]
#[command(author, version, args_conflicts_with_subcommands = true)]
pub struct Cli {
    /// If no else run open.
    #[command(flatten)]
    open: Open,

    /// What action to take.
    #[command(subcommand)]
    command: Option<Action>,
}

/// What action to take.
#[derive(Debug, Clone, Subcommand)]
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

impl From<Cli> for Action {
    fn from(value: Cli) -> Self {
        let Cli { open, command } = value;
        command.unwrap_or(Action::Open(open))
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
        builder.push(b'\n');

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
        shell
            .try_generate(
                &Cli::command(),
                &mut file.create().map_err(|err| eyre!(err))?,
            )
            .map_err(|err| eyre!(err))
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

#[derive(Debug, Clone, Args)]
/// Open line-viewer file.
pub struct Open {
    /// Theme to use for application.
    #[arg(long, short, value_enum, default_value_t)]
    pub theme: ThemeValueEnum,

    /// Use specified path as user home.
    #[arg(long)]
    pub home: Option<PathBuf>,

    /// File to open.
    #[arg(default_value_t)]
    pub file: InputArg,
}
