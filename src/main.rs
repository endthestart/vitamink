use std::fs;
use std::process::Command;

// ---- Data Types ----

// In Rust, `struct` is like a class with only data fields (no methods yet).
// `derive(Debug)` auto-generates a way to print the struct for debugging.
// `derive(PartialEq)` lets us compare values with == (needed for tests).
#[derive(Debug, PartialEq)]
enum DisplayState {
    Enabled,
    Disabled,
}

#[derive(Debug, PartialEq)]
enum ConnectionState {
    Connected,
    Disconnected,
}

#[derive(Debug, PartialEq)]
enum DpmsState {
    On,
    Off,
    Unknown,
}

// A single display mode like "1920x1080@60.00"
#[derive(Debug)]
struct Mode {
    id: u32,
    width: u32,
    height: u32,
    refresh: f64,
    preferred: bool, // marked with ! in kscreen-doctor output
    current: bool,   // marked with * in kscreen-doctor output
}

// A display output (monitor or dummy plug)
#[derive(Debug)]
struct Display {
    index: u32,
    name: String,
    uuid: String,
    state: DisplayState,
    connection: ConnectionState,
    modes: Vec<Mode>,
}

// ---- Wayland Environment ----

// kscreen-doctor needs Wayland environment variables to talk to the compositor.
// We bundle them here so every command gets the same environment.
fn wayland_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("WAYLAND_DISPLAY", "wayland-0"),
        ("DISPLAY", ":0"),
    ]
}

// ---- Shell Commands ----

// Runs kscreen-doctor with the Wayland environment and strips ANSI escape codes.
// Returns the cleaned output as a String.
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

// ANSI escape codes look like \x1b[31m (for colors). kscreen-doctor
// adds them to its output even when piped, so we strip them manually.
fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip until we hit a letter (the end of the escape sequence)
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

// Parses the full kscreen-doctor -o output into a Vec of Display structs.
//
// The output format looks like:
//   Output: 1 HDMI-A-1 <uuid>
//       enabled/disabled
//       connected/disconnected
//       ...
//       Modes: 1:1920x1080@60.00*! 2:4096x2160@59.94 ...
//
// Each "Output:" line starts a new display block.
fn parse_displays(output: &str) -> Result<Vec<Display>, String> {
    let mut displays = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut header_line: Option<&str> = None;

    for line in output.lines() {
        if line.starts_with("Output:") {
            // If we had a previous block, parse it
            if let Some(header) = header_line {
                displays.push(parse_single_display(header, &current_lines)?);
            }
            header_line = Some(line);
            current_lines.clear();
        } else if header_line.is_some() {
            current_lines.push(line);
        }
    }

    // Don't forget the last display block
    if let Some(header) = header_line {
        displays.push(parse_single_display(header, &current_lines)?);
    }

    Ok(displays)
}

// Parses one display block: the "Output: ..." header line + its indented body lines.
fn parse_single_display(header: &str, body: &[&str]) -> Result<Display, String> {
    // Header format: "Output: 1 HDMI-A-1 <uuid>"
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 4 {
        return Err(format!("Invalid display header: {header}"));
    }

    let index: u32 = parts[1].parse().map_err(|_| format!("Invalid index: {}", parts[1]))?;
    let name = parts[2].to_string();
    let uuid = parts[3].to_string();

    // Body lines are tab-indented. We look for specific keywords.
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
            _ => {} // We ignore fields we don't need yet
        }
    }

    Ok(Display { index, name, uuid, state, connection, modes })
}

// Parses the "Modes: 1:1920x1080@60.00*! 2:4096x2160@59.94 ..." line.
//
// Each mode token has the format: <id>:<width>x<height>@<refresh><flags>
// Flags: * = current mode, ! = preferred mode
fn parse_modes(line: &str) -> Result<Vec<Mode>, String> {
    let modes_str = line.strip_prefix("Modes:").unwrap_or(line).trim();
    let mut modes = Vec::new();

    for token in modes_str.split_whitespace() {
        // Split "1:1920x1080@60.00*!" into id part and spec part
        let (id_str, spec) = token.split_once(':')
            .ok_or_else(|| format!("Invalid mode token: {token}"))?;

        let id: u32 = id_str.parse()
            .map_err(|_| format!("Invalid mode id: {id_str}"))?;

        // Check for flags at the end
        let current = spec.contains('*');
        let preferred = spec.contains('!');
        let clean = spec.replace(['*', '!'], "");

        // Split "1920x1080@60.00" into resolution and refresh
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

// Reads the DPMS state directly from the kernel's sysfs interface.
// This is how we detect if the monitor is sleeping.
fn read_dpms(display_name: &str) -> DpmsState {
    // Try card1 first (NVIDIA), then card0
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

fn enable_dummy_plug(name: &str) -> Result<(), String> {
    let enable_arg = format!("output.{name}.enable");
    // Mode 1 is typically the first/default mode. We'll make this smarter later.
    let mode_arg = format!("output.{name}.mode.1");
    run_kscreen_doctor(&[&enable_arg, &mode_arg])?;
    Ok(())
}

fn disable_dummy_plug(name: &str) -> Result<(), String> {
    let disable_arg = format!("output.{name}.disable");
    run_kscreen_doctor(&[&disable_arg])?;
    Ok(())
}

// ---- Service Control ----

fn control_sunshine(action: &str) -> Result<(), String> {
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

fn is_sunshine_running() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "sunshine"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---- Main ----

fn main() {
    println!("VitaminK — Sunshine Lifecycle Manager\n");

    // 1. Read and parse displays
    let raw = match run_kscreen_doctor(&["-o"]) {
        Ok(output) => output,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let displays = match parse_displays(&raw) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Parse error: {e}");
            std::process::exit(1);
        }
    };

    // 2. Show what we found
    for display in &displays {
        let state = match display.state {
            DisplayState::Enabled => "enabled",
            DisplayState::Disabled => "disabled",
        };
        let conn = match display.connection {
            ConnectionState::Connected => "connected",
            ConnectionState::Disconnected => "disconnected",
        };
        let dpms = read_dpms(&display.name);

        println!("{} (Output {}): {state}, {conn}, DPMS: {dpms:?}", display.name, display.index);
        println!("  {} modes available", display.modes.len());

        // Show the current mode if there is one
        if let Some(current) = display.modes.iter().find(|m| m.current) {
            println!("  Current: {}x{}@{:.2}Hz", current.width, current.height, current.refresh);
        }
    }

    // 3. Show Sunshine status
    println!("\nSunshine: {}", if is_sunshine_running() { "running" } else { "stopped" });
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

