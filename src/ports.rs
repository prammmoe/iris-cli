use crate::domain::{GitCommit, GitOutput, RegisteredRepository, RemoteOperation, Todo, WorkLog};
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
    fn repository(&self, name: &str) -> Result<RegisteredRepository>;
    fn remove_repository(&mut self, name: &str) -> Result<()>;
}

pub trait GitReader {
    fn current_repository(&self) -> Result<RegisteredRepository>;
    fn validate_repository(&self, path: &Path) -> Result<RegisteredRepository>;
    fn repository_exists(&self, repository: &RegisteredRepository) -> bool;
    fn commits_today(&self, repository: &RegisteredRepository) -> Result<Vec<GitCommit>>;
    fn run_remote(
        &self,
        repository: &RegisteredRepository,
        operation: &RemoteOperation,
    ) -> Result<GitOutput>;
    fn staged_diff(&self, repository: &RegisteredRepository) -> Result<String>;
    fn commit(&self, repository: &RegisteredRepository, message: &str) -> Result<GitOutput>;
}

pub trait CommitComposer {
    fn compose(&self, diff: &str) -> Result<String>;
}
