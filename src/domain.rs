use chrono::{DateTime, Local};
use std::path::PathBuf;

pub struct Todo {
    pub id: i64,
    pub title: String,
}
pub struct WorkLog {
    pub content: String,
    pub created_at: DateTime<Local>,
}
pub struct RegisteredRepository {
    pub name: String,
    pub path: PathBuf,
}
pub struct GitCommit {
    pub timestamp: DateTime<Local>,
    pub message: String,
}
