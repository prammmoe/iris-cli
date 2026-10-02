use assert_cmd::Command;
use chrono::{Duration, Local, TimeZone, Utc};
use predicates::prelude::*;
use rusqlite::Connection;
use std::fs;
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

fn iris(data_dir: &TempDir) -> Command {
    let mut command = Command::cargo_bin("iris").unwrap();
    command.env("IRIS_DATA_DIR", data_dir.path());
    command
}

#[test]
fn todo_can_be_added_listed_and_completed() {
    let data_dir = TempDir::new().unwrap();

    iris(&data_dir)
        .args(["todo", "add", "Review backend PR"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added todo 1"));

    iris(&data_dir)
        .args(["todo", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1  ○  Review backend PR"));

    iris(&data_dir)
        .args(["todo", "done", "1"])
        .assert()
        .success();

    iris(&data_dir)
        .args(["todo", "list"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn log_is_recorded_and_shown_today() {
    let data_dir = TempDir::new().unwrap();

    iris(&data_dir)
        .args(["log", "Investigated authentication issue"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Logged"));

    iris(&data_dir)
        .args(["log", "today"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Investigated authentication issue",
        ));
}

fn git(args: &[&str], directory: &std::path::Path) {
    let status = ProcessCommand::new("git")
        .args(args)
        .current_dir(directory)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn dashboard_combines_local_data_and_git_activity() {
    let data_dir = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git(&["init"], repository.path());
    ProcessCommand::new("git")
        .args(["config", "user.email", "iris@example.com"])
        .current_dir(repository.path())
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .args(["config", "user.name", "Iris Test"])
        .current_dir(repository.path())
        .status()
        .unwrap();
    fs::write(repository.path().join("README.md"), "# Iris test\n").unwrap();
    git(&["add", "README.md"], repository.path());
    ProcessCommand::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(repository.path())
        .status()
        .unwrap();

    iris(&data_dir)
        .args(["todo", "add", "Ship Iris"])
        .assert()
        .success();
    iris(&data_dir)
        .args(["log", "Built dashboard"])
        .assert()
        .success();
    iris(&data_dir)
        .args(["repo", "add", repository.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Registered"));

    iris(&data_dir)
        .args(["git", "today", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("initial commit"));

    iris(&data_dir)
        .arg("today")
        .assert()
        .success()
        .stdout(predicate::str::contains("Ship Iris"))
        .stdout(predicate::str::contains("Built dashboard"))
        .stdout(predicate::str::contains("initial commit"));
}

#[test]
fn missing_registered_repository_warns_without_failing_git_activity() {
    let data_dir = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    let path = repository.path().to_path_buf();
    git(&["init"], &path);
    iris(&data_dir)
        .args(["repo", "add", path.to_str().unwrap()])
        .assert()
        .success();
    drop(repository);

    iris(&data_dir)
        .args(["git", "today", "--all"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Warning"));
}

#[test]
fn empty_repository_has_no_activity_warning() {
    let data_dir = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git(&["init"], repository.path());
    iris(&data_dir)
        .args(["repo", "add", repository.path().to_str().unwrap()])
        .assert()
        .success();

    iris(&data_dir)
        .args(["git", "today", "--all"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn log_today_uses_the_current_local_calendar_date_not_text_ordering() {
    let data_dir = TempDir::new().unwrap();
    iris(&data_dir).args(["log", "seed"]).assert().success();
    let today = Local::now().date_naive();
    let start = Local
        .from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap();
    let timestamp = (start + Duration::minutes(30))
        .with_timezone(&Utc)
        .to_rfc3339();
    let database = Connection::open(data_dir.path().join("iris.db")).unwrap();
    database
        .execute(
            "INSERT INTO logs (content, created_at) VALUES (?1, ?2)",
            ["local-day log", &timestamp],
        )
        .unwrap();

    iris(&data_dir)
        .args(["log", "today"])
        .assert()
        .success()
        .stdout(predicate::str::contains("local-day log"));
}

#[test]
fn database_starts_at_the_initial_schema_migration() {
    let data_dir = TempDir::new().unwrap();
    iris(&data_dir).arg("today").assert().success();

    let database = Connection::open(data_dir.path().join("iris.db")).unwrap();
    let version: i64 = database
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
}
