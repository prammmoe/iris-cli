use anyhow::{Context, Result, bail};
use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const FISH_FORMULA: &str = "fish";
pub const FONT_CASK: &str = "font-meslo-lg-nerd-font";
pub const ITERM_CASK: &str = "iterm2";
pub const TIDE_PLUGIN: &str = "IlanCosman/tide@v6";
const FISHER_BOOTSTRAP: &str = "curl -sL https://raw.githubusercontent.com/jorgebucaran/fisher/main/functions/fisher.fish | source && fisher install jorgebucaran/fisher";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MacArchitecture {
    AppleSilicon,
    Intel,
}

#[derive(Clone, Debug)]
pub struct ShellEnvironment {
    pub architecture: MacArchitecture,
    pub brew_path: Option<PathBuf>,
    pub brew_prefix: Option<PathBuf>,
    pub fish_path: Option<PathBuf>,
    pub current_shell: Option<PathBuf>,
    pub home_dir: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct InstalledShellState {
    pub fish_installed: bool,
    pub font_installed: bool,
    pub iterm_installed: bool,
    pub fish_registered: bool,
    pub fish_is_default: bool,
    pub fisher_installed: bool,
    pub tide_installed: bool,
    pub fish_config_current: bool,
    pub iterm_profile_current: bool,
}

#[derive(Clone, Debug)]
pub struct ShellSetupOptions {
    pub dry_run: bool,
    pub change_default_shell: bool,
    pub configure_iterm: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupAction {
    InstallFormula {
        name: &'static str,
    },
    InstallCask {
        name: &'static str,
    },
    RegisterShell {
        path: PathBuf,
    },
    ChangeDefaultShell {
        path: PathBuf,
    },
    InstallFisher {
        fish_path: PathBuf,
    },
    InstallFishPlugin {
        fish_path: PathBuf,
        plugin: &'static str,
    },
    WriteManagedFishConfig {
        path: PathBuf,
        content: String,
    },
    WriteItermDynamicProfile {
        path: PathBuf,
        content: String,
    },
}

#[derive(Clone, Debug, Default)]
pub struct SetupPlan {
    pub actions: Vec<SetupAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionMode {
    Apply,
    DryRun,
}

pub trait ProcessRunner {
    fn run(&self, program: &Path, args: &[&str], input: Option<&str>) -> Result<ProcessOutput>;
}

#[derive(Clone, Debug)]
pub struct ProcessOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub struct SystemRunner;

impl ProcessRunner for SystemRunner {
    fn run(&self, program: &Path, args: &[&str], input: Option<&str>) -> Result<ProcessOutput> {
        use std::io::Write;
        use std::process::Stdio;

        let mut command = Command::new(program);
        command
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if input.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("could not start {}", program.display()))?;
        if let Some(input) = input {
            child
                .stdin
                .as_mut()
                .context("could not open command input")?
                .write_all(input.as_bytes())?;
        }
        let output = child.wait_with_output()?;
        Ok(ProcessOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub fn setup(options: ShellSetupOptions) -> Result<Vec<String>> {
    let runner = SystemRunner;
    let environment = detect_environment(&runner)?;
    let state = inspect_state(&runner, &environment)?;
    let plan = build_setup_plan(&environment, &state, &options)?;
    let executor = SetupExecutor {
        mode: if options.dry_run {
            ExecutionMode::DryRun
        } else {
            ExecutionMode::Apply
        },
        runner: &runner,
    };
    let action_lines = executor.execute(&plan)?;
    if options.dry_run {
        let mut lines = vec!["Shell setup plan".into(), "✓ Homebrew detected".into()];
        lines.extend(action_lines);
        lines.push("Dry run: no changes made.".into());
        return Ok(lines);
    }
    if plan.actions.is_empty() {
        Ok(vec!["Nothing to do.".into()])
    } else {
        let mut lines = action_lines;
        lines.extend([
            "Shell setup complete.".into(),
            "Restart your terminal or run:".into(),
            "  exec fish".into(),
        ]);
        Ok(lines)
    }
}

pub fn status() -> Result<Vec<String>> {
    let runner = SystemRunner;
    let environment = inspect_environment(&runner)?;
    let state = inspect_state(&runner, &environment)?;
    Ok(render_diagnostics(
        "Iris Shell",
        &environment,
        &state,
        false,
    ))
}

pub fn doctor() -> Result<Vec<String>> {
    let runner = SystemRunner;
    let environment = inspect_environment(&runner)?;
    let state = inspect_state(&runner, &environment)?;
    Ok(render_diagnostics(
        "Iris Shell Doctor",
        &environment,
        &state,
        true,
    ))
}

pub fn detect_environment(runner: &impl ProcessRunner) -> Result<ShellEnvironment> {
    if env::consts::OS != "macos" {
        bail!("Iris shell setup supports macOS only")
    }
    let environment = inspect_environment(runner)?;
    if environment.brew_path.is_none() {
        bail!(
            "Homebrew was not found. Install it first from https://brew.sh, then run `iris shell setup` again."
        )
    }
    Ok(environment)
}

pub fn inspect_environment(runner: &impl ProcessRunner) -> Result<ShellEnvironment> {
    let architecture = architecture_from_machine(
        &command_stdout(runner, Path::new("/usr/bin/uname"), &["-m"]).unwrap_or_default(),
    )?;
    let brew_path = find_command(runner, "brew");
    let brew_prefix = brew_path.as_ref().and_then(|path| {
        command_stdout(runner, path, &["--prefix"])
            .ok()
            .map(PathBuf::from)
    });
    let fish_path = find_command(runner, "fish")
        .or_else(|| brew_prefix.as_ref().map(|prefix| prefix.join("bin/fish")));
    let home_dir = env::var_os("HOME")
        .map(PathBuf::from)
        .context("could not determine home directory")?;
    Ok(ShellEnvironment {
        architecture,
        brew_path,
        brew_prefix,
        fish_path,
        current_shell: login_shell(runner).or_else(|| env::var_os("SHELL").map(PathBuf::from)),
        home_dir,
    })
}

pub fn build_setup_plan(
    environment: &ShellEnvironment,
    state: &InstalledShellState,
    options: &ShellSetupOptions,
) -> Result<SetupPlan> {
    let fish_path = environment.fish_path.as_ref().context(
        "could not resolve the Fish path; install Fish with Homebrew and run setup again",
    )?;
    let mut actions = Vec::new();
    if !state.fish_installed {
        actions.push(SetupAction::InstallFormula { name: FISH_FORMULA });
    }
    if !state.font_installed {
        actions.push(SetupAction::InstallCask { name: FONT_CASK });
    }
    if options.configure_iterm && !state.iterm_installed {
        actions.push(SetupAction::InstallCask { name: ITERM_CASK });
    }
    if !state.fish_registered {
        actions.push(SetupAction::RegisterShell {
            path: fish_path.clone(),
        });
    }
    if options.change_default_shell && !state.fish_is_default {
        actions.push(SetupAction::ChangeDefaultShell {
            path: fish_path.clone(),
        });
    }
    if !state.fisher_installed {
        actions.push(SetupAction::InstallFisher {
            fish_path: fish_path.clone(),
        });
    }
    if !state.tide_installed {
        actions.push(SetupAction::InstallFishPlugin {
            fish_path: fish_path.clone(),
            plugin: TIDE_PLUGIN,
        });
    }
    let fish_config = managed_fish_config();
    let fish_config_path = environment.home_dir.join(".config/fish/conf.d/iris.fish");
    if !state.fish_config_current {
        actions.push(SetupAction::WriteManagedFishConfig {
            path: fish_config_path,
            content: fish_config,
        });
    }
    if options.configure_iterm && !state.iterm_profile_current {
        actions.push(SetupAction::WriteItermDynamicProfile {
            path: dynamic_profile_path(&environment.home_dir),
            content: render_dynamic_profile()?,
        });
    }
    Ok(SetupPlan { actions })
}

pub struct SetupExecutor<'a, R> {
    pub mode: ExecutionMode,
    pub runner: &'a R,
}

impl<R: ProcessRunner> SetupExecutor<'_, R> {
    pub fn execute(&self, plan: &SetupPlan) -> Result<Vec<String>> {
        if self.mode == ExecutionMode::DryRun {
            return Ok(plan.actions.iter().map(describe_action).collect());
        }
        let mut lines = Vec::new();
        for action in &plan.actions {
            if matches!(
                action,
                SetupAction::RegisterShell { .. } | SetupAction::ChangeDefaultShell { .. }
            ) {
                eprintln!(
                    "Iris is about to run a privileged shell action: {}",
                    describe_action(action)
                );
            }
            self.execute_action(action)?;
            lines.push(format!(
                "✓ {}",
                describe_action(action).trim_start_matches("→ ")
            ));
        }
        Ok(lines)
    }

