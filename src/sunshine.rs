// src/sunshine.rs — Sunshine systemd service control

use std::process::Command;

pub fn start() -> Result<(), String> {
    control("start")
}

pub fn stop() -> Result<(), String> {
    control("stop")
}

pub fn is_running() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "sunshine"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn control(action: &str) -> Result<(), String> {
    let output = Command::new("systemctl")
        .args(["--user", action, "sunshine"])
        .output()
        .map_err(|e| format!("Failed to run systemctl: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("systemctl {action} sunshine failed: {stderr}"));
    }

    Ok(())
}
