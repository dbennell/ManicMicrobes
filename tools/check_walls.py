#!/usr/bin/env python3
"""Is one cell drawn over another, and by how much?

`check_outline.py` measures a cell's edge where it faces open background. That is the easy half:
the hard half is the wall *between* two cells, where there is no background to measure against and
where an overlap actually looks like something. This measures those.

Every cell in the shader bench is a different hue, and the shader only ever scales that hue —
lambert, rim, grain and the membrane ring all multiply the colour rather than shifting it. So
normalising a pixel by its own brightness leaves the hue alone, and a pixel can be attributed to
whichever cell it belongs to whatever the shading is doing. Walking from one centre to the other
and finding where the attribution changes gives the boundary the eye sees, to a fraction of a
pixel; the geometry dump says where that boundary is supposed to be. The difference between them
is how far one cell is drawn into the other, and its sign only says which of the two won:

    positive — the lower-numbered cell is drawn past the wall, over its neighbour
    negative — the higher-numbered one is, which is the same fault the other way round

A *gap* is a different thing and is counted separately: it is background found between the two,
which means neither of them reached the wall.

Crossings are taken at several points along the shared chord as well as on the line of centres,
because a wall can be right in the middle and wrong at its ends — which is exactly what a badly
tapered swell does. The offsets are fractions of the chord's own half-length, worked out per pair
from the wall distance and the drawn radius: past the end of the chord the boundary between two
cells is not the seam plane at all, and measuring there would report a fault that is only the
measurement running off the end of the wall.

    MM_BENCH_LAYOUT=fifteen MM_BENCH_MOTION=jitter MM_BENCH_SERIES=8 MM_BENCH_PANEL=0 \\
      MM_BENCH_DUMP=/tmp/f.txt MM_BENCH_SHOT=/tmp/f.png ./target/release/shaderbench
    tools/check_walls.py /tmp/f.txt /tmp/f.png            # takes the whole numbered series
"""
import glob
import math
import sys

from PIL import Image

# How far either side of the expected wall to look, in pixels.
SEARCH = 30.0
STEP = 0.2
# Where along the shared chord to cross it, as a fraction of the chord's own half-length. Zero is
# the line of centres. Kept inside 0.8 so that a crossing is always well within the wall rather
# than at the corner where it gives out.
OFFSETS = (-0.8, -0.5, -0.25, 0.0, 0.25, 0.5, 0.8)
# A pixel this dark is the slide showing through, not a cell.
DARK = 0.06
# How far a pixel may sit off the line between the two cells' colours and still be called a blend
# of them, relative to its own brightness. Generous, because the grain moves a pixel a little and
# the two hues can be close together on a crowded slide; a third cell is far further off than this.
RESIDUAL = 0.12


def read_dump(path):
    cells = {}
    for line in open(path):
        if line.startswith("#"):
            continue
        p = line.split()
        if p[0] == "cell":
            cells[p[1]] = {
                "x": float(p[2]),
                "y": float(p[3]),
                "r": float(p[4]),
                "swell": float(p[5]),
                "rgb": (float(p[6]), float(p[7]), float(p[8])),
            }
        elif p[0] == "rays":
            cells[p[1]]["rays"] = [float(v) for v in p[3:]]
    return cells


def to_linear(v):
    """One sRGB byte to the linear light it stands for. The blend is linear; the file is not."""
    c = v / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


LINEAR = [to_linear(v) for v in range(256)]


def unit(rgb):
    n = math.sqrt(sum(c * c for c in rgb))
    return tuple(c / n for c in rgb) if n > 1e-6 else (0.0, 0.0, 0.0)


def sample(px, w, h, x, y):
    """The pixel at a continuous position, in linear light; `i + 0.5` is the centre of pixel `i`."""
    x, y = int(x - 0.5), int(y - 0.5)
    if x < 0 or y < 0 or x >= w or y >= h:
        return (0.0, 0.0, 0.0)
    p = px[x, y]
    return (LINEAR[p[0]], LINEAR[p[1]], LINEAR[p[2]])