    fn execute_action(&self, action: &SetupAction) -> Result<()> {
        match action {
            SetupAction::InstallFormula { name } => self.run_brew(&["install", name]),
            SetupAction::InstallCask { name } => self.run_brew(&["install", "--cask", name]),
            SetupAction::RegisterShell { path } => self
                .run(
                    Path::new("/usr/bin/sudo"),
                    &["/usr/bin/tee", "-a", "/etc/shells"],
                    Some(&format!("{}\n", path.display())),
                )
                .map(|_| ())
                .with_context(|| {
                    format!(
                        "Could not register Fish in /etc/shells.\nFish path:\n  {}\n\nTry:\n  sudo tee -a /etc/shells",
                        path.display()
                    )
                }),
            SetupAction::ChangeDefaultShell { path } => self
                .run(
                    Path::new("/usr/bin/chsh"),
                    &["-s", path.to_str().context("Fish path is not valid UTF-8")?],
                    None,
                )
                .map(|_| ())
                .with_context(|| {
                    format!(
                        "Could not set Fish as the default shell.\nFish path:\n  {}\n\nTry:\n  chsh -s {}",
                        path.display(),
                        path.display()
                    )
                }),
            SetupAction::InstallFisher { fish_path } => self
                .run(fish_path, &["--no-config", "-c", FISHER_BOOTSTRAP], None)
                .map(|_| ()),
            SetupAction::InstallFishPlugin { fish_path, plugin } => self
                .run(
                    fish_path,
                    &["--no-config", "-c", &format!("fisher install {plugin}")],
                    None,
                )
                .map(|_| ()),
            SetupAction::WriteManagedFishConfig { path, content }
            | SetupAction::WriteItermDynamicProfile { path, content } => {
                atomic_write(path, content)
            }
        }
    }

