use std::{
    io,
    process::{Command, Stdio},
};

use common::programs::{Program, ProgramStatus};
use thiserror::Error;
use uuid::Uuid;

use crate::supervisor::{self, Supervisor, SupervisorError};

#[derive(Debug, Error)]
pub enum ProgramError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("Program is already running")]
    AlreadyRunning,
    #[error("Could not start program: {0}")]
    Start(#[from] io::Error),
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

pub fn validate(program: &Program) -> Result<(), &'static str> {
    let name_len = program.name.chars().count();
    if name_len == 0 || name_len > 64 {
        return Err("Program name must be between 1 and 64 characters");
    }
    if !program.path.is_absolute() {
        return Err("Program path must be absolute");
    }
    let metadata = std::fs::metadata(&program.path).map_err(|_| "Program path does not exist")?;
    if !metadata.is_file() {
        return Err("Program path must be a file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("Program file is not executable");
        }
    }
    if program.args.len() > 32 {
        return Err("Program can have at most 32 arguments");
    }
    Ok(())
}

pub fn status(programs: &[Program]) -> Vec<ProgramStatus> {
    programs
        .iter()
        .cloned()
        .map(|program| ProgramStatus {
            running: is_running(&program),
            program,
        })
        .collect()
}

pub fn is_running(program: &Program) -> bool {
    let Ok(expected) = program.path.canonicalize() else {
        return false;
    };
    let system = supervisor::process_snapshot();
    supervisor::non_thread_processes(&system).any(|(_, process)| {
        process
            .exe()
            .is_some_and(|exe| supervisor::same_path(exe, &expected))
    })
}

pub fn start(program: &Program) -> Result<(), ProgramError> {
    validate(program).map_err(ProgramError::Invalid)?;
    if is_running(program) {
        return Err(ProgramError::AlreadyRunning);
    }

    let mut command = Command::new(&program.path);
    command
        .args(&program.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(parent) = program.path.parent() {
        command.current_dir(parent);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x00000008 | 0x08000000);
    }
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn stop(program: &Program, supervisor: &Supervisor) -> Result<(), ProgramError> {
    let Ok(expected) = program.path.canonicalize() else {
        return Ok(());
    };
    let system = supervisor::process_snapshot();
    let pids = supervisor::non_thread_processes(&system)
        .filter_map(|(pid, process)| {
            process
                .exe()
                .is_some_and(|exe| supervisor::same_path(exe, &expected))
                .then_some(pid.as_u32())
        })
        .collect::<Vec<_>>();
    for pid in pids {
        supervisor::terminate_process(pid, supervisor.shutdown_grace())?;
    }
    Ok(())
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
    use std::path::PathBuf;
    #[cfg(unix)]
    use std::{fs, time::Duration};

    use super::*;
    fn program(path: PathBuf) -> Program {
        Program {
            id: Uuid::new_v4(),
            name: "test program".into(),
            path,
            args: Vec::new(),
            start_with_obs: false,
            stop_with_obs: false,
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

        let supervisor = Supervisor::default();
        stop(&program, &supervisor).unwrap();
        assert!(!is_running(&program));
        fs::remove_dir_all(dir).unwrap();
    }
}
