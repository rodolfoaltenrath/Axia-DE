//! Session startup, independent of the compositor, applications and JS runtime.
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

pub mod environment;
mod supervisor;
pub type Environment = BTreeMap<OsString, OsString>;

fn checked_environment() -> io::Result<Environment> {
    if unsafe { libc::geteuid() } == 0 {
        return Err(io::Error::other("Start Axia DE as a regular user."));
    }
    let env: Environment = std::env::vars_os().collect();
    if !env
        .get(std::ffi::OsStr::new("XDG_RUNTIME_DIR"))
        .filter(|s| !s.is_empty())
        .is_some_and(|s| Path::new(s).is_dir())
    {
        return Err(io::Error::other(
            "A login session with XDG_RUNTIME_DIR is required (logind or seatd).",
        ));
    }
    Ok(env)
}

fn session_environment(mut env: Environment) -> Environment {
    for (key, value) in [
        ("XDG_SESSION_TYPE", "wayland"),
        ("XDG_SESSION_DESKTOP", "axia"),
        ("XDG_CURRENT_DESKTOP", "Axia"),
        ("DESKTOP_SESSION", "axia"),
    ] {
        env.insert(key.into(), value.into());
    }
    env
}

fn bus_missing(env: &Environment) -> bool {
    env.get(std::ffi::OsStr::new("DBUS_SESSION_BUS_ADDRESS"))
        .is_none_or(|s| s.is_empty())
}

pub fn entry(run: fn() -> io::Result<i32>) -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("Axia: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Public command: exec the compositor, preserving PID, arguments and signals.
pub fn direct() -> io::Result<i32> {
    let env = session_environment(checked_environment()?);
    let executable = std::env::current_exe()?;
    let bin = executable
        .parent()
        .ok_or_else(|| io::Error::other("session binary has no directory"))?;
    let mut command = if bus_missing(&env) {
        let mut command = Command::new("dbus-run-session");
        command.args([
            std::ffi::OsStr::new("--"),
            std::ffi::OsStr::new("./axia-server"),
        ]);
        command
    } else {
        Command::new("./axia-server")
    };
    Err(command
        .args(std::env::args_os().skip(1))
        .current_dir(bin)
        .envs(env)
        .exec())
}

/// Display-manager command: supervise the compositor and independent agent.
pub fn desktop() -> io::Result<i32> {
    let env = checked_environment()?;
    let executable = std::env::current_exe()?;
    if bus_missing(&env) {
        return Err(Command::new("dbus-run-session")
            .arg("--")
            .arg(&executable)
            .args(std::env::args_os().skip(1))
            .exec());
    }
    let root: PathBuf = executable
        .parent()
        .ok_or_else(|| io::Error::other("session binary has no directory"))?
        .into();
    supervisor::run(
        &root,
        session_environment(env),
        std::env::args_os().skip(1).collect(),
    )
}
