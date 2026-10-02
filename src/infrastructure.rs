use crate::domain::{GitCommit, RegisteredRepository, Todo, WorkLog};
use crate::ports::{GitReader, Store};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Local, TimeZone};
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
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS todos (id INTEGER PRIMARY KEY, title TEXT NOT NULL, completed INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, completed_at TEXT);
             CREATE TABLE IF NOT EXISTS logs (id INTEGER PRIMARY KEY, content TEXT NOT NULL, created_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS repositories (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, path TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL);",
        )?;
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
        let start = Local
            .from_local_datetime(&Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap())
            .single()
            .unwrap()
            .to_rfc3339();
        let mut statement = self.0.prepare(
            "SELECT content, created_at FROM logs WHERE created_at >= ?1 ORDER BY created_at",
        )?;
        Ok(statement
            .query_map([start], |row| {
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
            .collect::<rusqlite::Result<_>>()?)
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
    fn is_repository(&self, path: &Path) -> Result<bool> {
        Ok(Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(path)
            .output()?
            .status
            .success())
    }
    fn commits_today(&self, repository: &RegisteredRepository) -> Result<Vec<GitCommit>> {
        let start = Local
            .from_local_datetime(&Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap())
            .single()
            .unwrap();
        let end = start + Duration::days(1);
        let output = Command::new("git")
            .args([
                "log",
                "--format=%cI%x1f%s",
                "--since",
                &start.to_rfc3339(),
                "--until",
                &end.to_rfc3339(),
            ])
            .current_dir(&repository.path)
            .output()?;
        if !output.status.success() {
            bail!("git log failed");
        }
        String::from_utf8(output.stdout)?
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
            .collect()
    }
}
