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
    Todo(TodoArgs),
    Log(LogArgs),
    Repo(RepoArgs),
    Git(GitArgs),
    Compose(ComposeArgs),
    Forge(ForgeArgs),
    Dispatch,
    Realm(RealmArgs),
    Vault,
    Chronicle,
    Inspect,
    Outpost(OutpostArgs),
    Today,
}
#[derive(Args)]
struct ForgeArgs {
    #[command(subcommand)]
    command: Option<ForgeCommand>,
}
#[derive(Args)]
struct RealmArgs {
    #[command(subcommand)]
    command: Option<RealmCommand>,
}
#[derive(Subcommand)]
enum RealmCommand {
    All,
}
#[derive(Subcommand)]
enum ForgeCommand {
    All,
}
#[derive(Args)]
struct OutpostArgs {
    #[command(subcommand)]
    command: Option<OutpostCommand>,
}
#[derive(Subcommand)]
enum OutpostCommand {
    From { url: String },
}
#[derive(Args)]
struct ComposeArgs {
    #[arg(long)]
    repo: Option<String>,
    #[command(subcommand)]
    command: ComposeCommand,
}
#[derive(Subcommand)]
enum ComposeCommand {
    Commit,
}

#[derive(Args)]
struct TodoArgs {
    #[command(subcommand)]
    command: TodoCommand,
}
#[derive(Subcommand)]
enum TodoCommand {
    Add { title: String },
    List,
    Done { id: i64 },
}

#[derive(Args)]
struct LogArgs {
    #[command(subcommand)]
    command: Option<LogCommand>,
    message: Option<String>,
}
#[derive(Subcommand)]
enum LogCommand {
    Today,
}

#[derive(Args)]
struct RepoArgs {
    #[command(subcommand)]
    command: RepoCommand,
}
#[derive(Subcommand)]
enum RepoCommand {
    Add { path: PathBuf },
    List,
    Remove { name: String },
}

#[derive(Args)]
struct GitArgs {
    #[arg(long)]
    repo: Option<String>,
    #[command(subcommand)]
    command: GitCommand,
}
#[derive(Subcommand)]
enum GitCommand {
    Today {
        #[arg(long)]
        all: bool,
    },
    Remote(RemoteArgs),
    Fetch {
        remote: Option<String>,
    },
    Pull {
        remote: Option<String>,
        branch: Option<String>,
    },
    Push {
        remote: Option<String>,
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
    Add { name: String, url: String },
    List,
    Remove { name: String },
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
        Command::Dispatch => run_git(&git, GitOperation::Push)?,
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
