use log::info;
use anyhow::Result;

// このサンプルを実行する際には、Cargo.tomlでライブラリクレートが
// 依存関係として正しく設定されている必要があります。
// 通常、同じワークスペース内のライブラリを参照する場合、
// [dependencies]
// simple_ds18b20_temp_sensor = { path = "../../" }
// のように指定しますが、公開するライブラリの場合は
// simple_ds18b20_temp_sensor = "0.1.0" のようになります。
// 今回は同じクレート内のexampleなので、クレート名を直接使えます。
use simple_ds18b20_temp_sensor::TempSensor;


fn main() -> Result<()> {
    // パッチの適用とロガーの初期化
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("Starting ESP-RS DS18B20 temperature logging example...");

    const WAIT_DURATION_MS: u32 = 5 * 1_000; // 5秒をミリ秒に変換

    // 第一引数はpower pin、第二引数はdata pin
    let mut sensor = TempSensor::new(16, 17)?; // 例: GPIO16, GPIO17

    loop {
        let temp = sensor.read_temperature()?;
        println!("Measured Temperature: {:.2}°C", temp);
        esp_idf_svc::hal::delay::FreeRtos::delay_ms(WAIT_DURATION_MS);
    }
}
