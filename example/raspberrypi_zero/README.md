# Raspberry Pi Zero W で DS18B20 温度センサーを使う手順

## 必要なもの
- Raspberry Pi Zero W（Raspberry Pi OSインストール済み）
- DS18B20 温度センサー
- 4.7kΩ 抵抗
- ジャンパワイヤ

## 配線図

- DS18B20 の VCC → Pi の 3.3V
- DS18B20 の GND → Pi の GND
- DS18B20 の DQ  → Pi の GPIO4（物理ピン7）
- 4.7kΩ 抵抗を VCC と DQ の間に接続

## OS側の設定
1. /boot/config.txt を編集し、1-Wire を有効化

```sh
sudo nano /boot/config.txt
```

ファイル末尾に以下を追加：
```
dtoverlay=w1-gpio
```

2. 再起動
```sh
sudo reboot
```

3. モジュールをロード
```sh
sudo modprobe w1-gpio
sudo modprobe w1-therm
```

4. デバイス認識確認
```sh
ls /sys/bus/w1/devices/
```
28-xxxxxx のようなディレクトリがあれば認識OK

## 必要なPythonパッケージ
```sh
sudo apt update
sudo apt install python3-pip
pip3 install w1thermsensor
```

## サンプルコード
example/read_ds18b20.py を参照
