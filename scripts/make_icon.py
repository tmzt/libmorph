#!/usr/bin/env python3
"""Make an app's Android launcher icons from one square image.

    python3 -I scripts/make_icon.py --source art.png --out icon/
                                    [--key RRGGBB|auto] [--key-tolerance T]
                                    [--background RRGGBB] [--hue DEG]
                                    [--full-bleed] [--no-monochrome]

Needs Python 3 with Pillow and numpy.

The source is a square image (PNG, JPEG or anything Pillow reads), at least
432 px (1024 recommended):
  - by default, the artwork on transparency. It is placed inside the
    adaptive icon's 66 dp safe circle, and drawn over a rounded square of
    the background colour for the legacy icons;
  - with --key, the artwork on a flat colour, such as a JPEG. Pixels of
    the key colour become transparent, and edges are unmixed from it
    (alpha from each pixel's distance to the key, its colour solved from
    p = a*C + (1-a)*key), so no fringe of the old colour is left. `auto`
    takes the key from the image's corners. --key-tolerance (0..1,
    default 0.03) is how close to the key still counts as transparent,
    which absorbs JPEG noise;
  - with --full-bleed, an image made for the whole 108 dp adaptive canvas
    (artwork inside its centre 66 dp), used as the foreground unchanged.

Writes, under --out:
  icon-1024.png, icon-512.png   masters: the artwork on its rounded square
  preview.png                   contact sheet of the masters and every size
  android/res/mipmap-*/ic_launcher{,_round,_foreground}.png
  android/res/mipmap-anydpi-v26/ic_launcher{,_round}.xml
  android/res/values/ic_launcher_background.xml

--hue DEG turns the artwork's HSV hue by DEG degrees, positive or negative
(0, the default, leaves it unchanged); the background colour is not
turned.

The adaptive icon's monochrome layer (Android 13 themed icons) is the
foreground, which Android tints by its alpha; --no-monochrome leaves it out.

This is the reference for libmobile's `icons` generator
(docs/design/20261024_03_APP_ICON_PLAN.md in libmobile); once that is
released, apps declare the icon in build.rs instead. Adapted from handlr's
apps/companion/icon/make_icon.py (handlr 64465f6).
"""

import argparse
import os
import sys

import numpy as np
from PIL import Image, ImageDraw

DEFAULT_BACKGROUND = "FFFFFF"

MASTER = 1024
# Corner radius of the legacy rounded square, as a fraction of its side.
CORNER = 0.223
SS = 4  # supersampling for the drawn shapes

DENSITIES = {"mdpi": 1.0, "hdpi": 1.5, "xhdpi": 2.0, "xxhdpi": 3.0, "xxxhdpi": 4.0}
LEGACY_DP = 48
ADAPTIVE_DP = 108
SAFE_DP = 66  # diameter of the adaptive icon's guaranteed-visible circle
# Legacy icons: the shape fills 44 of the 48 dp, as Material's keylines do.
LEGACY_FILL = 44 / 48
MIN_SOURCE = 432


def rounded_mask(size, inset=0, corner=CORNER):
    """Anti-aliased rounded-square mask (L) of side `size`."""
    big = Image.new("L", (size * SS, size * SS), 0)
    r = corner * (size - 2 * inset) * SS
    ImageDraw.Draw(big).rounded_rectangle(
        (inset * SS, inset * SS, (size - inset) * SS - 1, (size - inset) * SS - 1),
        radius=r,
        fill=255,
    )
    return big.resize((size, size), Image.LANCZOS)


def circle_mask(size):
    big = Image.new("L", (size * SS, size * SS), 0)
    ImageDraw.Draw(big).ellipse((0, 0, size * SS - 1, size * SS - 1), fill=255)
    return big.resize((size, size), Image.LANCZOS)


def flat(size, rgb, mask):
    img = Image.new("RGBA", (size, size), rgb + (0,))
    img.putalpha(mask)
    return img


def over(base, top):
    out = base.copy()
    out.alpha_composite(top)
    return out


def content_box(art):
    a = np.asarray(art.getchannel("A"))
    ys, xs = np.nonzero(a > 4)
    if len(xs) == 0:
        sys.exit("make_icon: the source is fully transparent")
    return xs.min(), ys.min(), xs.max() + 1, ys.max() + 1


