//! Persistent, user-started Autopilot missions.
//!
//! Autopilot is intentionally independent from the auto-fix scheduler
//! (Mr. Robot). This module owns only mission state and its controller
//! boundary. Worker adapters and the headless Director can be added behind
//! these contracts without making the UI or a terminal process the source of
//! truth.

mod controller;
mod director;
mod storage;
pub mod types;
mod worker;

pub use controller::*;
