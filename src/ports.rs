use crate::domain::{GitCommit, RegisteredRepository, Todo, WorkLog};
use anyhow::Result;
use std::path::Path;

pub trait Store {
    fn add_todo(&mut self, title: &str) -> Result<Todo>;
    fn open_todos(&self) -> Result<Vec<Todo>>;
    fn complete_todo(&mut self, id: i64) -> Result<()>;
    fn add_log(&mut self, content: &str) -> Result<()>;
    fn today_logs(&self) -> Result<Vec<WorkLog>>;
    fn add_repository(&mut self, repository: &RegisteredRepository) -> Result<()>;
    fn repositories(&self) -> Result<Vec<RegisteredRepository>>;
    fn remove_repository(&mut self, name: &str) -> Result<()>;
}

pub trait GitReader {
    fn validate_repository(&self, path: &Path) -> Result<RegisteredRepository>;
    fn repository_exists(&self, repository: &RegisteredRepository) -> bool;
    fn commits_today(&self, repository: &RegisteredRepository) -> Result<Vec<GitCommit>>;
}
