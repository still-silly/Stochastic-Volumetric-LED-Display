// Legacy SVLED UDP receiver for ESP8266. This uses SVLED's custom five-byte
// packet and is not the WLED protocol. Prefer serial where possible.
#include <NeoPixelBus.h>
#include <ESP8266WiFi.h>
#include <WiFiUdp.h>

#define LED_PIN 2
#define LED_COUNT 150
RgbColor red(255, 0, 0);
RgbColor green(0, 255, 0);
RgbColor clear(0);

int port = 8888;
#ifndef WIFI_SSID
#define WIFI_SSID "CHANGE_ME"
#endif
#ifndef WIFI_PASSWORD
#define WIFI_PASSWORD "CHANGE_ME"
#endif
const char *ssid = WIFI_SSID;
const char *password = WIFI_PASSWORD;

WiFiUDP UDP;
uint8_t packet[5];
char reply[] = "A";
char reply_bad[] = "BAD";

NeoPixelBus<NeoRgbFeature, Neo800KbpsMethod> strip(LED_COUNT, LED_PIN);

void setup()
{
  Serial.begin(115200);
  Serial.println("Serial Begin");

  strip.Begin(); // This took way to long to figure out, this is not ok
  strip.Show();
  WiFi.begin(ssid, password);

  Serial.print("Connecting to ");
  Serial.print(ssid);

  while (WiFi.status() != WL_CONNECTED)
  {
    delay(500);
    Serial.print(".");
    strip.SetPixelColor(0, red);
    strip.Show();
    delay(500);
    strip.SetPixelColor(0, clear);
    strip.Show();
  }
  strip.SetPixelColor(0, green);
  strip.Show();
  Serial.println(" \nConnected!");
  Serial.print("IP: ");
  Serial.println(WiFi.localIP());
  Serial.print("Port: ");
  Serial.println(port);
  UDP.begin(port);
  Serial.println("Listening for packets...");
}

void loop()
{

  // // If packet received...
  // int packetSize = UDP.parsePacket();
  // if (packetSize)
  // {
  //   // Serial.print("Received packet! Size: ");
  //   // Serial.println(packetSize);
  //   int len = UDP.read(packet, 12);

  //   if (len > 0)
  //   {
  //     packet[len] = '\0';
  //   }
  //   // Serial.print("Packet received: ");
  //   // Serial.println(packet);
  //   String packetStr(packet);
  //   String n = packetStr.substring(0, 3);
  //   String r = packetStr.substring(3, 6);
  //   String g = packetStr.substring(6, 9);
  //   String b = packetStr.substring(9, 12);

  //   RgbColor color(r.toInt(), g.toInt(), b.toInt());
  //   strip.SetPixelColor(n.toInt(), color);
  //   strip.Show();

  //   UDP.beginPacket(UDP.remoteIP(), UDP.remotePort());
  //   UDP.write(reply);
  //   UDP.endPacket();
  // }
  int packetSize = UDP.parsePacket();
  if (packetSize)
  {
    int len = UDP.read(packet, 5);

    if (packetSize == 5 && len == 5)
    {
      byte n1 = packet[0];
      byte n2 = packet[1];
      byte r = packet[2];
      byte g = packet[3];
      byte b = packet[4];

      int n = (n2 << 8) | n1; // Convert n1 and n2 to a uint16_t

      bool validIndex = n < LED_COUNT;
      if (validIndex)
      {
        RgbColor color(r, g, b);
        strip.SetPixelColor(n, color);
        strip.Show();
      }

      UDP.beginPacket(UDP.remoteIP(), UDP.remotePort());
      UDP.write(reinterpret_cast<const uint8_t *>(validIndex ? reply : reply_bad), validIndex ? 1 : 3);
      UDP.endPacket();
    }
    else
    {
      UDP.beginPacket(UDP.remoteIP(), UDP.remotePort());
      UDP.write(reinterpret_cast<const uint8_t *>(reply_bad), 3);
      UDP.endPacket();
    }
  }
}
