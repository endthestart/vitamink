// src/display.rs — Display types, parsing, and control via kscreen-doctor
//
// Rust module system: each .rs file in src/ is a module.
// main.rs uses `mod display;` to include it, then accesses items with `display::`.
// Items need `pub` to be visible outside the module.

use std::fs;
use std::process::Command;

// ---- Data Types ----

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum DisplayState {
    Enabled,
    Disabled,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ConnectionState {
    Connected,
    Disconnected,
}

// Clone + Copy: these are small enums (just a tag, no heap data).
// Clone lets you call .clone(), Copy makes assignment automatically copy
// instead of "move" (Rust's default ownership transfer).
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum DpmsState {
    On,
    Off,
    Unknown,
}

#[derive(Debug)]
pub struct Mode {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub refresh: f64,
    pub preferred: bool,
    pub current: bool,
}

#[derive(Debug)]
pub struct Display {
    pub index: u32,
    pub name: String,
    pub uuid: String,
    pub state: DisplayState,
    pub connection: ConnectionState,
    pub modes: Vec<Mode>,
}

// ---- Wayland Environment ----

fn wayland_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("WAYLAND_DISPLAY", "wayland-0"),
        ("DISPLAY", ":0"),
    ]
}

// ---- Shell Commands ----

fn run_kscreen_doctor(args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("kscreen-doctor");
    for (key, val) in wayland_env() {
        cmd.env(key, val);
    }
    for arg in args {
        cmd.arg(arg);
    }

    let output = cmd.output().map_err(|e| format!("Failed to run kscreen-doctor: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("kscreen-doctor failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(strip_ansi(&stdout))
}

fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            for esc_ch in chars.by_ref() {
                if esc_ch.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

// ---- Parsing ----

pub fn get_displays() -> Result<Vec<Display>, String> {
    let raw = run_kscreen_doctor(&["-o"])?;
    parse_displays(&raw)
}

fn parse_displays(output: &str) -> Result<Vec<Display>, String> {
    let mut displays = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut header_line: Option<&str> = None;

    for line in output.lines() {
        if line.starts_with("Output:") {
            if let Some(header) = header_line {
                displays.push(parse_single_display(header, &current_lines)?);
            }
            header_line = Some(line);
            current_lines.clear();
        } else if header_line.is_some() {
            current_lines.push(line);
        }
    }

    if let Some(header) = header_line {
        displays.push(parse_single_display(header, &current_lines)?);
    }

    Ok(displays)
}

fn parse_single_display(header: &str, body: &[&str]) -> Result<Display, String> {
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 4 {
        return Err(format!("Invalid display header: {header}"));
    }

    let index: u32 = parts[1].parse().map_err(|_| format!("Invalid index: {}", parts[1]))?;
    let name = parts[2].to_string();
    let uuid = parts[3].to_string();

    let mut state = DisplayState::Disabled;
    let mut connection = ConnectionState::Disconnected;
    let mut modes = Vec::new();

    for line in body {
        let trimmed = line.trim();
        match trimmed {
            "enabled" => state = DisplayState::Enabled,
            "disabled" => state = DisplayState::Disabled,
            "connected" => connection = ConnectionState::Connected,
            "disconnected" => connection = ConnectionState::Disconnected,
            _ if trimmed.starts_with("Modes:") => {
                modes = parse_modes(trimmed)?;
            }
            _ => {}
        }
    }

    Ok(Display { index, name, uuid, state, connection, modes })
}

fn parse_modes(line: &str) -> Result<Vec<Mode>, String> {
    let modes_str = line.strip_prefix("Modes:").unwrap_or(line).trim();
    let mut modes = Vec::new();

    for token in modes_str.split_whitespace() {
        let (id_str, spec) = token.split_once(':')
            .ok_or_else(|| format!("Invalid mode token: {token}"))?;

        let id: u32 = id_str.parse()
            .map_err(|_| format!("Invalid mode id: {id_str}"))?;

        let current = spec.contains('*');
        let preferred = spec.contains('!');
        let clean = spec.replace(['*', '!'], "");

        let (res, refresh_str) = clean.split_once('@')
            .ok_or_else(|| format!("Invalid mode spec: {clean}"))?;

        let (w_str, h_str) = res.split_once('x')
            .ok_or_else(|| format!("Invalid resolution: {res}"))?;

        let width: u32 = w_str.parse().map_err(|_| format!("Invalid width: {w_str}"))?;
        let height: u32 = h_str.parse().map_err(|_| format!("Invalid height: {h_str}"))?;
        let refresh: f64 = refresh_str.parse().map_err(|_| format!("Invalid refresh: {refresh_str}"))?;

        modes.push(Mode { id, width, height, refresh, preferred, current });
    }

    Ok(modes)
}

// ---- DPMS ----

pub fn read_dpms(display_name: &str) -> DpmsState {
    let paths = [
        format!("/sys/class/drm/card1-{display_name}/dpms"),
        format!("/sys/class/drm/card0-{display_name}/dpms"),
    ];

    for path in &paths {
        if let Ok(content) = fs::read_to_string(path) {
            return match content.trim() {
                "On" => DpmsState::On,
                "Off" => DpmsState::Off,
                _ => DpmsState::Unknown,
            };
        }
    }

    DpmsState::Unknown
}

// ---- Display Control ----

pub fn enable_dummy_plug(name: &str) -> Result<(), String> {
    let enable_arg = format!("output.{name}.enable");
    let mode_arg = format!("output.{name}.mode.1");
    run_kscreen_doctor(&[&enable_arg, &mode_arg])?;
    Ok(())
}

pub fn disable_dummy_plug(name: &str) -> Result<(), String> {
    let disable_arg = format!("output.{name}.disable");
    run_kscreen_doctor(&[&disable_arg])?;
    Ok(())
}

// Checks that a display has an active DRM framebuffer by reading sysfs.
// Sunshine uses KMS/DRM to capture — it needs `enabled` to be "enabled"
// at the kernel level, not just in KDE.
pub fn is_drm_active(name: &str) -> bool {
    let paths = [
        format!("/sys/class/drm/card1-{name}/enabled"),
        format!("/sys/class/drm/card0-{name}/enabled"),
    ];

    for path in &paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            return content.trim() == "enabled";
        }
    }

    false
}

// Waits up to `timeout` for DRM to report the display as active.
// KDE's kscreen-doctor enables the display asynchronously — there's a
// brief delay before the kernel DRM layer reflects the change.
pub fn wait_for_drm_active(name: &str, timeout: std::time::Duration) -> Result<(), String> {
    use std::time::Instant;

    let start = Instant::now();
    let poll = std::time::Duration::from_millis(500);

    while start.elapsed() < timeout {
        if is_drm_active(name) {
            return Ok(());
        }
        std::thread::sleep(poll);
    }

    Err(format!("Timed out waiting for {name} DRM framebuffer to become active"))
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("\x1b[31mhello\x1b[0m"), "hello");
        assert_eq!(strip_ansi("no escapes"), "no escapes");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn test_parse_modes() {
        let line = "Modes:  1:1920x1080@60.00*!  2:4096x2160@59.94";
        let modes = parse_modes(line).unwrap();
        assert_eq!(modes.len(), 2);
        assert_eq!(modes[0].width, 1920);
        assert_eq!(modes[0].height, 1080);
        assert!(modes[0].current);
        assert!(modes[0].preferred);
        assert_eq!(modes[1].width, 4096);
        assert!(!modes[1].current);
        assert!(!modes[1].preferred);
    }

    #[test]
    fn test_parse_displays() {
        let input = "\
Output: 1 HDMI-A-1 some-uuid-here
\tenabled
\tconnected
\tpriority 0
\tHDMI
\tModes:  1:1920x1080@60.00*!  2:3840x2160@60.00
\tGeometry: 0,0 1920x1080
Output: 2 DP-2 other-uuid-here
\tdisabled
\tconnected
\tpriority 1
\tDisplayPort
\tModes:  3:3840x2160@240.02*  4:1920x1080@60.00!
\tGeometry: 0,0 3200x1800";

        let displays = parse_displays(input).unwrap();
        assert_eq!(displays.len(), 2);

        assert_eq!(displays[0].name, "HDMI-A-1");
        assert_eq!(displays[0].state, DisplayState::Enabled);
        assert_eq!(displays[0].connection, ConnectionState::Connected);
        assert_eq!(displays[0].modes.len(), 2);

        assert_eq!(displays[1].name, "DP-2");
        assert_eq!(displays[1].state, DisplayState::Disabled);
        assert_eq!(displays[1].connection, ConnectionState::Connected);
        assert_eq!(displays[1].modes.len(), 2);
        assert_eq!(displays[1].modes[0].refresh, 240.02);
    }
}
