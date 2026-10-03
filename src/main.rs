use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use iris::application::{self, Today};
use iris::domain::{GitOperation, RemoteOperation};
use iris::infrastructure::{OllamaComposer, ProcessGit, SqliteStore};
use iris::ports::GitReader;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "iris", version, about = "Your local developer companion")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage local todos.
    Todo(TodoArgs),
    /// Record and review work-log entries.
    Log(LogArgs),
    /// Register repositories for activity reporting.
    Repo(RepoArgs),
    /// Run Git remote operations for the current or a registered repository.
    Git(GitArgs),
    /// Generate a commit message from staged changes with local Ollama.
    Compose(ComposeArgs),
    /// Stage changed files in the current repository.
    Forge(ForgeArgs),
    /// Push commits from the current repository.
    Dispatch(DispatchArgs),
    /// List local or remote branches in the current repository.
    Realm(RealmArgs),
    /// Stash current changes in the current repository.
    Vault,
    /// Show commit history for the current repository.
    Chronicle,
    /// Show working-tree status for the current repository.
    Inspect,
    /// View or configure the origin remote for the current repository.
    Outpost(OutpostArgs),
    /// Show open todos, today's logs, and repository activity.
    Today,
    /// Set up, inspect, or diagnose a macOS Fish shell environment.
    Shell(ShellArgs),
}
#[derive(Args)]
struct ShellArgs {
    #[command(subcommand)]
    command: ShellCommand,
}
#[derive(Subcommand)]
enum ShellCommand {
    /// Install and configure the Iris Fish shell stack on macOS.
    Setup {
        /// Print planned changes without running commands or writing files.
        #[arg(long)]
        dry_run: bool,
        /// Do not make Fish the login shell.
        #[arg(long)]
        no_default_shell: bool,
        /// Do not install iTerm2 or write the Iris iTerm2 profile.
        #[arg(long)]
        skip_iterm: bool,
    },
    /// Show the detected Iris shell environment without changing it.
    Status,
    /// Diagnose shell setup problems and suggest the setup command.
    Doctor,
}
#[derive(Args)]
struct ForgeArgs {
    #[command(subcommand)]
    command: Option<ForgeCommand>,
}
#[derive(Args)]
struct DispatchArgs {
    #[command(subcommand)]
    command: Option<DispatchCommand>,
}
#[derive(Subcommand)]
enum DispatchCommand {
    /// Push main to origin and set origin/main as its upstream branch.
    Upstream,
}
#[derive(Args)]
struct RealmArgs {
    #[command(subcommand)]
    command: Option<RealmCommand>,
}
#[derive(Subcommand)]
enum RealmCommand {
    /// Include remote-tracking branches.
    All,
}
#[derive(Subcommand)]
enum ForgeCommand {
    /// Stage every changed file.
    All,
}
#[derive(Args)]
struct OutpostArgs {
    #[command(subcommand)]
    command: Option<OutpostCommand>,
}
#[derive(Subcommand)]
enum OutpostCommand {
    /// Add the URL as the origin remote.
    From {
        #[arg(help = "Remote URL")]
        url: String,
    },
}
#[derive(Args)]
struct ComposeArgs {
    #[arg(
        long,
        help = "Use a registered repository by name instead of the current directory"
    )]
    repo: Option<String>,
    #[command(subcommand)]
    command: ComposeCommand,
}
#[derive(Subcommand)]
enum ComposeCommand {
    /// Generate, preview, and optionally create a commit from staged changes.
    Commit,
}

#[derive(Args)]
struct TodoArgs {
    #[command(subcommand)]
    command: TodoCommand,
}
#[derive(Subcommand)]
enum TodoCommand {
    /// Add an open todo.
    Add {
        #[arg(help = "Todo text")]
        title: String,
    },
    /// List open todos.
    List,
    /// Mark an open todo as complete.
    Done {
        #[arg(help = "Open todo ID")]
        id: i64,
    },
}

#[derive(Args)]
struct LogArgs {
    #[command(subcommand)]
    command: Option<LogCommand>,
    #[arg(help = "Work-log message")]
    message: Option<String>,
}
#[derive(Subcommand)]
enum LogCommand {
    /// Show work-log entries from the current local calendar day.
    Today,
}

