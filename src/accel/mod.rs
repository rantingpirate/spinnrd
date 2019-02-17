//! # accel
//!
//! Traits for representing accelerometers.


/// How long to "average" the accelerometer readings over
/// when low-pass filtering (in ms)
const DEFAULT_HYSTERESIS: u32 = 1000;