def place(art, canvas, scale_px, centre):
    """The artwork (cropped to its content) scaled so its farthest pixel lies
    `scale_px` from its centre, pasted centred at `centre` on a transparent
    canvas of side `canvas`."""
    x0, y0, x1, y1 = content_box(art)
    cropped = art.crop((x0, y0, x1, y1))
    a = np.asarray(cropped.getchannel("A")) > 4
    ys, xs = np.nonzero(a)
    cx, cy = (x1 - x0) / 2.0, (y1 - y0) / 2.0
    radius = np.sqrt((xs + 0.5 - cx) ** 2 + (ys + 0.5 - cy) ** 2).max()
    k = scale_px / radius
    w, h = max(1, round(cropped.width * k)), max(1, round(cropped.height * k))
    cropped = premul_resize(cropped, (w, h))
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    out.alpha_composite(cropped, (round(centre - w / 2), round(centre - h / 2)))
    return out


def premul_resize(img, size):
    """Lanczos resize in premultiplied alpha, so transparent pixels' colour
    does not bleed into edges."""
    arr = np.asarray(img).astype(np.float64)
    a = arr[..., 3:4] / 255.0
    pre = np.dstack([arr[..., :3] * a, arr[..., 3:4]])
    chans = [
        np.asarray(Image.fromarray(pre[..., k].astype(np.float32), "F").resize(size, Image.LANCZOS))
        for k in range(4)
    ]
    pre = np.stack(chans, axis=-1)
    al = np.clip(pre[..., 3], 0, 255)
    rgb = np.where(al[..., None] > 0, pre[..., :3] / np.maximum(al[..., None], 1e-6) * 255.0, 0)
    out = np.dstack([np.clip(rgb, 0, 255), al]).round().astype(np.uint8)
    return Image.fromarray(out, "RGBA")


def rotate_hue(c, deg):
    """Add `deg` to the HSV hue of every pixel of `c` (an array of RGB in
    0..255, any shape ending in 3), keeping saturation and value."""
    flat = c.reshape(-1, 3) / 255.0
    r, g, b = flat[:, 0], flat[:, 1], flat[:, 2]
    mx = flat.max(axis=1)
    mn = flat.min(axis=1)
    d = mx - mn
    h = np.zeros_like(mx)
    nz = d > 1e-9
    rm = nz & (mx == r)
    gm = nz & (mx == g) & ~rm
    bm = nz & ~rm & ~gm
    h[rm] = ((g - b)[rm] / d[rm]) % 6.0
    h[gm] = (b - r)[gm] / d[gm] + 2.0
    h[bm] = (r - g)[bm] / d[bm] + 4.0
    h = (h + deg / 60.0) % 6.0
    s = np.where(mx > 0, d / np.maximum(mx, 1e-9), 0.0)
    v = mx
    i = np.floor(h).astype(int) % 6
    f = h - np.floor(h)
    pp = v * (1 - s)
    q = v * (1 - s * f)
    t = v * (1 - s * (1 - f))
    choices = [
        np.stack([v, t, pp], 1),
        np.stack([q, v, pp], 1),
        np.stack([pp, v, t], 1),
        np.stack([pp, q, v], 1),
        np.stack([t, pp, v], 1),
        np.stack([v, pp, q], 1),
    ]
    out = np.zeros_like(flat)
    for k in range(6):
        out[i == k] = choices[k][i == k]
    return (out * 255.0).reshape(c.shape)


def turn_hue(img, deg):
    """`img` (RGBA) with its colours' hue turned by `deg`; alpha unchanged."""
    if deg % 360 == 0:
        return img
    arr = np.asarray(img).astype(np.float64)
    rgb = np.clip(rotate_hue(arr[..., :3], deg), 0, 255)
    return Image.fromarray(np.dstack([rgb, arr[..., 3:4]]).round().astype(np.uint8), "RGBA")


def write_png(img, out, *parts):
    path = os.path.join(out, *parts)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    img.save(path, optimize=True)
    return path


def write_text(text, out, *parts):
    path = os.path.join(out, *parts)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


ADAPTIVE_XML = """<?xml version="1.0" encoding="utf-8"?>
<!-- GENERATED by libmorph scripts/make_icon.py. Do not edit. -->
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/ic_launcher_background" />
    <foreground android:drawable="@mipmap/ic_launcher_foreground" />
{monochrome}</adaptive-icon>
"""
MONOCHROME_XML = '    <monochrome android:drawable="@mipmap/ic_launcher_foreground" />\n'

COLOR_XML = """<?xml version="1.0" encoding="utf-8"?>
<!-- GENERATED by libmorph scripts/make_icon.py. Do not edit. -->
<resources>
    <color name="ic_launcher_background">#{hex}</color>
</resources>
"""


def load_source(path):
    try:
        src = Image.open(path)
        src.load()
    except (OSError, ValueError) as error:
        sys.exit(f"make_icon: cannot read {path}: {error}")
    if src.width != src.height:
        sys.exit(f"make_icon: {path} is not square ({src.width}x{src.height})")
    if src.width < MIN_SOURCE:
        print(f"make_icon: warning: {path} is {src.width} px; scaling up to {MASTER}",
              file=sys.stderr)
    return src.convert("RGBA")


