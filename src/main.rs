use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use iris::application::{self, Today};
use iris::infrastructure::{ProcessGit, SqliteStore};
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
    Today,
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
    #[command(subcommand)]
    command: GitCommand,
}
#[derive(Subcommand)]
enum GitCommand {
    Today {
        #[arg(long)]
        all: bool,
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
        },
        Command::Today => render_today(application::today(&store, &git)?),
    }
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
