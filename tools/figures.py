#!/usr/bin/env python3
"""Szeptusie: the cast of Szept's reactions and animated stickers.

Each figure is drawn on a 64 x 64 grid from a few simple shapes, grouped in
parts that move on their own. This one file is the source; it writes the
data for both apps, so they always draw the same thing:

    python3 tools/figures.py

writes crates/desktop/ui/figures-data.js and
android/app/src/main/java/org/komunikator/app/FiguresData.kt.

Shapes:
    ("c", cx, cy, r, fill, stroke, width)            circle
    ("e", cx, cy, rx, ry, fill, stroke, width)       ellipse
    ("r", x, y, w, h, radius, fill, stroke, width)   rounded rectangle
    ("p", "svg path", fill, stroke, width)           path ("pe" for even-odd fill)

Animations (kind, period in ms, amount, phase 0..1), about the part's origin:
    bob    up and down by `amount`          sway   rocks by `amount` degrees
    spin   full turns                       pulse  grows by `amount` (0.1 = 10 %)
    blink  closes now and then              rise   drifts up by `amount` and fades
    shake  trembles sideways by `amount`    hop    jumps up by `amount`, then rests
A negative `rise` falls instead.
"""

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
INK = "#2b2233"
WHITE = "#ffffff"
BLUSH = "#ff8fa3"


def part(shapes, anim=None, origin=(32, 32), rotate=0):
    return {"origin": list(origin), "rotate": rotate, "anim": list(anim) if anim else None, "shapes": shapes}


def eyes(left, right, r=2.2, period=3800, phase=0.0):
    """Two dark eyes with a glint, blinking now and then."""
    (lx, ly), (rx, ry) = left, right
    return part(
        [
            ("c", lx, ly, r, INK, None, 0),
            ("c", rx, ry, r, INK, None, 0),
            ("c", lx + r * 0.35, ly - r * 0.35, r * 0.35, WHITE, None, 0),
            ("c", rx + r * 0.35, ry - r * 0.35, r * 0.35, WHITE, None, 0),
        ],
        ("blink", period, 0, phase),
        origin=((lx + rx) / 2, ly),
    )


def line(d, color=INK, width=1.8):
    return ("p", d, None, color, width)


def blush(left, right):
    return [("e", left[0], left[1], 3, 1.8, BLUSH, None, 0), ("e", right[0], right[1], 3, 1.8, BLUSH, None, 0)]


FIGURES = []


def figure(id, name, caption, parts):
    FIGURES.append({"id": id, "name": name, "caption": caption, "parts": parts})


# Pierożek: a dumpling in love, with a heart drifting up.
figure("pierog", "Pierożek miłości", "Lepię się do Ciebie", [
    part([
        ("c", 16, 31, 3.6, "#f6e2b3", "#c9a15a", 1.4),
        ("c", 22.5, 24.5, 3.6, "#f6e2b3", "#c9a15a", 1.4),
        ("c", 32, 21.5, 3.6, "#f6e2b3", "#c9a15a", 1.4),
        ("c", 41.5, 24.5, 3.6, "#f6e2b3", "#c9a15a", 1.4),
        ("c", 48, 31, 3.6, "#f6e2b3", "#c9a15a", 1.4),
        ("p", "M9 41 Q32 9 55 41 Q32 50 9 41 Z", "#f6e2b3", "#c9a15a", 1.6),
        *blush((22, 40), (42, 40)),
        line("M28.5 40 Q32 43.5 35.5 40"),
    ], ("bob", 1600, 2, 0), origin=(32, 44)),
    eyes((26, 35.5), (38, 35.5), period=3600),
    part([("p", "M48 20 C41 15 43 8.5 48 12 C53 8.5 55 15 48 20 Z", "#ff4d6d", "#c9184a", 1)],
         ("rise", 2200, 9, 0), origin=(48, 15)),
])

