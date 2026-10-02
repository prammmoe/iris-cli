// This is a test file for the Iris CLI application. It uses the `assert_cmd` crate to run the CLI commands and check their output. The tests cover various functionalities of the Iris application, including adding and completing todos, logging activities, combining local data with git activity, handling missing repositories, and ensuring that the database starts at the initial schema migration.

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

#[test]
fn remote_commands_work_for_current_and_registered_repositories() {
    let data_dir = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    let remote = TempDir::new().unwrap();
    git(&["init"], repository.path());
    git(&["init", "--bare"], remote.path());
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
    fs::write(repository.path().join("README.md"), "remote test\n").unwrap();
    git(&["add", "README.md"], repository.path());
    git(&["commit", "-m", "remote test"], repository.path());

    iris(&data_dir)
        .current_dir(repository.path())
        .args([
            "git",
            "remote",
            "add",
            "origin",
            remote.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    iris(&data_dir)
        .current_dir(repository.path())
        .args(["git", "remote", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("origin"));
    iris(&data_dir)
        .args(["repo", "add", repository.path().to_str().unwrap()])
        .assert()
        .success();
    iris(&data_dir)
        .args([
            "git",
            "--repo",
            repository.path().file_name().unwrap().to_str().unwrap(),
            "push",
            "origin",
            "HEAD",
        ])
        .assert()
        .success();
    iris(&data_dir)
        .args([
            "git",
            "--repo",
            repository.path().file_name().unwrap().to_str().unwrap(),
            "fetch",
            "origin",
        ])
        .assert()
        .success();
    iris(&data_dir)
        .current_dir(repository.path())
        .args(["git", "remote", "remove", "origin"])
        .assert()
        .success();
}

#[test]
fn push_without_an_upstream_keeps_git_error_output() {
    let data_dir = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git(&["init"], repository.path());
    iris(&data_dir)
        .current_dir(repository.path())
        .args(["git", "push"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Git remote operation failed"));
}

#[test]
fn git_shortcuts_use_the_current_repository() {
    let data_dir = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    let remote = TempDir::new().unwrap();
    git(&["init"], repository.path());
    git(&["init", "--bare"], remote.path());
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
    fs::write(repository.path().join("hello world.txt"), "hello\n").unwrap();

    iris(&data_dir)
        .current_dir(repository.path())
        .args(["forge", "all"])
        .assert()
        .success();
    assert!(
        !ProcessCommand::new("git")
            .args(["diff", "--staged", "--quiet", "--", "hello world.txt"])
            .current_dir(repository.path())
            .status()
            .unwrap()
            .success()
    );

    iris(&data_dir)
        .current_dir(repository.path())
        .arg("inspect")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello world.txt"));
    iris(&data_dir)
        .current_dir(repository.path())
        .arg("realm")
        .assert()
        .success();
    iris(&data_dir)
        .current_dir(repository.path())
        .args(["realm", "all"])
        .assert()
        .success();
    iris(&data_dir)
        .current_dir(repository.path())
        .args(["outpost", "from", remote.path().to_str().unwrap()])
        .assert()
        .success();
    iris(&data_dir)
        .current_dir(repository.path())
        .arg("outpost")
        .assert()
        .success()
        .stdout(predicate::str::contains("origin"));
    git(&["commit", "-m", "initial"], repository.path());
    git(&["push", "-u", "origin", "HEAD"], repository.path());
    fs::write(repository.path().join("second.txt"), "second\n").unwrap();
    git(&["add", "second.txt"], repository.path());
    git(&["commit", "-m", "second"], repository.path());
    iris(&data_dir)
        .current_dir(repository.path())
        .arg("dispatch")
        .assert()
        .success();
    iris(&data_dir)
        .current_dir(repository.path())
        .arg("vault")
        .assert()
        .success();
    iris(&data_dir)
        .current_dir(repository.path())
        .arg("chronicle")
        .assert()
        .success()
        .stdout(predicate::str::contains("second"));
}

#[test]
fn forge_accepts_unique_number_and_path_selectors() {
    let data_dir = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git(&["init"], repository.path());
    fs::write(repository.path().join("first file.txt"), "first\n").unwrap();
    fs::write(repository.path().join("second.txt"), "second\n").unwrap();

    iris(&data_dir)
        .current_dir(repository.path())
        .arg("forge")
        .write_stdin("1,second.txt,1\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("1  first file.txt"))
        .stdout(predicate::str::contains("2  second.txt"));
    for path in ["first file.txt", "second.txt"] {
        assert!(
            !ProcessCommand::new("git")
                .args(["diff", "--staged", "--quiet", "--", path])
                .current_dir(repository.path())
                .status()
                .unwrap()
                .success()
        );
    }
}

#[test]
fn forge_cancels_for_empty_or_invalid_selection() {
    let data_dir = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git(&["init"], repository.path());
    fs::write(repository.path().join("new.txt"), "new\n").unwrap();

    iris(&data_dir)
        .current_dir(repository.path())
        .arg("forge")
        .write_stdin("99\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Forge cancelled"));
    iris(&data_dir)
        .current_dir(repository.path())
        .arg("forge")
        .write_stdin("\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Forge cancelled"));
    assert!(
        ProcessCommand::new("git")
            .args(["diff", "--staged", "--quiet"])
            .current_dir(repository.path())
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn forge_cancels_when_there_are_no_changes() {
    let data_dir = TempDir::new().unwrap();
    let repository = TempDir::new().unwrap();
    git(&["init"], repository.path());

    iris(&data_dir)
        .current_dir(repository.path())
        .arg("forge")
        .assert()
        .success()
        .stdout(predicate::str::contains("Forge cancelled: no changes"));
}

#[test]
fn shortcuts_report_when_current_directory_is_not_a_repository() {
    let data_dir = TempDir::new().unwrap();
    let directory = TempDir::new().unwrap();
    iris(&data_dir)
        .current_dir(directory.path())
        .arg("inspect")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a Git repository"));
}
