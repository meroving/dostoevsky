#!/usr/bin/env python3
"""The app icon, drawn from numbers (dsky-wxv): an axe, close up, leaning into the plate.

    scripts/make_icon.py                 # packaging/icon.svg, icon-1024.png, Dostoevsky.icns,
                                         # crates/app/assets/icon.png (About), docs/icon.png (README, site),
                                         # favicons and the touch icon in target/icon
    scripts/make_icon.py --look night    # the same axe in another look (see LOOKS); writes the same files
    scripts/make_icon.py --sheet         # also target/icon/sheet.png: every size, the small ones enlarged
    scripts/make_icon.py --looks         # only target/icon/looks.png: every look side by side, nothing replaced

Coordinates are Apple's macOS grid: a 1024 canvas with an 824 plate. The 16, 32 and 64 px
sizes are drawings of their own: a heavier handle, a lighter shadow or none.
Needs Chrome to turn SVG into PNG, and iconutil.
The look in use is F7 of the icon-axe-4 mockup.
"""
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "target/icon"
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
NAVY, CREAM, PURPLE, WHITE = "#070c1f", "#f5cab3", "#8a80d8", "#fbf3ea"

# The look in use, and the owner's second and third choices (2026-11-13), kept so that changing
# is one word here or one flag. A plate runs from `top` to `bottom`; `shadow` is its opacity.
LOOK = "violet"
LOOKS = {
    "violet": dict(top="#9d94e6", bottom="#5a4fc8", blade=WHITE, handle=NAVY, shadow=0.35),   # F7
    "night": dict(top="#1c2452", bottom="#070b1c", blade=CREAM, handle=PURPLE, shadow=0),      # F2
    "deep": dict(top="#7268d2", bottom="#3d3399", blade=CREAM, handle=NAVY, shadow=0),         # F4
}

# The axe upright, blade to the right, in its own coordinates (about -45…255 wide, -320…350 tall).
HANDLE = ("M-25 -306 C-25 -318 25 -318 25 -306 C27 -120 19 60 25 200 C29 262 46 300 42 334 "
          "C40 354 -32 354 -36 334 C-38 300 -25 252 -23 200 C-19 60 -27 -120 -25 -306 Z")
HEAD = "M20 -272 C90 -276 160 -290 222 -318 C264 -230 264 -90 204 14 C170 -50 110 -108 20 -118 Z"
POLL = '<rect x="-44" y="-282" width="76" height="174" rx="17"'
# Where it stands on the canvas: turned 28° and nearly twice life size, so the blade's edge and
# the handle's foot leave the plate.
POSE = "translate(318 598) rotate(28) scale(1.95)"

# What changes with size: `fat` thickens the handle (in the axe's own units, each side),
# `shadow` scales the look's shadow, `blur` is its softness.
SIZES = {
    "big": dict(fat=0, shadow=1.0, blur=20),     # 128 px and up
    "64": dict(fat=3, shadow=0.8, blur=14),
    "32": dict(fat=8, shadow=0, blur=0),
    "16": dict(fat=18, shadow=0, blur=0),
}
# iconutil's names → (pixels, drawing)
ICONSET = [("16x16", 16, "16"), ("16x16@2x", 32, "32"), ("32x32", 32, "32"), ("32x32@2x", 64, "64"),
           ("128x128", 128, "big"), ("128x128@2x", 256, "big"), ("256x256", 256, "big"),
           ("256x256@2x", 512, "big"), ("512x512", 512, "big"), ("512x512@2x", 1024, "big")]


def plate():
    """Apple's rounded square: 824 wide, corners that ease in (not a plain arc)."""
    l, t, r, b, k = 100.0, 100.0, 924.0, 924.0, 185.4
    a, c1, c2, e1x, e1y, f1, f2 = 1.528665, 1.08849296, 0.86840694, 0.63149379, 0.07491139, 0.37282383, 0.16905956

    def corner(ox, oy, sx, sy, back):
        # one corner, leaving the horizontal edge for the vertical one (back: the other way round)
        pts = [(c1, 0), (c2, 0), (e1x, e1y), (f1, f2), (f2, f1), (e1y, e1x), (0, c2), (0, c1), (0, a)]
        if back:
            pts = list(reversed(pts[:-1])) + [(a, 0)]
        out = []
        for i in range(0, 9, 3):
            out.append("C" + " ".join(f"{ox + sx * x * k:.2f} {oy + sy * y * k:.2f}" for x, y in pts[i:i + 3]))
        return " ".join(out)

    d = f"M{l + a * k:.2f} {t} L{r - a * k:.2f} {t} " + corner(r, t, -1, 1, False)
    d += f" L{r} {b - a * k:.2f} " + corner(r, b, -1, -1, True)
    d += f" L{l + a * k:.2f} {b} " + corner(l, b, 1, -1, False)
    d += f" L{l} {t + a * k:.2f} " + corner(l, t, 1, 1, True) + " Z"
    return d


