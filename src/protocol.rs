//! Wire formats shared by the controller transports.
//!
//! The custom serial and UDP layouts are hardware-tested legacy protocols. Keep
//! them stable. WLED uses its documented DNRGB realtime format and is deliberately
//! a separate communication mode.

pub const COMMUNICATION_MODE_SVLED_UDP: i8 = 1;
pub const COMMUNICATION_MODE_SERIAL: i8 = 2;
pub const COMMUNICATION_MODE_WLED: i8 = 3;

pub const DEFAULT_SVLED_UDP_PORT: i32 = 8888;
pub const DEFAULT_WLED_UDP_PORT: i32 = 21324;
pub const DEFAULT_SERIAL_BAUD_RATE: u32 = 921_600;

pub const SERIAL_PACKET_START: [u8; 2] = [0xFF, 0xBB];
pub const WLED_DNRGB_PROTOCOL: u8 = 4;
pub const WLED_REALTIME_TIMEOUT_SECONDS: u8 = 2;

pub fn serial_color_packet(index: u16, r: u8, g: u8, b: u8) -> [u8; 7] {
    let [index_low, index_high] = index.to_le_bytes();
    [
        SERIAL_PACKET_START[0],
        SERIAL_PACKET_START[1],
        index_low,
        index_high,
        r,
        g,
        b,
    ]
}

pub fn svled_udp_color_packet(index: u16, r: u8, g: u8, b: u8) -> [u8; 5] {
    let [index_low, index_high] = index.to_le_bytes();
    [index_low, index_high, r, g, b]
}

pub fn wled_dnrgb_color_packet(index: u16, first: [u8; 3], second: [u8; 3]) -> [u8; 10] {
    let [index_low, index_high] = index.to_le_bytes();
    [
        WLED_DNRGB_PROTOCOL,
        WLED_REALTIME_TIMEOUT_SECONDS,
        index_high,
        index_low,
        first[0],
        first[1],
        first[2],
        second[0],
        second[1],
        second[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_hardware_tested_serial_layout() {
        assert_eq!(
            serial_color_packet(0x1234, 0x56, 0x78, 0x9A),
            [0xFF, 0xBB, 0x34, 0x12, 0x56, 0x78, 0x9A]
        );
    }

    #[test]
    fn preserves_legacy_svled_udp_layout() {
        assert_eq!(
            svled_udp_color_packet(0x1234, 0x56, 0x78, 0x9A),
            [0x34, 0x12, 0x56, 0x78, 0x9A]
        );
    }

    #[test]
    fn encodes_wled_dnrgb_layout() {
        assert_eq!(
            wled_dnrgb_color_packet(0x1234, [0x56, 0x78, 0x9A], [0xAB, 0xCD, 0xEF]),
            [4, 2, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xAB, 0xCD, 0xEF]
        );
    }
}
