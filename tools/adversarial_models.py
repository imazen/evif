#!/usr/bin/env python3
"""Fetch and authenticate the two publicly served bakes; never fetch a dataset."""
import argparse
import hashlib
from pathlib import Path
from urllib.request import urlopen

REV = "aad4c311d850defb8d3224bb2f963779bfdd1123"
PINS = {"general": "e460a214c1e0c4f48bc7e3bb4705df548f9f172807cc63f39ba17cb1d94e05a0",
        "codec": "f7ecd60299482210fe33586e8d4674fe9ba163f17f2dbaf54c6e99d5ef44af63"}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("directory", type=Path)
    a = p.parse_args()
    a.directory.mkdir(parents=True, exist_ok=True)
    for profile, expected in PINS.items():
        name = f"rev7_{profile}_h128_2026-10-27.bin"
        dest = a.directory / name
        if dest.exists():
            payload = dest.read_bytes()
        else:
            url = f"https://raw.githubusercontent.com/imazen/zensim/{REV}/zensim/weights/{name}"
            with urlopen(url, timeout=60) as response:
                payload = response.read(1_000_000)
        if hashlib.sha256(payload).hexdigest() != expected:
            raise ValueError(f"model hash mismatch: {profile}")
        if not dest.exists():
            with dest.open("xb") as f:
                f.write(payload)
        print(profile, expected)


if __name__ == "__main__":
    main()
