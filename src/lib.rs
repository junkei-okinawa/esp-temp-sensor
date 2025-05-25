pub mod temp_sensor;

#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
pub use temp_sensor::TempSensor; // Changed from `use temp_sensor::temp_sensor;`

// library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");