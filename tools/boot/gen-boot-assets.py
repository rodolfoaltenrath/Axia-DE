#!/usr/bin/env python3
"""gen-boot-assets.py: renders the boot splash (Plymouth) and boot menu (GRUB) images.

Everything comes from our own vectors: the crown is assets/brand/crown.svg and the wordmark is the
text of assets/brand/logo-horizontal-dark.svg (already converted to paths, Plus Jakarta Sans). The
bar, the glow and the menu pieces are drawn here as small SVGs. Needs rsvg-convert.

    python3 tools/boot/gen-boot-assets.py

Writes tools/boot/plymouth/lordos/*.png (1x and @2x) and tools/iso/grub/lordos/*.png. The GRUB
fonts (*.pf2) are made with grub-mkfont, see docs/ISO.md, "Boot splash and boot menu".
See docs/brand/README.md, "Boot splash".
"""
import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PLY = os.path.join(ROOT, "tools/boot/plymouth/lordos")
GRUB = os.path.join(ROOT, "tools/iso/grub/lordos")
CROWN = os.path.join(ROOT, "assets/brand/crown.svg")
LOGO = os.path.join(ROOT, "assets/brand/logo-horizontal-dark.svg")

# brand colours (docs/LORDOS-BRAND.md)
BG_TOP, BG_BOTTOM = "#12122A", "#0B0B1C"
VIOLET, LILAC = "#6E4BFF", "#B46CFF"


def render(svg, out, w=None, h=None):
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as f:
        f.write(svg)
        src = f.name
    try:
        cmd = ["rsvg-convert", "-o", out]
        if w:
            cmd += ["-w", str(w)]
        if h:
            cmd += ["-h", str(h)]
        subprocess.run(cmd + [src], check=True)
    finally:
        os.unlink(src)


def render_file(src, out, w):
    subprocess.run(["rsvg-convert", "-w", str(w), "-o", out, src], check=True)


def svg(w, h, body, defs=""):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">'
            f"<defs>{defs}</defs>{body}</svg>")


def wordmark_svg():
    """The wordmark alone: the paths of logo-horizontal-dark.svg that come after the crown group."""
    s = open(LOGO).read()
    # the crown is the first <g transform=...>…</g> block (with its nested clip group); the
    # wordmark is the remaining top-level paths
    head, rest = s.split("<g transform=", 1)
    depth, i = 0, 0
    while True:
        o = rest.find("<g", i)
        c = rest.find("</g>", i)
        if o != -1 and o < c:
            depth += 1
            i = o + 2
        else:
            if depth == 0:
                tail = rest[c + 4:]
                break
            depth -= 1
            i = c + 4
    paths = "".join(re.findall(r"<path [^>]*/>", tail))
    if not paths:
        sys.exit("gen-boot-assets: wordmark paths not found in " + LOGO)
    # the text spans x 247..800, y 72..186 in the 812x240 lockup
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="244 66 562 126" width="562" height="126">'
            + paths + "</svg>")


def bar(w, h, fill, alpha=1.0):
    r = h / 2
    return svg(w, h, f'<rect width="{w}" height="{h}" rx="{r}" fill="{fill}" fill-opacity="{alpha}"/>')


