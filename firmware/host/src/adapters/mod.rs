//! One module per port trait. Each is small on purpose: the ESP32-P4 crate has
//! a file of the same name doing the same job against the board's peripherals.

pub mod button;
pub mod camera;
pub mod clock;
pub mod log;
pub mod net;
pub mod ring;
pub mod store;