def icon(look=None, size="big", trim=False, square=False):
    """One drawing. `trim`: the plate to the picture's edge, without the Dock's margin.
    `square`: no corners either (iOS rounds a touch icon itself)."""
    c, z = LOOKS[look or LOOK], SIZES[size]
    defs = (f'<linearGradient id="plate" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{c["top"]}"/>'
            f'<stop offset="1" stop-color="{c["bottom"]}"/></linearGradient>')
    fat = f' stroke="{c["handle"]}" stroke-width="{2 * z["fat"]}" stroke-linejoin="round"' if z["fat"] else ""
    axe = (f'<g transform="{POSE}"><path d="{HANDLE}" fill="{c["handle"]}"{fat}/>'
           f'<path d="{HEAD}" fill="{c["blade"]}"/>{POLL} fill="{c["blade"]}"/></g>')
    shadow = c["shadow"] * z["shadow"]
    if shadow:
        defs += (f'<filter id="shadow" x="-30%" y="-30%" width="160%" height="170%"><feDropShadow dx="0" dy="22" '
                 f'stdDeviation="{z["blur"]}" flood-color="#05081a" flood-opacity="{shadow:.2f}"/></filter>')
        axe = f'<g filter="url(#shadow)">{axe}</g>'
    box = "100 100 824 824" if trim or square else "0 0 1024 1024"
    back = '<rect x="100" y="100" width="824" height="824" fill="url(#plate)"/>'
    if square:
        return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{box}"><defs>{defs}</defs>{back}{axe}</svg>'
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{box}">'
            f'<defs><clipPath id="clip"><path d="{plate()}"/></clipPath>{defs}</defs>'
            f'<g clip-path="url(#clip)">{back}{axe}</g></svg>')


def render(svg_path, png_path, px):
    """SVG → PNG at exactly px × px, transparent outside the plate."""
    page = OUT / "page.html"
    page.write_text(f'<!doctype html><body style="margin:0;background:transparent">'
                    f'<img src="{svg_path}" width="{px}" height="{px}" style="display:block">')
    subprocess.run([CHROME, "--headless=new", "--disable-gpu", "--hide-scrollbars",
                    "--force-device-scale-factor=1", "--default-background-color=00000000",
                    "--allow-file-access-from-files", f"--window-size={px},{px}",
                    f"--screenshot={png_path}", f"file://{page}"],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def shoot(html, png, width, height):
    """A page of pictures → one PNG, for looking at."""
    page = OUT / "sheet.html"
    page.write_text('<!doctype html><meta charset="utf-8"><style>body{margin:16px;background:#1d2030;color:#aaa;'
                    'font:12px monospace;display:flex;flex-wrap:wrap;gap:16px;align-items:flex-end}figure{margin:0}'
                    f'img{{display:block;image-rendering:pixelated}}</style>{html}')
    subprocess.run([CHROME, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=1",
                    "--allow-file-access-from-files", f"--window-size={width},{height}",
                    f"--screenshot={png}", f"file://{page}"],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    look = LOOK
    if "--look" in sys.argv:
        look = sys.argv[sys.argv.index("--look") + 1]
        if look not in LOOKS:
            sys.exit(f"no such look: {look} (there are: {', '.join(LOOKS)})")

    if "--looks" in sys.argv:
        cells = ""
        for name in LOOKS:
            for size, px, zoom in (("big", 256, 1), ("32", 32, 4), ("16", 16, 8)):
                svg = OUT / f"look-{name}-{size}.svg"
                svg.write_text(icon(name, size))
                render(svg, OUT / f"look-{name}-{size}.png", px)
                cells += (f'<figure><img src="{OUT}/look-{name}-{size}.png" width="{px * zoom}" height="{px * zoom}">'
                          f'<figcaption>{name}{" (in use)" if name == LOOK else ""} {px} px</figcaption></figure>')
        shoot(cells, OUT / "looks.png", 1800, 330)
        print("target/icon/looks.png")
        return

    iconset = OUT / "Dostoevsky.iconset"
    iconset.mkdir(exist_ok=True)
    svgs = {}
    for size in SIZES:
        svgs[size] = OUT / f"icon-{size}.svg"
        svgs[size].write_text(icon(look, size))
    (ROOT / "packaging/icon.svg").write_text(icon(look) + "\n")
    for name, px, size in ICONSET:
        render(svgs[size], iconset / f"icon_{name}.png", px)
    subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(ROOT / "packaging/Dostoevsky.icns")], check=True)
    render(svgs["big"], ROOT / "packaging/icon-1024.png", 1024)
    render(svgs["big"], ROOT / "crates/app/assets/icon.png", 256)            # About: the plate with its margin
    trimmed = OUT / "icon-trimmed.svg"
    trimmed.write_text(icon(look, trim=True))
    render(trimmed, ROOT / "docs/icon.png", 256)                             # README, site: the plate to the edge
    # A browser tab has 16 points for it: the small drawings, without the margin.
    for name, px, size in (("favicon-16", 16, "16"), ("favicon-32", 32, "32"), ("favicon", 64, "64")):
        svg = OUT / f"{name}.svg"
        svg.write_text(icon(look, size, trim=True))
        render(svg, OUT / f"{name}.png", px)
    touch = OUT / "apple-touch-icon.svg"
    touch.write_text(icon(look, square=True))
    render(touch, OUT / "apple-touch-icon.png", 180)
    # The site's builder copies these favicons from target/icon/.
    if "--sheet" in sys.argv:
        cells = "".join(
            f'<figure><img src="{iconset}/icon_{n}.png" width="{px * z}" height="{px * z}"><figcaption>{n} ×{z}</figcaption></figure>'
            for n, px, z in (("512x512", 512, 1), ("128x128", 128, 2), ("32x32@2x", 64, 6), ("32x32", 32, 12), ("16x16", 16, 24)))
        cells += "".join(
            f'<figure><img src="{OUT}/{n}.png" width="{px * z}" height="{px * z}"><figcaption>{n} ×{z}</figcaption></figure>'
            for n, px, z in (("favicon-32", 32, 6), ("favicon-16", 16, 12)))
        shoot(cells, OUT / "sheet.png", 1500, 1000)
    print(f"look: {look}. packaging/icon.svg, packaging/icon-1024.png, packaging/Dostoevsky.icns, "
          "crates/app/assets/icon.png, docs/icon.png, target/icon/favicon{,-32,-16}.png, target/icon/apple-touch-icon.png")


if __name__ == "__main__":
    main()
