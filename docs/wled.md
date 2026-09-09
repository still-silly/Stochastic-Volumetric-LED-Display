# WLED hardware test checklist

WLED support is intentionally separate from the hardware-tested SVLED serial and custom UDP
protocols. It uses WLED's documented DNRGB realtime packet format.

1. Install WLED normally and configure the correct LED type, pin, count, color order, brightness,
   and current limit.
2. In WLED's Sync settings, enable **Receive UDP realtime** and note the realtime UDP port
   (normally `21324`).
3. Configure SVLED with `communication_mode = 3`, the WLED device IP in `host`, and the port if
   it differs from the default.
4. Start with `svled set-color 0 32 0 0`, then test the last configured LED.
5. If the device has more than 256 LEDs, explicitly test indexes on both sides of 255 to verify
   the DNRGB index byte order.
6. Verify that WLED leaves realtime mode roughly two seconds after the final packet.

The first realtime packet may clear LEDs that have not yet been assigned; that is WLED realtime
mode behavior. This implementation sends one DNRGB packet per changed LED. Each packet includes
the changed LED and one adjacent color retained by the host because current WLED builds reject the
seven-byte, single-pixel form. Frame batching should be considered only after basic hardware
behavior is confirmed.

Protocol reference: <https://kno.wled.ge/interfaces/udp-realtime/>