    fn run_brew(&self, args: &[&str]) -> Result<()> {
        let brew = find_command(self.runner, "brew").context("Homebrew was not found")?;
        self.run(&brew, args, None).map(|_| ())
    }

    fn run(&self, program: &Path, args: &[&str], input: Option<&str>) -> Result<ProcessOutput> {
        let output = self.runner.run(program, args, input)?;
        if !output.success {
            bail!("{} failed: {}", program.display(), output.stderr.trim())
        }
        Ok(output)
    }
}

pub fn managed_fish_config() -> String {
    "# Managed by Iris. Manual changes may be replaced.\n".into()
}

pub fn dynamic_profile_path(home: &Path) -> PathBuf {
    home.join("Library/Application Support/iTerm2/DynamicProfiles/Iris.json")
}

pub fn render_dynamic_profile() -> Result<String> {
    Ok(serde_json::to_string_pretty(&json!({
        "Profiles": [{
            "Name": "Iris",
            "Guid": "9EDEB684-64CC-4DA0-8DF5-49D4B0FAD495",
            "Normal Font": "MesloLGS Nerd Font Mono 13",
            "Columns": 120,
            "Rows": 40,
            "Custom Command": "No"
        }]
    }))? + "\n")
}

fn inspect_state(
    runner: &impl ProcessRunner,
    environment: &ShellEnvironment,
) -> Result<InstalledShellState> {
    let fish_path = environment.fish_path.as_deref();
    let fish_installed = environment
        .brew_path
        .as_ref()
        .is_some_and(|brew| brew_installed(runner, brew, "--formula", FISH_FORMULA));
    let font_installed = environment
        .brew_path
        .as_ref()
        .is_some_and(|brew| brew_installed(runner, brew, "--cask", FONT_CASK));
    let iterm_installed = environment
        .brew_path
        .as_ref()
        .is_some_and(|brew| brew_installed(runner, brew, "--cask", ITERM_CASK));
    let fish_registered = fish_path.is_some_and(|path| shell_is_registered(path).unwrap_or(false));
    let fish_is_default =
        fish_path.is_some_and(|path| environment.current_shell.as_deref() == Some(path));
    let fisher_installed = fish_path.is_some_and(|path| fish_has(runner, path, "fisher"));
    let tide_installed = fish_path.is_some_and(|path| fish_has(runner, path, "tide"));
    let fish_config_current =
        fs::read_to_string(environment.home_dir.join(".config/fish/conf.d/iris.fish"))
            .map(|content| content == managed_fish_config())
            .unwrap_or(false);
    let iterm_profile_current = fs::read_to_string(dynamic_profile_path(&environment.home_dir))
        .map(|content| render_dynamic_profile().is_ok_and(|expected| content == expected))
        .unwrap_or(false);
    Ok(InstalledShellState {
        fish_installed,
        font_installed,
        iterm_installed,
        fish_registered,
        fish_is_default,
        fisher_installed,
        tide_installed,
        fish_config_current,
        iterm_profile_current,
    })
}

fn render_diagnostics(
    title: &str,
    environment: &ShellEnvironment,
    state: &InstalledShellState,
    doctor: bool,
) -> Vec<String> {
    let mark = |ok| {
        if ok {
            "✓"
        } else if doctor {
            "!"
        } else {
            "✗"
        }
    };
    let mut lines = vec![title.into(), String::new(), "Fish".into()];
    lines.push(format!("{} fish installed", mark(state.fish_installed)));
    lines.push(format!("{} fish registered", mark(state.fish_registered)));
    lines.push(format!(
        "{} default shell is fish",
        mark(state.fish_is_default)
    ));
    lines.push(String::new());
    lines.push("Tools".into());
    lines.push(format!("{} fisher", mark(state.fisher_installed)));
    lines.push(format!("{} tide", mark(state.tide_installed)));
    lines.push(String::new());
    lines.push("Fonts".into());
    lines.push(format!("{} MesloLGS Nerd Font", mark(state.font_installed)));
    lines.push(String::new());
    lines.push("Terminal".into());
    lines.push(format!("{} iTerm2", mark(state.iterm_installed)));
    lines.push(format!(
        "{} Iris dynamic profile",
        mark(state.iterm_profile_current)
    ));
    lines.push(String::new());
    lines.push("Configuration".into());
    lines.push(format!(
        "{} {}",
        mark(state.fish_config_current),
        environment
            .home_dir
            .join(".config/fish/conf.d/iris.fish")
            .display()
    ));
    if doctor {
        if state.fish_installed
            && state.fish_registered
            && state.fish_is_default
            && state.fisher_installed
            && state.tide_installed
            && state.font_installed
            && state.iterm_profile_current
            && state.fish_config_current
        {
            lines.push(String::new());
            lines.push("No issues found.".into());
        } else {
            lines.push(String::new());
            lines.push("Suggested fixes:".into());
            lines.push("  iris shell setup".into());
        }
    }
    lines
}

fn describe_action(action: &SetupAction) -> String {
    match action {
        SetupAction::InstallFormula { name } => format!("→ Install {name}"),
        SetupAction::InstallCask { name } => format!("→ Install {name}"),
        SetupAction::RegisterShell { path } => {
            format!("→ Register Fish in /etc/shells ({})", path.display())
        }
        SetupAction::ChangeDefaultShell { path } => {
            format!("→ Set Fish as default shell ({})", path.display())
        }
        SetupAction::InstallFisher { .. } => "→ Install Fisher".into(),
        SetupAction::InstallFishPlugin { plugin, .. } => format!("→ Install {plugin}"),
        SetupAction::WriteManagedFishConfig { path, .. } => format!("→ Write {}", path.display()),
        SetupAction::WriteItermDynamicProfile { path, .. } => format!("→ Write {}", path.display()),
    }
}

fn architecture_from_machine(machine: &str) -> Result<MacArchitecture> {
    match machine.trim() {
        "arm64" => Ok(MacArchitecture::AppleSilicon),
        "x86_64" => Ok(MacArchitecture::Intel),
        other => bail!("unsupported macOS architecture '{other}'"),
    }
}

fn find_command(runner: &impl ProcessRunner, name: &str) -> Option<PathBuf> {
    command_stdout(runner, Path::new("/usr/bin/which"), &[name])
        .ok()
        .map(PathBuf::from)
}

fn login_shell(runner: &impl ProcessRunner) -> Option<PathBuf> {
    let user = env::var("USER").ok()?;
    let output = command_stdout(
        runner,
        Path::new("/usr/bin/dscl"),
        &[".", "-read", &format!("/Users/{user}"), "UserShell"],
    )
    .ok()?;
    output
        .split_once(':')
        .map(|(_, shell)| PathBuf::from(shell.trim()))
}

fn command_stdout(runner: &impl ProcessRunner, program: &Path, args: &[&str]) -> Result<String> {
    let output = runner.run(program, args, None)?;
    if !output.success {
        bail!("{} failed: {}", program.display(), output.stderr.trim())
    }
    Ok(output.stdout.trim().into())
}

fn brew_installed(runner: &impl ProcessRunner, brew: &Path, kind: &str, name: &str) -> bool {
    runner
        .run(brew, &["list", "--versions", kind, name], None)
        .is_ok_and(|output| output.success)
}

fn fish_has(runner: &impl ProcessRunner, fish: &Path, command: &str) -> bool {
    runner
        .run(
            fish,
            &["--no-config", "-c", &format!("type -q {command}")],
            None,
        )
        .is_ok_and(|output| output.success)
}

fn shell_is_registered(path: &Path) -> Result<bool> {
    Ok(fs::read_to_string("/etc/shells")?
        .lines()
        .any(|line| line == path.to_string_lossy()))
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("managed file has no parent directory")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .context("managed file has no name")?
            .to_string_lossy()
    ));
    fs::write(&temporary, content)?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use tempfile::TempDir;

