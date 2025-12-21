use nix::libc;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::process::Command;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignalError {
    #[error("Failed to find waybar process")]
    WaybarNotFound,
    #[error("Timed out finding waybar process")]
    Timeout,
    #[error("Failed to send signal: {0}")]
    SendFailed(#[from] nix::errno::Errno),
    #[error("Invalid signal number: {0}")]
    InvalidSignal(u8),
}

/// Signal waybar to refresh. Returns Ok even if waybar not found or timeout.
/// This is best-effort - the state file is already written, waybar will poll eventually.
pub fn signal_waybar(signal_num: u8) -> Result<(), SignalError> {
    let pids = match find_waybar_pids() {
        Ok(pids) => pids,
        Err(SignalError::WaybarNotFound | SignalError::Timeout) => return Ok(()),
        Err(e) => return Err(e),
    };

    let sig = Signal::try_from(libc::SIGRTMIN() + signal_num as i32)
        .map_err(|_| SignalError::InvalidSignal(signal_num))?;

    for pid in pids {
        let _ = signal::kill(Pid::from_raw(pid), sig); // Best effort
    }

    Ok(())
}

fn find_waybar_pids() -> Result<Vec<i32>, SignalError> {
    use std::process::Stdio;
    use std::io::Read;

    let mut child = Command::new("pgrep")
        .arg("-x")
        .arg("waybar")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| SignalError::WaybarNotFound)?;

    // Wait with timeout (500ms should be plenty for pgrep under normal load)
    let timeout = Duration::from_millis(500);
    let start = std::time::Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err(SignalError::WaybarNotFound);
                }
                break;
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err(SignalError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return Err(SignalError::WaybarNotFound),
        }
    }

    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut output);
    }

    let pids: Vec<i32> = output
        .lines()
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if pids.is_empty() {
        return Err(SignalError::WaybarNotFound);
    }

    Ok(pids)
}
