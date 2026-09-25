"""Trace the ground plan from the "The Map" spread of City of Darkness (1993)
into a georeferenced label image the sim samples.

    python tools/trace_map.py <path to book pdf> [out_dir=data]

Outputs (out_dir):
    plan_labels.png  RGB label image: R,G = building id (1-based, 0 = none),
                     B = class (0 outside, 1 lane/open, 2 building, 3 low grey structure)
    plan.json        affine lon/lat -> label px, lane-name polylines and
                     numbered-place points (in label px), provenance.
    plan_debug.png   overlay for eyeballing.

The book image itself is not written out; only derived geometry.
"""

import json
import math
import sys
from pathlib import Path

import numpy as np
import pymupdf
from PIL import Image
from scipy import ndimage as ndi

MAP_PAGE = 107  # 1-based page index of "The Map" (book pp. 214-215)
CROP = (330, 120, 2110, 1400)  # native px: the city and a margin of its surroundings


def load_map(pdf):
    d = pymupdf.open(pdf)
    p = d[MAP_PAGE - 1]
    xref = p.get_images()[0][0]
    raw = d.extract_image(xref)["image"]
    import io

    im = Image.open(io.BytesIO(raw)).convert("RGB")
    return np.asarray(im.crop(CROP)).astype(np.float32) / 255.0


def hsv(rgb):
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    mx = rgb.max(-1)
    mn = rgb.min(-1)
    v = mx
    s = np.where(mx > 0, (mx - mn) / np.maximum(mx, 1e-6), 0)
    d = np.maximum(mx - mn, 1e-6)
    h = np.where(
        mx == r, ((g - b) / d) % 6, np.where(mx == g, (b - r) / d + 2, (r - g) / d + 4)
    ) * 60.0
    return h, s, v


def classify(rgb):
    h, s, v = hsv(rgb)
    orange = (s > 0.22) & (h > 12) & (h < 58) & (v > 0.45)
    dark = v < 0.42  # outlines, labels, number discs
    grey = (s < 0.16) & (v > 0.42) & (v < 0.80)  # hatched low structures (east strip)
    pale = (v >= 0.80) & (s < 0.22)  # lanes, courtyards, roads, paper
    green = (h > 60) & (h < 170) & (s > 0.12)
    return orange, dark, grey, pale, green


def disk(r):
    y, x = np.ogrid[-r : r + 1, -r : r + 1]
    return x * x + y * y <= r * r


def lanes_and_city(orange, dark, grey, pale):
    built = orange | grey | dark
    # City = the solid mass of buildings with its lanes: close the lanes, fill holes.
    city = ndi.binary_closing(built, disk(12))
    city = ndi.binary_fill_holes(city)
    city = ndi.binary_opening(city, disk(8))
    lab, n = ndi.label(city)
    sizes = ndi.sum(city, lab, range(1, n + 1))
    city = lab == (1 + int(np.argmax(sizes)))
    # Lanes: pale inside the city, minus pencil highlights (small enclosed blobs).
    lane = pale & city
    lane = ndi.binary_opening(lane, disk(2))
    lab, n = ndi.label(lane)
    sizes = ndi.sum(lane, lab, range(1, n + 1))
    keep = np.zeros(n + 1, bool)
    keep[1:] = sizes >= 900
    lane = keep[lab]
    # Letters and number discs sit on lanes: a majority filter heals them.
    lane = ndi.median_filter(lane.astype(np.uint8), size=9).astype(bool) | lane
    return lane & city, city


def buildings(rgb, mask):
    """Split the building mass into individual buildings along the drawn outlines
    (thin lines darker than their surroundings)."""
    v = rgb.max(-1)
    local = ndi.median_filter(v, size=9)
    ridge = (v < local - 0.07) | (v < 0.5)
    ridge = ndi.binary_dilation(ridge, disk(1))
    interior = mask & ~ridge
    interior = ndi.binary_opening(interior, disk(2))
    labels, n = ndi.label(interior)
    # Grow labels back over the outline pixels (nearest interior label).
    idx = ndi.distance_transform_edt(labels == 0, return_distances=False, return_indices=True)
    grown = labels[idx[0], idx[1]]
    grown[~mask] = 0
    return grown, n


def main():
    pdf = sys.argv[1]
    out = Path(sys.argv[2] if len(sys.argv) > 2 else "data")
    out.mkdir(parents=True, exist_ok=True)
    rgb = load_map(pdf)
    orange, dark, grey, pale, green = classify(rgb)
    dbg = np.zeros(rgb.shape, np.uint8)
    dbg[pale] = (235, 235, 235)
    dbg[orange] = (230, 140, 40)
    dbg[grey] = (120, 120, 160)
    dbg[dark] = (20, 20, 20)
    dbg[green] = (60, 160, 60)
    Image.fromarray(dbg).save(out / "classes_debug.png")

    lane, city = lanes_and_city(orange, dark, grey, pale)
    labels, n = buildings(rgb, city & ~lane)
    print("buildings (raw components):", n)
    rng = np.random.default_rng(1)
    pal = rng.integers(60, 255, (n + 1, 3)).astype(np.uint8)
    pal[0] = (240, 240, 240)
    seg = pal[labels]
    seg[~city] = (30, 30, 30)
    Image.fromarray(seg).save(out / "buildings_debug.png")


if __name__ == "__main__":
    main()
