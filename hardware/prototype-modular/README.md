# Daily Mirror — modular prototype comparison

Research date: September 14, 2026. USD. Purchasing candidates, not a verified working kit.

## Preferred first prototype

Use a purchased processor board, a ribbon-connected camera, and hand-soldered lighting and controls. This avoids the setup cost of assembling a new processor PCB for the first unit. Keep the custom PCB placement study in `../pcb-p4/` for later development.

The first decision remains **camera compatibility**. The existing 16 MP Arducam IMX519 is required. Neither its driver nor full-resolution still capture on the ESP32-P4 has been demonstrated here. A matching-looking MIPI connector does not establish compatible pinout, voltage, cable contact orientation, driver, or image pipeline. Do not order a P4 board on the assumption that this camera will work without development.

## Parts and budget

| Item | One prototype | Ten prototypes | Basis |
| --- | ---: | ---: | --- |
| Waveshare ESP32-P4-NANO basic board | $18.99 | $189.90 | Store lists from $18.99; ten uses the same unit price, not a bulk quote |
| White photo lights and driver | $10–20 | $100–200 | Planning allowance; exact light/driver pairing remains to be selected |
| Perfboard, wiring, button, RGB indicator, resistors | $5–10 | $50–100 | Planning allowance; existing parts may reduce this |
| **Base parts budget** | **$33.99–48.99** | **$339.90–489.90** | Calculated from the rows above |
| Optional VEML7700 light sensor, Adafruit #4162 | $4.95 | $44.60 | In stock when checked; ten-unit price $4.46 each |
| Optional LIS3DH accelerometer, Adafruit #2809 | $4.95 | $44.60 | In stock when checked; ten-unit price $4.46 each |
| **With both sensors** | **$43.89–58.89** | **$429.10–579.10** | Sensor cables may add cost |

Excludes camera, camera cable/adapters, USB power supply/cable, enclosure and diffuser, shipping, tax, tools, and soldering labor. No Amazon listing, delivered checkout price, or complete compatible kit has been verified. These are budgeting numbers, not a vendor quote.

The NANO includes an ESP32-C6 for Wi-Fi/Bluetooth, USB-C, and a MIPI camera connection. The P4 chip itself does not include Wi-Fi. The board's documented camera examples use other sensors, including OV5647. The reference files below are for this particular NANO board; do not substitute another P4 variant without checking its schematic and firmware support.

## What the optional sensors do

- **Ambient light sensor:** measures room brightness. Sample it before the photo lights turn on, and give it a window or position that limits glare from the device's own lights. It can help choose a lighting preset; camera exposure still needs control.
- **Accelerometer:** measures the direction of gravity while stationary, enough to decide which way is up and rotate a photo. A gyroscope is unnecessary for that basic use. It does not determine compass heading, and motion can temporarily disturb its tilt reading.
- Both recommended breakouts support I²C, allowing shared data/clock wiring with suitable free GPIOs. Final pins and bus pull-ups depend on the processor-board schematic. No pin assignment has been approved yet.

## Physical arrangement

Mount the purchased processor board behind the camera/light faceplate. Run the original camera ribbon directly to a compatible CSI connection. Put white LEDs around the camera opening, with a snap-in diffuser above them and the separate RGB status LED/button accessible from outside. A small two-layer controls/light carrier can be considered after the camera and lights work on the bench.

Keep a proper current-regulated light driver and check pulse duration, power supply capacity, and LED temperature. A short flash reduces average heat, but does not eliminate peak current or thermal limits. The Temu CL651-3S image alone does not establish its electrical ratings; it is not a qualified lighting part in this budget.

## Saved references and sources

- [Waveshare NANO product](https://www.waveshare.com/esp32-p4-nano.htm)
- [Waveshare NANO documentation](https://docs.waveshare.com/ESP32-P4-NANO)
- [Official resources and examples](https://docs.waveshare.com/ESP32-P4-NANO/Resources-And-Documents)
- [Downloaded official schematic](reference/ESP32-P4-NANO-schematic.pdf), fetched from https://files.waveshare.com/wiki/ESP32-P4-NANO/ESP32-P4-NANO-schematic.pdf. Vendor reference PDF; not an editable KiCad design or a verified redistribution license for manufacturing copies.
- [Adafruit VEML7700 breakout #4162](https://www.adafruit.com/product/4162)
- [Adafruit LIS3DH breakout #2809](https://www.adafruit.com/product/2809)
- [Custom board quote findings](../pcb-p4/quotes/2026-09-14/quote-findings.md)

## Next engineering work

1. Establish a supported path for the IMX519's required still resolution before committing to a processor board.
2. Select and bench-test a white LED/driver combination, then measure required pulse power and diffusion.
3. Assign free GPIOs and make a wiring diagram for RGB, button, and optional sensors.
4. Measure the purchased boards and camera to design the new enclosure and snap-in diffuser.

No parts have been purchased and no fabrication order has been placed.
