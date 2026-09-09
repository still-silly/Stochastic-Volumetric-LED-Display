// #define FASTLED_ESP32_I2S true // For up to 24 pin paralellism
// #define FASTLED_OVERCLOCK 1.1
#include <Arduino.h>
#include <FastLED.h>
#include "../../common/svled_serial_protocol.h"
// Manually set pins inside of setup(), and set LED_COUNT_PER_STRIP and NUM_STRIPS. This FastLED setup section can also be used for any other scripts.
// ESP8266: WS2811_PORTA - pins 12, 13, 14 and 15 (or pins 6,7,5 and 8 on the NodeMCU boards/pin layout).   From FastLED wiki
// ESP32: Manually set pins

// TODO: Queue on one CPU with LED assignments on another

#define LED_COUNT_PER_STRIP 50
#define NUM_STRIPS 2
#define COLOR_ORDER GRB // Assuming the LED strip color order is GRB
bool sendBack = false;  // Should I send back what instructions I just carried out? For debugging.

#define BAUD_RATE 921600

TaskHandle_t fastledTask;
QueueHandle_t queue;
int msg[5];
int msg_rcv[5];

CRGB leds[LED_COUNT_PER_STRIP * NUM_STRIPS];

int cycle = 0;
int set_every = 0; // run show() every n assignments
int n1, n2, n, r, g, b;
byte ack;

void task0(void *pvParameters);

void setup()
{
    Serial.begin(BAUD_RATE);

    // For ESP8266
    // FastLED.addLeds<WS2811_PORTA,NUM_STRIPS, RGB>(leds, LED_COUNT_PER_STRIP);

    // For ESP32
    FastLED.addLeds<WS2811, 27, RGB>(leds, LED_COUNT_PER_STRIP);
    FastLED.addLeds<WS2811, 13, RGB>(leds + LED_COUNT_PER_STRIP, LED_COUNT_PER_STRIP);
    // FastLED.addLeds<WS2811, 12, GRB>(leds + LED_COUNT_PER_STRIP, LED_COUNT_PER_STRIP);
    // FastLED.addLeds<WS2811, 13, RGB>(leds, LED_COUNT_PER_STRIP, LED_COUNT_PER_STRIP);
    // FastLED.addLeds<WS2811, 14, RGB>(leds, 2 * LED_COUNT_PER_STRIP, LED_COUNT_PER_STRIP);
    // FastLED.addLeds<WS2811, 26, RGB>(leds, 3 * LED_COUNT_PER_STRIP, LED_COUNT_PER_STRIP);

    // FastLED.setBrightness(255);
    // fill_solid(leds, LED_COUNT_PER_STRIP * NUM_STRIPS, CRGB::Red);
    // FastLED.show();
    for (int i = 0; i < 3; i++)
    {
        leds[0] = CRGB::White;
        FastLED.show();
        delay(100);
        leds[0] = CRGB::Black;
        FastLED.show();
        delay(100);
    }

    queue = xQueueCreate(10, sizeof(msg));

    xTaskCreatePinnedToCore(
        task0,           // Function to implement the task
        "LEDUpdateTask", // Name of the task
        10000,           // Stack size in words
        NULL,            // Task input parameter
        1,               // Priority of the task
        NULL,            // Task handle
        1);
}

void loop()
{
    svled_protocol::ColorCommand command;
    if (svled_protocol::readColorCommand(Serial, command))
    {
        msg[0] = command.index & 0xFF;
        msg[1] = command.index >> 8;
        msg[2] = command.red;
        msg[3] = command.green;
        msg[4] = command.blue;

        xQueueSend(queue, &msg, portMAX_DELAY);
        // Preserve the existing acknowledgement: current hardware uses this as queue depth.
        Serial.write(uxQueueMessagesWaiting(queue));
    }
}

void task0(void *pvParameters)
{
    for (;;)
    {
        if (xQueueReceive(queue, &msg_rcv, portMAX_DELAY) == pdTRUE)
        {
            int n = (msg_rcv[1] << 8) | msg_rcv[0]; // Convert n1 and n2 to a uint16_t

            if (n < LED_COUNT_PER_STRIP * NUM_STRIPS)
            {
                leds[n] = CRGB(msg_rcv[2], msg_rcv[3], msg_rcv[4]);
                FastLED.show();
            }
        }
    }
}
