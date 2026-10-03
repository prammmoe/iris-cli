use crate::domain::{GitCommit, GitOperation, GitOutput, RegisteredRepository, Todo, WorkLog};
use crate::ports::{GitReader, Store};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use directories::ProjectDirs;
use rusqlite::{Connection, params};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

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
pub struct OllamaComposer;

impl crate::ports::CommitComposer for OllamaComposer {
    fn compose(&self, diff: &str) -> Result<String> {
        let prompt = crate::application::build_commit_prompt(diff);
        let message = run_ollama(&prompt)?;
        if crate::application::validate_commit_message(&message).is_ok() {
            return Ok(message);
        }
        let retry = run_ollama(&format!(
            "{prompt}\n\nYour prior response did not match the required structure. Output only a valid commit message now."
        ))?;
        if crate::application::validate_commit_message(&retry).is_err() {
            bail!("Ollama returned an invalid commit message:\n{retry}");
        }
        Ok(retry)
    }
}

fn run_ollama(prompt: &str) -> Result<String> {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "type": { "type": "string", "enum": ["feat", "fix", "docs", "refactor", "perf", "test", "build", "ci", "chore"] },
            "subject": { "type": "string", "description": "actual lowercase English commit subject based on the diff" },
            "changed": { "type": "array", "items": { "type": "string" }, "description": "actual changes visible in the diff" },
            "why": { "type": "array", "items": { "type": "string" }, "description": "reasons supported by the diff" },
            "bug": { "type": "string", "description": "actual bug for fix commits, otherwise empty" },
            "fix": { "type": "string", "description": "actual fix for fix commits, otherwise empty" }
        },
        "required": ["type", "subject", "changed", "why", "bug", "fix"],
        "additionalProperties": false
    });
    let payload = serde_json::json!({
        "model": "qwen3:1.7b",
        "system": "Return only the requested JSON object. Never explain the input.",
        "prompt": prompt,
        "format": schema,
        "stream": false,
        "options": { "num_ctx": 16384, "temperature": 0 }
    })
    .to_string();
    let mut stream = TcpStream::connect("127.0.0.1:11434")
        .context("could not connect to Ollama; ensure it is running")?;
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    let request = format!(
        "POST /api/generate HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .context("Ollama returned an invalid HTTP response")?;
    if !headers.starts_with("HTTP/1.1 200") {
        bail!("Ollama compose failed:\n{body}");
    }
    let body = if headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        decode_chunked(body)?
    } else {
        body.to_owned()
    };
    let body: serde_json::Value =
        serde_json::from_str(&body).context("Ollama returned invalid JSON")?;
    let generated = body["response"]
        .as_str()
        .context("Ollama response did not contain generated text")?;
    let message = structured_or_plain_message(generated)?;
    if message.is_empty() {
        bail!("Ollama returned an empty commit message");
    }
    Ok(message)
}

fn structured_or_plain_message(generated: &str) -> Result<String> {
    let generated = generated
        .trim()
        .strip_prefix("```json")
        .or_else(|| generated.trim().strip_prefix("```"))
        .unwrap_or(generated.trim())
        .strip_suffix("```")
        .unwrap_or(generated.trim())
        .trim();
    match serde_json::from_str::<serde_json::Value>(generated) {
        Ok(parts) => format_commit_message(&parts),
        Err(_) => Ok(generated.to_owned()),
    }
}