def corner_key(img):
    """The key colour: the median of four 16 px corner patches."""
    arr = np.asarray(img.convert("RGB")).astype(np.float64)
    n = max(1, min(16, img.width // 8))
    patches = [arr[:n, :n], arr[:n, -n:], arr[-n:, :n], arr[-n:, -n:]]
    return np.median(np.concatenate([p.reshape(-1, 3) for p in patches]), axis=0)


def unmix_key(img, key, tolerance):
    """`img` with the key colour made transparent: each pixel's alpha is the
    least `a` for which p = a*C + (1-a)*key has C within 0..255, less the
    tolerance, and C is solved from it."""
    arr = np.asarray(img).astype(np.float64)
    p = arr[..., :3]
    k = np.asarray(key, dtype=np.float64)
    up = np.where(p > k, (p - k) / np.maximum(255.0 - k, 1e-6), 0.0)
    down = np.where(p < k, (k - p) / np.maximum(k, 1e-6), 0.0)
    a = np.maximum(up, down).max(axis=2)
    a = np.clip((a - tolerance) / max(1.0 - tolerance, 1e-6), 0.0, 1.0)
    # The source's own alpha still counts (a PNG with a key colour too).
    a = a * (arr[..., 3] / 255.0)
    safe = np.maximum(a, 1e-3)[..., None]
    c = np.clip((p - (1.0 - a[..., None]) * k) / safe, 0.0, 255.0)
    out = np.dstack([c, a * 255.0]).round().astype(np.uint8)
    return Image.fromarray(out, "RGBA")


def parse_rgb(text, option="--background"):
    text = text.lstrip("#")
    if len(text) != 6:
        sys.exit(f"make_icon: {option} {text!r} is not RRGGBB")
    try:
        return tuple(int(text[i : i + 2], 16) for i in (0, 2, 4))
    except ValueError:
        sys.exit(f"make_icon: {option} {text!r} is not RRGGBB")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--source", required=True,
                    help="square image (PNG, JPEG, ...), 1024 px recommended")
    ap.add_argument("--key", help="RRGGBB, or 'auto' from the corners: the colour to make transparent")
    ap.add_argument("--key-tolerance", type=float, default=0.03,
                    help="0..1: how close to the key still counts as transparent (default 0.03)")
    ap.add_argument("--out", required=True, help="output directory")
    ap.add_argument("--background", default=DEFAULT_BACKGROUND, help="RRGGBB")
    ap.add_argument("--hue", type=float, default=0.0,
                    help="degrees to turn the artwork's hue: positive or negative, 0 for none")
    ap.add_argument("--full-bleed", action="store_true",
                    help="the source fills the 108 dp adaptive canvas; use it unchanged")
    ap.add_argument("--no-monochrome", action="store_true",
                    help="leave the themed-icon layer out")
    args = ap.parse_args()

    src = load_source(args.source)
    if args.key:
        if not 0.0 <= args.key_tolerance < 1.0:
            sys.exit("make_icon: --key-tolerance must be at least 0 and below 1")
        key = corner_key(src) if args.key == "auto" else parse_rgb(args.key, "--key")
        print("make_icon: key #%02X%02X%02X" % tuple(int(round(x)) for x in key))
        src = unmix_key(src, key, args.key_tolerance)
    elif not args.full_bleed and np.asarray(src.getchannel("A")).min() == 255:
        print("make_icon: warning: the source has no transparency; the whole square is "
              "the artwork (use --key to drop a flat background)", file=sys.stderr)
    art = turn_hue(premul_resize(src, (MASTER, MASTER)), args.hue)
    bg = parse_rgb(args.background)
    hexbg = "%02X%02X%02X" % bg
    out = args.out

    # The adaptive foreground: 108 dp canvas, drawn at 4x of xxxhdpi.
    fbig = 4 * 432
    if args.full_bleed:
        foreground = premul_resize(art, (fbig, fbig))
        # What shows of it: the centre 72 dp, which the masters use.
        cut = round(MASTER * 18 / 108)
        visible = premul_resize(art.crop((cut, cut, MASTER - cut, MASTER - cut)), (MASTER, MASTER))
    else:
        foreground = place(art, fbig, fbig * (SAFE_DP / 2) / ADAPTIVE_DP, fbig / 2)
        visible = place(art, MASTER, MASTER * (SAFE_DP / 2) / 72, MASTER / 2)

    # Masters: the visible artwork on its rounded square.
    master = over(flat(MASTER, bg, rounded_mask(MASTER)), visible)
    write_png(master, out, "icon-1024.png")
    write_png(premul_resize(master, (512, 512)), out, "icon-512.png")

    # Legacy icons, drawn at 4x of xxxhdpi and downscaled. The shape fills
    # LEGACY_FILL of the canvas.
    big = 4 * 192
    shape = round(big * LEGACY_FILL)
    pad = (big - shape) // 2
    legacy = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    legacy.alpha_composite(premul_resize(master, (shape, shape)), (pad, pad))
    # Round: a background circle with the visible artwork inside it.
    circle = flat(shape, bg, circle_mask(shape))
    circle.alpha_composite(premul_resize(visible, (shape, shape)).copy(), (0, 0))
    circle.putalpha(Image.fromarray(np.minimum(
        np.asarray(circle.getchannel("A")), np.asarray(circle_mask(shape)))))
    round_icon = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    round_icon.alpha_composite(circle, (pad, pad))

    sizes = []
    for name, k in DENSITIES.items():
        px = round(LEGACY_DP * k)
        fpx = round(ADAPTIVE_DP * k)
        d = f"mipmap-{name}"
        outs = (
            premul_resize(legacy, (px, px)),
            premul_resize(round_icon, (px, px)),
            premul_resize(foreground, (fpx, fpx)),
        )
        write_png(outs[0], out, "android", "res", d, "ic_launcher.png")
        write_png(outs[1], out, "android", "res", d, "ic_launcher_round.png")
        write_png(outs[2], out, "android", "res", d, "ic_launcher_foreground.png")
        sizes.append((name, px, fpx, outs))
    xml = ADAPTIVE_XML.format(monochrome="" if args.no_monochrome else MONOCHROME_XML)
    write_text(xml, out, "android", "res", "mipmap-anydpi-v26", "ic_launcher.xml")
    write_text(xml, out, "android", "res", "mipmap-anydpi-v26", "ic_launcher_round.xml")
    write_text(COLOR_XML.format(hex=hexbg), out, "android", "res", "values",
               "ic_launcher_background.xml")

    preview(out, master, foreground, bg, sizes)
    print(f"make_icon: wrote {out} (background #{hexbg}, hue {args.hue:+g})")


def adaptive_preview(foreground, bg, size, shape):
    """The adaptive icon as a launcher would show it: 72 of 108 dp visible,
    masked to a circle or a rounded square."""
    full = Image.new("RGBA", foreground.size, bg + (255,))
    full.alpha_composite(foreground)
    n = foreground.width
    cut = round(n * (18 / 108))
    vis = full.crop((cut, cut, n - cut, n - cut))
    vis = premul_resize(vis, (size, size))
    vis.putalpha(circle_mask(size) if shape == "circle" else rounded_mask(size, corner=0.30))
    return vis


def preview(out, master, foreground, bg, sizes):
    """Contact sheet: the same panel on a light and on a dark backdrop. Each
    panel: the master (shown at 512), the adaptive icon under a circle and a
    rounded-square mask; every density's legacy square and round icon at
    actual size; every density's adaptive foreground at actual size, on the
    background colour."""
    pad, cap = 24, 16
    W = 1420
    panel_h = (cap + 512 + pad) + (cap + 192 + pad) + (cap + 432 + pad)
    sheet = Image.new("RGBA", (W, 2 * panel_h), (236, 236, 240, 255))
    sheet.alpha_composite(Image.new("RGBA", (W, panel_h), (40, 40, 48, 255)), (0, panel_h))
    draw = ImageDraw.Draw(sheet)

    for top, fill in ((0, (20, 20, 20)), (panel_h, (230, 230, 230))):

        def put(img, x, y, text, fill=fill):
            sheet.alpha_composite(img, (x, y + cap))
            draw.text((x, y + 2), text, fill=fill)

        y = top + pad // 2
        x = pad
        put(premul_resize(master, (512, 512)), x, y, "icon-1024 (shown at 512)")
        x += 512 + pad
        put(adaptive_preview(foreground, bg, 192, "circle"), x, y, "adaptive, circle mask")
        put(
            adaptive_preview(foreground, bg, 192, "rounded"),
            x,
            y + 192 + cap + pad,
            "adaptive, rounded mask",
        )

        y += cap + 512 + pad
        x = pad
        for name, px, _fpx, outs in sizes:
            base = y + (192 - px)
            put(outs[0], x, base, f"{name} {px}")
            put(outs[1], x + px + 8, base, "round")
            x += 2 * px + 8 + pad

        y += cap + 192 + pad
        x = pad
        for name, _px, fpx, outs in sizes:
            fgv = Image.new("RGBA", outs[2].size, bg + (255,))
            fgv.alpha_composite(outs[2])
            put(fgv, x, y + (432 - fpx), f"foreground {name} {fpx}")
            x += fpx + pad
    write_png(sheet.convert("RGB"), out, "preview.png")


if __name__ == "__main__":
    main()
