// src/daemon.rs — State machine and polling daemon
//
// New Rust concepts in this file:
//
// - `impl` blocks: attach methods to a struct (like class methods in OOP).
//   Methods take `&self` (read-only borrow) or `&mut self` (mutable borrow).
//
// - `std::time::Instant`: monotonic clock for measuring elapsed time.
//   We use it for the grace period timer — it can't go backwards or be
//   affected by NTP adjustments.
//
// - `std::thread::sleep`: pauses the current thread. Simple polling.
//
// - `eprintln!`: prints to stderr (good for daemon logging alongside journald).

use std::thread;
use std::time::{Duration, Instant};

use crate::display::{self, DpmsState};
use crate::sunshine;

// ---- Configuration ----

pub struct Config {
    pub main_display: String,
    pub dummy_plug: String,
    pub poll_interval: Duration,
    pub grace_period: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            main_display: "DP-2".to_string(),
            dummy_plug: "HDMI-A-1".to_string(),
            poll_interval: Duration::from_secs(5),
            grace_period: Duration::from_secs(10),
        }
    }
}

// ---- State Machine ----

// The two states VitaminK can be in.
// `AtDesk`: user is present, main monitor on, Sunshine stopped.
// `Away`: user is away, dummy plug on, Sunshine running.
#[derive(Debug, PartialEq, Clone, Copy)]
enum State {
    AtDesk,
    Away,
}

// `impl` attaches methods to a type. This gives State a human-readable label.
impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            State::AtDesk => write!(f, "AtDesk"),
            State::Away => write!(f, "Away"),
        }
    }
}

pub struct Daemon {
    config: Config,
    state: State,
    // Tracks when we first saw a DPMS change.
    // `Option<Instant>` is either Some(timestamp) or None.
    // We use this to implement the grace period: only transition
    // after the new DPMS state has been stable for `grace_period`.
    transition_started: Option<Instant>,
}

impl Daemon {
    pub fn new(config: Config) -> Self {
        // Start by checking current DPMS to set initial state correctly
        let dpms = display::read_dpms(&config.main_display);
        let initial_state = match dpms {
            DpmsState::Off => State::Away,
            _ => State::AtDesk,
        };

        eprintln!("[vitamink] Starting in state: {initial_state} (DPMS: {dpms:?})");

        Self {
            config,
            state: initial_state,
            transition_started: None,
        }
    }

    // Main loop — runs forever, polling DPMS and managing state transitions.
    pub fn run(&mut self) {
        // Apply the initial state so hardware matches
        if let Err(e) = self.apply_state() {
            eprintln!("[vitamink] Error applying initial state: {e}");
        }

        loop {
            thread::sleep(self.config.poll_interval);

            if let Err(e) = self.poll() {
                eprintln!("[vitamink] Poll error: {e}");
            }
        }
    }

    fn poll(&mut self) -> Result<(), String> {
        let dpms = display::read_dpms(&self.config.main_display);
        let desired = match dpms {
            DpmsState::Off => State::Away,
            DpmsState::On => State::AtDesk,
            DpmsState::Unknown => {
                eprintln!("[vitamink] DPMS unknown, holding current state");
                return Ok(());
            }
        };

        if desired == self.state {
            // Already in the right state — clear any pending transition
            self.transition_started = None;
            return Ok(());
        }

        // We want to transition, but we wait for the grace period first.
        // This avoids flapping if the monitor briefly blinks off/on.
        match self.transition_started {
            None => {
                eprintln!("[vitamink] DPMS changed to {dpms:?}, waiting grace period...");
                self.transition_started = Some(Instant::now());
            }
            Some(started) if started.elapsed() >= self.config.grace_period => {
                eprintln!("[vitamink] Grace period elapsed, transitioning: {} → {desired}", self.state);
                self.state = desired;
                self.transition_started = None;
                self.apply_state()?;
            }
            Some(started) => {
                let remaining = self.config.grace_period - started.elapsed();
                eprintln!("[vitamink] Waiting... {:.0}s remaining", remaining.as_secs_f64());
            }
        }

        Ok(())
    }

    // Makes the hardware match the current state.
    fn apply_state(&self) -> Result<(), String> {
        match self.state {
            State::Away => {
                eprintln!("[vitamink] → Enabling dummy plug");
                display::enable_dummy_plug(&self.config.dummy_plug)?;

                eprintln!("[vitamink] → Starting Sunshine");
                sunshine::start()?;

                eprintln!("[vitamink] Away mode active");
            }
            State::AtDesk => {
                if sunshine::is_running() {
                    eprintln!("[vitamink] → Stopping Sunshine");
                    sunshine::stop()?;
                }

                eprintln!("[vitamink] → Disabling dummy plug");
                display::disable_dummy_plug(&self.config.dummy_plug)?;

                eprintln!("[vitamink] At desk mode active");
            }
        }

        Ok(())
    }
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_display() {
        assert_eq!(format!("{}", State::AtDesk), "AtDesk");
        assert_eq!(format!("{}", State::Away), "Away");
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.main_display, "DP-2");
        assert_eq!(config.dummy_plug, "HDMI-A-1");
        assert_eq!(config.poll_interval, Duration::from_secs(5));
        assert_eq!(config.grace_period, Duration::from_secs(10));
    }
}
