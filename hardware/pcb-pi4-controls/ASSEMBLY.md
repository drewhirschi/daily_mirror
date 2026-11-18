# Control board: build, cable and test

For the Pi 4 case control board, rev 0.4 (draft). This guide has not been
tried on real hardware yet. Board layout: [README.md](README.md). Case:
[`../enclosure-pi4`](../enclosure-pi4).

## Pinout

One straight six-wire ribbon. It has a 1x6 housing at each end and no crossed
wires, so J1 pin n goes to Pi pin 2n-1.

| J1 | Net | Pi physical pin | BCM | Board part |
| --- | --- | --- | --- | --- |
| 1 | 3V3 | 1 | 3.3 V | D1 long leg (+) |
| 2 | BTN | 3 | GPIO 2 | SW1 |
| 3 | LED_B | 5 | GPIO 3 | R1 330 -> D1 blue |
| 4 | LED_R | 7 | GPIO 4 | R3 330 -> D1 red |
| 5 | GND | 9 | GND | SW1 |
| 6 | LED_G | 11 | GPIO 17 | R2 330 -> D1 green |

A LOW pin turns its colour on. GPIO 2 and 3 have 1.8 k pull-ups on the Pi,
so the button and the blue channel rest HIGH (blue off). **I2C must stay off
on this Pi** because GPIO 2/3 are the I2C pins, and **1-Wire must stay off**
because GPIO 4 is its default pin. To check on the Pi:

```sh
sudo raspi-config nonint get_i2c          # prints 1 when I2C is disabled (0 = enabled)
grep -nE 'i2c_arm|w1-gpio' /boot/firmware/config.txt   # no active dtparam=i2c_arm=on, no dtoverlay=w1-gpio
ls /dev/i2c-1                             # should not exist
```

## Parts

| Ref | Part | Qty |
| --- | --- | --- |
| PCB | this board, 30 x 27.1 x 1.6 mm | 1 |
| SW1 | Omron B3F-4055 12 mm tactile switch | 1 |
| D1 | EBOOT 5 mm clear RGB LED, common anode | 1 |
| R1-R3 | 330 ohm 1/4 W axial resistor | 3 |
| J1 | 1x6 2.54 mm straight male header | 1 |
| Cable | 6 female-female Dupont jumpers of the same length (a strip peeled off a rainbow ribbon is ideal) and 2 empty 1x6 Dupont housings | 1 |
| Screws | M2 x 6 thread-forming | 4 |

The board's two sides:

- **Front ("LID SIDE")** carries the switch and the LED.
- **Back** carries J1 and the resistors, and has the pin table printed on it.

## Soldering order

1. **R1-R3** go on the back, lying flat. Solder them from the front and clip
   the leads flush, because the switch body sits 1.15 mm from those joints.
   Their values are all the same, so it does not matter which goes where.
2. **J1** goes on the back with its long pins pointing away from the board.
   Solder one pin, check that the header is square, then solder the rest from
   the front. Clip the tails short.
3. **SW1** goes on the front. Its two pegs drop into the larger 2.0 mm
   holes. Press it until the four legs snap into their 1.1 mm holes and the
   body sits flat on the board. Then solder all four terminals from the back.
4. **D1 goes in last. Set its height in the lid:**
   - **Orientation:** the long leg (+) goes in the **square pad**. The red leg
     is the outer leg next to the long one, and it goes at the end marked
     **R**, toward the Pi. From the R end the order is R, +, G, B.
   - **Height:** push the leads through the board, but do not solder yet.
   - Drop the button cap into the lid. Screw the board onto the four lid posts
     with the four M2 x 6 screws.
   - Push the LED until its dome is flush with the lid's front face. A ruler
     laid across the face helps.
   - Solder one lead, check the dome again, then solder the other three and
     clip the leads.
   - The flange should end up about 2.9 mm above the board.

## The cable

1. Take six jumpers that sit side by side in the ribbon. Pull their single
   housings off by lifting each plastic latch with a pin and sliding the crimp
   out.
2. At **one end**, push the six crimps into a 1x6 housing in ribbon order.
   The latch window faces up, and you should hear each crimp click.
3. At the **other end**, do the same in the **same colour order**. Lay the
   ribbon flat without twisting it, and the two housings should look
   identical.
4. **Mark pin 1** with a paint pen or marker dot on the same edge wire at both
   housings.
5. Fill in the table below and keep it with the case.

| Pin (both ends) | Net | Wire colour |
| --- | --- | --- |
| 1 (marked) | 3V3 | |
| 2 | BTN | |
| 3 | LED_B | |
| 4 | LED_R | |
| 5 | GND | |
| 6 | LED_G | |

**On the board:** the pin-1 mark goes to J1 pin 1. That is the square pad
labelled "1", the end of J1 nearest the Pi.

**On the Pi:**

- The plug goes on the header's **inner row**, the one nearer the middle of
  the board. The outer row, along the board edge, starts with two 5 V pins.
  **Never use that row.**
- It sits at the **end nearest the microSD card**. Pin 1 is that corner pin
  in the inner row.
