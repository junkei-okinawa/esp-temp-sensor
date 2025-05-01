from w1thermsensor import W1ThermSensor
import time

sensor = W1ThermSensor()

while True:
    temperature = sensor.get_temperature()
    print(f"Temperature: {temperature:.2f} °C")
    time.sleep(2)
