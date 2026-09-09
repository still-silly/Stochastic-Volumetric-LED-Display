# Setting up LEDs
## Supported LED chipsets
Anything that the FastLED library supports will work fine, although WS2811 and friends will be limited to 400 FPS max(When using a ESP32 and serial). This is due to a intentional limitation in FastLED, although it is possible to attempt overclocking the LEDs! See the FastLED wiki for more info.
## Setting up 
If you want to use features such as the builtin web server, stored animations, or the ability to send gyroscope data, you will need to use a ESP32, since these all take advantage of the dual cores. If you just want to push commands to the LEDs from a seperate machine, a ESP8266 does fine (or any PIO compatible microcontroller), although it can only achieve up to 250 FPS on serial.  
Depending on your configuration, flash the appropriate PlatformIO project from `esp-code`.
See [`esp-code/README.md`](../esp-code/README.md) for the current firmware map and protocol
compatibility notes.

### Recommended projects

- **`receive_esp_serial`** - Portable direct serial receiver and the best starting point.
- **`receive_esp_serial_parallel_async`** - Queued ESP32/C3 receiver for the high-throughput setup.
- **`read_vled_esp32`** - Plays binary animation data stored on the controller.
- **`esp_main`** - Integrated demo firmware with serial reception, web controls, and sensors.

The two UDP receiver sketches implement the older custom SVLED protocol and are retained for
compatibility. WLED mode communicates directly with standard WLED firmware and does not use an
SVLED sketch.

Find pin assignments and any variables that you need to set inside the files. And if you are not using WS2811, then make sure to change the chipset type in the FastLED setup inside of `setup()`!
You can flash these using PlatformIO or Arduino.  
Once flashed, you can move on to calibrating the LEDs!  
*Please note that these scripts are still quite messy, and will be updated in the future to be cleaner and more usable.*
