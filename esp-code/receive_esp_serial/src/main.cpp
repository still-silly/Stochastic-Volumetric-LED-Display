#include <FastLED.h> // This just accepts commands via serial, and should work fine on basically any microprocessor.
#include "../../common/svled_serial_protocol.h"

#define LED_PIN 14
#define LED_COUNT 150
#define COLOR_ORDER GRB // Assuming the LED strip color order is GRB
#define BAUD_RATE 921600

int cycle = 0;
int set_every = 5;     // run show() every n assignments
bool sendBack = false; // Should I send back what instructions I just carried out? For debugging.

CRGB leds[LED_COUNT];

void setup()
{
    Serial.begin(BAUD_RATE);                                        // Set baud rate
    FastLED.addLeds<WS2811, LED_PIN, COLOR_ORDER>(leds, LED_COUNT); // Define LED strip
    FastLED.setBrightness(255);                                     // Set initial brightness
    FastLED.clear();                                                // Clear the LED strip
    FastLED.show();                                                 // Update LED strip
}

void loop()
{
    svled_protocol::ColorCommand command;
    if (svled_protocol::readColorCommand(Serial, command))
    {
        if (command.index < LED_COUNT)
        {
            leds[command.index] = CRGB(command.red, command.green, command.blue);
            FastLED.show();
        }

        if (sendBack)
        {
            String message = String(command.index) + "|" + String(command.red) + "|" +
                             String(command.green) + "|" + String(command.blue);
            Serial.println(message);
        }
        else
        {
            // Preserve the existing one-byte acknowledgement and its timing.
            Serial.write(0x01);
        }
    }
}
