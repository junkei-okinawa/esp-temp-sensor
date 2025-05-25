/// DS18B20の生データから温度値(℃)を計算
pub fn ds18b20_raw_to_celsius(lsb: u8, msb: u8) -> f32 {
    let temp_raw: i16 = (i16::from(msb) << 8) | i16::from(lsb);
    f32::from(temp_raw) / 16.0
}

#[cfg(test)]
#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
mod tests {
    use super::*;
    #[test]
    fn test_positive() {
        assert!((ds18b20_raw_to_celsius(0x91, 0x01) - 25.0625).abs() < 0.001);
    }
    #[test]
    fn test_negative() {
        assert!((ds18b20_raw_to_celsius(0x5F, 0xFF) + 10.0625).abs() < 0.001);
    }
    #[test]
    fn test_zero() {
        assert_eq!(ds18b20_raw_to_celsius(0x00, 0x00), 0.0);
    }
}
