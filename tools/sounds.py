#!/usr/bin/env python3
"""Szept's sounds, made from plain maths: no recordings, no samples.

This one file is the source of every sound in both apps. It writes short
mono WAV files to crates/desktop/ui/sounds/ and to the Android raw
resources, so both apps play exactly the same thing:

    python3 tools/sounds.py

Each sound is a function of time built from a few pieces: oscillators that
glide in pitch, filtered noise, and envelopes. Only the standard library is
needed, and the output is the same on every run (the noise is seeded).
"""

import array
import math
import pathlib
import random
import wave

ROOT = pathlib.Path(__file__).resolve().parent.parent
RATE = 24_000
TAU = 2 * math.pi

# The order is the order of the settings list. The first is the default.
# (id, name in the settings, what it is)
CHOICES = [
    ("babel", "Bąbelek", "bulgocze jak bańka w lemoniadzie"),
    ("psst", "Psst!", "szepcze, jak przystało na Szept"),
    ("kropla", "Kropelka", "kapie z liścia"),
    ("cwierk", "Ćwierk", "wróbel za oknem"),
    ("kalimba", "Kalimba", "trzy ciepłe nutki"),
    ("kumkum", "Kum-kum", "żaba z naklejek rechocze"),
    ("boing", "Boing!", "sprężynka z kreskówki"),
    ("moneta", "Monetka", "jak w starej grze"),
    ("pyk", "Pyk", "korek z oranżady"),
    ("dzyn", "Dzyń", "szklany dzwoneczek"),
]

# Played by the app itself, never chosen: a message leaving, money arriving.
EXTRA = ["fiu", "kaching"]


def samples(seconds):
    return [0.0] * int(seconds * RATE)


def mix(out, sound, start=0.0, gain=1.0):
    offset = int(start * RATE)
    for index, value in enumerate(sound):
        if offset + index < len(out):
            out[offset + index] += value * gain
    return out


def tone(seconds, freq, attack=0.004, decay=None, shape="sine", partials=((1, 1),)):
    """An oscillator whose pitch `freq(t)` may glide, with a quick attack and
    an exponential decay (`decay` is the time constant in seconds)."""
    out = samples(seconds)
    phase = 0.0
    for index in range(len(out)):
        t = index / RATE
        phase += TAU * freq(t) / RATE
        value = 0.0
        for ratio, level in partials:
            p = phase * ratio
            if shape == "sine":
                value += level * math.sin(p)
            elif shape == "square":
                value += level * (1.0 if math.sin(p) >= 0 else -1.0)
            elif shape == "triangle":
                value += level * (2 / math.pi) * math.asin(math.sin(p))
        env = min(1.0, t / attack) if attack else 1.0
        if decay:
            env *= math.exp(-t / decay)
        # Fade the last 5 ms so nothing clicks.
        env *= min(1.0, (seconds - t) / 0.005)
        out[index] = value * env
    return out


def noise(seconds, seed):
    rng = random.Random(seed)
    return [rng.uniform(-1, 1) for _ in range(int(seconds * RATE))]


def bandpass(signal, centre, q):
    """A biquad band-pass whose centre `centre(t)` may move."""
    out = []
    x1 = x2 = y1 = y2 = 0.0
    for index, x in enumerate(signal):
        w = TAU * centre(index / RATE) / RATE
        alpha = math.sin(w) / (2 * q)
        b0, b2 = alpha, -alpha
        a0, a1, a2 = 1 + alpha, -2 * math.cos(w), 1 - alpha
        y = (b0 * x + b2 * x2 - a1 * y1 - a2 * y2) / a0
        x2, x1, y2, y1 = x1, x, y1, y
        out.append(y)
    return out


def lowpass(signal, cutoff):
    out = []
    x1 = x2 = y1 = y2 = 0.0
    w = TAU * cutoff / RATE
    alpha = math.sin(w) / (2 * 0.707)
    cos = math.cos(w)
    b0 = b2 = (1 - cos) / 2
    b1 = 1 - cos
    a0, a1, a2 = 1 + alpha, -2 * cos, 1 - alpha
    for x in signal:
        y = (b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) / a0
        x2, x1, y2, y1 = x1, x, y1, y
        out.append(y)
    return out


def shaped(signal, envelope):
    return [value * envelope(index / RATE) for index, value in enumerate(signal)]


def glide(start, end, seconds):
    """Exponential pitch glide from `start` to `end` Hz over `seconds`."""
    return lambda t: start * (end / start) ** min(1.0, t / seconds)


