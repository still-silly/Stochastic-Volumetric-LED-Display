/*
This is the main script to use the project. Uncomment and comment the #define lines that apply to your setup.
After that, go to the user defined variables and set them
*/

#define USE_GYRO    // Uncomment if you want to use a MPU6050 Gyroscope
#define USE_NETWORK // Uncomment if you are using a device with networking capabilities
#define MULTICORE   // Uncomment if you are using a multicore device (e.g ESP32)

#include <Arduino.h>
#include <FastLED.h>
#include "../../common/svled_serial_protocol.h"
#ifdef USE_NETWORK
#include <WiFi.h>
#include <AsyncTCP.h>
#include <ESPAsyncWebServer.h>
#endif

/*
Begin MPU6050 Code
*/
#ifdef USE_GYRO
#include "I2Cdev.h"
#include "MPU6050_6Axis_MotionApps20.h"

MPU6050 mpu;

#define INTERRUPT_PIN 2

volatile bool mpuInterrupt = false; // Interrupt flag
bool dmpReady = false;              // DMP status flag
uint8_t mpuIntStatus;               // MPU interrupt status
uint8_t devStatus;                  // Device status
uint16_t packetSize;                // DMP packet size
uint8_t fifoBuffer[64];             // FIFO buffer

Quaternion q;        // Quaternion data container
VectorFloat gravity; // Gravity vector
float ypr[3];        // Yaw/Pitch/Roll angles

void dmpDataReady()
{
    mpuInterrupt = true;
}
#endif
/*
End MPU6050 code
*/

// Animation data section
// Place animations that you would like to be able to play back here.
// const uint8_t animationZero[] = {0x01, 0x02, 0x03, 0x04, 0xff, 0x01, 0xff, 0x00, 0x00, 0x02, 0x00, 0xff, 0x00, 0x03, 0x00, 0x00, 0xff}; // # This will wait for 255 ms, set LED 1 to red, 2, to green, and 3 to blue.
// Pink block up and down
const uint8_t animationZero[] = {};
int sizeOfAnimationZero = sizeof(animationZero) / sizeof(animationZero[0]);

// Blue block left and right
const uint8_t animationOne[] = {};
int sizeOfAnimationOne = sizeof(animationOne) / sizeof(animationOne[0]);

// This plays when the microphone detects sound, after filling the string.
const uint8_t playOnTrigger[] = {};
int sizeOfAnimationPlayOnTrigger = sizeof(playOnTrigger) / sizeof(playOnTrigger[0]);
const uint8_t *activeAnimationArray;

// End animation data

// Change variables below me!

#define BAUD_RATE 921600 // Baud rate for serial communication
#define LED_COUNT 100    // How many LEDs are you using? For more advanced usage, such as parallel outputs, just change the FastLED setup section. As long as the leds array is setup, the rest of the program will work.
#define LED_PIN 27       // What pin are we using for the data line?

#ifdef USE_NETWORK                        // Only change this if you are using WiFi
const char *ssid = "SSID";                // WiFi network SSID
const char *password = "PASS";            // WiFi network password
const char *udpTarget = "192.168.86.111"; // Where should gyro data be sent? (If using gyroscope)
const int udpPort = 5014;                 // What port should gyro data be sent on?
#endif

const int microphonePin = 26; // If you are using a microphone, it should be connected to this pin.

// If using a gyroscope, then you need to set the offsets. These are printed to serial on boot, in the following format:
// >.....>.......ignoreMe,  ignoreMe,   zAccelOffset,   xGyroOffset,    yGyroOffset,    zGyroOffset
const int xGyroOffset = -8;    // 54, -14
const int yGyroOffset = 13;    // -4, 1
const int zGyroOffset = 30;    // -20, 30
const int zAccelOffset = 4940; // 5086, 4940

// Change variables above me!

#ifdef MULTICORE
TaskHandle_t animationTask; // Task for multicore
#endif

