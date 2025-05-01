use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::*; // gpio は引き続き使用
use esp_idf_svc::hal::peripherals::Peripherals;
// use esp_idf_svc::hal::rmt::config::TransmitConfig; // TransmitConfig は OWDriver::new で内部的に使われるため通常不要
use esp_idf_svc::hal::onewire::{OWAddress, OWCommand, OWDriver}; // OWDriver 関連をインポート
use esp_idf_sys::EspError; // EspError をインポート
use anyhow::Result; // anyhow マクロは使わないので Result のみインポート
use core::time::Duration; // Duration は core::time から
use esp_idf_sys::{esp_deep_sleep_start, esp_sleep_enable_timer_wakeup}; // 正しい関数をインポート

// --- ここから例のヘルパー関数とenumを追加 ---
#[allow(dead_code)]
#[repr(u8)]
enum Ds18b20Command {
    ConvertTemp = 0x44,
    WriteScratch = 0x4E,
    ReadScratch = 0xBE,
}

fn ds18b20_send_command(addr: &OWAddress, bus: &OWDriver, cmd: u8) -> Result<(), EspError> {
    let mut buf = [0; 10];
    buf[0] = OWCommand::MatchRom as _;
    let addr_bytes = addr.address().to_le_bytes(); // to_le_bytes() を使用
    buf[1..9].copy_from_slice(&addr_bytes);
    buf[9] = cmd;

    bus.write(&buf)
}

fn ds18b20_trigger_temp_conversion(addr: &OWAddress, bus: &OWDriver) -> Result<(), EspError> {
    // reset bus and check if the ds18b20 is present
    bus.reset()?;

    ds18b20_send_command(addr, bus, Ds18b20Command::ConvertTemp as u8)?;

    // delay proper time for temp conversion,
    // assume max resolution (12-bits)
    // std::thread::sleep は no_std 環境では使えないため FreeRtos::delay_ms を使用
    FreeRtos::delay_ms(800);

    Ok(())
}

fn ds18b20_get_temperature(addr: &OWAddress, bus: &OWDriver) -> Result<f32, EspError> {
    bus.reset()?;

    ds18b20_send_command(addr, bus, Ds18b20Command::ReadScratch as u8)?;

    let mut buf = [0u8; 10]; // Scratchpad は 9 バイト + CRC
    bus.read(&mut buf)?;
    let lsb = buf[0];
    let msb = buf[1];

    // CRC チェック (オプションだが推奨)
    // let crc_read = buf[8];
    // let crc_calc = one_wire_bus::crc::crc8(&buf[0..8]); // one_wire_bus クレートが必要になる
    // if crc_read != crc_calc {
    //     log::error!("CRC mismatch: read={}, calculated={}", crc_read, crc_calc);
    //     // 必要に応じてエラーを返す
    //     // return Err(EspError::from_infallible::<ESP_ERR_INVALID_CRC>());
    // }


    let temp_raw: i16 = (i16::from(msb) << 8) | i16::from(lsb); // 符号付きで処理

    Ok(f32::from(temp_raw) / 16.0)
}
// --- ヘルパー関数ここまで ---


fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting RMT Onewire temperature sensor reading with Deep Sleep...");

    let peripherals = Peripherals::take()?;

    // --- RMT と GPIO の設定 ---
    let channel = peripherals.rmt.channel0;
    let onewire_gpio_pin = peripherals.pins.gpio17;
    let pin_number = onewire_gpio_pin.pin();
    log::info!("Using GPIO {} with RMT channel 0", pin_number);

    let mut onewire_bus = OWDriver::new(onewire_gpio_pin, channel)?;

    // --- デバイス検索 ---
    log::info!("Searching for devices...");
    let device_address = loop {
        match onewire_bus.search()?.next() {
            Some(Ok(addr)) => {
                log::info!(
                    "Found Device: {:?}, family code = {}",
                    addr,
                    addr.family_code()
                );
                if addr.family_code() != 0x28 {
                    log::warn!("Found non-DS18B20 device: {:?}", addr);
                }
                break addr;
            }
            Some(Err(e)) => {
                log::error!("Error during device search: {:?}", e);
                log::warn!("Retrying search in 5 seconds due to error...");
                FreeRtos::delay_ms(5000);
            }
            None => {
                log::warn!("No device found on the bus. Check wiring (GPIO pin: {}) and pull-up resistor. Retrying in 5 seconds...", pin_number);
                FreeRtos::delay_ms(5000);
            }
        }
    };

    // --- 温度読み取り (Deep Sleep 前に1回実行) ---
    match ds18b20_trigger_temp_conversion(&device_address, &onewire_bus) {
        Ok(_) => {
            match ds18b20_get_temperature(&device_address, &onewire_bus) {
                Ok(temp) => {
                    log::info!("Temperature: {:.2}°C", temp);
                }
                Err(e) => {
                    log::error!("Failed to get temperature: {:?}", e);
                }
            }
        }
        Err(e) => {
            log::error!("Failed to trigger temperature conversion: {:?}", e);
            // エラーによってはバスリセットなどを試みる
            match onewire_bus.reset() {
                Ok(_) => log::info!("OneWire bus reset successful."),
                Err(reset_err) => log::error!("Failed to reset OneWire bus: {:?}", reset_err),
            }
        }
    }

    // --- Deep Sleep 設定 ---
    log::info!("Entering deep sleep for 30 seconds...");

    // 30秒をマイクロ秒に変換
    let sleep_duration_us = Duration::from_secs(30).as_micros() as u64;

    unsafe {
        // タイマーウェイクアップを設定 (マイクロ秒単位)
        esp_sleep_enable_timer_wakeup(sleep_duration_us);
        // ディープスリープを開始 (この関数から戻ることはない)
        esp_deep_sleep_start();
    }

    // この行は esp_deep_sleep_start が戻らないため到達不能だが、
    // main 関数の戻り値の型 Result<()> を満たすために必要
    Ok(())
}
