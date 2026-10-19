#!/usr/bin/env python3
"""Convert a single-device `libinput record` YAML file into an edgepad raw dump.

usage: libinput-record-to-ev.py <recording.yml> [<from_s>-<to_s> ...] > capture.raw.ev
then:  edgepad replay-raw capture.raw.ev --config <edgepad.toml>

Optional ranges keep only frames whose timestamp falls inside one of them.
Start each range while the pad is idle, so no contact is cut in half.
"""
import re
import sys

EVENT_TYPES = {0: "EV_SYN", 1: "EV_KEY", 3: "EV_ABS", 4: "EV_MSC"}
EVENT = re.compile(r"^\s*- \[\s*(\d+),\s*(\d+),\s*(\d+),\s*(\d+),\s*(-?\d+)\]")
ABSINFO = re.compile(r"^\s*(47|53|54): \[\s*(-?\d+),\s*(-?\d+),")


def main(path, ranges):
    axes = {}
    lines = []
    frame = []
    with open(path) as recording:
        for line in recording:
            absinfo = ABSINFO.match(line)
            if absinfo and absinfo.group(1) not in axes:
                axes[absinfo.group(1)] = f"{absinfo.group(2)}..={absinfo.group(3)}"
                continue
            event = EVENT.match(line)
            if not event:
                continue
            sec, usec, kind, code, value = (int(field) for field in event.groups())
            name = EVENT_TYPES.get(kind, f"EV_{kind}")
            if kind != 0:
                frame.append(f"{name} {code} {value}")
            elif code == 0:
                timestamp_us = sec * 1_000_000 + usec
                if not ranges or any(start <= timestamp_us < end for start, end in ranges):
                    lines.extend(frame)
                    lines.append(f"{name} {code} {value} {timestamp_us}")
                frame = []

    print(f"# converted from {path}")
    print(f"# slots: {axes['47']}")
    print(f"# x: {axes['53']}")
    print(f"# y: {axes['54']}")
    print("\n".join(lines))


def parse_range(text):
    start, end = text.split("-")
    return round(float(start) * 1_000_000), round(float(end) * 1_000_000)


if __name__ == "__main__":
    main(sys.argv[1], [parse_range(text) for text in sys.argv[2:]])
