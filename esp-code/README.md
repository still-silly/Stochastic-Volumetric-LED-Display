# ESP firmware

The serial protocol is the hardware-tested default. Its wire format is shared in
`common/svled_serial_protocol.h` and mirrored by `src/protocol.rs` in the Rust host:

```
0xFF 0xBB index-low index-high red green blue
```

Do not change this layout, byte order, acknowledgement timing, or `FastLED.show()` timing
without testing both the Rust host and physical controllers.

## Recommended projects

- `receive_esp_serial`: portable direct serial receiver. Start here for a single controller.
- `receive_esp_serial_parallel_async`: ESP32/C3 receiver with a FreeRTOS queue. Use this for
  the high-throughput multi-controller setup after configuring pins, LED counts, and baud rate.
- `esp_main`: integrated demo firmware with networking, stored animations, sensors, and serial
  reception. It shares the same serial parser but intentionally remains a separate application.

`receive_esp_serial_parallel` is retained as a compatibility configuration for the older ESP32
pin/baud setup. It now shares the canonical parser, so packet handling is no longer duplicated.

## UDP status

`receive_esp32_udp` and `receive_esp8266_udp_neopixel` implement the legacy custom SVLED UDP
protocol. They are retained for compatibility but are not the recommended hardware path. The
custom packet remains exactly five bytes (`index-low index-high red green blue`) and returns `A`
or `BAD` to the sender.

WLED support does not use these sketches. Install WLED normally and select communication mode
`3` in `svled.toml`; the Rust host then sends WLED DNRGB realtime packets directly.

## Wi-Fi credentials

The UDP examples compile with `CHANGE_ME` placeholders. Supply credentials locally with
PlatformIO build flags or another ignored local configuration; never commit real credentials.
For example:

```ini
build_flags =
    -D WIFI_SSID=\"your-network\"
    -D WIFI_PASSWORD=\"your-password\"
```

## Serial hardware regression checklist

Before merging or releasing, exercise the same firmware with both `use_queue = false` and
`use_queue = true`:

1. Set index `0`, the last LED, and indexes on both sides of `255` if the controller has enough
   LEDs.
2. Confirm the first command after startup is applied; the host previously lost the command that
   initialized its worker queues.
3. With multiple serial controllers, test the last index on one controller and the first index on
   the next to verify host-side partition mapping.
4. Run `clear`, a short animation, and the speed test with confirmation enabled.
5. Repeat the normal workload at the actual deployed baud rate and queue size. These are the
   timing-sensitive values that a compile check cannot validate.