def mixture(p, ca, cb):
    """How much of `p` is cell `a` rather than cell `b`, and how well that explains it at all.

    A pixel on a wall is not one cell or the other: the two antialiased edges overlap, so it is a
    blend, and the boundary the eye sees is where the blend is half and half. Nearest-hue
    classification cannot see that — a half-and-half pixel is a *third* hue, and in a crowd of
    fifteen it will confidently be some other cell's. Which is a wrong answer rather than no
    answer, and it reads as an enormous overlap between a pair that is perfectly walled.

    So: fit `p = k * (alpha * ca + (1 - alpha) * cb)`. Two unknowns, three channels, one spare to
    check the fit with — `k` absorbs everything the shader does to brightness (lambert, rim,
    grain, the dark membrane ring), `alpha` is the blend, and a residual that will not come down
    means the pixel is neither of these two cells and the crossing must be thrown away.
    """
    best = None
    steps = 40
    for n in range(steps + 1):
        alpha = n / steps
        q = tuple(alpha * ca[i] + (1 - alpha) * cb[i] for i in range(3))
        qq = sum(v * v for v in q)
        if qq < 1e-9:
            continue
        k = sum(p[i] * q[i] for i in range(3)) / qq
        resid = math.sqrt(sum((p[i] - k * q[i]) ** 2 for i in range(3)))
        if best is None or resid < best[1]:
            best = (alpha, resid, k)
    alpha, resid, k = best
    scale = math.sqrt(sum(v * v for v in p)) or 1e-6
    return alpha, resid / scale


def attribution(rgb, a, b):
    """Negative where the pixel is cell `a`'s colour, positive where it is `b`'s.

    Brightness is divided out first, so shading, the dark membrane ring and the grain cannot move
    this — only hue can, and hue is the one thing the shader never changes.
    """
    n = unit(rgb)
    da = sum((n[i] - a[i]) ** 2 for i in range(3))
    db = sum((n[i] - b[i]) ** 2 for i in range(3))
    return da - db


def whose(rgb, hues):
    """Which cell a pixel belongs to, against *every* cell on the slide.

    Against every one, and not just the two whose wall is being measured, because in a crowd a
    third cell can be drawn across the gap between two others — and asked only about the two, this
    would confidently call that third cell whichever of them its hue happened to be nearer. That
    is a wrong answer rather than no answer, and it reads as a huge overlap between a pair that is
    in fact perfectly walled.
    """
    n = unit(rgb)
    best, second, who = None, None, None
    for cid, c in hues.items():
        d = sum((n[i] - c[i]) ** 2 for i in range(3))
        if best is None or d < best:
            best, second, who = d, best, cid
        elif second is None or d < second:
            second = d
    return who, best, second


def covers(c, x, y):
    """Is this point inside that cell's own outline?"""
    dx, dy = x - c["x"], y - c["y"]
    n = len(c["rays"])
    k = int(round(math.atan2(dy, dx) / (2 * math.pi) * n)) % n
    return math.hypot(dx, dy) < c["rays"][k]


def crossings(dump_path, png_path):
    cells = read_dump(dump_path)
    im = Image.open(png_path).convert("RGB")
    w, h = im.size
    px = im.load()
    hues = {cid: unit(c["rgb"]) for cid, c in cells.items()}
    out = []
    ids = sorted(cells)
    for n, i in enumerate(ids):
        for j in ids[n + 1 :]:
            a, b = cells[i], cells[j]
            dx, dy = b["x"] - a["x"], b["y"] - a["y"]
            d = math.hypot(dx, dy)
            if d < 1e-3:
                continue
            ux, uy = dx / d, dy / d
            # Where each says its outline ends towards the other. If they sum to the distance
            # between the centres they share a wall; if they fall short there is no contact here.
            ka = int(round(math.atan2(uy, ux) / (2 * math.pi) * len(a["rays"]))) % len(a["rays"])
            kb = int(round(math.atan2(-uy, -ux) / (2 * math.pi) * len(b["rays"]))) % len(b["rays"])
            wall_a, wall_b = a["rays"][ka], b["rays"][kb]
            if wall_a + wall_b < d - 0.5:
                continue  # not touching: there is nothing here that should be a wall
            ca, cb = unit(a["rgb"]), unit(b["rgb"])  # noqa: F841 — kept for the record
            # Half the chord the two outlines actually share: the wall is a line, and it ends
            # where the two circles cross. Beyond that there is no shared wall to be right about.
            half_chord = min(
                math.sqrt(max(0.0, a["r"] ** 2 - wall_a**2)),
                math.sqrt(max(0.0, b["r"] ** 2 - wall_b**2)),
            )
            if half_chord < 2.0:
                continue  # a wall a couple of pixels long is not a wall
            # Where the wall is, measured out from `a` along the line of centres. The seam is a
            # plane perpendicular to that line, so it is the same distance along every parallel.
            want = wall_a
            for frac in OFFSETS:
                # Step off the line of centres, along the wall.
                off = frac * half_chord
                ox, oy = -uy * off, ux * off
                # But only where the wall is still *these two* cells' boundary. Two circles cross
                # at a chord; in a packed sheet the shared edge is shorter than that chord, because
                # a third cell arrives and takes over before it ends. Measuring past that point
                # compares the boundary between i and some third cell against the wall between i
                # and j, and reports a large fault that is only the measurement running off the end
                # of the wall — every one of the worst readings on a raft was exactly this.
                mx = a["x"] + ux * want + ox
                my = a["y"] + uy * want + oy
                if any(
                    covers(c, mx, my) for cid, c in cells.items() if cid != i and cid != j
                ):
                    continue
                found = None
                previous = None
                gap = False
                third = False
                t = want - SEARCH
                while t < want + SEARCH:
                    x = a["x"] + ux * t + ox
                    y = a["y"] + uy * t + oy
                    rgb = sample(px, w, h, x, y)
                    if max(rgb) < DARK:
                        # The slide, between two cells that are supposed to be pressed together.
                        gap = True
                        previous = None
                        t += STEP
                        continue
                    alpha, resid = mixture(rgb, ca, cb)
                    if resid > RESIDUAL:
                        # Neither of these two cells: something else is drawn here.
                        third = True
                        previous = None
                        t += STEP
                        continue
                    # Walking from `a` towards `b`, so alpha falls from one to nothing. The wall is
                    # where it passes a half.
                    if previous is not None and previous[1] >= 0.5 > alpha:
                        span = previous[1] - alpha
                        found = t - STEP * ((0.5 - alpha) / span if span > 1e-9 else 0.5)
                        break
                    previous = (t, alpha)
                    t += STEP
                if found is not None:
                    out.append((i, j, frac, found - want, None))
                elif third:
                    out.append((i, j, frac, None, True))
                elif gap:
                    out.append((i, j, frac, None, None))
    return out