# Ziemniak: a potato in shock, trembling.
figure("ziemniak", "Ziemniak w szoku", "Jestem w szoku (i w mundurku)", [
    part([
        ("p", "M13 35 C11 19 25 10 36 12 C50 14 55 26 53 38 C51 52 35 57 24 53 C16 50 14 43 13 35 Z",
         "#c99a5b", "#8a6233", 1.6),
        ("c", 20, 24, 1.5, "#a87a42", None, 0),
        ("c", 46, 46, 1.3, "#a87a42", None, 0),
        ("c", 44, 19, 1.1, "#a87a42", None, 0),
        ("c", 26, 30, 5.2, WHITE, INK, 1.2),
        ("c", 40, 30, 5.2, WHITE, INK, 1.2),
        ("c", 26, 30.5, 2, INK, None, 0),
        ("c", 40, 30.5, 2, INK, None, 0),
        line("M21 21.5 Q25.5 18 30 21"),
        line("M36 21 Q40.5 18 45 21.5"),
        ("e", 33, 44, 3.2, 4.2, "#4a1d2a", None, 0),
    ], ("shake", 500, 1.2, 0)),
    part([("p", "M52 14 Q49 19 52 21 Q55 19 52 14 Z", "#7fd3ff", "#3d9bd1", 0.8)], ("rise", 1600, -6, 0.2),
         origin=(52, 18)),
])

# Ogórek: a pickle rocking with laughter.
figure("ogorek", "Kiszę się ze śmiechu", "Kiszę się ze śmiechu", [
    part([
        ("e", 32, 34, 14, 24, "#5aa83e", "#3d7a29", 1.6),
        ("c", 23, 18, 1.6, "#7cc35c", None, 0),
        ("c", 42, 24, 1.6, "#7cc35c", None, 0),
        ("c", 22, 46, 1.6, "#7cc35c", None, 0),
        ("c", 41, 50, 1.6, "#7cc35c", None, 0),
        line("M24.5 28 Q27 24.5 29.5 28", width=2.2),
        line("M34.5 28 Q37 24.5 39.5 28", width=2.2),
        ("p", "M24 34 Q32 47 40 34 Z", "#7a2034", INK, 1.2),
        ("e", 32, 40, 3.5, 2, "#ff8fa3", None, 0),
        ("c", 21.5, 31, 1.7, "#7fd3ff", None, 0),
        ("c", 42.5, 31, 1.7, "#7fd3ff", None, 0),
    ], ("sway", 700, 12, 0), origin=(32, 56), rotate=-12),
])

# Mózg: a brain boiling over, steam puffing out.
figure("mozg", "Mózg paruje", "Mózg się gotuje", [
    part([("c", 22, 15, 4, "#d9dde8", None, 0)], ("rise", 1500, 9, 0), origin=(22, 15)),
    part([("c", 32, 11, 4.5, "#d9dde8", None, 0)], ("rise", 1500, 9, 0.33), origin=(32, 11)),
    part([("c", 42, 15, 4, "#d9dde8", None, 0)], ("rise", 1500, 9, 0.66), origin=(42, 15)),
    part([
        ("c", 23, 35, 10.5, "#ff9ec4", "#d9608f", 1.5),
        ("c", 33, 28, 11.5, "#ff9ec4", "#d9608f", 1.5),
        ("c", 42, 36, 10.5, "#ff9ec4", "#d9608f", 1.5),
        ("c", 32, 41, 10.5, "#ff9ec4", "#d9608f", 1.5),
        ("c", 32, 35, 9, "#ff9ec4", None, 0),
        line("M33 20 Q29 30 33 38", "#d9608f", 1.4),
        line("M20 31 Q24 29 26 32", "#d9608f", 1.2),
        line("M40 30 Q43 33 46 31", "#d9608f", 1.2),
        ("c", 27, 36, 2.6, WHITE, INK, 1),
        ("c", 38, 36, 2.6, WHITE, INK, 1),
        ("c", 27.6, 36.3, 1.1, INK, None, 0),
        ("c", 37.4, 35.7, 1.1, INK, None, 0),
        line("M27 45 Q29.5 43 32 45 Q34.5 47 37 45", INK, 1.6),
    ], ("pulse", 900, 0.05, 0), origin=(32, 50)),
])

