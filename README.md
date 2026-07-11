# SVLED
## Stochastic Volumetric LEDs (3D Mapped LEDs [pretty looking lights])
[![Linux x86_64](https://github.com/timothyhay256/Stochastic-Volumetric-LED-Display/actions/workflows/rust-linux.yml/badge.svg)](https://github.com/timothyhay256/Stochastic-Volumetric-LED-Display/actions/workflows/rust-linux.yml) [![RPI armv7](https://github.com/timothyhay256/Stochastic-Volumetric-LED-Display/actions/workflows/rust-raspbian.yml/badge.svg)](https://github.com/timothyhay256/Stochastic-Volumetric-LED-Display/actions/workflows/rust-raspbian.yml) [![Build for MacOS x86_64](https://github.com/timothyhay256/Stochastic-Volumetric-LED-Display/actions/workflows/rust-macos.yml/badge.svg)](https://github.com/timothyhay256/Stochastic-Volumetric-LED-Display/actions/workflows/rust-macos.yml) [![Build for MacOS arm64](https://github.com/timothyhay256/Stochastic-Volumetric-LED-Display/actions/workflows/rust-macos-apple-silicon.yml/badge.svg)](https://github.com/timothyhay256/Stochastic-Volumetric-LED-Display/actions/workflows/rust-macos-apple-silicon.yml) [![Build for Windows x86_64](https://github.com/timothyhay256/Stochastic-Volumetric-LED-Display/actions/workflows/rust-windows.yml/badge.svg)](https://github.com/timothyhay256/Stochastic-Volumetric-LED-Display/actions/workflows/rust-windows.yml)  

*i wrote this shit by fucking hand btw, none of this is generated code fuck my life*

*Hello those from Open Sauce!*

This project allows you to scan a container full of individually addressible RGB leds, and create a representation of those LEDs in 3d space in Unity/Houdini, where you can apply whatever effects in 3d that you desire.  
It also has the ability to save animations either to a file, or directly to the LED controller (ESP32 and friends). 

Additionally, it supports multiple controllers on the same strip, meaning that it can drive very large amounts of LEDs very fast (~20,000 LEDs per second with 40 controllers and 2,000 LEDs).  

https://github.com/user-attachments/assets/2d306b9b-878b-488d-bfa9-6dc4f7c6ec3e

## Usage

If you want to use this project, you will need a couple of things:
 - LEDs - anything that is supported by FastLED will work fine.
 - Any microcontroller supported by FastLED and PlatformIO. You may need a dual core controller depending on the number of LEDs and features that you want.  
 
Once you have the hardware, go to the [Wiki.](https://github.com/timothyhay256/Stochastic-volumetric-LED-display/wiki/Setting-up-LEDs)  