fn format_commit_message(parts: &serde_json::Value) -> Result<String> {
    let kind = parts["type"]
        .as_str()
        .context("Ollama response did not contain a commit type")?;
    let subject = parts["subject"]
        .as_str()
        .context("Ollama response did not contain a commit subject")?;
    let changed = message_bullets(&parts["changed"], "changed")?;
    let why = message_bullets(&parts["why"], "why")?;
    let mut message = format!("{kind}: {subject}\n\nChanged\n{changed}\nWhy\n{why}");
    if kind == "fix" {
        let bug = parts["bug"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .context("Ollama response did not contain a bug description")?;
        let fix = parts["fix"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .context("Ollama response did not contain a fix description")?;
        message.push_str(&format!("\nBugs And Fixes\n- Bug: {bug}\n- Fix: {fix}"));
    }
    Ok(message)
}

fn message_bullets(value: &serde_json::Value, name: &str) -> Result<String> {
    let bullets = value
        .as_array()
        .context(format!("Ollama response did not contain {name} bullets"))?;
    if bullets.is_empty() {
        bail!("Ollama response did not contain {name} bullets");
    }
    bullets
        .iter()
        .map(|bullet| {
            let bullet = bullet
                .as_str()
                .filter(|bullet| !bullet.trim().is_empty() && !bullet.contains('\n'))
                .context("Ollama returned an invalid bullet")?;
            Ok(format!("- {bullet}"))
        })
        .collect::<Result<Vec<_>>>()
        .map(|bullets| bullets.join("\n"))
}

fn decode_chunked(body: &str) -> Result<String> {
    let mut body = body;
    let mut decoded = String::new();
    loop {
        let (size, rest) = body
            .split_once("\r\n")
            .context("Ollama returned malformed chunked data")?;
        let size = usize::from_str_radix(size.split(';').next().unwrap_or_default(), 16)
            .context("Ollama returned an invalid chunk size")?;
        if size == 0 {
            return Ok(decoded);
        }
        if rest.len() < size + 2 || !rest[size..].starts_with("\r\n") {
            bail!("Ollama returned a truncated chunk");
        }
        decoded.push_str(&rest[..size]);
        body = &rest[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_chunked, format_commit_message, structured_or_plain_message};

    #[test]
    fn decodes_a_chunked_ollama_response() {
        assert_eq!(
            decode_chunked("6\r\n{\"ok\":\r\n5\r\ntrue}\r\n0\r\n\r\n").unwrap(),
            "{\"ok\":true}"
        );
    }

    #[test]
    fn formats_structured_model_output_as_a_valid_commit_message() {
        let parts = serde_json::json!({
            "type": "feat",
            "subject": "add git shortcuts",
            "changed": ["add current-directory git commands"],
            "why": ["make common actions concise"],
            "bug": "",
            "fix": ""
        });
        let message = format_commit_message(&parts).unwrap();
        assert!(crate::application::validate_commit_message(&message).is_ok());
    }

    #[test]
    fn accepts_plain_text_when_ollama_ignores_the_schema() {
        let message =
            "feat: add shortcuts\n\nChanged\n- add git aliases\nWhy\n- make common actions concise";
        assert_eq!(structured_or_plain_message(message).unwrap(), message);
    }
}

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
    fn run_git(
        &self,
        repository: &RegisteredRepository,
        operation: GitOperation,
    ) -> Result<GitOutput> {
        let args: Vec<String> = match operation {
            GitOperation::AddAll => vec!["add".into(), ".".into()],
            GitOperation::Add { paths } => {
                let mut args = vec!["add".into(), "--".into()];
                args.extend(paths);
                args
            }
            GitOperation::Push => vec!["push".into()],
            GitOperation::PushUpstreamMain => vec![
                "push".into(),
                "--set-upstream".into(),
                "origin".into(),
                "main".into(),
            ],
            GitOperation::Branch { all: false } => vec!["branch".into()],
            GitOperation::Branch { all: true } => vec!["branch".into(), "--all".into()],
            GitOperation::Stash => vec!["stash".into()],
            GitOperation::Log => vec!["log".into()],
            GitOperation::Status => vec!["status".into()],
            GitOperation::StatusPorcelain => {
                vec!["status".into(), "--porcelain=v1".into(), "-z".into()]
            }
            GitOperation::Remote => vec!["remote".into()],
            GitOperation::AddOrigin { url } => {
                vec!["remote".into(), "add".into(), "origin".into(), url]
            }
        };
        let output = Command::new("git")
            .args(&args)
            .current_dir(&repository.path)
            .output()?;
        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        if !output.status.success() {
            bail!("Git operation failed:\n{stderr}");
        }
        Ok(GitOutput { stdout, stderr })
    }
    fn staged_diff(&self, repository: &RegisteredRepository) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "--staged", "--no-ext-diff"])
            .current_dir(&repository.path)
            .output()?;
        if !output.status.success() {
            bail!("could not read staged changes");
        }
        Ok(String::from_utf8(output.stdout)?)
    }
    fn commit(
        &self,
        repository: &RegisteredRepository,
        message: &str,
    ) -> Result<crate::domain::GitOutput> {
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&repository.path)
            .output()?;
        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        if !output.status.success() {
            bail!("Git commit failed:\n{stderr}");
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