#[derive(Args)]
struct RepoArgs {
    #[command(subcommand)]
    command: RepoCommand,
}
#[derive(Subcommand)]
enum RepoCommand {
    /// Register an existing Git work tree.
    Add {
        #[arg(help = "Path to a Git work tree")]
        path: PathBuf,
    },
    /// List registered repositories.
    List,
    /// Remove a registered repository by name.
    Remove {
        #[arg(help = "Registered repository name")]
        name: String,
    },
}

#[derive(Args)]
struct GitArgs {
    #[arg(
        long,
        help = "Use a registered repository by name instead of the current directory"
    )]
    repo: Option<String>,
    #[command(subcommand)]
    command: GitCommand,
}
#[derive(Subcommand)]
enum GitCommand {
    /// Show today's commits for registered repositories.
    Today {
        #[arg(long, help = "Include all registered repositories")]
        all: bool,
    },
    /// Manage remotes for the current or selected repository.
    Remote(RemoteArgs),
    /// Fetch from a remote, or the default remote when omitted.
    Fetch {
        #[arg(help = "Remote name")]
        remote: Option<String>,
    },
    /// Pull from an optional remote and branch.
    Pull {
        #[arg(help = "Remote name")]
        remote: Option<String>,
        #[arg(help = "Branch name")]
        branch: Option<String>,
    },
    /// Push to an optional remote and branch.
    Push {
        #[arg(help = "Remote name")]
        remote: Option<String>,
        #[arg(help = "Branch name")]
        branch: Option<String>,
    },
}
#[derive(Args)]
struct RemoteArgs {
    #[command(subcommand)]
    command: RemoteCommand,
}
#[derive(Subcommand)]
enum RemoteCommand {
    /// Add a named remote.
    Add {
        #[arg(help = "Remote name")]
        name: String,
        #[arg(help = "Remote URL")]
        url: String,
    },
    /// List configured remote names.
    List,
    /// Remove a remote by name.
    Remove {
        #[arg(help = "Remote name")]
        name: String,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut store = SqliteStore::open()?;
    let git = ProcessGit;
    match cli.command {
        Command::Todo(args) => match args.command {
            TodoCommand::Add { title } => println!(
                "Added todo {}",
                application::add_todo(&mut store, title)?.id
            ),
            TodoCommand::List => {
                for todo in application::open_todos(&store)? {
                    println!("{}  ○  {}", todo.id, todo.title);
                }
            }
            TodoCommand::Done { id } => {
                application::complete_todo(&mut store, id)?;
                println!("Completed todo {id}");
            }
        },
        Command::Log(args) => match (args.command, args.message) {
            (Some(LogCommand::Today), None) => {
                for log in application::today_logs(&store)? {
                    println!("{}  {}", log.created_at.format("%H:%M"), log.content);
                }
            }
            (None, Some(message)) => {
                application::add_log(&mut store, message)?;
                println!("Logged");
            }
            _ => anyhow::bail!("use `iris log <message>` or `iris log today`"),
        },
        Command::Repo(args) => match args.command {
            RepoCommand::Add { path } => {
                let repo = application::register_repo(&mut store, &git, path)?;
                println!("Registered {}", repo.name);
            }
            RepoCommand::List => {
                for repo in application::repositories(&store)? {
                    println!("{}  {}", repo.name, repo.path.display());
                }
            }
            RepoCommand::Remove { name } => {
                application::remove_repo(&mut store, &name)?;
                println!("Removed {name}");
            }
        },
        Command::Git(args) => match args.command {
            GitCommand::Today { all: true } => render_git(application::git_today(&store, &git)?),
            GitCommand::Today { all: false } => anyhow::bail!("use `iris git today --all`"),
            GitCommand::Remote(remote) => run_remote(
                &store,
                &git,
                args.repo.as_deref(),
                match remote.command {
                    RemoteCommand::Add { name, url } => RemoteOperation::Add { name, url },
                    RemoteCommand::List => RemoteOperation::List,
                    RemoteCommand::Remove { name } => RemoteOperation::Remove { name },
                },
            )?,
            GitCommand::Fetch { remote } => run_remote(
                &store,
                &git,
                args.repo.as_deref(),
                RemoteOperation::Fetch { remote },
            )?,
            GitCommand::Pull { remote, branch } => run_remote(
                &store,
                &git,
                args.repo.as_deref(),
                RemoteOperation::Pull { remote, branch },
            )?,
            GitCommand::Push { remote, branch } => run_remote(
                &store,
                &git,
                args.repo.as_deref(),
                RemoteOperation::Push { remote, branch },
            )?,
        },
        Command::Compose(args) => match args.command {
            ComposeCommand::Commit => compose_commit(&store, &git, args.repo.as_deref())?,
        },
        Command::Forge(args) => match args.command {
            Some(ForgeCommand::All) => run_git(&git, GitOperation::AddAll)?,
            None => forge(&git)?,
        },
        Command::Dispatch(args) => run_git(
            &git,
            match args.command {
                Some(DispatchCommand::Upstream) => GitOperation::PushUpstreamMain,
                None => GitOperation::Push,
            },
        )?,
        Command::Realm(args) => run_git(
            &git,
            GitOperation::Branch {
                all: matches!(args.command, Some(RealmCommand::All)),
            },
        )?,
        Command::Vault => run_git(&git, GitOperation::Stash)?,
        Command::Chronicle => run_git(&git, GitOperation::Log)?,
        Command::Inspect => run_git(&git, GitOperation::Status)?,
        Command::Outpost(args) => run_git(
            &git,
            match args.command {
                Some(OutpostCommand::From { url }) => GitOperation::AddOrigin { url },
                None => GitOperation::Remote,
            },
        )?,
        Command::Today => render_today(application::today(&store, &git)?),
        Command::Shell(args) => {
            let lines = match args.command {
                ShellCommand::Setup {
                    dry_run,
                    no_default_shell,
                    skip_iterm,
                } => iris::shell::setup(iris::shell::ShellSetupOptions {
                    dry_run,
                    change_default_shell: !no_default_shell,
                    configure_iterm: !skip_iterm,
                })?,
                ShellCommand::Status => iris::shell::status()?,
                ShellCommand::Doctor => iris::shell::doctor()?,
            };
            for line in lines {
                println!("{line}");
            }
        }
    }
    Ok(())
}

fn forge(git: &ProcessGit) -> Result<()> {
    let files = application::changed_files(git)?;
    if files.is_empty() {
        println!("Forge cancelled: no changes.");
        return Ok(());
    }
    for (index, path) in files.iter().enumerate() {
        println!("{}  {path}", index + 1);
    }
    print!("Select files to add (for example 1,3,src/main.rs): ");
    io::stdout().flush()?;
    let mut selectors = String::new();
    io::stdin().read_line(&mut selectors)?;
    let Ok(paths) = application::select_changed_files(&files, selectors.trim()) else {
        println!("Forge cancelled.");
        return Ok(());
    };
    if paths.is_empty() {
        println!("Forge cancelled.");
        return Ok(());
    }
    run_git(git, GitOperation::Add { paths })
}

fn run_git(git: &ProcessGit, operation: GitOperation) -> Result<()> {
    let output = application::run_git(git, operation)?;
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    Ok(())
}

fn compose_commit(store: &SqliteStore, git: &ProcessGit, target: Option<&str>) -> Result<()> {
    let (repository, diff, message) =
        application::compose_commit(store, git, &OllamaComposer, target)?;
    println!("\n{message}\n");
    print!("Create this commit? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
        println!("Commit cancelled.");
        return Ok(());
    }
    if git.staged_diff(&repository)? != diff {
        anyhow::bail!("staged changes changed; compose again");
    }
    let output = git.commit(&repository, &message)?;
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    Ok(())
}

fn run_remote(
    store: &SqliteStore,
    git: &ProcessGit,
    target: Option<&str>,
    operation: RemoteOperation,
) -> Result<()> {
    let output = application::run_remote(store, git, target, operation)?;
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    Ok(())
}

fn render_git(activity: iris::application::GitToday) {
    for warning in &activity.warnings {
        eprintln!("Warning: {warning}");
    }
    for repository in &activity.repositories {
        println!("{}", repository.repository.name);
        for commit in &repository.commits {
            println!("  {}  {}", commit.timestamp.format("%H:%M"), commit.message);
        }
    }
}

fn render_today(today: Today) {
    println!("TODO");
    for todo in today.todos {
        println!("{}  ○  {}", todo.id, todo.title);
    }
    println!("\nWORK LOG");
    for log in today.logs {
        println!("{}  {}", log.created_at.format("%H:%M"), log.content);
    }
    println!("\nGIT ACTIVITY");
    render_git(today.git);
}