# Rakieta: a rocket ready to go, flame flickering.
figure("rakieta", "Odpalamy!", "Odpalamy!", [
    part([
        ("p", "M25.5 43 Q32 62 38.5 43 Z", "#ffb347", None, 0),
        ("p", "M28.5 43 Q32 55 35.5 43 Z", "#ff5a2a", None, 0),
    ], ("pulse", 260, 0.18, 0), origin=(32, 43)),
    part([
        ("p", "M22 34 L13 47 L23 44 Z", "#ff5a5f", "#4b5a8a", 1.3),
        ("p", "M42 34 L51 47 L41 44 Z", "#ff5a5f", "#4b5a8a", 1.3),
        ("p", "M32 5 C43 13 45 28 42.5 44 L21.5 44 C19 28 21 13 32 5 Z", "#f1f3f8", "#4b5a8a", 1.6),
        ("p", "M25 12 C28 8.5 36 8.5 39 12 C36 10.5 28 10.5 25 12 Z", "#ff5a5f", None, 0),
        ("c", 32, 24, 5.5, "#7fd3ff", "#4b5a8a", 1.5),
        ("c", 30, 23.5, 1.1, INK, None, 0),
        ("c", 34, 23.5, 1.1, INK, None, 0),
        line("M30.3 26 Q32 27.5 33.7 26", INK, 1),
        ("r", 28, 36, 8, 3, 1.5, "#c9d3ea", None, 0),
    ], ("bob", 1200, 3, 0)),
])

# Chmurka: a little raincloud feeling down.
figure("chmurka", "Chmurka smutku", "Mam deszczowy dzień", [
    part([("p", "M22 44 Q19.5 49 22 51 Q24.5 49 22 44 Z", "#5aa9ff", None, 0)], ("rise", 1100, -12, 0),
         origin=(22, 47)),
    part([("p", "M32 46 Q29.5 51 32 53 Q34.5 51 32 46 Z", "#5aa9ff", None, 0)], ("rise", 1100, -12, 0.4),
         origin=(32, 49)),
    part([("p", "M42 44 Q39.5 49 42 51 Q44.5 49 42 44 Z", "#5aa9ff", None, 0)], ("rise", 1100, -12, 0.7),
         origin=(42, 47)),
    part([
        ("c", 21, 31, 9, "#b8c2d6", "#7f8aa3", 1.5),
        ("c", 32, 24, 11.5, "#b8c2d6", "#7f8aa3", 1.5),
        ("c", 44, 31, 9, "#b8c2d6", "#7f8aa3", 1.5),
        ("r", 13, 29, 39, 13, 6.5, "#b8c2d6", "#7f8aa3", 1.5),
        ("r", 14.5, 27, 36, 13.5, 6, "#b8c2d6", None, 0),
        line("M23.5 28.5 L29 26.5", INK, 1.5),
        line("M41 28.5 L35.5 26.5", INK, 1.5),
        line("M28 37 Q32 33.5 36 37", INK, 1.6),
    ], ("bob", 2400, 1.5, 0)),
    eyes((27.5, 31), (37, 31), r=1.8, period=4200),
])

# Kaktus: a cactus going in for a hug, flower on top.
figure("kaktus", "Kaktusowy przytulas", "Przytulas (ostrożnie)", [
    part([
        ("r", 12, 30, 14, 6, 3, "#4caf63", "#2e7d47", 1.3),
        ("r", 12, 20, 6, 15, 3, "#4caf63", "#2e7d47", 1.3),
        ("r", 13.3, 31.3, 12, 3.4, 1.7, "#4caf63", None, 0),
    ], ("sway", 900, 14, 0), origin=(25, 33)),
    part([
        ("r", 38, 30, 14, 6, 3, "#4caf63", "#2e7d47", 1.3),
        ("r", 46, 20, 6, 15, 3, "#4caf63", "#2e7d47", 1.3),
        ("r", 38.7, 31.3, 12, 3.4, 1.7, "#4caf63", None, 0),
    ], ("sway", 900, -14, 0), origin=(39, 33)),
    part([
        ("r", 23, 13, 18, 36, 9, "#4caf63", "#2e7d47", 1.5),
        line("M27 18 L26 16", "#2e7d47", 1), line("M37 20 L38.5 18.5", "#2e7d47", 1),
        line("M26 42 L24.5 43", "#2e7d47", 1), line("M38 44 L39.5 45", "#2e7d47", 1),
        *blush((26.5, 34), (37.5, 34)),
        line("M29.5 34 Q32 36.5 34.5 34"),
        ("p", "M20 48 L44 48 L41 60 L23 60 Z", "#d9773b", "#a4532a", 1.3),
        ("r", 18, 45, 28, 5, 2, "#e88c50", "#a4532a", 1.3),
        ("c", 29.5, 10.5, 3, "#ff8fb1", None, 0),
        ("c", 34.5, 10.5, 3, "#ff8fb1", None, 0),
        ("c", 32, 8, 3, "#ff8fb1", None, 0),
        ("c", 32, 10, 1.6, "#ffd166", None, 0),
    ]),
    eyes((28, 29), (36, 29), r=1.8),
])

