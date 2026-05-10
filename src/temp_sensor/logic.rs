/// DS18B20の生データから温度値(℃)を計算
pub fn ds18b20_raw_to_celsius(lsb: u8, msb: u8) -> f32 {
    let temp_raw: i16 = (i16::from(msb) << 8) | i16::from(lsb);
    f32::from(temp_raw) / 16.0
}

/// Dallas 1-Wire CRC-8 (poly=0x31, LSB-first)
///
/// DS18B20 scratchpad の整合性検証に使用。scratch[0..8] の CRC が scratch[8] と一致すること。
pub fn compute_crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in data {
        let mut b = byte;
        for _ in 0..8 {
            if (crc ^ b) & 0x01 != 0 {
                crc = (crc >> 1) ^ 0x8C;
            } else {
                crc >>= 1;
            }
            b >>= 1;
        }
    }
    crc
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

    #[test]
    fn test_crc8_empty() {
        assert_eq!(compute_crc8(&[]), 0x00);
    }

    #[test]
    fn test_crc8_appended_gives_zero() {
        // Dallas 1-Wire CRC property: CRC over data[0..n] + CRC_byte == 0x00
        let data: [u8; 8] = [0x50, 0x05, 0x4B, 0x46, 0x7F, 0xFF, 0x0C, 0x10];
        let crc = compute_crc8(&data);
        let mut full = data.to_vec();
        full.push(crc);
        assert_eq!(compute_crc8(&full), 0x00);
    }

    #[test]
    fn test_crc8_corrupt_detected() {
        let data: [u8; 8] = [0x50, 0x05, 0x4B, 0x46, 0x7F, 0xFF, 0x0C, 0x10];
        let crc = compute_crc8(&data);
        // Flip one bit in data — CRC should differ
        let mut corrupt = data;
        corrupt[0] ^= 0x01;
        assert_ne!(compute_crc8(&corrupt), crc);
    }
}