# The sounds. Each returns a list of samples.

def babel():
    """Two bubbles rising to the surface: blup-blip."""
    out = samples(0.36)
    mix(out, tone(0.16, glide(320, 980, 0.09), decay=0.05))
    mix(out, tone(0.14, glide(520, 1500, 0.07), decay=0.04), 0.12, 0.75)
    mix(out, tone(0.1, glide(900, 2100, 0.05), decay=0.025), 0.21, 0.4)
    return out


def psst():
    """A whisper: a tiny "p", a hissing "sss" and a "t"."""
    out = samples(0.5)
    mix(out, tone(0.02, lambda t: 140, attack=0.001, decay=0.006), 0.0, 0.6)
    hiss = bandpass(noise(0.34, 1), lambda t: 5200 + 900 * t, 2.2)
    hiss = shaped(hiss, lambda t: min(1.0, t / 0.05) * (1 - t / 0.34) ** 0.6)
    mix(out, hiss, 0.03, 2.6)
    tick = bandpass(noise(0.03, 2), lambda t: 3500, 1.5)
    tick = shaped(tick, lambda t: math.exp(-t / 0.006))
    mix(out, tick, 0.42, 2.2)
    return out


def kropla():
    """A drop: a quick upward plink that rings for a moment."""
    out = samples(0.32)
    mix(out, tone(0.3, glide(700, 1900, 0.035), attack=0.002, decay=0.07))
    mix(out, tone(0.2, lambda t: 1900, attack=0.002, decay=0.04), 0.035, 0.25)
    return out


def chirp():
    """One sparrow chirp: a fast sweep with a flutter."""
    return tone(
        0.075,
        lambda t: 2900 + 1300 * math.sin(math.pi * t / 0.075) + 180 * math.sin(TAU * 70 * t),
        attack=0.004,
        decay=0.05,
    )


def cwierk():
    out = samples(0.36)
    mix(out, chirp())
    mix(out, chirp(), 0.1, 0.85)
    mix(out, tone(0.13, glide(4200, 2600, 0.12), decay=0.05), 0.2, 0.6)
    return out


def pluck(freq, seconds=0.6):
    """A kalimba tine: a fundamental and its odd, quickly fading overtones."""
    return tone(seconds, lambda t: freq, attack=0.002, decay=0.18,
                partials=((1, 1.0), (5.4, 0.25), (8.9, 0.08)))


def kalimba():
    out = samples(0.75)
    for start, note in ((0.0, 659.3), (0.1, 784.0), (0.2, 1046.5)):
        mix(out, pluck(note), start, 0.7)
    return out


def croak(start_freq):
    """A frog croak: a buzzy low tone pulsing 32 times a second, muffled."""
    raw = tone(0.17, glide(start_freq, start_freq * 0.8, 0.17), attack=0.01, shape="square")
    raw = shaped(raw, lambda t: 0.55 + 0.45 * math.sin(TAU * 32 * t))
    return lowpass(raw, 1100)


def kumkum():
    out = samples(0.5)
    mix(out, croak(210), 0.0, 0.9)
    mix(out, croak(250), 0.24, 0.9)
    return out


def boing():
    """A cartoon spring: the pitch wobbles, the wobble dies down."""
    return tone(
        0.55,
        lambda t: 300 * (1 + 0.9 * t) + 140 * math.exp(-t / 0.18) * math.sin(TAU * 13 * t),
        attack=0.003,
        decay=0.2,
        partials=((1, 1.0), (2, 0.2)),
    )


def moneta():
    """The classic 8-bit coin: B5, then a long E6."""
    out = samples(0.42)
    mix(out, tone(0.07, lambda t: 987.8, attack=0.001, shape="square"), 0.0, 0.35)
    mix(out, tone(0.34, lambda t: 1318.5, attack=0.001, decay=0.12, shape="square"), 0.07, 0.35)
    return lowpass(out, 6000)


def pyk():
    """A cork popping out: a short thump with a falling squeak."""
    out = samples(0.22)
    pop = bandpass(noise(0.04, 3), lambda t: 900, 1.2)
    mix(out, shaped(pop, lambda t: math.exp(-t / 0.008)), 0.0, 3.0)
    mix(out, tone(0.14, glide(1300, 420, 0.1), attack=0.001, decay=0.04), 0.005, 0.8)
    return out


