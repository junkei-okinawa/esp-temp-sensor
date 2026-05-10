pub mod logic; // Declares src/temp_sensor/logic.rs

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
mod sensor; // Declares src/temp_sensor/sensor.rs

// Re-export TempSensor struct so it can be used as `crate::temp_sensor::TempSensor`
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub use self::sensor::TempSensor;