def plymouth(scale):
    sfx = "" if scale == 1 else "@2x"
    p = lambda n: os.path.join(PLY, f"{n}{sfx}.png")
    s = scale
    render_file(CROWN, p("crown"), 112 * s)
    # soft violet glow behind the crown
    g = 420 * s
    render(svg(g, g, f'<circle cx="{g/2}" cy="{g/2}" r="{g/2}" fill="url(#r)"/>',
               f'<radialGradient id="r"><stop offset="0" stop-color="{VIOLET}" stop-opacity=".30"/>'
               f'<stop offset=".45" stop-color="{VIOLET}" stop-opacity=".12"/>'
               f'<stop offset="1" stop-color="{VIOLET}" stop-opacity="0"/></radialGradient>'), p("glow"))
    # the progress bar: track, the two round caps of the fill and its body (scaled by the script)
    bw, bh = 200 * s, 6 * s
    render(bar(bw, bh, "#FFFFFF", 0.12), p("track"))
    render(svg(bh, bh, f'<circle cx="{bh/2}" cy="{bh/2}" r="{bh/2}" fill="{LILAC}"/>'), p("cap-start"))
    render(svg(bh, bh, f'<circle cx="{bh/2}" cy="{bh/2}" r="{bh/2}" fill="{VIOLET}"/>'), p("cap-end"))
    render(svg(64, bh, '<rect width="64" height="%d" fill="url(#l)"/>' % bh,
               f'<linearGradient id="l"><stop offset="0" stop-color="{LILAC}"/>'
               f'<stop offset="1" stop-color="{VIOLET}"/></linearGradient>'), p("fill"))
    # the wordmark under the bar (white; the script sets its opacity)
    render(wordmark_svg(), p("wordmark"), h=12 * s)
    # the password field (encrypted disks) and its dots
    ew, eh = 280 * s, 40 * s
    render(svg(ew, eh, f'<rect x="{s/2}" y="{s/2}" width="{ew-s}" height="{eh-s}" rx="{12*s}" '
                       f'fill="#FFFFFF" fill-opacity=".07" stroke="#FFFFFF" stroke-opacity=".22" stroke-width="{s}"/>'),
           p("entry"))
    d = 8 * s
    render(svg(d, d, f'<circle cx="{d/2}" cy="{d/2}" r="{d/2}" fill="#FFFFFF"/>'), p("bullet"))
    # a small lock above the field
    render(svg(18, 22, '<path d="M5 9V6.5a4 4 0 0 1 8 0V9" fill="none" stroke="#FFFFFF" stroke-opacity=".8" stroke-width="2"/>'
                       '<rect x="2" y="9" width="14" height="11" rx="3" fill="#FFFFFF" fill-opacity=".8"/>'),
           p("lock"), w=18 * s)


def grub():
    W, H = 1920, 1080
    render(svg(W, H,
               f'<rect width="{W}" height="{H}" fill="url(#v)"/>'
               f'<ellipse cx="{W/2}" cy="{H*0.30}" rx="{H*0.42}" ry="{H*0.36}" fill="url(#r)"/>',
               f'<linearGradient id="v" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{BG_TOP}"/>'
               f'<stop offset="1" stop-color="{BG_BOTTOM}"/></linearGradient>'
               f'<radialGradient id="r"><stop offset="0" stop-color="{VIOLET}" stop-opacity=".20"/>'
               f'<stop offset=".5" stop-color="{VIOLET}" stop-opacity=".07"/>'
               f'<stop offset="1" stop-color="{VIOLET}" stop-opacity="0"/></radialGradient>'),
           os.path.join(GRUB, "background.png"))
    render_file(CROWN, os.path.join(GRUB, "crown.png"), 96)
    # 9-slice pieces of the selected entry (rounded, translucent violet), 12 px corners
    # GRUB draws the corners and edges *around* the item box, so they stay small (8 px)
    c = 8
    full = svg(3 * c, 3 * c, f'<rect width="{3*c}" height="{3*c}" rx="{c}" fill="{VIOLET}" fill-opacity=".34"/>'
                             f'<rect x=".5" y=".5" width="{3*c-1}" height="{3*c-1}" rx="{c-.5}" fill="none" '
                             f'stroke="{LILAC}" stroke-opacity=".45"/>')
    tmp = os.path.join(GRUB, ".select-full.png")
    render(full, tmp)
    crop9(tmp, "select", c)
    os.unlink(tmp)
    # the box GRUB clears while loading the kernel: navy instead of black
    render(svg(8, 8, f'<rect width="8" height="8" fill="{BG_BOTTOM}"/>'), os.path.join(GRUB, "terminal_box_c.png"))


