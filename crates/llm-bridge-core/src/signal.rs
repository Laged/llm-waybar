use nix::libc;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignalError {
    #[error("Failed to find waybar process")]
    WaybarNotFound,
    #[error("Failed to send signal: {0}")]
    SendFailed(#[from] nix::errno::Errno),
    #[error("Invalid signal number: {0}")]
    InvalidSignal(u8),
}

pub fn signal_waybar(signal_num: u8) -> Result<(), SignalError> {
    let pids = find_waybar_pids()?;

    let sig = Signal::try_from(libc::SIGRTMIN() + signal_num as i32)
        .map_err(|_| SignalError::InvalidSignal(signal_num))?;

    for pid in pids {
        signal::kill(Pid::from_raw(pid), sig)?;
    }

    Ok(())
}

fn find_waybar_pids() -> Result<Vec<i32>, SignalError> {
    let output = Command::new("pgrep")
        .arg("-x")
        .arg("waybar")
        .output()
        .map_err(|_| SignalError::WaybarNotFound)?;

    if !output.status.success() {
        return Err(SignalError::WaybarNotFound);
    }

    let pids: Vec<i32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if pids.is_empty() {
        return Err(SignalError::WaybarNotFound);
    }

    Ok(pids)
}
