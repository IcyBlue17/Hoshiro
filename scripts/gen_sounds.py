#!/usr/bin/env python3
"""Generate Hoshiro's UI sounds (stdlib only). Output: src-tauri/resources/sounds/*.wav (44.1 kHz mono 16-bit).

Soft, short, plucked tones in the spirit of macOS / Typeless interaction sounds - not beeps.
Run: python3 scripts/gen_sounds.py
"""
import math
import os
import random
import struct
import wave

SR = 44100
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src-tauri", "resources", "sounds")
PEAK = 0.10  # about -20 dBFS


def tri(phase):
    return 2.0 * abs(2.0 * (phase / (2 * math.pi) % 1.0) - 1.0) - 1.0


def pluck(buf, t0, freq, dur, tau, amp=1.0, attack=0.004, glide=0.0):
    """Soft pluck: sine + a little triangle/2nd/3rd harmonic, fast attack, exponential decay."""
    n0 = int(t0 * SR)
    n = int(dur * SR)
    phase = 0.0
    for i in range(n):
        t = i / SR
        f = freq * (1.0 + glide * math.exp(-t / 0.03))
        phase += 2 * math.pi * f / SR
        tone = (
            0.78 * math.sin(phase)
            + 0.14 * math.sin(2 * phase)
            + 0.05 * math.sin(3 * phase)
            + 0.10 * tri(phase)
        )
        env = math.exp(-t / tau)
        if t < attack:
            env *= 0.5 - 0.5 * math.cos(math.pi * t / attack)
        tail = min(1.0, (dur - t) / 0.012)  # fade the last 12 ms
        if n0 + i < len(buf):
            buf[n0 + i] += amp * tone * env * tail


def thud(buf, t0, dur, f0=150.0, f1=70.0, tau=0.035, amp=1.0):
    n0 = int(t0 * SR)
    n = int(dur * SR)
    phase = 0.0
    rng = random.Random(7)
    lp = 0.0
    for i in range(n):
        t = i / SR
        f = f1 + (f0 - f1) * math.exp(-t / 0.025)
        phase += 2 * math.pi * f / SR
        env = math.exp(-t / tau)
        if t < 0.003:
            env *= t / 0.003
        lp += 0.12 * (rng.uniform(-1, 1) - lp)  # muffled click
        tail = min(1.0, (dur - t) / 0.015)
        if n0 + i < len(buf):
            buf[n0 + i] += amp * (math.sin(phase) * 0.9 + lp * 0.5 * math.exp(-t / 0.008)) * env * tail


def finish(buf, peak=PEAK):
    m = max(abs(x) for x in buf) or 1.0
    return [x / m * peak for x in buf]


def write(name, buf):
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, name)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1.0, min(1.0, x)) * 32767)) for x in buf))
    print(f"wrote {path} ({len(buf) / SR * 1000:.0f} ms)")


def make(dur):
    return [0.0] * int(dur * SR)


def start():
    b = make(0.110)
    pluck(b, 0.000, 783.99, 0.070, 0.028, 0.8)   # G5
    pluck(b, 0.042, 1046.50, 0.068, 0.030, 1.0)  # C6
    return finish(b)


def stop():
    b = make(0.120)
    pluck(b, 0.000, 1046.50, 0.075, 0.030, 0.9)  # C6
    pluck(b, 0.048, 698.46, 0.072, 0.034, 1.0)   # F5
    return finish(b)


def lock():
    b = make(0.130)
    pluck(b, 0.000, 1318.5, 0.045, 0.012, 0.9, attack=0.002)
    pluck(b, 0.062, 1568.0, 0.066, 0.016, 1.0, attack=0.002)
    return finish(b)


def cancel():
    b = make(0.130)
    thud(b, 0.0, 0.13)
    return finish(b, 0.12)


def error():
    b = make(0.300)
    pluck(b, 0.000, 659.25, 0.150, 0.060, 0.9, attack=0.006)   # E5
    pluck(b, 0.115, 523.25, 0.185, 0.075, 1.0, attack=0.006)   # C5 (minor third down)
    return finish(b)


if __name__ == "__main__":
    for fn in (start, stop, lock, cancel, error):
        write(fn.__name__ + ".wav", fn())
