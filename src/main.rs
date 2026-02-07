// src/main.rs — VitaminK entry point
//
// New Rust concept: `mod` declarations.
// `mod display;` tells Rust to look for src/display.rs and include it.
// Each module is its own namespace: `display::get_displays()`, etc.

mod daemon;
mod display;
mod sunshine;

use std::env;

fn main() {
    // Simple argument handling: `vitamink daemon` runs the polling loop,
    // anything else (or no args) prints system status.
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(|s| s.as_str());

    match command {
        Some("daemon") => run_daemon(),
        _ => print_status(),
    }
}

fn run_daemon() {
    eprintln!("[vitamink] VitaminK Daemon starting...");
    let config = daemon::Config::default();
    let mut daemon = daemon::Daemon::new(config);
    daemon.run();
}

fn print_status() {
    println!("VitaminK — Sunshine Lifecycle Manager\n");

    let displays = match display::get_displays() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    for d in &displays {
        let state = match d.state {
            display::DisplayState::Enabled => "enabled",
            display::DisplayState::Disabled => "disabled",
        };
        let conn = match d.connection {
            display::ConnectionState::Connected => "connected",
            display::ConnectionState::Disconnected => "disconnected",
        };
        let dpms = display::read_dpms(&d.name);

        println!("{} (Output {}): {state}, {conn}, DPMS: {dpms:?}", d.name, d.index);
        println!("  {} modes available", d.modes.len());

        if let Some(current) = d.modes.iter().find(|m| m.current) {
            println!("  Current: {}x{}@{:.2}Hz", current.width, current.height, current.refresh);
        }
    }

    println!("\nSunshine: {}", if sunshine::is_running() { "running" } else { "stopped" });
}

