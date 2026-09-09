# Configuration

By default, the  program will attempt to read from `svled.toml` within the local directory. To pass a config file, you can use `--config` or `-c`.

### Options

The top-level sections are required. Communication fields are mode-specific: serial does not
require `host`/`port`, and UDP modes do not require `serial_port_paths`/`baud_rate`.

```
num_led = 2000                                # Number of LEDS TOTAL, not per strip.
num_strips = 40                               # Number of strips

[communication]
communication_mode = 2                        # 1: legacy SVLED UDP, 2: serial, 3: WLED DNRGB
host = "192.168.86.53"                        # Required for UDP/WLED modes
port = 8888                                   # Defaults: legacy UDP 8888, WLED 21324
baud_rate = 921600                            # Serial only (Default is 921600)

serial_port_paths = ["/path/to/serial-port"]  # One path per host-side strip/controller partition

[recording]
record_data = true                            # If true, all commands will be recorded during the session
record_esp_data = true                        # If true, all commands will be recorded in a format that can be read by the ESP
unity_controls_recording = false              # If true, above two will be ignored and controlled by Unity
record_data_file = "record_data.vled"         # File to record vled data to
record_esp_data_file = "esp_data.bvled"       # File to record bvled data to

[camera]
multi_camera = false                          # If you are using one camera from the front, and one from the side (or overhead)
camera_index_1 = "0"                          # Can either be an index, or an RTSP address.
#camera_index_2 = "0"
#video_width = 1280                           # If set, attempt to override the resolution
#video_height = 720

[scan]
scan_mode = 0                                 # 0 indicates to find the brightest point, 1 indicates to filter by color
filter_color = 0                              # Color to filter by, 0 is R, 1 is G, 2 is B
filter_range = 90                             # HSV Range to use for default color filter
color_bright = 255                            # How bright the LED should be set when calibrating

[unity_options]
num_container = 1                             # Number of LED containers you are using
unity_ip = "127.0.0.1"                        # Where to connect to Unity/Houdini on
unity_ports = [5001]                          # Port(s) to send LED info on (Default is 5001)
unity_position_files = ["largetank.json"]     # Position file to send to Unity
scale = 0.0008                                # Position scale
```
### Advanced options

These should be enough to get started. However, if you need more advanced options, there are many more optional options:
```
[advanced.communication]
serial_read_timeout = 5                       # Timeout for reading back confirmation from the controller         
udp_read_timeout = 100                        # Timeout for using UDP
con_fail_limit = 15                           # How many consecutive timeouts before the program will exit
use_queue = true                              # Serial only: use a dedicated thread per controller
queue_size = 50                               # Size of queue
skip_confirmation = false                     # Skip waiting for controller to confirm that it received the command

[advanced.camera]
no_video = false                              # Disable any video output
get_events_streams_video = false              # Make most recent frame accessible via the API
get_events_video_widgets = false              # Draw circles on illuminated LEDs
get_events_widgets_pos_index = 0              # When drawing circles, which position file index to use from unity_position_files
capture_frames = 3                            # Number of frames to capture before using the most recent
cam2_overhead = false                         # If the second cam is overhead instead of to the side          
cam2_overhead_flip = false                    # If the overhead cam is upside down relative to the front cam
no_background_frame_consumer = false          # If the background thread to consume frames should be disabled (If enabled, you may be able to decrease capture_frames)

[advanced.hsv_overrides]
hsv_red_override = []                         # Automatically use specified HSV color filter, formatted in [h_lower, s_lower, v_lower, h_upper, s_upper, v_upper]
hsv_green_override = []
hsv_blue_override = []

[advanced.transform]
crop_override = []                            # Automatically use specified crop, formatted in [x_lower, y_lower, x_upper, y_upper]
x_perspect_distort_adjust = 0                 # Adjust for perspective distortion on the X axis, where the max distortion will be offset by the specified amount to make the resulting positions more orthographic
y_perspect_distort_adjust = 0
z_perspect_distort_adjust = 0

[advanced.misc]
print_send_back = false                       # Print the command the controller received, must be specified in the ESPs code aswell. Only useful for debugging
no_controller = false                         # Don't check if the controller is valid. Useful for debugging
```

### Notes

When using `use_queue` with serial controllers, you can get a massive performance boost with multiple controllers (one thread is spawned per controller) at the cost of potential latency if the queue length is set too high. Commands are sent to the appropriate controller queue, while the thread assigned to that queue drains it. UDP and WLED modes ignore this setting.

In serial mode, `num_strips` is the number of host-side controller partitions: provide the same
number of serial paths and divide `num_led` evenly between them. A single ESP that drives multiple
FastLED outputs still counts as one host-side partition if it receives one contiguous index range.

`queue_length` shouldn't be too high, as the higher it is, the more inaccuracy in the display you will get.

`skip_confirmation` should really be avoided, since the program will likely end up sending the next command while the device is processing the current one.

### WLED

Set `communication_mode = 3`, set `host` to the WLED device address, and either omit `port`
or set it to the realtime UDP port configured in WLED (normally `21324`). Enable **Receive UDP
realtime** in WLED's Sync settings. SVLED sends DNRGB packets in this mode and does not wait for
an acknowledgement. The existing custom UDP protocol remains available as mode `1` and its
five-byte packet plus acknowledgement behavior has not changed.

`capture_frames` can be decreased to improve scanning performance, but if it is too low, and your camera has a high enough frame rate, you may get completely scrambled and useless data.

`x(yz)_perspect_distort_adjust` is only really useful when using a very large amount of LEDs. It should be used in conjunction with `TODO` to prevent the LEDs becoming very squished.