void secondCoreManager(void *parameter); // Needed for PIO
void animateSoundReactive();
void sendGyroData();
void receiveCommands();

int activeAnimation = 0;
int currentLoop = 0;
int activeR = 0;
int activeG = 0;
int activeB = 0;
int activeTime = 0;
int activeIndex = 0;
bool animationZeroActive = true;
bool animationOneActive = false;
bool soundModeActive = false;
bool sendReceiveActive = false; // Accept LED commands, send Gyroscope data
bool sendBack = false;          // Should I send back the values I set? For debugging only
String brightnessValue;         // How bright the strip should be
int speedValue = 50;            // Relative range of how fast animations should play (0 much slower, 50 regular speed, 100 fastest possible)
float maxSpeedMultiplier = 2.0; // Multiply the delay by this value when speedValue is at 0 (e.g, at 2, the slowest speed is 2x as slow) [not working yet]

int selectedMode = 0; // Default to animation zero
int numOfModes = 3;   // How many modes are there? (Total modes)
int test = 0;

int wifiFailCounter = 0;

int activeByteIndex = 0; // Track which byte we are looking at (index, r, g, b, timing)
int currentIndex = 0;    // Track how far into the array we are

// #define LED_COUNT_PER_STRIP 50
// #define NUM_STRIPS
const char *PARAM_MESSAGE = "message";

CRGB leds[LED_COUNT];

#ifdef USE_NETWORK
AsyncWebServer server(80);
WiFiUDP udp;

void notFound(AsyncWebServerRequest *request)
{
    request->send(404, "text/plain", "Not found");
}

