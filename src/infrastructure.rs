use crate::domain::{GitCommit, RegisteredRepository, Todo, WorkLog};
use crate::ports::{GitReader, Store};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use directories::ProjectDirs;
use rusqlite::{Connection, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct SqliteStore(Connection);

impl SqliteStore {
    pub fn open() -> Result<Self> {
        let directory = std::env::var_os("IRIS_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                ProjectDirs::from("com", "iris", "iris")
                    .map(|dirs| dirs.data_local_dir().to_path_buf())
            })
            .context("could not determine Iris data directory")?;
        fs::create_dir_all(&directory)?;
        let connection = Connection::open(directory.join("iris.db"))?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        match version {
            0 => connection.execute_batch(
                "BEGIN;
                 CREATE TABLE todos (id INTEGER PRIMARY KEY, title TEXT NOT NULL, completed INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, completed_at TEXT);
                 CREATE TABLE logs (id INTEGER PRIMARY KEY, content TEXT NOT NULL, created_at TEXT NOT NULL);
                 CREATE TABLE repositories (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, path TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL);
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?,
            1 => {}
            _ => bail!("database schema version {version} is newer than Iris supports"),
        }
        Ok(Self(connection))
    }
}

impl Store for SqliteStore {
    fn add_todo(&mut self, title: &str) -> Result<Todo> {
        self.0.execute(
            "INSERT INTO todos (title, created_at) VALUES (?1, ?2)",
            params![title, Local::now().to_rfc3339()],
        )?;
        Ok(Todo {
            id: self.0.last_insert_rowid(),
            title: title.to_owned(),
        })
    }
    fn open_todos(&self) -> Result<Vec<Todo>> {
        let mut statement = self
            .0
            .prepare("SELECT id, title FROM todos WHERE completed = 0 ORDER BY id")?;
        Ok(statement
            .query_map([], |row| {
                Ok(Todo {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    fn complete_todo(&mut self, id: i64) -> Result<()> {
        if self.0.execute(
            "UPDATE todos SET completed = 1, completed_at = ?1 WHERE id = ?2 AND completed = 0",
            params![Local::now().to_rfc3339(), id],
        )? == 0
        {
            bail!("open todo {id} was not found");
        }
        Ok(())
    }
    fn add_log(&mut self, content: &str) -> Result<()> {
        self.0.execute(
            "INSERT INTO logs (content, created_at) VALUES (?1, ?2)",
            params![content, Local::now().to_rfc3339()],
        )?;
        Ok(())
    }
    fn today_logs(&self) -> Result<Vec<WorkLog>> {
        let today = Local::now().date_naive();
        let mut statement = self.0.prepare("SELECT content, created_at FROM logs")?;
        let mut logs = statement
            .query_map([], |row| {
                let timestamp: String = row.get(1)?;
                let created_at = DateTime::parse_from_rfc3339(&timestamp)
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?
                    .with_timezone(&Local);
                Ok(WorkLog {
                    content: row.get(0)?,
                    created_at,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // ponytail: scans personal logs; add indexed UTC ranges if volume becomes material.
        logs.retain(|log| log.created_at.date_naive() == today);
        logs.sort_by_key(|log| log.created_at);
        Ok(logs)
    }
    fn add_repository(&mut self, repository: &RegisteredRepository) -> Result<()> {
        self.0
            .execute(
                "INSERT INTO repositories (name, path, created_at) VALUES (?1, ?2, ?3)",
                params![
                    repository.name,
                    repository.path.to_string_lossy(),
                    Local::now().to_rfc3339()
                ],
            )
            .map_err(|error| anyhow::anyhow!("could not register repository: {error}"))?;
        Ok(())
    }
    fn repositories(&self) -> Result<Vec<RegisteredRepository>> {
        let mut statement = self
            .0
            .prepare("SELECT name, path FROM repositories ORDER BY name")?;
        Ok(statement
            .query_map([], |row| {
                Ok(RegisteredRepository {
                    name: row.get(0)?,
                    path: PathBuf::from(row.get::<_, String>(1)?),
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    fn repository(&self, name: &str) -> Result<RegisteredRepository> {
        self.0
            .query_row(
                "SELECT name, path FROM repositories WHERE name = ?1",
                [name],
                |row| {
                    Ok(RegisteredRepository {
                        name: row.get(0)?,
                        path: PathBuf::from(row.get::<_, String>(1)?),
                    })
                },
            )
            .map_err(|_| anyhow::anyhow!("registered repository '{name}' was not found"))
    }
    fn remove_repository(&mut self, name: &str) -> Result<()> {
        if self
            .0
            .execute("DELETE FROM repositories WHERE name = ?1", [name])?
            == 0
        {
            bail!("repository '{name}' was not found");
        }
        Ok(())
    }
}

pub struct ProcessGit;

impl GitReader for ProcessGit {
    fn current_repository(&self) -> Result<RegisteredRepository> {
        self.validate_repository(&std::env::current_dir()?)
    }
    fn validate_repository(&self, path: &Path) -> Result<RegisteredRepository> {
        let path = path
            .canonicalize()
            .map_err(|_| anyhow::anyhow!("repository path does not exist"))?;
        if !path.is_dir()
            || !Command::new("git")
                .args(["rev-parse", "--is-inside-work-tree"])
                .current_dir(&path)
                .output()?
                .status
                .success()
        {
            bail!("path is not a Git repository");
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("repository path has no name")?
            .to_owned();
        Ok(RegisteredRepository { name, path })
    }
    fn repository_exists(&self, repository: &RegisteredRepository) -> bool {
        repository.path.exists()
    }
    fn commits_today(&self, repository: &RegisteredRepository) -> Result<Vec<GitCommit>> {
        let output = Command::new("git")
            .args(["log", "--format=%cI%x1f%s"])
            .current_dir(&repository.path)
            .output()?;
        if !output.status.success() {
            if Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&repository.path)
                .output()?
                .status
                .success()
            {
                return Ok(Vec::new());
            }
            bail!("git log failed");
        }
        let today = Local::now().date_naive();
        let mut commits = String::from_utf8(output.stdout)?
            .lines()
            .map(|line| {
                let (timestamp, message) = line
                    .split_once('\u{1f}')
                    .context("unexpected git log output")?;
                Ok(GitCommit {
                    timestamp: DateTime::parse_from_rfc3339(timestamp)?.with_timezone(&Local),
                    message: message.to_owned(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        // ponytail: scans each repository history; restore Git date bounds if profiling needs it.
        commits.retain(|commit| commit.timestamp.date_naive() == today);
        commits.sort_by_key(|commit| commit.timestamp);
        Ok(commits)
    }
    fn run_remote(
        &self,
        repository: &RegisteredRepository,
        operation: &crate::domain::RemoteOperation,
    ) -> Result<crate::domain::GitOutput> {
        let args = match operation {
            crate::domain::RemoteOperation::Add { name, url } => {
                vec!["remote".into(), "add".into(), name.clone(), url.clone()]
            }
            crate::domain::RemoteOperation::List => vec!["remote".into()],
            crate::domain::RemoteOperation::Remove { name } => {
                vec!["remote".into(), "remove".into(), name.clone()]
            }
            crate::domain::RemoteOperation::Fetch { remote } => {
                with_optional(vec!["fetch".into()], remote)
            }
            crate::domain::RemoteOperation::Pull { remote, branch } => {
                with_optional(with_optional(vec!["pull".into()], remote), branch)
            }
            crate::domain::RemoteOperation::Push { remote, branch } => {
                with_optional(with_optional(vec!["push".into()], remote), branch)
            }
        };
        let output = Command::new("git")
            .args(&args)
            .current_dir(&repository.path)
            .output()?;
        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        if !output.status.success() {
            bail!("Git remote operation failed:\n{stderr}");
        }
        Ok(crate::domain::GitOutput { stdout, stderr })
    }
}

fn with_optional(mut args: Vec<String>, value: &Option<String>) -> Vec<String> {
    if let Some(value) = value {
        args.push(value.clone());
    }
    args
}
