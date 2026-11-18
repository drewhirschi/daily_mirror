#!/usr/bin/env python3
"""Bench test for the Pi 4 case control board. Run ON THE PI, with the
daily-mirror-device service stopped so the pins are free:

    sudo systemctl stop daily-mirror-device.service
    python3 board_test.py                # this board's pins
    python3 board_test.py --red 22 --green 17 --blue 27 --button 2   # other wiring
    sudo systemctl start daily-mirror-device.service

Lights red, green, blue one at a time (common anode: a pin LOW turns its colour
on), then prints button presses and releases until Ctrl-C.

Pins are BCM numbers. Defaults come from the same environment variables the
device service reads (DAILY_MIRROR_RED_LED_PIN, ..._GREEN_LED_PIN,
..._YELLOW_LED_PIN for blue, ..._BUTTON_PIN), and fall back to this board's
wiring: red 4, green 17, blue 3, button 2. Needs python3-gpiozero (installed on
Raspberry Pi OS). Untested on hardware.
"""
import argparse
import os
import signal
import sys
import time


def env_pin(name, default):
    value = os.environ.get(name)
    return int(value) if value else default


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--red", type=int, default=env_pin("DAILY_MIRROR_RED_LED_PIN", 4))
    ap.add_argument("--green", type=int, default=env_pin("DAILY_MIRROR_GREEN_LED_PIN", 17))
    ap.add_argument("--blue", type=int, default=env_pin("DAILY_MIRROR_YELLOW_LED_PIN", 3),
                    help="the service calls this the YELLOW pin in rgb-common-anode mode")
    ap.add_argument("--button", type=int, default=env_pin("DAILY_MIRROR_BUTTON_PIN", 2))
    ap.add_argument("--seconds", type=float, default=1.5, help="how long each colour stays on")
    ap.add_argument("--rounds", type=int, default=2, help="how many times to cycle R, G, B")
    args = ap.parse_args()

    try:
        from gpiozero import LED, Button
    except ImportError:
        sys.exit("gpiozero is missing: sudo apt install python3-gpiozero")

    print(f"red BCM {args.red}, green BCM {args.green}, blue BCM {args.blue}, button BCM {args.button}")
    try:
        # active_high=False: on() drives the pin LOW, which lights a common-anode channel.
        leds = {name: LED(pin, active_high=False, initial_value=False)
                for name, pin in (("red", args.red), ("green", args.green), ("blue", args.blue))}
        button = Button(args.button, pull_up=True, bounce_time=0.03)
    except Exception as exc:  # pin busy (service still running) or not a Pi
        sys.exit(f"could not open the pins: {exc}\nIs daily-mirror-device.service stopped?")

    for n in range(args.rounds):
        for name, led in leds.items():
            print(f"  {name} ON  (BCM {led.pin.number} LOW)")
            led.on()
            time.sleep(args.seconds)
            led.off()
    print("all off. If a colour was wrong or missing, see ASSEMBLY.md 'Powered test'.")

    presses = [0]

    def pressed():
        presses[0] += 1
        print(f"  button PRESSED  ({presses[0]})", flush=True)

    button.when_pressed = pressed
    button.when_released = lambda: print("  button released", flush=True)
    print("Press the button a few times; Ctrl-C to finish.")
    print(f"  button is now {'PRESSED (check for a short BTN-GND)' if button.is_pressed else 'released'}")
    try:
        signal.pause()
    except KeyboardInterrupt:
        pass
    for led in leds.values():
        led.off()
    print(f"\n{presses[0]} press(es) seen. Done; restart the service.")


if __name__ == "__main__":
    main()
