use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use common::programs::{Program, ProgramStatus};
use thiserror::Error;
use uuid::Uuid;

use crate::supervisor::{self, SupervisorError};
use crate::{config::HostPlatform, relay::PcObsState};

#[derive(Debug, Error)]
pub enum ProgramError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("Program is already running")]
    AlreadyRunning,
    #[error("Could not start program: {0}")]
    Start(#[from] io::Error),
    #[error("Could not start program: {0}")]
    LaunchFailed(String),
    #[error("Could not stop program: {0}")]
    Stop(#[from] SupervisorError),
}

pub const DASHBOARD_PROGRAM_EDITING_DISABLED: &str =
    "Program editing from the dashboard is turned off on this PC";

pub fn require_dashboard_edits_allowed(allow: bool) -> Result<(), &'static str> {
    if allow {
        Ok(())
    } else {
        Err(DASHBOARD_PROGRAM_EDITING_DISABLED)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObsLifecycleAction {
    Start,
    Stop,
}

pub fn obs_is_running(state: &PcObsState) -> bool {
    matches!(
        state,
        PcObsState::Starting
            | PcObsState::Connected
            | PcObsState::NotResponding
            | PcObsState::WaitingForInput
            | PcObsState::SafeMode
    )
}

pub fn lifecycle_for_transition(
    previous: Option<bool>,
    current: &PcObsState,
    watchdog_restarts: bool,
) -> Option<ObsLifecycleAction> {
    if *current == PcObsState::Unknown {
        return None;
    }
    let running = obs_is_running(current);
    match (previous, running) {
        (None, true) | (Some(false), true) => Some(ObsLifecycleAction::Start),
        (Some(true), false) if *current == PcObsState::Crashed && watchdog_restarts => None,
        (Some(true), false) => Some(ObsLifecycleAction::Stop),
        _ => None,
    }
}

pub fn validate(program: &Program) -> Result<(), &'static str> {
    validate_for_platform(program, HostPlatform::current())
}

fn validate_for_platform(program: &Program, platform: HostPlatform) -> Result<(), &'static str> {
    let name_len = program.name.chars().count();
    if name_len == 0 || name_len > 64 {
        return Err("Program name must be between 1 and 64 characters");
    }
    if !program.path.is_absolute() {
        return Err("Program path must be absolute");
    }
    let metadata = std::fs::metadata(&program.path).map_err(|_| "Program path does not exist")?;
    let is_app_bundle =
        platform == HostPlatform::MacOS && metadata.is_dir() && is_app_bundle_path(&program.path);
    if !metadata.is_file() && !is_app_bundle {
        return Err("Program path must be a file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.is_file() && metadata.permissions().mode() & 0o111 == 0 {
            return Err("Program file is not executable");
        }
    }
    if program.args.len() > 32 {
        return Err("Program can have at most 32 arguments");
    }
    Ok(())
}

pub fn status(programs: &[Program]) -> Vec<ProgramStatus> {
    let system = supervisor::process_snapshot();
    status_with(programs, &system)
}

pub fn status_with(programs: &[Program], system: &sysinfo::System) -> Vec<ProgramStatus> {
    programs
        .iter()
        .cloned()
        .map(|program| ProgramStatus {
            running: is_running_with(&program, system),
            program,
        })
        .collect()
}

pub fn is_running(program: &Program) -> bool {
    let system = supervisor::process_snapshot();
    is_running_with(program, &system)
}

fn is_running_with(program: &Program, system: &sysinfo::System) -> bool {
    let path = resolved_program_path(&program.path, HostPlatform::current());
    let Ok(expected) = path.canonicalize() else {
        return false;
    };
    supervisor::non_thread_processes(system)
        .any(|(_, process)| supervisor::process_executable_matches(process, &path, &expected))
}

pub fn start(program: &Program) -> Result<(), ProgramError> {
    validate(program).map_err(ProgramError::Invalid)?;
    if is_running(program) {
        return Err(ProgramError::AlreadyRunning);
    }

    let launch = command_for_program(&program.path, &program.args, HostPlatform::current());
    let mut command = Command::new(&launch.executable);
    command.args(&launch.args).stdin(Stdio::null());
    if launch.wait_for_exit {
        supervisor::apply_child_env(&mut command);
        let output = command.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let message = if stderr.is_empty() {
                format!("launcher exited with {}", output.status)
            } else {
                stderr
            };
            return Err(ProgramError::LaunchFailed(message));
        }
        return Ok(());
    }
    command.stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(parent) = launch.current_dir {
        command.current_dir(parent);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008 | 0x08000000);
    }
    supervisor::apply_child_env(&mut command);
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub const PROGRAM_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