- The plug covers pins 1, 3, 5, 7, 9 and 11. Its far end stops beside pin 12,
  and everything from pin 13 on stays free.

### If it is on wrong

The board has no protection, so check the plug before powering the Pi. Here
is what each mistake does:

| Mistake | What happens | Risk |
| --- | --- | --- |
| Rotated end for end, still on pins 1-11 | 3.3 V lands on a resistor and GND on the button. No supply-to-ground short. The LED stays dark or shows odd colours, and all LED current goes through a 330 ohm resistor. | Harmless. Unplug and turn it round. |
| One pin along the column, on pins 3-13 | The board's GND net goes to GPIO 17 and BTN to GPIO 3. **Pressing the button then ties two Pi outputs together.** Red or green may glow faintly at idle. | **Can damage a GPIO.** Do not press the button. Unplug. |
| On the outer row | The board gets **5 V**. **The button would put 5 V on GPIO 15.** Blue lights as soon as the Pi powers up. | **Can damage the Pi.** Power off at once. |

The meter checks below catch all three before power is applied.

## Checks with a meter, before power

Do these with the Pi **off and unplugged**.

**Board alone (cable not connected):**

| Measure | Expect |
| --- | --- |
| J1-1 to J1-5 (3V3 to GND) | open (OL) |
| Each neighbouring pair: 1-2, 2-3, 3-4, 4-5, 5-6 | no short (no 0 ohm). The diode range may show a reading on 1-3, 1-4 and 1-6 through the LED; see below |
| J1-1 to D1 square pad | 0 ohm |
| J1-3 to D1 B pad / J1-4 to D1 R pad / J1-6 to D1 G pad | 330 ohm each |
| J1-2 to J1-5 | open at rest, about 0 ohm while you hold the button |

**LED, using the meter's diode test:**

- Put the **red probe on J1-1** and the black probe on J1-4 (red), then J1-3
  (blue), then J1-6 (green).
- Red should read about 1.7-2.0 V and may glow faintly.
- Green and blue need about 2.6-3.2 V. Many meters cannot reach that, so they
  may show OL or not light. That is not a fault.
- The 330 ohm resistors in series barely change the reading.
- With the probes swapped, all three must read OL.

**Cable, after plugging both ends in** (Pi still off):

- J1-5 to the Pi's USB-A or Ethernet metal shell (ground) should read 0 ohm.
- J1-1 to Pi pin 17 (the other 3.3 V pin) should read 0 ohm.
- If either check fails, the plug is shifted or on the wrong row.

## Powered test

The test script `board_test.py` (in this folder) lights each colour, then
reports the button. It has only been syntax-checked and run against
gpiozero's mock pins, not on a real Pi.

1. Copy it to the Pi. For example:
   `scp hardware/pcb-pi4-controls/board_test.py drew@<pi>.local:`
2. **Stop the service so the pins are free**, then run the test:

   ```sh
   sudo systemctl stop daily-mirror-device.service
   python3 board_test.py          # defaults: red 4, green 17, blue 3, button 2
   ```

3. Expect **red, green, blue**, one at a time, twice.
4. After that it prints `button PRESSED` and `button released` as you press.
   Press Ctrl-C to finish.
5. Then restart the service:

   ```sh
   sudo systemctl start daily-mirror-device.service
   ```

If the script says the pins are busy, the service is still running.

| Symptom | Likely cause |
| --- | --- |
| Nothing lights at all | LED in backwards (long leg not on the square pad), or the plug is on wrong |
| Colours in the wrong order | LED turned end for end (R end away from the Pi) or the cable is shifted |
| One colour missing | That colour's joint, its resistor, or the meter checks above |
| Button reports PRESSED when not pressed | Short between BTN and GND (J1-2 and J1-5), or I2C is enabled |

The service has no hardware self-test of its own. After the next step, its
admin page has LED test buttons for green, yellow (red + green) and red.

## Final step: service settings

In `/home/drew/daily-mirror-device/.env` on this Pi, replace the pin and
LED-mode lines with these. The same lines are in `device/.env.example` as a
commented preset.

```sh
DAILY_MIRROR_BUTTON_PIN=2
DAILY_MIRROR_GREEN_LED_PIN=17
DAILY_MIRROR_YELLOW_LED_PIN=3
DAILY_MIRROR_RED_LED_PIN=4
DAILY_MIRROR_LED_MODE=rgb-common-anode
```

`YELLOW_LED_PIN` is the physical **blue** channel in RGB mode. Then:

```sh
sudo systemctl restart daily-mirror-device.service
journalctl -u daily-mirror-device.service -n 20 --no-pager   # "ready: button BCM 2, LEDs green/yellow/red BCM 17/3/4"
```

In normal use:

- The LED is **off** while idle.
- After a press it gives **three yellow pulses** (red + green) as a
  countdown. It then stays yellow until the photo is taken, so hold still.
- It flashes **green twice** when the photo is saved or uploaded.
- It shows **steady red** after an error, or while a photo waits to upload.
- Blue is not used yet.
