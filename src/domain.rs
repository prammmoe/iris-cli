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
pub enum RemoteOperation {
    Add {
        name: String,
        url: String,
    },
    List,
    Remove {
        name: String,
    },
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
pub enum GitOperation {
    AddAll,
    Add { paths: Vec<String> },
    Push,
    Branch { all: bool },
    Stash,
    Log,
    Status,
    StatusPorcelain,
    Remote,
    AddOrigin { url: String },
}
pub struct GitOutput {
    pub stdout: String,
    pub stderr: String,
}