    struct FakeRunner {
        calls: RefCell<Vec<(PathBuf, Vec<String>)>>,
    }
    impl FakeRunner {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
            }
        }
    }
    impl ProcessRunner for FakeRunner {
        fn run(&self, program: &Path, args: &[&str], _: Option<&str>) -> Result<ProcessOutput> {
            self.calls.borrow_mut().push((
                program.to_path_buf(),
                args.iter().map(|arg| (*arg).into()).collect(),
            ));
            Ok(ProcessOutput {
                success: true,
                stdout: "/opt/custom".into(),
                stderr: String::new(),
            })
        }
    }

    fn environment(home: &Path) -> ShellEnvironment {
        ShellEnvironment {
            architecture: MacArchitecture::AppleSilicon,
            brew_path: Some("/opt/custom/bin/brew".into()),
            brew_prefix: Some("/opt/custom".into()),
            fish_path: Some("/opt/custom/bin/fish".into()),
            current_shell: Some("/bin/zsh".into()),
            home_dir: home.into(),
        }
    }

    #[test]
    fn maps_supported_macos_architectures() {
        assert_eq!(
            architecture_from_machine("arm64").unwrap(),
            MacArchitecture::AppleSilicon
        );
        assert_eq!(
            architecture_from_machine("x86_64").unwrap(),
            MacArchitecture::Intel
        );
    }

    #[test]
    fn plan_is_deterministic_and_honors_optional_steps() {
        let temp = TempDir::new().unwrap();
        let environment = environment(temp.path());
        let plan = build_setup_plan(
            &environment,
            &InstalledShellState::default(),
            &ShellSetupOptions {
                dry_run: true,
                change_default_shell: false,
                configure_iterm: false,
            },
        )
        .unwrap();
        assert_eq!(
            plan.actions
                .iter()
                .filter(|action| matches!(
                    action,
                    SetupAction::ChangeDefaultShell { .. }
                        | SetupAction::InstallCask { name: ITERM_CASK }
                        | SetupAction::WriteItermDynamicProfile { .. }
                ))
                .count(),
            0
        );
        assert!(matches!(
            plan.actions.first(),
            Some(SetupAction::InstallFormula { name: FISH_FORMULA })
        ));
    }

    #[test]
    fn managed_fish_file_is_isolated_and_idempotent() {
        let temp = TempDir::new().unwrap();
        let config = temp.path().join(".config/fish/config.fish");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, "set -gx USER_SETTING keep\n").unwrap();
        let target = temp.path().join(".config/fish/conf.d/iris.fish");
        atomic_write(&target, &managed_fish_config()).unwrap();
        atomic_write(&target, &managed_fish_config()).unwrap();
        assert_eq!(
            fs::read_to_string(config).unwrap(),
            "set -gx USER_SETTING keep\n"
        );
        assert_eq!(fs::read_to_string(target).unwrap(), managed_fish_config());
    }

    #[test]
    fn iterm_profile_is_valid_json_in_the_dynamic_profiles_directory() {
        let temp = TempDir::new().unwrap();
        let profile = render_dynamic_profile().unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&profile).unwrap()["Profiles"][0]["Name"],
            "Iris"
        );
        assert!(
            dynamic_profile_path(temp.path())
                .ends_with("Library/Application Support/iTerm2/DynamicProfiles/Iris.json")
        );
    }

    #[test]
    fn dry_run_executor_does_not_run_actions() {
        let runner = FakeRunner::new();
        let plan = SetupPlan {
            actions: vec![SetupAction::InstallFormula { name: FISH_FORMULA }],
        };
        let output = SetupExecutor {
            mode: ExecutionMode::DryRun,
            runner: &runner,
        }
        .execute(&plan)
        .unwrap();
        assert_eq!(output, ["→ Install fish"]);
        assert!(runner.calls.borrow().is_empty());
    }
}
