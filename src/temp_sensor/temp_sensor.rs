use crate::temp_sensor::logic::ds18b20_raw_to_celsius;
use anyhow::Result;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::{AnyIOPin, AnyOutputPin, Output, PinDriver};
use esp_idf_svc::hal::onewire::{OWAddress, OWCommand, OWDriver};
use esp_idf_svc::hal::rmt::CHANNEL0;
use esp_idf_sys::EspError;

pub struct TempSensor {
    power_pin: PinDriver<'static, AnyOutputPin, Output>,
    onewire_bus: OWDriver<'static>,
    device_address: Option<OWAddress>,
}

impl TempSensor {
    /// Creates a new `TempSensor`.
    ///
    /// # Arguments
    ///
    /// * `power_pin_num` - The GPIO pin number for powering the sensor.
    /// * `data_pin_num` - The GPIO pin number for data communication with the sensor.
    /// * `rmt_channel0` - The RMT channel (CHANNEL0) to be used for the OneWire protocol.
    ///                    This channel should be exclusively available for this sensor.
    pub fn new(power_pin_num: i32, data_pin_num: i32, rmt_channel0: CHANNEL0) -> Result<Self> {
        let power_pin = PinDriver::output(unsafe { AnyOutputPin::new(power_pin_num) })?;
        let onewire_bus = OWDriver::new(unsafe { AnyIOPin::new(data_pin_num) }, rmt_channel0)?;
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
        // 電源ON
        self.power_pin.set_high()?;
        FreeRtos::delay_ms(500);

        // デバイスアドレス検索
        let addr = match self.device_address {
            Some(a) => a,
            None => {
                let found = self.search_device()?;
                self.device_address = Some(found);
                found
            }
        };

        // 温度変換コマンド送信
        self.onewire_bus.reset()?;
        let mut buf = [0; 10];
        buf[0] = OWCommand::MatchRom as _;
        let addr_bytes = addr.address().to_le_bytes();
        buf[1..9].copy_from_slice(&addr_bytes);
        buf[9] = 0x44; // ConvertTemp
        self.onewire_bus.write(&buf)?;
        FreeRtos::delay_ms(800);

        // Scratchpad読み出し
        self.onewire_bus.reset()?;
        buf[9] = 0xBE; // ReadScratch
        self.onewire_bus.write(&buf)?;
        let mut scratch = [0u8; 9];
        self.onewire_bus.read(&mut scratch)?;

        let lsb = scratch[0];
        let msb = scratch[1];
        let temp = ds18b20_raw_to_celsius(lsb, msb);

        // 電源OFF
        self.power_pin.set_low()?;

        Ok(temp)
    }
}