# Ślimak: a snail promising to sort it out. Soon.
figure("slimak", "Ślimak: zaraz to ogarnę", "Zaraz to ogarnę", [
    part([
        line("M47 42 L45 29", "#7da83f", 2),
        line("M52 42 L55 29", "#7da83f", 2),
        ("c", 45, 28, 2.8, WHITE, "#7da83f", 1),
        ("c", 55, 28, 2.8, WHITE, "#7da83f", 1),
        ("c", 45.5, 28.3, 1.2, INK, None, 0),
        ("c", 55.5, 28.3, 1.2, INK, None, 0),
    ], ("sway", 1800, 7, 0), origin=(50, 42)),
    part([
        ("p", "M7 51 Q9 42 20 42 L47 42 Q56 42 57 51 Z", "#c5e384", "#7da83f", 1.5),
        line("M48 47 Q51 49.5 54 47", INK, 1.4),
    ]),
    part([
        ("c", 29, 33, 13.5, "#f2a65a", "#b86b2b", 1.6),
        ("p", "M29 33 a3 3 0 1 1 3 3 a6 6 0 1 1 -6 -6 a9.5 9.5 0 1 1 9.5 9.5", None, "#b86b2b", 1.5),
    ], ("bob", 2400, 1.2, 0), origin=(29, 46)),
])

# Kapcie: the slippers that flew off, with a sparkle.
figure("kapec", "Kapcie spadły", "Kapcie mi spadły", [
    part([
        ("e", 21, 47, 11, 5.5, "#ff7aa8", "#c2457a", 1.5),
        ("p", "M12 45 Q16 36 26 39 Q30 41 30 45 Z", "#ffb3cf", "#c2457a", 1.3),
        ("c", 21, 39.5, 3.2, WHITE, "#e0d6e8", 1),
    ], ("hop", 1000, 7, 0), origin=(21, 52)),
    part([
        ("e", 43, 39, 11, 5.5, "#ff7aa8", "#c2457a", 1.5),
        ("p", "M34 37 Q38 28 48 31 Q52 33 52 37 Z", "#ffb3cf", "#c2457a", 1.3),
        ("c", 43, 31.5, 3.2, WHITE, "#e0d6e8", 1),
    ], ("hop", 1000, 7, 0.5), origin=(43, 44)),
    part([("p", "M50 8 L52 13 L57 15 L52 17 L50 22 L48 17 L43 15 L48 13 Z", "#ffd166", "#e0a800", 1)],
         ("spin", 2400, 0, 0), origin=(50, 15)),
    part([("p", "M13 12 L14.2 15 L17 16 L14.2 17 L13 20 L11.8 17 L9 16 L11.8 15 Z", "#ffd166", None, 0)],
         ("pulse", 900, 0.4, 0), origin=(13, 16)),
])

# Kot: a cat peeking out of a cardboard box. Reading quietly.
figure("kot", "Kot w kartonie: czytam po cichu", "Mnie tu nie ma", [
    part([
        ("p", "M21 30 L21 15 L30 24 Z", "#6e6e7a", "#4a4a55", 1.3),
        ("p", "M43 30 L43 15 L34 24 Z", "#6e6e7a", "#4a4a55", 1.3),
        ("p", "M23 26 L23 19 L27 24 Z", BLUSH, None, 0),
        ("p", "M41 26 L41 19 L37 24 Z", BLUSH, None, 0),
        ("c", 32, 34, 12.5, "#6e6e7a", "#4a4a55", 1.4),
        ("e", 27, 31, 2.6, 3.2, "#ffd166", None, 0),
        ("e", 37, 31, 2.6, 3.2, "#ffd166", None, 0),
        ("r", 26.3, 29, 1.4, 4.2, 0.7, INK, None, 0),
        ("r", 36.3, 29, 1.4, 4.2, 0.7, INK, None, 0),
        ("p", "M30.5 35.5 L33.5 35.5 L32 37 Z", BLUSH, None, 0),
    ], ("bob", 2000, 3, 0), origin=(32, 40)),
    part([
        ("p", "M10 36 L4 27 L22 29 L23 36 Z", "#e8b67d", "#9c6b35", 1.3),
        ("p", "M54 36 L60 27 L42 29 L41 36 Z", "#e8b67d", "#9c6b35", 1.3),
        ("r", 10, 36, 44, 23, 2, "#d9a066", "#9c6b35", 1.5),
        line("M17 44 L27 44", "#9c6b35", 1.2),
        line("M37 50 L47 50", "#9c6b35", 1.2),
    ]),
])

