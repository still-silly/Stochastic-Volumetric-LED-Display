#pragma once

#include <Arduino.h>

// Hardware-tested SVLED serial protocol. Keep this layout synchronized with
// src/protocol.rs:
//   FF BB | index low | index high | red | green | blue
namespace svled_protocol
{
constexpr uint8_t PACKET_START_0 = 0xFF;
constexpr uint8_t PACKET_START_1 = 0xBB;
constexpr size_t PACKET_SIZE = 7;

struct ColorCommand
{
    uint16_t index;
    uint8_t red;
    uint8_t green;
    uint8_t blue;
};

inline bool readColorCommand(Stream &stream, ColorCommand &command)
{
    if (stream.available() < static_cast<int>(PACKET_SIZE))
    {
        return false;
    }

    // This deliberately preserves the original receiver's byte-at-a-time
    // framing behavior. Do not change resynchronization without hardware tests.
    if (stream.read() != PACKET_START_0 || stream.read() != PACKET_START_1)
    {
        return false;
    }

    const int indexLow = stream.read();
    const int indexHigh = stream.read();
    const int red = stream.read();
    const int green = stream.read();
    const int blue = stream.read();
    if (indexLow < 0 || indexHigh < 0 || red < 0 || green < 0 || blue < 0)
    {
        return false;
    }

    command.index = static_cast<uint16_t>(indexLow) |
                    (static_cast<uint16_t>(indexHigh) << 8);
    command.red = static_cast<uint8_t>(red);
    command.green = static_cast<uint8_t>(green);
    command.blue = static_cast<uint8_t>(blue);
    return true;
}
} // namespace svled_protocol
