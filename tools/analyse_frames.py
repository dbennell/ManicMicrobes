#!/usr/bin/env python3
"""Turn a series of frames into numbers: how holed the tissue is, and how much it flickers."""
import sys, glob
from PIL import Image, ImageChops

VIEWPORT = (0, 22, 1020, 700)   # the slide, without the menu bar or the metrics rail

def frames(stem):
    return [Image.open(f).convert("RGB").crop(VIEWPORT) for f in sorted(glob.glob(stem + "_*.png"))]

def is_background(px):
    # The slide with no overlay is near-black; cells are grey/green and much brighter.
    return px[0] < 40 and px[1] < 40 and px[2] < 40

def analyse(stem, label):
    fs = frames(stem)
    if not fs:
        print(f"{label}: no frames"); return
    w, h = fs[0].size
    a = fs[0].load()

    # Rows and columns that contain any cell, so "inside the colony" means inside its extent
    # rather than inside the window. Holes are background pixels in there; the black around the
    # colony is not a hole.
    # Per row, the span between the first and last cell pixel. A bounding rectangle counts the
    # black corners outside a rounded colony as holes, which put the perfectly-tiled bench at
    # 47% and made the number meaningless.
    inside = holes = 0
    for y in range(0, h, 2):
        row = [x for x in range(0, w, 2) if not is_background(a[x, y])]
        if len(row) < 2:
            continue
        for x in range(row[0], row[-1], 2):
            inside += 1
            if is_background(a[x, y]):
                holes += 1
    if inside == 0:
        print(f"{label}: nothing on the slide"); return

    # Flicker: how much of the picture changes from one frame to the next, at a threshold well
    # above dithering, so it counts boundaries moving rather than shading noise.
    churn = []
    for p, q in zip(fs, fs[1:]):
        d = ImageChops.difference(p, q).convert("L").point(lambda v: 255 if v > 24 else 0)
        moved = sum(d.getdata()) / 255
        churn.append(100.0 * moved / (w * h))

    print(
        f"{label:<26} holes {100.0*holes/max(inside,1):5.2f}% of the colony"
        + (f"   flicker {sum(churn)/len(churn):5.2f}% of pixels per tick" if churn else "")
        + f"   ({len(fs)} frames)"
    )

if __name__ == "__main__":
    analyse(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else sys.argv[1])