# Herbatka: a mug of tea, calm and steaming.
figure("herbatka", "Spokojnie, herbatka", "Spokojnie, herbatka", [
    part([line("M21 21 Q18 17 21 13 Q24 9 21 5", "#c9cfdc", 2)], ("rise", 2000, 6, 0), origin=(21, 13)),
    part([line("M29 21 Q26 17 29 13 Q32 9 29 5", "#c9cfdc", 2)], ("rise", 2000, 6, 0.5), origin=(29, 13)),
    part([line("M37 21 Q34 17 37 13 Q40 9 37 5", "#c9cfdc", 2)], ("rise", 2000, 6, 0.25), origin=(37, 13)),
    part([
        ("p", "M43 31 Q55 30 54 40 Q53 49 43 48", None, "#3f6f9e", 3.2),
        ("r", 13, 25, 31, 32, 7, "#7fb7e8", "#3f6f9e", 1.6),
        ("e", 28.5, 26.5, 13.5, 3, "#a0522d", "#3f6f9e", 1.2),
        line("M20 39 Q22.5 41.5 25 39"),
        line("M32 39 Q34.5 41.5 37 39"),
        *blush((19.5, 44), (37.5, 44)),
        line("M26 45 Q28.5 47.5 31 45"),
    ], ("sway", 3000, 3, 0), origin=(28, 57)),
])

# Tost: a slice of toast raising a toast.
figure("tost", "Wznoszę tost!", "Wznoszę tost!", [
    part([
        ("p", "M14 27 C9 14 22 7 32 12 C42 7 55 14 50 27 L48 55 L16 55 Z", "#e2a75a", "#a8702c", 1.6),
        ("p", "M18 28 C14 18 24 13 32 17 C40 13 50 18 46 28 L44.5 51 L19.5 51 Z", "#fbe3b4", None, 0),
        *blush((23, 40), (41, 40)),
        ("p", "M26 40 Q32 48 38 40 Z", "#7a2034", INK, 1.2),
    ], ("sway", 1200, 6, 0), origin=(32, 55)),
    eyes((26, 33), (38, 33), period=3000, phase=0.3),
    part([("p", "M52 6 L53.5 10.5 L58 12 L53.5 13.5 L52 18 L50.5 13.5 L46 12 L50.5 10.5 Z", "#ffd166", "#e0a800", 1)],
         ("spin", 2000, 0, 0), origin=(52, 12)),
    part([("p", "M10 8 L11 11 L14 12 L11 13 L10 16 L9 13 L6 12 L9 11 Z", "#ffd166", None, 0)],
         ("pulse", 800, 0.4, 0.5), origin=(10, 12)),
])

# Jajo: an egg in full panic.
figure("jajo", "Jajo w panice", "Jajo w panice", [
    part([
        ("e", 32, 35, 16, 21, "#fff7e8", "#d8c7a6", 1.6),
        ("e", 26, 21, 3, 5, WHITE, None, 0),
        ("c", 26.5, 32, 4.6, WHITE, INK, 1.2),
        ("c", 38.5, 32, 4.6, WHITE, INK, 1.2),
        ("c", 27.5, 33, 1.6, INK, None, 0),
        ("c", 37.5, 31, 1.6, INK, None, 0),
        line("M25.5 45 L28.3 42.5 L31 45 L33.7 42.5 L36.5 45 L39 42.5", INK, 1.6),
    ], ("shake", 300, 1.6, 0), origin=(32, 56)),
    part([("p", "M50 12 Q46.5 18 50 20.5 Q53.5 18 50 12 Z", "#7fd3ff", "#3d9bd1", 0.8)], ("rise", 1300, -8, 0),
         origin=(50, 16)),
    part([("p", "M13 20 Q10 25 13 27 Q16 25 13 20 Z", "#7fd3ff", "#3d9bd1", 0.8)], ("rise", 1300, -8, 0.5),
         origin=(13, 24)),
])

