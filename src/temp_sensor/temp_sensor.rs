use crate::temp_sensor::logic::{compute_crc8, ds18b20_raw_to_celsius};
use anyhow::Result;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::{AnyIOPin, AnyOutputPin, Output, PinDriver};
use esp_idf_svc::hal::onewire::{OWAddress, OWCommand, OWDriver};
use esp_idf_svc::hal::peripheral::Peripheral;
use esp_idf_svc::hal::rmt::RmtChannel;
use esp_idf_sys::EspError;
use log::warn;

pub struct TempSensor {
    power_pin: PinDriver<'static, AnyOutputPin, Output>,
    onewire_bus: OWDriver<'static>,
    device_address: Option<OWAddress>,
}

impl TempSensor {
    /// 外部Peripherals管理でTempSensorを作成（推奨API）
    ///
    /// # Arguments
    /// * `power_pin_num` - 電源制御用GPIOピン番号
    /// * `data_pin_num` - データピン番号
    /// * `rmt_channel` - RMTチャンネル
    ///
    /// # Example
    /// ```no_run
    /// use esp_idf_svc::hal::peripherals::Peripherals;
    /// use simple_ds18b20_temp_sensor::TempSensor;
    ///
    /// let peripherals = Peripherals::take().unwrap();
    /// let mut temp_sensor = TempSensor::new(2, 3, peripherals.rmt.channel0)?;
    /// let temperature = temp_sensor.read_temperature()?;
    /// ```
    pub fn new<C: esp_idf_svc::hal::rmt::RmtChannel>(
        power_pin_num: i32,
        data_pin_num: i32,
        rmt_channel: impl Peripheral<P = C> + 'static
    ) -> Result<Self> {
        let power_pin = PinDriver::output(unsafe { AnyOutputPin::new(power_pin_num) })?;
        let onewire_bus = OWDriver::new(
            unsafe { AnyIOPin::new(data_pin_num) },
            rmt_channel,
        )?;

        Ok(Self {
            power_pin,
            onewire_bus,
            device_address: None,
        })
    }

    fn search_device(&mut self) -> Result<OWAddress, EspError> {
        let mut addr = None;
        for dev in self.onewire_bus.search()? {
            if let Ok(a) = dev {
                if a.family_code() == 0x28 {
                    addr = Some(a);
                    break;
                }
            }
        }
        addr.ok_or_else(|| EspError::from(esp_idf_sys::ESP_ERR_NOT_FOUND as i32).unwrap())
    }

    pub fn read_temperature(&mut self) -> Result<f32, EspError> {
        const MAX_RETRIES: u8 = 3;
        let mut last_err = EspError::from(esp_idf_sys::ESP_ERR_INVALID_RESPONSE as i32).unwrap();

        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                // リトライ前にパワーサイクルしてデバイスをリセット
                self.power_pin.set_low()?;
                FreeRtos::delay_ms(50);
                self.device_address = None; // 再検索を強制
            }

            self.power_pin.set_high()?;
            FreeRtos::delay_ms(500);

            let addr = match self.device_address {
                Some(a) => a,
                None => match self.search_device() {
                    Ok(a) => {
                        self.device_address = Some(a);
                        a
                    }
                    Err(e) => {
                        warn!("DS18B20 search failed (attempt {}/{}): {e}", attempt + 1, MAX_RETRIES);
                        last_err = e;
                        continue;
                    }
                },
            };

            // 温度変換コマンド送信
            if let Err(e) = self.onewire_bus.reset() {
                warn!("1-Wire reset failed (attempt {}/{}): {e}", attempt + 1, MAX_RETRIES);
                last_err = e;
                continue;
            }
            let mut buf = [0u8; 10];
            buf[0] = OWCommand::MatchRom as _;
            buf[1..9].copy_from_slice(&addr.address().to_le_bytes());
            buf[9] = 0x44; // ConvertTemp
            if let Err(e) = self.onewire_bus.write(&buf) {
                warn!("ConvertTemp failed (attempt {}/{}): {e}", attempt + 1, MAX_RETRIES);
                last_err = e;
                continue;
            }
            FreeRtos::delay_ms(800);

            // Scratchpad読み出し
            if let Err(e) = self.onewire_bus.reset() {
                warn!("1-Wire reset failed (attempt {}/{}): {e}", attempt + 1, MAX_RETRIES);
                last_err = e;
                continue;
            }
            buf[9] = 0xBE; // ReadScratchpad
            if let Err(e) = self.onewire_bus.write(&buf) {
                warn!("ReadScratch failed (attempt {}/{}): {e}", attempt + 1, MAX_RETRIES);
                last_err = e;
                continue;
            }
            let mut scratch = [0u8; 9];
            if let Err(e) = self.onewire_bus.read(&mut scratch) {
                warn!("Scratchpad read failed (attempt {}/{}): {e}", attempt + 1, MAX_RETRIES);
                last_err = e;
                continue;
            }

            // CRC検証: DS18B20 scratchpad[8] は scratchpad[0..8] のCRC
            if compute_crc8(&scratch[0..8]) != scratch[8] {
                warn!(
                    "DS18B20 CRC mismatch on attempt {}/{}: scratch={:02X?}",
                    attempt + 1,
                    MAX_RETRIES,
                    scratch
                );
                last_err = EspError::from(esp_idf_sys::ESP_ERR_INVALID_CRC as i32).unwrap();
                continue;
            }

            let temp = ds18b20_raw_to_celsius(scratch[0], scratch[1]);

            // 85°C は DS18B20 の電源ON時デフォルト値（POR）。変換未完了または電源不安定を示す。
            // NOTE: DS18B20 の測定範囲は -55〜+125°C なので、農業・室内環境での実測 85°C は
            // 現実的でなく POR と見なして差し支えない。85°C が実際に現れる環境では、
            // scratchpad byte2〜4（TH/TL/Config）を POR デフォルト値と比較することで
            // 誤検出を低減できる（将来の拡張ポイント）。
            if (temp - 85.0_f32).abs() < 0.01 {
                warn!(
                    "DS18B20 returned POR default (85.0°C) on attempt {}/{}",
                    attempt + 1,
                    MAX_RETRIES
                );
                last_err = EspError::from(esp_idf_sys::ESP_ERR_INVALID_RESPONSE as i32).unwrap();
                continue;
            }

            self.power_pin.set_low()?;
            return Ok(temp);
        }

        let _ = self.power_pin.set_low();
        Err(last_err)
    }
}