def bell(freq, seconds=0.9):
    """A small glass bell: inharmonic partials that ring and fade."""
    return tone(seconds, lambda t: freq, attack=0.001, decay=0.22,
                partials=((1, 1.0), (2.32, 0.4), (4.25, 0.2), (6.63, 0.08)))


def dzyn():
    out = samples(0.9)
    mix(out, bell(1760), 0.0, 0.7)
    mix(out, bell(2637), 0.09, 0.35)
    return out


def fiu():
    """A message flying off: air rushing past, rising."""
    whoosh = bandpass(noise(0.26, 4), glide(700, 4200, 0.24), 3.0)
    return shaped(whoosh, lambda t: math.sin(math.pi * min(1.0, t / 0.26)) ** 1.5)


def kaching():
    """Money in the till: a drawer bell and coins settling."""
    out = samples(0.85)
    mix(out, bell(2093, 0.8), 0.0, 0.6)
    mix(out, bell(3136, 0.7), 0.06, 0.45)
    rng = random.Random(5)
    for index in range(7):
        start = 0.16 + index * 0.045 + rng.uniform(0, 0.02)
        mix(out, tone(0.08, lambda t, f=rng.uniform(4200, 6500): f, attack=0.001, decay=0.015),
            start, 0.25 * (1 - index / 9))
    return out


SOUNDS = {
    "babel": babel, "psst": psst, "kropla": kropla, "cwierk": cwierk, "kalimba": kalimba,
    "kumkum": kumkum, "boing": boing, "moneta": moneta, "pyk": pyk, "dzyn": dzyn,
    "fiu": fiu, "kaching": kaching,
}


def loudness_gain(signal, target_db=-17.0, ceiling=0.9):
    """The gain that brings the sound's loudness (RMS while it sounds) to
    `target_db`, without letting any peak above `ceiling`."""
    top = max(abs(value) for value in signal) or 1.0
    active = [value for value in signal if abs(value) > top * 0.05] or signal
    rms = math.sqrt(sum(value * value for value in active) / len(active)) or 1.0
    return min(10 ** (target_db / 20) / rms, ceiling / top)


def write(path, signal):
    """16-bit mono WAV, scaled to a common loudness."""
    gain = loudness_gain(signal)
    data = array.array("h", (round(max(-1.0, min(1.0, value * gain)) * 32767) for value in signal))
    if data.itemsize != 2:
        raise SystemExit("unexpected sample size")
    if array.array("h", [1]).tobytes()[0] != 1:
        data.byteswap()
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(RATE)
        out.writeframes(data.tobytes())


def write_js():
    lines = [
        "// Generated by tools/sounds.py. Do not edit: change the script and run it again.",
        '"use strict";',
        "const SOUND_CHOICES = [",
    ]
    for id, name, about in CHOICES:
        lines.append(f'  {{ id: "{id}", name: "{name}", about: "{about}" }},')
    lines.append("];")
    (ROOT / "crates/desktop/ui/sounds-data.js").write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_kotlin():
    lines = [
        "// Generated by tools/sounds.py. Do not edit: change the script and run it again.",
        "package org.komunikator.app",
        "",
        "/** A notification sound: its id (also the raw resource's name), name and a few words about it. */",
        "data class SoundChoice(val id: String, val name: String, val about: String)",
        "",
        "val SOUND_CHOICES: List<SoundChoice> = listOf(",
    ]
    for id, name, about in CHOICES:
        lines.append(f'    SoundChoice("{id}", "{name}", "{about}"),')
    lines.append(")")
    lines.append("")
    lines.append("/** Every sound's raw resource, the chosen ones and those the app plays itself. */")
    lines.append("val SOUND_RESOURCES: Map<String, Int> = mapOf(")
    for id in SOUNDS:
        lines.append(f'    "{id}" to R.raw.szept_{id},')
    lines.append(")")
    path = ROOT / "android/app/src/main/java/org/komunikator/app/SoundsData.kt"
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    assert [id for id, _, _ in CHOICES] + EXTRA == list(SOUNDS)
    for id, make in SOUNDS.items():
        signal = make()
        # Android resource names allow only lower-case letters, digits and "_".
        write(ROOT / f"crates/desktop/ui/sounds/{id}.wav", signal)
        write(ROOT / f"android/app/src/main/res/raw/szept_{id}.wav", signal)
    write_js()
    write_kotlin()
    print(f"{len(SOUNDS)} sounds, {len(CHOICES)} to choose from")