# Dino: a little dinosaur that has had enough of Mondays.
figure("dino", "Rawr, poniedziałek", "Rawr, poniedziałek", [
    part([
        ("p", "M18 30 L21 22 L25 28 Z", "#3f9a52", None, 0),
        ("p", "M24 24 L28 15 L32 22 Z", "#3f9a52", None, 0),
        ("p", "M31 21 L36 13 L39 20 Z", "#3f9a52", None, 0),
        ("p", "M13 58 L13 36 Q13 19 32 19 Q52 19 52 32 L52 37 Q52 44 45 44 L31 44 L31 58 Z",
         "#7bd389", "#3f9a52", 1.6),
        ("c", 41, 28, 3.4, WHITE, INK, 1),
        ("c", 42, 28.5, 1.5, INK, None, 0),
        line("M37 23.5 L44 25.5", INK, 1.6),
        ("p", "M52 37 L37 37 L37 41 L45 44 Z", "#7a2034", None, 0),
        ("p", "M39 37 L40.5 39.5 L42 37 Z", WHITE, None, 0),
        ("p", "M44 37 L45.5 39.5 L47 37 Z", WHITE, None, 0),
        ("c", 49, 31, 0.9, "#3f9a52", None, 0),
        ("c", 20, 44, 2, "#a8e6b2", None, 0),
        ("c", 24, 50, 1.6, "#a8e6b2", None, 0),
    ], ("sway", 1400, 5, 0), origin=(22, 58)),
    part([line("M54 16 L58 12", INK, 1.6), line("M55 21 L60 21", INK, 1.6), line("M50 13 L51 8", INK, 1.6)],
         ("pulse", 500, 0.25, 0), origin=(54, 16)),
])

# Balonik: a balloon fit to burst with joy.
figure("balon", "Pękam z radości", "Pękam z radości", [
    part([
        ("p", "M32 42 Q28 49 33 55 Q37 60 32 64", None, "#6b6b7a", 1.2),
    ], ("sway", 1500, 10, 0), origin=(32, 42)),
    part([
        ("e", 32, 23, 15.5, 18.5, "#ff5a7a", "#c2304f", 1.6),
        ("e", 25, 15, 3.5, 5.5, "#ff9db0", None, 0),
        ("p", "M29 41 L35 41 L32 44.5 Z", "#c2304f", None, 0),
        line("M24 22 Q26.5 19.5 29 22", INK, 1.8),
        line("M35 22 Q37.5 19.5 40 22", INK, 1.8),
        ("p", "M26 27 Q32 35 38 27 Z", "#7a2034", INK, 1.1),
        *blush((23, 28), (41, 28)),
    ], ("bob", 2200, 3.5, 0), origin=(32, 42)),
])

# Pączek: a doughnut handing out comfort.
figure("paczek", "Pączuś na pocieszenie", "Pączuś na pocieszenie", [
    part([
        ("pe", "M32 12 a21 21 0 1 0 0.01 0 Z M32 27 a6 6 0 1 1 -0.01 0 Z", "#d99a4e", "#a8692a", 1.6),
        ("pe", "M32 15 a18 18 0 1 0 0.01 0 Z M32 26 a7.5 7.5 0 1 1 -0.01 0 Z", "#ff8fc7", None, 0),
        ("r", 21, 22, 4, 1.6, 0.8, "#ffd166", None, 0),
        ("r", 40, 21, 4, 1.6, 0.8, "#7fd3ff", None, 0),
        ("r", 44, 33, 4, 1.6, 0.8, WHITE, None, 0),
        ("r", 17, 33, 4, 1.6, 0.8, "#7bd389", None, 0),
        ("r", 30, 17.5, 4, 1.6, 0.8, WHITE, None, 0),
        line("M24 43 Q26 45 28 43", INK, 1.6),
        line("M36 43 Q38 45 40 43", INK, 1.6),
        line("M29.5 47.5 Q32 49.5 34.5 47.5", INK, 1.6),
    ], ("sway", 1800, 6, 0), origin=(32, 54)),
])