def crop(src, out, x, y, w, h):
    """Crops with rsvg-convert itself (an SVG that embeds the PNG): no other image tools needed."""
    import base64
    data = base64.b64encode(open(src, "rb").read()).decode()
    iw, ih = png_size(src)
    render(f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
           f'width="{w}" height="{h}" viewBox="{x} {y} {w} {h}"><image width="{iw}" height="{ih}" '
           f'xlink:href="data:image/png;base64,{data}"/></svg>', out)


def png_size(p):
    import struct
    with open(p, "rb") as f:
        f.read(16)
        return struct.unpack(">II", f.read(8))


def crop9(src, name, c):
    parts = {"nw": (0, 0), "n": (c, 0), "ne": (2 * c, 0), "w": (0, c), "c": (c, c), "e": (2 * c, c),
             "sw": (0, 2 * c), "s": (c, 2 * c), "se": (2 * c, 2 * c)}
    for k, (x, y) in parts.items():
        crop(src, os.path.join(GRUB, f"{name}_{k}.png"), x, y, c, c)


def uri(path):
    import base64
    return "data:image/png;base64," + base64.b64encode(open(path, "rb").read()).decode()


def preview(out, progress=0.6, password=False, W=1920, H=1080):
    """A static picture of the splash, laid out exactly like lordos.script does at 1x."""
    img = lambda n: os.path.join(PLY, n + ".png")
    def place(name, x, y, opacity=1.0, w=None):
        iw, ih = png_size(img(name))
        w = iw if w is None else w
        return (f'<image x="{x}" y="{y}" width="{w}" height="{ih}" opacity="{opacity}" '
                f'preserveAspectRatio="none" xlink:href="{uri(img(name))}"/>')
    cw, ch = png_size(img("crown"))
    gw, gh = png_size(img("glow"))
    tw, th = png_size(img("track"))
    ww, wh = png_size(img("wordmark"))
    cx, cy = W / 2, H / 2
    crown_y = cy - ch / 2 - 40
    bar_y = crown_y + ch + 56
    body = [f'<rect width="{W}" height="{H}" fill="url(#v)"/>',
            place("glow", cx - gw / 2, crown_y + ch / 2 - gh / 2),
            place("crown", cx - cw / 2, crown_y)]
    if password:
        ew, eh = png_size(img("entry"))
        lw, lh = png_size(img("lock"))
        ey = bar_y - 8
        body += [place("lock", cx - lw / 2, ey - lh - 12, 1.0), place("entry", cx - ew / 2, ey)]
        bw, _ = png_size(img("bullet"))
        for i in range(8):
            body.append(place("bullet", cx - ew / 2 + 18 + i * (bw + 8), ey + eh / 2 - bw / 2))
    else:
        fw = max(th, round(tw * progress))
        x0 = cx - tw / 2
        body += [place("track", x0, bar_y),
                 place("fill", x0 + th / 2, bar_y, w=max(0, fw - th)),
                 place("cap-start", x0, bar_y),
                 place("cap-end", x0 + fw - th, bar_y)]
        body.append(place("wordmark", cx - ww / 2, bar_y + th + 28, 0.55))
    render(f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
           f'width="{W}" height="{H}"><defs><linearGradient id="v" x1="0" y1="0" x2="0" y2="1">'
           f'<stop offset="0" stop-color="{BG_TOP}"/><stop offset="1" stop-color="{BG_BOTTOM}"/></linearGradient></defs>'
           + "".join(body) + "</svg>", out)


def main():
    os.makedirs(PLY, exist_ok=True)
    os.makedirs(GRUB, exist_ok=True)
    for s in (1, 2):
        plymouth(s)
    grub()
    print("boot assets written to", os.path.relpath(PLY, ROOT), "and", os.path.relpath(GRUB, ROOT))
    if "--preview" in sys.argv:
        d = os.path.join(ROOT, "docs/boot")
        os.makedirs(d, exist_ok=True)
        preview(os.path.join(d, "splash-preview.png"))
        preview(os.path.join(d, "splash-preview-password.png"), password=True)
        print("previews written to docs/boot/")


if __name__ == "__main__":
    main()
