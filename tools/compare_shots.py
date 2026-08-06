#!/usr/bin/env python3
"""Do two screenshots show the same picture, and if not, how far apart are they?

`cmp` answers the first half and nothing else: two frames that differ by one unit in the last
place of one float, on fourteen pixels of nine hundred thousand, are "different" to `cmp` and
identical to anyone looking. That is not a hypothetical — it is exactly what separates the two
entry points in `cell.wgsl`, which compute the same outline by arithmetic that the driver is free
to reassociate.

So: the count of differing pixels, the worst channel delta, and where the differences are. A
change that alters the picture moves thousands of pixels by tens of levels; a change that only
alters the last bit moves a handful by one or two, and every one of them sits on an edge.

    tools/compare_shots.py /tmp/a_000.png /tmp/b_000.png
    tools/compare_shots.py /tmp/a_000.png /tmp/b_000.png --max-delta 2 --max-pixels 100

Exits non-zero when the difference is larger than the tolerance given, so it can gate a change
rather than merely describe one. With no tolerance it reports and exits zero.
"""
import argparse
import sys

from PIL import Image

# How steep a gradient a pixel must sit on to count as "on an edge". Rounding differences are
# invisible in flat regions and only show where the shader is ramping steeply, so this separates
# "the last bit moved" from "something is drawn in a different place".
EDGE_SPREAD = 16


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument(
        "--max-delta",
        type=int,
        default=None,
        help="fail if any channel differs by more than this",
    )
    ap.add_argument(
        "--max-pixels",
        type=int,
        default=None,
        help="fail if more than this many pixels differ",
    )
    args = ap.parse_args()

    a = Image.open(args.a).convert("RGB")
    b = Image.open(args.b).convert("RGB")
    if a.size != b.size:
        print(f"different sizes: {a.size} and {b.size}")
        return 1

    pa, pb = a.load(), b.load()
    w, h = a.size
    total = w * h
    differing = 0
    worst = 0
    on_edge = 0
    hist = {}
    for y in range(h):
        for x in range(w):
            ca, cb = pa[x, y], pb[x, y]
            delta = max(abs(ca[i] - cb[i]) for i in range(3))
            if not delta:
                continue
            differing += 1
            worst = max(worst, delta)
            hist[delta] = hist.get(delta, 0) + 1
            neighbours = [
                pa[x + dx, y + dy]
                for dx, dy in ((-1, 0), (1, 0), (0, -1), (0, 1))
                if 0 <= x + dx < w and 0 <= y + dy < h
            ]
            spread = max(
                max(abs(n[i] - ca[i]) for i in range(3)) for n in neighbours
            )
            if spread >= EDGE_SPREAD:
                on_edge += 1

    print(f"{w}x{h}, {total} pixels")
    print(f"differing:  {differing}  ({100.0 * differing / total:.4f}%)")
    print(f"worst delta: {worst} of 255")
    if differing:
        print(f"on an edge:  {on_edge} of {differing}")
        print("deltas:     ", dict(sorted(hist.items())))
        if worst <= 2 and on_edge >= differing * 0.8:
            print("\nrounding at the edges, not a different picture.")

    failed = False
    if args.max_delta is not None and worst > args.max_delta:
        print(f"\nFAIL: worst delta {worst} exceeds {args.max_delta}")
        failed = True
    if args.max_pixels is not None and differing > args.max_pixels:
        print(f"\nFAIL: {differing} differing pixels exceeds {args.max_pixels}")
        failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