pub fn stop(program: &Program, shutdown_grace: Duration) -> Result<(), ProgramError> {
    let path = resolved_program_path(&program.path, HostPlatform::current());
    let Ok(expected) = path.canonicalize() else {
        return Ok(());
    };
    let system = supervisor::process_snapshot();
    let pids = supervisor::non_thread_processes(&system)
        .filter_map(|(pid, process)| {
            process
                .exe()
                .is_some_and(|exe| supervisor::process_path_matches(exe, &path, &expected))
                .then_some(pid.as_u32())
        })
        .collect::<Vec<_>>();
    for pid in pids {
        supervisor::terminate_process(pid, shutdown_grace)?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct ProgramCommand {
    executable: PathBuf,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
    wait_for_exit: bool,
}

fn command_for_program(path: &Path, args: &[String], platform: HostPlatform) -> ProgramCommand {
    if platform == HostPlatform::MacOS
        && let Some(bundle) = app_bundle_ancestor(path)
    {
        let mut command_args = vec![
            OsString::from("-a"),
            bundle.as_os_str().to_os_string(),
            OsString::from("--args"),
        ];
        command_args.extend(args.iter().map(OsString::from));
        return ProgramCommand {
            executable: PathBuf::from("/usr/bin/open"),
            args: command_args,
            current_dir: None,
            wait_for_exit: true,
        };
    }
    ProgramCommand {
        executable: path.to_path_buf(),
        args: args.iter().map(OsString::from).collect(),
        current_dir: path.parent().map(Path::to_path_buf),
        wait_for_exit: false,
    }
}

fn is_app_bundle_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("app"))
}

fn app_bundle_ancestor(path: &Path) -> Option<&Path> {
    path.ancestors()
        .find(|ancestor| is_app_bundle_path(ancestor))
}

fn resolved_program_path(path: &Path, platform: HostPlatform) -> PathBuf {
    if platform != HostPlatform::MacOS {
        return path.to_path_buf();
    }
    let Some(bundle) = app_bundle_ancestor(path) else {
        return path.to_path_buf();
    };
    let fallback = bundle.file_stem().unwrap_or_default().to_os_string();
    let executable = plist::Value::from_file(bundle.join("Contents/Info.plist"))
        .ok()
        .and_then(|value| {
            value
                .as_dictionary()?
                .get("CFBundleExecutable")?
                .as_string()
                .map(OsString::from)
        })
        .unwrap_or(fallback);
    bundle.join("Contents/MacOS").join(executable)
}

pub fn find(programs: &[Program], id: Uuid) -> Option<&Program> {
    programs.iter().find(|program| program.id == id)
}

pub fn add(programs: &mut Vec<Program>, program: Program) -> Result<(), &'static str> {
    if find(programs, program.id).is_some() {
        return Err("Program already exists");
    }
    programs.push(program);
    Ok(())
}

pub fn replace(programs: &mut [Program], program: Program) -> Result<Program, &'static str> {
    let Some(existing) = programs.iter_mut().find(|item| item.id == program.id) else {
        return Err("Program not found");
    };
    Ok(std::mem::replace(existing, program))
}

pub fn require_stopped_for_path_change(
    existing: &Program,
    replacement: &Program,
    running: bool,
) -> Result<(), &'static str> {
    if running && existing.path != replacement.path {
        Err("Stop the program before changing its executable")
    } else {
        Ok(())
    }
}