# Reactions are the small, quick ones; stickers the big scenes with captions.
REACTIONS = ["pierog", "ogorek", "ziemniak", "mozg", "rakieta", "kaktus", "chmurka", "slimak", "kapec", "kot"]
STICKERS = ["pierog", "ogorek", "ziemniak", "mozg", "rakieta", "kot", "herbatka", "tost", "jajo", "dino",
            "balon", "paczek"]


def color(value):
    return None if value is None else value


def normalise(shape):
    kind = shape[0]
    if kind == "c":
        _, cx, cy, r, fill, stroke, width = shape
        return {"t": "c", "n": [cx, cy, r], "fill": fill, "stroke": stroke, "w": width}
    if kind == "e":
        _, cx, cy, rx, ry, fill, stroke, width = shape
        return {"t": "e", "n": [cx, cy, rx, ry], "fill": fill, "stroke": stroke, "w": width}
    if kind == "r":
        _, x, y, w, h, radius, fill, stroke, width = shape
        return {"t": "r", "n": [x, y, w, h, radius], "fill": fill, "stroke": stroke, "w": width}
    _, d, fill, stroke, width = shape
    return {"t": "p", "d": d, "fill": fill, "stroke": stroke, "w": width, "evenodd": kind == "pe"}


def data():
    figures = []
    for fig in FIGURES:
        figures.append({
            "id": fig["id"],
            "name": fig["name"],
            "caption": fig["caption"],
            "parts": [
                {"origin": p["origin"], "rotate": p["rotate"], "anim": p["anim"],
                 "shapes": [normalise(s) for s in p["shapes"]]}
                for p in fig["parts"]
            ],
        })
    ids = {fig["id"] for fig in figures}
    assert all(code in ids for code in REACTIONS + STICKERS)
    return figures


def kotlin_color(value):
    return "null" if value is None else "0xFF" + value.lstrip("#").upper()


def f(value):
    return f"{float(value)}f"


def write_js(figures):
    body = json.dumps({"figures": figures, "reactions": REACTIONS, "stickers": STICKERS},
                      ensure_ascii=False, separators=(",", ":"))
    out = ROOT / "crates/desktop/ui/figures-data.js"
    out.write_text(
        "// Generated by tools/figures.py. Do not edit: change the script and run it again.\n"
        f"\"use strict\";\nconst FIGURE_DATA = {body};\n",
        encoding="utf-8",
    )


def write_kotlin(figures):
    lines = [
        "// Generated by tools/figures.py. Do not edit: change the script and run it again.",
        "package org.komunikator.app",
        "",
        "val FIGURES: Map<String, Figure> = listOf(",
    ]
    for fig in figures:
        lines.append(f"    Figure(\"{fig['id']}\", \"{fig['name']}\", \"{fig['caption']}\", listOf(")
        for p in fig["parts"]:
            anim = p["anim"]
            anim_code = "null" if anim is None else (
                f"Anim(\"{anim[0]}\", {int(anim[1])}, {f(anim[2])}, {f(anim[3])})")
            lines.append(
                f"        Part({f(p['origin'][0])}, {f(p['origin'][1])}, {f(p['rotate'])}, {anim_code}, listOf(")
            for s in p["shapes"]:
                numbers = ", ".join(f(n) for n in s.get("n", []))
                d = "null" if s["t"] != "p" else json.dumps(s["d"])
                lines.append(
                    f"            Shape('{s['t']}', floatArrayOf({numbers}), {d}, {kotlin_color(s['fill'])}, "
                    f"{kotlin_color(s['stroke'])}, {f(s['w'])}, {str(s.get('evenodd', False)).lower()}),")
            lines.append("        )),")
        lines.append("    )),")
    lines.append(").associateBy { it.id }")
    lines.append("")
    lines.append("val REACTIONS: List<String> = listOf(" + ", ".join(f"\"{r}\"" for r in REACTIONS) + ")")
    lines.append("val STICKERS: List<String> = listOf(" + ", ".join(f"\"{s}\"" for s in STICKERS) + ")")
    out = ROOT / "android/app/src/main/java/org/komunikator/app/FiguresData.kt"
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    figures = data()
    write_js(figures)
    write_kotlin(figures)
    print(f"{len(figures)} figures, {len(REACTIONS)} reactions, {len(STICKERS)} stickers")