def main(dump_glob, png_glob):
    dumps = sorted(glob.glob(dump_glob))
    pngs = sorted(glob.glob(png_glob))
    # Loudly, and before reading anything. A run that wrote its geometry but not its photographs
    # is a real thing that happens — the screenshot is saved by an observer a few frames after it
    # is asked for — and silently measuring three dumps against two pictures pairs the wrong frames
    # together and reports a fault that is only the mismatch.
    if not dumps or not pngs or len(dumps) != len(pngs):
        sys.exit(
            f"{len(dumps)} geometry dumps ({dump_glob}) against {len(pngs)} photographs "
            f"({png_glob}) — they must pair up one for one"
        )
    every = []
    gaps = 0
    thirds = 0
    worst = None
    for dump_path, png_path in zip(dumps, pngs):
        for i, j, frac, err, third in crossings(dump_path, png_path):
            if third is not None:
                thirds += 1
                continue
            if err is None:
                gaps += 1
                continue
            every.append(abs(err))
            if worst is None or abs(err) > abs(worst[3]):
                worst = (png_path, i, j, err, frac)
    if not every and not gaps:
        sys.exit("no wall between any two cells was found — is anything touching?")
    every.sort()
    n = max(1, len(every))
    total = len(every) + gaps + thirds

    def q(p):
        return every[min(n - 1, int(n * p))] if every else 0.0

    print(f"{len(pngs)} frames, {total} wall crossings measured")
    print("how far the visible boundary is from the shared wall, in pixels:")
    print(f"   median {q(0.5):.2f}   p90 {q(0.9):.2f}   p99 {q(0.99):.2f}   worst {every[-1]:.2f}")
    for limit in (1.0, 2.0, 4.0, 8.0):
        over = sum(1 for e in every if e > limit)
        print(f"   one cell over the other by more than {limit:>4.0f} px: "
              f"{over:>5} ({100.0*over/total:.1f}%)")
    print(f"   background between two cells that should be touching: "
          f"{gaps:>5} ({100.0*gaps/total:.1f}%)")
    print(f"   a third cell drawn across the wall:                    "
          f"{thirds:>5} ({100.0*thirds/total:.1f}%)")
    if worst:
        png, i, j, err, frac = worst
        who = i if err > 0 else j
        print(f"   worst: cell {who} over cell {j if err > 0 else i} by {abs(err):.2f} px, "
              f"{png.split('/')[-1]} at chord {frac:+.2f}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    # A single file works; a stem with a wildcard takes the whole series.
    d, p = sys.argv[1], sys.argv[2]
    if "*" not in d and "_000" not in d:
        d = d.replace(".txt", "_*.txt")
    if "*" not in p and "_000" not in p:
        p = p.replace(".png", "_*.png")
    main(d, p)