String createHtml()
{
    String response = R"(
      <!DOCTYPE html><html>
        <head>
          <title>Cool jar thingy</title>
          <meta name="viewport" content="width=device-width, initial-scale=1">
          <style>
            html { font-family: sans-serif; text-align: center; }
            body { display: inline-flex; flex-direction: column; }
            .slider { -webkit-appearance: none; margin: 14px; width: 360px; height: 25px; background: #333333;
            outline: none; -webkit-transition: .2s; transition: opacity .2s;}
            .slider::-webkit-slider-thumb {-webkit-appearance: none; appearance: none; width: 35px; height: 35px; background: #003249; cursor: pointer;}
            .slider::-moz-range-thumb { width: 35px; height: 35px; background: #003249; cursor: pointer; } 
            h1 { margin-bottom: 1.2em; } 
            h2 { margin: 0; }
            div { display: grid; grid-template-columns: 1fr 1fr; grid-template-rows: auto auto; grid-auto-flow: column; grid-gap: 1em; }
            .btn { background-color: #5B5; border: none; color: #fff; padding: 0.5em 1em;
                   font-size: 2em; text-decoration: none }
            .btn.OFF { background-color: #333; }
          </style>
        </head>
              
        <body>
          <h1>Select your animation</h1>
          <div>
            <h2>Animation 1</h2>
            <a href="?toggle=1" class="btn ANIME1_TEXT">ANIME1_TEXT</a>
            <h2>Animation 2</h2>
            <a href="?toggle=2" class="btn ANIME2_TEXT">ANIME2_TEXT</a>
            <h2>Sound Reactive Mode</h2>
            <a href="?toggle=50" class="btn SOUND_MODE">SOUND_MODE</a>
          </div>
          <h1>Special modes</h1>
          <div>
            <h2>Receive commands, send gyro</h2>
            <a href="?toggle=60" class="btn SPECIAL1_TEXT">SPECIAL1_TEXT</a>
          </div>
          <h1>Settings</h1>
          <div>
            <h2 style="margin-top: 5px;">Brightness</h2>
  <p><input type="range" onchange="updateBrightness(this) " id="pwmSlider" min="0" max="255" value="%SLIDERVALUE%" step="1" class="slider"></p>
          </div>
          <script>
            function updateBrightness(element) {
            var sliderValue = document.getElementById("pwmSlider").value;
            document.getElementById("textSliderValue").innerHTML = sliderValue;
            console.log(sliderValue);
            var xhr = new XMLHttpRequest();
            xhr.open("GET", "/slider?value="+sliderValue, true);
            xhr.send();
            }
          </script>
        </body>
      </html>
    )";
    response.replace("ANIME1_TEXT", animationZeroActive ? "ON" : "OFF");
    response.replace("ANIME2_TEXT", animationOneActive ? "ON" : "OFF");
    response.replace("SOUND_MODE", soundModeActive ? "ON" : "OFF");
    response.replace("SPECIAL1_TEXT", sendReceiveActive ? "ON" : "OFF");
    response.replace("SLIDERVALUE", brightnessValue);
    return response;
}
#endif

void setup()
{
    Serial.begin(BAUD_RATE);

    pinMode(microphonePin, INPUT);

    // FastLED.addLeds<WS2811, LED_PIN, RGB>(leds, LED_COUNT);

    FastLED.addLeds<WS2811, 27, BGR>(leds, 50);
    FastLED.addLeds<WS2811, 13, BGR>(leds + 50, 50);

    FastLED.setBrightness(255);
    FastLED.clear();
    FastLED.show();

#ifdef MULTICORE
    xTaskCreatePinnedToCore( // Start animation process
        secondCoreManager,   /* Function to implement the task */
        "animationTask",     /* Name of the task */
        10000,               /* Stack size in words */
        NULL,                /* Task input parameter */
        0,                   /* Priority of the task */
        &animationTask,      /* Task handle. */
        0);                  /* Core where the task should run */
#endif

#ifdef USE_NETWORK
    WiFi.mode(WIFI_STA);
    WiFi.begin(ssid, password);

    Serial.print("Connecting to ");
    Serial.print(ssid);
    while (WiFi.status() != WL_CONNECTED)
    {
        delay(500);
        Serial.print(".");
        wifiFailCounter++;

        if (wifiFailCounter >= 20)
        {
            Serial.println("Resetting ESP due to failure to connect to WiFi in time...");
            ESP.restart();
        }
    }
    Serial.println(" \nConnected!");
    Serial.print("IP: ");
    Serial.println(WiFi.localIP());

    server.on("/", HTTP_GET, [](AsyncWebServerRequest *request)
              {
        if(request->hasParam("toggle")) {
            const AsyncWebParameter* led = request->getParam("toggle");
            Serial.print("Toggle animation #");
            Serial.println(led->value());
      
            switch (led->value().toInt()) {
                case 1:
                    activeAnimation = 0;
                    animationZeroActive = true;
                    animationOneActive = false;
                    soundModeActive = false;
                    sendReceiveActive = false;
                    FastLED.clear();
                    break;
            
                case 2:
                    activeAnimation = 1;
                    animationZeroActive = false;
                    animationOneActive = true;
                    soundModeActive = false;
                    sendReceiveActive = false;
                    FastLED.clear();
                    break; 
                case 50:
                    activeAnimation = 50;
                    animationZeroActive = false;
                    animationOneActive = false;
                    soundModeActive = true;
                    sendReceiveActive = false;
                    FastLED.clear();
                    break;
                case 60:
                    activeAnimation = 60;
                    animationZeroActive = false;
                    animationOneActive = false;
                    soundModeActive = false;
                    sendReceiveActive = true;
                    FastLED.clear();
                    break;
            }
        }
  
        request->send(200, "text/html", createHtml()); });

    // Send a GET request to <IP>/get?message=<message>
    server.on("/get", HTTP_GET, [](AsyncWebServerRequest *request)
              {
        String message;
        if (request->hasParam(PARAM_MESSAGE)) {
            message = request->getParam(PARAM_MESSAGE)->value();
        } else {
            message = "No message sent";
        }
        request->send(200, "text/plain", "Hello, GET: " + message); });

    // Send a POST request to <IP>/post with a form field message set to <message>
    server.on("/post", HTTP_POST, [](AsyncWebServerRequest *request)
              {
        String message;
        if (request->hasParam(PARAM_MESSAGE, true)) {
            message = request->getParam(PARAM_MESSAGE, true)->value();
        } else {
            message = "No message sent";
        }
        request->send(200, "text/plain", "Hello, POST: " + message); });

    server.on("/slider", HTTP_GET, [](AsyncWebServerRequest *request)
              {
    String inputMessage;
    // GET input1 value on <ESP_IP>/slider?value=<inputMessage>
    if (request->hasParam("value")) {
      inputMessage = request->getParam("value")->value();
      brightnessValue = inputMessage;
      Serial.println("Set brightness to "+brightnessValue);
      FastLED.setBrightness(brightnessValue.toInt());
    }
    else {
      inputMessage = "No message sent";
    }
    Serial.println(inputMessage);
    request->send(200, "text/plain", "OK"); });

    server.onNotFound(notFound);

    server.begin();

    Serial.println("HTTP server started.");
#endif

#ifdef USE_GYRO
    Serial.println("Starting MPU6050 initialization...");

    /*
    MPU6050 Setup Begin
    */
    Wire.begin();
    Wire.setClock(400000); // Optional, adjust as necessary
    Serial.begin(BAUD_RATE);
    while (!Serial)
        ;

    pinMode(INTERRUPT_PIN, INPUT);

    Serial.println(F("Initializing I2C devices..."));
    mpu.initialize();

    Serial.println(F("Testing device connections..."));
    Serial.println(mpu.testConnection() ? F("MPU6050 connection successful") : F("MPU6050 connection failed"));

    Serial.println(F("Initializing DMP..."));
    devStatus = mpu.dmpInitialize();

    mpu.setXGyroOffset(xGyroOffset);
    mpu.setYGyroOffset(yGyroOffset);
    mpu.setZGyroOffset(zGyroOffset);
    mpu.setZAccelOffset(zAccelOffset); // Adjust as necessary

    if (devStatus == 0)
    {
        mpu.CalibrateAccel(6);
        mpu.CalibrateGyro(6);
        mpu.PrintActiveOffsets();

        Serial.println(F("Enabling DMP..."));
        mpu.setDMPEnabled(true);

        Serial.print(F("Enabling interrupt detection (Arduino external interrupt "));
        Serial.print(digitalPinToInterrupt(INTERRUPT_PIN));
        Serial.println(F(")..."));
        attachInterrupt(digitalPinToInterrupt(INTERRUPT_PIN), dmpDataReady, RISING);
        mpuIntStatus = mpu.getIntStatus();

        Serial.println(F("DMP ready! Waiting for first interrupt..."));
        dmpReady = true;

        packetSize = mpu.dmpGetFIFOPacketSize();
    }
    else
    {
        Serial.print(F("DMP Initialization failed (code "));
        Serial.print(devStatus);
        Serial.println(F(")"));
    }
#endif

    /*
    MPU6050 Setup End
    */

    Serial.println("Setup successful");
}

void loop()
{
    if (sendReceiveActive)
    {
        receiveCommands();
    }
    else if (touchRead(4) <= 20)
    {
        while (touchRead(4) <= 20)
        {
            delay(20);
        }
        selectedMode += 1;
        if (selectedMode > (numOfModes - 1))
        {
            selectedMode = 0;
        }
        Serial.print("Current selectedMode: ");
        Serial.println(selectedMode);
        FastLED.clear();
        switch (selectedMode)
        {
        case 0:
            activeAnimation = 0;
            animationZeroActive = true;
            animationOneActive = false;
            soundModeActive = false;
            break;
        case 1:
            activeAnimation = 1;
            animationZeroActive = false;
            animationOneActive = true;
            soundModeActive = false;
            break;
        case 2:
            Serial.println("Starting sound sensitive mode");
            activeAnimation = 50;
            animationZeroActive = false;
            animationOneActive = false;
            soundModeActive = true;
            break;
        }
    }
    else
    {
        delay(40); // Allow CPU to do other tasks
    }
}

void secondCoreManager(void *parameter)
{ // Manages all second core activities
    for (;;)
    {
        // delay(1000);
        switch (activeAnimation)
        {
        case 0:
            activeAnimationArray = animationZero;
            if (currentIndex >= sizeOfAnimationZero)
            {
                currentIndex = 0;
                // Serial.println("Resetting index");
            }
            break;
        case 1:
            activeAnimationArray = animationOne;
            if (currentIndex >= sizeOfAnimationOne)
            {
                currentIndex = 0;
            }
            break;
        case 50: // Special modes should be some number that will not be feasibly reached by stored animations
            // Serial.println("Running animateSoundReactive");
            animateSoundReactive();
            break;
        case 60:
#ifdef USE_GYRO
            sendGyroData();
#endif
            break;
        }
        if (activeAnimation != 50 && activeAnimation != 60)
        { // Add any special modes or conditions here that should prevent the regular animation from running
            if (activeAnimationArray[currentIndex] == 1 && activeAnimationArray[currentIndex + 1] == 2 && activeAnimationArray[currentIndex + 2] == 3 && activeAnimationArray[currentIndex + 3] == 4)
            {
                if (speedValue = 50)
                {
                    delay(activeAnimationArray[currentIndex + 4]);
                }
                if (0 <= speedValue < 50)
                {
                    delay(activeAnimationArray[currentIndex + 4]);
                }
                currentIndex += 5;
            }
            else
            { // No timing instruction, assume it is a LED instruction
                activeIndex = activeAnimationArray[currentIndex];
                activeR = activeAnimationArray[currentIndex + 1];
                activeG = activeAnimationArray[currentIndex + 2];
                activeB = activeAnimationArray[currentIndex + 3];

                leds[activeIndex] = CRGB(activeR, activeG, activeB);
                FastLED.show();

                currentIndex += 4;
            }
        }
    }
}

void animateSoundReactive()
{ // Plays animation once when sound threshold is exceeded
    // Serial.println(digitalRead(microphonePin));
    if (digitalRead(microphonePin) == HIGH)
    {
        Serial.println("Exceeded sound threshold!");
        fill_solid(leds, 50, CRGB(255, 255, 25));
        FastLED.show();
        currentIndex = 0;
        while (currentIndex < sizeOfAnimationPlayOnTrigger)
        {
            if (playOnTrigger[currentIndex] == 1 && playOnTrigger[currentIndex + 1] == 2 && playOnTrigger[currentIndex + 2] == 3 && playOnTrigger[currentIndex + 3] == 4)
            { // TODO: Delay multiplier to change speed of animations
                delay(playOnTrigger[currentIndex + 4]);
                currentIndex += 5;
            }
            else
            { // No timing instruction, assume it is a LED instruction
                activeIndex = playOnTrigger[currentIndex];
                activeR = playOnTrigger[currentIndex + 1];
                activeG = playOnTrigger[currentIndex + 2];
                activeB = playOnTrigger[currentIndex + 3];

                leds[activeIndex] = CRGB(activeR, activeG, activeB);
                FastLED.show();

                currentIndex += 4;
            }
        }
    }
}

#ifdef USE_GYRO
void sendGyroData()
{
    if (!dmpReady)
        return;
    delay(10);
    if (mpu.dmpGetCurrentFIFOPacket(fifoBuffer))
    {
        // Serial.println("fds");
        mpu.dmpGetQuaternion(&q, fifoBuffer);

        udp.beginPacket(udpTarget, udpPort);
        udp.print(q.w);
        // Serial.println(q.w);
        udp.print(",");
        udp.print(q.x);
        // Serial.println(q.x);
        udp.print(",");
        udp.print(q.y);
        // Serial.println(q.y);
        udp.print(",");
        udp.println(q.z);
        // Serial.println(q.z);
        udp.endPacket();
    }
}
#endif

void receiveCommands()
{ // Listen for and get commands over Serial
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
