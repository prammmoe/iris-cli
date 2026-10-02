use crate::domain::{RegisteredRepository, Todo, WorkLog};
use crate::ports::{GitReader, Store};
use anyhow::{Result, bail};
use std::path::PathBuf;

pub struct RepositoryActivity {
    pub repository: RegisteredRepository,
    pub commits: Vec<crate::domain::GitCommit>,
}
pub struct GitToday {
    pub repositories: Vec<RepositoryActivity>,
    pub warnings: Vec<String>,
}
pub struct Today {
    pub todos: Vec<Todo>,
    pub logs: Vec<WorkLog>,
    pub git: GitToday,
}

pub fn add_todo(store: &mut impl Store, title: String) -> Result<Todo> {
    let title = title.trim();
    if title.is_empty() {
        bail!("todo title cannot be empty");
    }
    store.add_todo(title)
}
pub fn open_todos(store: &impl Store) -> Result<Vec<Todo>> {
    store.open_todos()
}
pub fn complete_todo(store: &mut impl Store, id: i64) -> Result<()> {
    store.complete_todo(id)
}
pub fn add_log(store: &mut impl Store, content: String) -> Result<()> {
    let content = content.trim();
    if content.is_empty() {
        bail!("log message cannot be empty");
    }
    store.add_log(content)
}
pub fn today_logs(store: &impl Store) -> Result<Vec<WorkLog>> {
    store.today_logs()
}
pub fn register_repo(
    store: &mut impl Store,
    git: &impl GitReader,
    path: PathBuf,
) -> Result<RegisteredRepository> {
    let path = path
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("repository path does not exist"))?;
    if !path.is_dir() || !git.is_repository(&path)? {
        bail!("path is not a Git repository");
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("repository path has no name"))?
        .to_owned();
    let repository = RegisteredRepository { name, path };
    store.add_repository(&repository)?;
    Ok(repository)
}
pub fn repositories(store: &impl Store) -> Result<Vec<RegisteredRepository>> {
    store.repositories()
}
pub fn remove_repo(store: &mut impl Store, name: &str) -> Result<()> {
    store.remove_repository(name)
}
pub fn git_today(store: &impl Store, git: &impl GitReader) -> Result<GitToday> {
    let mut result = GitToday {
        repositories: Vec::new(),
        warnings: Vec::new(),
    };
    for repository in store.repositories()? {
        if !repository.path.exists() {
            result
                .warnings
                .push(format!("repository '{}' no longer exists", repository.name));
            continue;
        }
        match git.commits_today(&repository) {
            Ok(commits) => result.repositories.push(RepositoryActivity {
                repository,
                commits,
            }),
            Err(_) => result.warnings.push(format!(
                "could not read Git history for '{}'",
                repository.name
            )),
        }
    }
    Ok(result)
}
pub fn today(store: &impl Store, git: &impl GitReader) -> Result<Today> {
    Ok(Today {
        todos: store.open_todos()?,
        logs: store.today_logs()?,
        git: git_today(store, git)?,
    })
}
