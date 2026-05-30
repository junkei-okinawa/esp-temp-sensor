use crate::temp_sensor::logic::{compute_crc8, ds18b20_raw_to_celsius};
use anyhow::Result;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::{AnyIOPin, AnyOutputPin, Output, PinDriver};
use esp_idf_svc::hal::onewire::{OWAddress, OWCommand, OWDriver};
use esp_idf_svc::hal::peripheral::Peripheral;
use esp_idf_svc::hal::rmt::CHANNEL0;
use esp_idf_sys::EspError;
use log::warn;

pub struct TempSensor {
    power_pin: PinDriver<'static, AnyOutputPin, Output>,
    onewire_bus: Option<OWDriver<'static>>,
    device_address: Option<OWAddress>,
    data_pin_num: i32,
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
        rmt_channel: impl Peripheral<P = C> + 'static,
    ) -> Result<Self> {
        let power_pin = PinDriver::output(unsafe { AnyOutputPin::new(power_pin_num) })?;
        let onewire_bus = OWDriver::new(unsafe { AnyIOPin::new(data_pin_num) }, rmt_channel)?;

        Ok(Self {
            power_pin,
            onewire_bus: Some(onewire_bus),
            device_address: None,
            data_pin_num,
        })
    }

    fn search_device(&mut self) -> Result<OWAddress, EspError> {
        let mut addr = None;
        let mut last_bus_err: Option<EspError> = None;
        let bus = self.onewire_bus.as_mut().expect("OWDriver not initialized");
        for dev in bus.search()? {
            match dev {
                Ok(a) if a.family_code() == 0x28 => {
                    addr = Some(a);
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    last_bus_err = Some(e);
                    break; // バスエラー発生時はイテレーションを即停止する。
                           // onewire_bus_rmt_reset タイムアウト後は RMT FSM が RMT_FSM_RUN
                           // で詰まるため、継続するとエラーが無限ループになる。
                }
            }
        }
        addr.ok_or_else(|| {
            last_bus_err.unwrap_or_else(|| EspError::from(esp_idf_sys::ESP_ERR_NOT_FOUND).unwrap())
        })
    }

    /// 1-Wire バスを再初期化して RMT チャンネルの FSM をリセットする。
    ///
    /// `espressif/onewire_bus v1.0.2` では `onewire_bus_rmt_reset` がタイムアウトしたとき
    /// RMT RX チャンネルの FSM が `RMT_FSM_RUN` で詰まったままになるバグがある。
    ///
    /// `take()` で旧 `OWDriver` を先に解放（`onewire_bus_del` → `rmt_disable` +
    /// `rmt_del_channel`）してから新しい `OWDriver` を生成することで、
    /// 旧チャンネルが完全に解放された後に新チャンネルが確保されることを保証する。
    /// Rust の代入は右辺を評価してから旧値をドロップするため、フィールドへの直接代入
    /// (`self.onewire_bus = OWDriver::new(...)`) では旧ドライバが生存したまま
    /// 新ドライバが生成されてしまい、意図した順序にならない。
    ///
    /// CHANNEL0 はRustの所有権システム用の型トークンとして渡すだけで、
    /// `onewire_new_bus_rmt` は内部で ESP-IDF プールから独自に RMT チャンネルを確保するため
    /// 渡すチャンネルの種類は問わない。
    fn reinit_bus(&mut self) -> Result<(), EspError> {
        // 旧ドライバを先にドロップして RMT チャンネルを解放する
        drop(self.onewire_bus.take());
        // 旧チャンネルが解放された後に新しい OWDriver を生成する
        self.onewire_bus = Some(OWDriver::new(
            unsafe { AnyIOPin::new(self.data_pin_num) },
            unsafe { CHANNEL0::new() },
        )?);
        Ok(())
    }

    pub fn read_temperature(&mut self) -> Result<f32, EspError> {
        const MAX_RETRIES: u8 = 3;
        let mut last_err = EspError::from(esp_idf_sys::ESP_ERR_INVALID_RESPONSE).unwrap();

        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                // リトライ前にパワーサイクルしてデバイスをリセット
                self.power_pin.set_low()?;
                FreeRtos::delay_ms(50);
                self.device_address = None; // 再検索を強制
                // RMT チャンネルの FSM が詰まっている可能性があるため再初期化する
                if let Err(e) = self.reinit_bus() {
                    warn!(
                        "Failed to reinit 1-Wire bus (attempt {}/{}): {e}",
                        attempt + 1,
                        MAX_RETRIES
                    );
                    last_err = e;
                    continue;
                }
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
                        warn!(
                            "DS18B20 search failed (attempt {}/{}): {e}",
                            attempt + 1,
                            MAX_RETRIES
                        );
                        last_err = e;
                        continue;
                    }
                },
            };

            let bus = self.onewire_bus.as_mut().expect("OWDriver not initialized");

            // 温度変換コマンド送信
            if let Err(e) = bus.reset() {
                warn!(
                    "1-Wire reset failed (attempt {}/{}): {e}",
                    attempt + 1,
                    MAX_RETRIES
                );
                last_err = e;
                continue;
            }
            let mut buf = [0u8; 10];
            buf[0] = OWCommand::MatchRom as _;
            buf[1..9].copy_from_slice(&addr.address().to_le_bytes());
            buf[9] = 0x44; // ConvertTemp
            if let Err(e) = bus.write(&buf) {
                warn!(
                    "ConvertTemp failed (attempt {}/{}): {e}",
                    attempt + 1,
                    MAX_RETRIES
                );
                last_err = e;
                continue;
            }
            FreeRtos::delay_ms(800);

            // Scratchpad読み出し
            if let Err(e) = bus.reset() {
                warn!(
                    "1-Wire reset failed (attempt {}/{}): {e}",
                    attempt + 1,
                    MAX_RETRIES
                );
                last_err = e;
                continue;
            }
            buf[9] = 0xBE; // ReadScratchpad
            if let Err(e) = bus.write(&buf) {
                warn!(
                    "ReadScratch failed (attempt {}/{}): {e}",
                    attempt + 1,
                    MAX_RETRIES
                );
                last_err = e;
                continue;
            }
            let mut scratch = [0u8; 9];
            if let Err(e) = bus.read(&mut scratch) {
                warn!(
                    "Scratchpad read failed (attempt {}/{}): {e}",
                    attempt + 1,
                    MAX_RETRIES
                );
                last_err = e;
                continue;
            }

            // CRC検証: scratchpad の先頭 8 バイト (byte 0〜7) の CRC が byte[8] と一致すること
            if compute_crc8(&scratch[0..8]) != scratch[8] {
                warn!(
                    "DS18B20 CRC mismatch on attempt {}/{}: scratch={:02X?}",
                    attempt + 1,
                    MAX_RETRIES,
                    scratch
                );
                last_err = EspError::from(esp_idf_sys::ESP_ERR_INVALID_CRC).unwrap();
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
                last_err = EspError::from(esp_idf_sys::ESP_ERR_INVALID_RESPONSE).unwrap();
                continue;
            }

            self.power_pin.set_low()?;
            return Ok(temp);
        }

        if let Err(e) = self.power_pin.set_low() {
            warn!("Failed to power down DS18B20 after all retries: {e}");
        }
        Err(last_err)
    }
}