pub fn for_obs_action(programs: &[Program], action: ObsLifecycleAction) -> Vec<&Program> {
    programs
        .iter()
        .filter(|program| match action {
            ObsLifecycleAction::Start => program.start_with_obs,
            ObsLifecycleAction::Stop => program.stop_with_obs,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::time::Duration;
    use std::{fs, path::PathBuf};

    use super::*;
    fn program(path: PathBuf) -> Program {
        Program {
            id: Uuid::new_v4(),
            name: "test program".into(),
            path,
            args: Vec::new(),
            start_with_obs: false,
            stop_with_obs: false,
            close_when_idle: false,
            added_from_dashboard_by: None,
        }
    }

    #[test]
    fn validation_rejects_relative_paths_missing_files_and_too_many_arguments() {
        let mut relative = program(PathBuf::from("relative"));
        assert_eq!(validate(&relative), Err("Program path must be absolute"));

        let mut missing = program(std::env::temp_dir().join(Uuid::new_v4().to_string()));
        assert_eq!(validate(&missing), Err("Program path does not exist"));

        let executable = std::env::current_exe().unwrap();
        let mut too_many = program(executable);
        too_many.args = vec!["arg".into(); 33];
        assert_eq!(
            validate(&too_many),
            Err("Program can have at most 32 arguments")
        );

        relative.args.clear();
        missing.args.clear();
    }

    #[cfg(unix)]
    #[test]
    fn validation_rejects_non_executable_files() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!("program-{}", Uuid::new_v4()));
        fs::write(&path, b"not executable").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            validate(&program(path.clone())),
            Err("Program file is not executable")
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn lifecycle_selects_programs_in_configuration_order() {
        let path = std::env::current_exe().unwrap();
        let mut start = program(path.clone());
        start.name = "start".into();
        start.start_with_obs = true;
        let mut stop = program(path.clone());
        stop.name = "stop".into();
        stop.stop_with_obs = true;
        let mut both = program(path);
        both.name = "both".into();
        both.start_with_obs = true;
        both.stop_with_obs = true;
        let programs = vec![start, stop, both];

        assert_eq!(
            for_obs_action(&programs, ObsLifecycleAction::Start)
                .into_iter()
                .map(|program| program.name.as_str())
                .collect::<Vec<_>>(),
            ["start", "both"]
        );
        assert_eq!(
            for_obs_action(&programs, ObsLifecycleAction::Stop)
                .into_iter()
                .map(|program| program.name.as_str())
                .collect::<Vec<_>>(),
            ["stop", "both"]
        );
    }

    #[test]
    fn lifecycle_for_transition_handles_obs_state_changes() {
        let cases = [
            (
                None,
                PcObsState::Connected,
                false,
                Some(ObsLifecycleAction::Start),
            ),
            (None, PcObsState::NotRunning, false, None),
            (
                Some(false),
                PcObsState::WaitingForInput,
                false,
                Some(ObsLifecycleAction::Start),
            ),
            (Some(true), PcObsState::NotResponding, false, None),
            (
                Some(true),
                PcObsState::NotRunning,
                false,
                Some(ObsLifecycleAction::Stop),
            ),
            (Some(true), PcObsState::Crashed, true, None),
            (
                Some(true),
                PcObsState::Crashed,
                false,
                Some(ObsLifecycleAction::Stop),
            ),
            (Some(false), PcObsState::NotRunning, false, None),
            (None, PcObsState::Unknown, false, None),
        ];

        for (previous, current, watchdog_restarts, expected) in cases {
            assert_eq!(
                lifecycle_for_transition(previous, &current, watchdog_restarts),
                expected,
                "previous={previous:?}, current={current:?}, watchdog_restarts={watchdog_restarts}"
            );
        }
    }

    #[test]
    fn dashboard_edits_can_be_refused_per_setup() {
        assert!(require_dashboard_edits_allowed(true).is_ok());
        assert_eq!(
            require_dashboard_edits_allowed(false),
            Err(DASHBOARD_PROGRAM_EDITING_DISABLED)
        );
    }

    #[test]
    fn add_and_edit_distinguish_duplicate_and_missing_programs() {
        let existing = program(PathBuf::from("/tmp/existing"));
        let id = existing.id;
        let mut programs = vec![existing];
        let duplicate = programs[0].clone();
        assert_eq!(add(&mut programs, duplicate), Err("Program already exists"));
        assert_eq!(
            replace(&mut programs, program(PathBuf::from("/tmp/missing"))),
            Err("Program not found")
        );

        let mut edited = programs[0].clone();
        edited.name = "renamed".into();
        let previous = programs[0].clone();
        assert_eq!(replace(&mut programs, edited.clone()), Ok(previous));
        assert_eq!(find(&programs, id).unwrap().name, "renamed");
    }

    #[test]
    fn a_running_program_must_be_stopped_before_its_path_changes() {
        let existing = program(PathBuf::from("/tmp/original"));
        let mut replacement = existing.clone();
        replacement.path = PathBuf::from("/tmp/new");
        assert_eq!(
            require_stopped_for_path_change(&existing, &replacement, true),
            Err("Stop the program before changing its executable")
        );
        assert!(require_stopped_for_path_change(&existing, &replacement, false).is_ok());
        assert!(require_stopped_for_path_change(&existing, &existing, true).is_ok());
    }

    #[test]
    fn macos_app_commands_use_open_and_non_bundle_paths_remain_direct() {
        let bundle = PathBuf::from("/Applications/TextEdit.app");
        let args = vec!["--example".to_owned()];
        let inside = command_for_program(
            &bundle.join("Contents/MacOS/TextEdit"),
            &args,
            HostPlatform::MacOS,
        );
        assert_eq!(inside.executable, PathBuf::from("/usr/bin/open"));
        assert_eq!(
            inside.args,
            [
                OsString::from("-a"),
                bundle.as_os_str().to_os_string(),
                OsString::from("--args"),
                OsString::from("--example"),
            ]
        );
        assert!(inside.wait_for_exit);

        let selected_bundle = command_for_program(&bundle, &[], HostPlatform::MacOS);
        assert_eq!(selected_bundle.executable, PathBuf::from("/usr/bin/open"));
        assert_eq!(selected_bundle.args[1], bundle.as_os_str());

        let direct =
            command_for_program(Path::new("/usr/local/bin/tool"), &args, HostPlatform::MacOS);
        assert_eq!(direct.executable, PathBuf::from("/usr/local/bin/tool"));
        assert_eq!(direct.args, [OsString::from("--example")]);
        assert!(!direct.wait_for_exit);
    }

    #[test]
    fn macos_bundle_executable_uses_info_plist() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("Sample.app");
        let contents = bundle.join("Contents");
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleExecutable</key><string>SampleBinary</string></dict></plist>"#,
        )
        .unwrap();
        assert_eq!(
            resolved_program_path(&bundle.join("Contents/MacOS/Other"), HostPlatform::MacOS),
            bundle.join("Contents/MacOS/SampleBinary")
        );
    }

    #[test]
    fn macos_validation_accepts_existing_app_bundle_directories() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = temp.path().join("Sample.app");
        fs::create_dir_all(&bundle).unwrap();
        let mut app = program(bundle);
        app.path = app.path.canonicalize().unwrap();
        assert!(validate_for_platform(&app, HostPlatform::MacOS).is_ok());
        assert_eq!(
            validate_for_platform(&app, HostPlatform::Linux),
            Err("Program path must be a file")
        );
    }

    #[cfg(unix)]
    fn copied_sleep() -> (PathBuf, Program) {
        use std::os::unix::fs::PermissionsExt;

        let source = ["/usr/bin/sleep", "/bin/sleep"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.is_file() && !path.is_symlink())
            .expect("a system sleep binary is required for process tests");
        let dir = std::env::temp_dir().join(format!("program-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sleep");
        fs::copy(source, &path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let mut program = program(path.clone());
        program.args = vec!["600".into()];
        (dir, program)
    }

    #[cfg(unix)]
    #[test]
    fn start_refuses_an_already_running_program_and_stop_terminates_it() {
        let (dir, program) = copied_sleep();
        start(&program).unwrap();
        for _ in 0..50 {
            if is_running(&program) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(is_running(&program));
        assert!(matches!(start(&program), Err(ProgramError::AlreadyRunning)));

        stop(&program, Duration::from_secs(1)).unwrap();
        assert!(!is_running(&program));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn program_shutdown_grace_is_five_seconds() {
        assert_eq!(PROGRAM_SHUTDOWN_GRACE, Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn stop_terminates_copied_sleep_with_short_grace() {
        let (dir, program) = copied_sleep();
        start(&program).unwrap();
        for _ in 0..50 {
            if is_running(&program) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(is_running(&program));

        stop(&program, Duration::from_millis(100)).unwrap();

        assert!(!is_running(&program));
        fs::remove_dir_all(dir).unwrap();
    }
}
