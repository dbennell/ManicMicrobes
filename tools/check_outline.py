#!/usr/bin/env python3
"""Does the cell shader draw the outline it was given?

The shader bench (`crates/mm-app/src/bin/shaderbench.rs`) can photograph a frame and write that
same frame's geometry as numbers. This compares them: for every direction out of every cell, where
the data said the outline was, against where the shader actually put it.

Coverage is recovered exactly rather than guessed at. A frame photographed against two different
backgrounds gives, per pixel, `P = a*C + (1-a)*B`, so two shots with the same C and different B
solve for `a` with no assumption about what colour a cell is or how it is shaded:

    a = 1 - (P_light - P_dark) / (B_light - B_dark)

The outline is the half-coverage contour, found to a fraction of a pixel by interpolating along
each ray. Only rays that leave the clump can be measured — between two cells there is no coverage
edge to find, because both sides are covered. A shared wall is drawn by two cells agreeing, and
what checks *that* is the bench's own arithmetic, in `tests/shader_probe.rs`.

    MM_BENCH_AT=30 MM_BENCH_MOTION=still MM_BENCH_ZOOM=110 \\
      MM_BENCH_DUMP=/tmp/f.txt MM_BENCH_SHOT=/tmp/dark.png  ./target/release/shaderbench
    MM_BENCH_AT=30 MM_BENCH_MOTION=still MM_BENCH_ZOOM=110 MM_BENCH_BG=1,1,1 \\
      MM_BENCH_SHOT=/tmp/light.png ./target/release/shaderbench
    tools/check_outline.py /tmp/f.txt /tmp/dark.png /tmp/light.png
"""
import sys
import math

from PIL import Image

# How far either side of the expected radius to look for the half-coverage crossing. Wide enough
# to find a genuine disagreement and report it, narrow enough not to wander onto a neighbour.
SEARCH = 24.0
STEP = 0.25


def read_dump(path):
    cells = {}
    for line in open(path):
        if line.startswith("#"):
            continue
        parts = line.split()
        if parts[0] == "cell":
            cells[parts[1]] = {"x": float(parts[2]), "y": float(parts[3]), "r": float(parts[4])}
        elif parts[0] == "rays":
            cells[parts[1]]["rays"] = [float(v) for v in parts[3:]]
    return cells


def to_linear(v):
    """One sRGB byte to the linear light it stands for.

    The blend happens in linear space and the file is written in sRGB, so the difference between
    the two photographs is only proportional to coverage once this is undone. Skipping it biases
    the recovered coverage everywhere the ramp is dark, which is most of a one-pixel edge.
    """
    c = v / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


LINEAR = [to_linear(v) for v in range(256)]


def coverage(dark_png, light_png):
    """Per-pixel alpha, from the same frame over two backgrounds."""
    a, b = Image.open(dark_png).convert("RGB"), Image.open(light_png).convert("RGB")
    if a.size != b.size:
        sys.exit("the two photographs are different sizes")
    w, h = a.size
    pa, pb = a.load(), b.load()
    # The two backgrounds, as the corners of the picture see them.
    dark_bg = LINEAR[pa[2, h - 3][1]]
    light_bg = LINEAR[pb[2, h - 3][1]]
    span = light_bg - dark_bg
    if span < 0.5:
        sys.exit(
            f"the two photographs have backgrounds too close together ({dark_bg:.3f}, "
            f"{light_bg:.3f}) — one of them should be MM_BENCH_BG=1,1,1"
        )
    cov = [[0.0] * w for _ in range(h)]
    for y in range(h):
        row = cov[y]
        for x in range(w):
            # Green alone: the backgrounds differ across the whole range in every channel, and one
            # channel is enough. Averaging three only adds noise from the deband dither.
            row[x] = 1.0 - (LINEAR[pb[x, y][1]] - LINEAR[pa[x, y][1]]) / span
    return cov, w, h


def sample(cov, w, h, x, y):
    """Coverage at a continuous position, bilinearly.

    `x` and `y` are positions in the picture, where the *centre* of pixel `i` is at `i + 0.5` —
    which is why they are shifted before indexing. Half a pixel sounds like nothing and is not:
    left out, it tilts every measured radius by up to 0.7 px in a direction that depends on the
    angle, which reads as the shader and the data disagreeing by a couple of pixels when they
    agree to a hundredth of one.
    """
    x, y = x - 0.5, y - 0.5
    if x < 0 or y < 0 or x >= w - 1 or y >= h - 1:
        return 0.0
    x0, y0 = int(x), int(y)
    fx, fy = x - x0, y - y0
    return (
        cov[y0][x0] * (1 - fx) * (1 - fy)
        + cov[y0][x0 + 1] * fx * (1 - fy)
        + cov[y0 + 1][x0] * (1 - fx) * fy
        + cov[y0 + 1][x0 + 1] * fx * fy
    )


def main(dump_path, dark_png, light_png):
    cells = read_dump(dump_path)
    cov, w, h = coverage(dark_png, light_png)
    errors = []
    measured = 0
    walled = 0
    for cid, c in sorted(cells.items()):
        rays = c["rays"]
        n = len(rays)
        for k, want in enumerate(rays):
            theta = 2 * math.pi * k / n
            dx, dy = math.cos(theta), math.sin(theta)
            # Walk outward from well inside the expected radius. The first fall through a half
            # is the outline; if coverage never falls, this ray is into a neighbour.
            r = max(2.0, want - SEARCH)
            previous = sample(cov, w, h, c["x"] + dx * r, c["y"] + dy * r)
            found = None
            while r < want + SEARCH:
                r += STEP
                now = sample(cov, w, h, c["x"] + dx * r, c["y"] + dy * r)
                if previous >= 0.5 > now:
                    # Linear between the two samples, which is what a one-pixel ramp is.
                    span = previous - now
                    found = r - STEP * ((0.5 - now) / span if span > 1e-6 else 0.5)
                    break
                previous = now
            if found is None:
                walled += 1
                continue
            measured += 1
            errors.append(found - want)

    if not errors:
        sys.exit("no ray left the clump — nothing could be measured")
    errors.sort()
    n = len(errors)
    def q(p):
        return errors[min(n - 1, int(n * p))]
    print(f"{len(cells)} cells, {measured} rays measured, {walled} into a neighbour and skipped")
    print("where the shader drew the outline, minus where the data said, in pixels:")
    print(f"   median {q(0.5):+.2f}   p10 {q(0.1):+.2f}   p90 {q(0.9):+.2f}")
    print(f"   worst  {errors[0]:+.2f} / {errors[-1]:+.2f}   mean {sum(errors)/n:+.3f}")
    inside = sum(1 for e in errors if abs(e) <= 1.0)
    print(f"   within one pixel: {100.0*inside/n:.1f}%")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2], sys.argv[3])
