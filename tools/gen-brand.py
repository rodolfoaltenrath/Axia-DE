#!/usr/bin/env python3
# gen-brand.py — the LordOS logo (assets/brand/*.svg) and its PNG renders (assets/icons/).
#
#   python3 tools/gen-brand.py          render the PNGs from the SVGs (needs rsvg-convert:
#                                       `brew install librsvg` or `apt install librsvg2-bin`)
#   python3 tools/gen-brand.py --svg    also rewrite the SVGs first (needs fontTools: `pip install fonttools`)
#
# The SVGs are real vectors, drawn here by hand: the crown is a mirrored half (exactly symmetric),
# stroke-free shapes with 2-3 gradient layers for the gloss, gem facets as separate polygons. The
# wordmark ("Lord" Plus Jakarta Sans ExtraBold + "OS" Medium, assets/fonts, SIL OFL 1.1) is
# converted to paths from those files, so the SVGs don't depend on installed fonts. Sizes of 32 px and less use the pixel-grid
# variants (app-icon-small.svg, crown-mono-small.svg). See docs/brand/README.md.
import os, shutil, subprocess, sys

REPO = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
OUT = os.path.join(REPO, 'assets/brand')
ICONS = os.path.join(REPO, 'assets/icons')
# The wordmark face: Plus Jakarta Sans (SIL OFL 1.1, assets/fonts), "Lord" ExtraBold + "OS" Medium.
# The same files are the 'brand' fontFamily of the UI (LordOSWordmark in sdk/logo.js).
WORD_BOLD, WORD_REGULAR = 'PlusJakartaSans-ExtraBold.ttf', 'PlusJakartaSans-Medium.ttf'
TRACK = -0.01                      # letter spacing, in em (the UI uses the same)


def author():
    from fontTools.ttLib import TTFont
    from fontTools.pens.svgPathPen import SVGPathPen
    from fontTools.pens.transformPen import TransformPen
    os.makedirs(OUT, exist_ok=True)
    def n(v):
        return ('%.1f' % v).rstrip('0').rstrip('.')


    def P(*pts):
        return ' '.join(f'{n(x)} {n(y)}' for x, y in pts)


    # ---- crown silhouette, 512 design box. Left half from the bottom centre to the apex ----
    # each segment: ('C', c1, c2, end) | ('Q', c, end) | ('L', end)
    START = (256, 446)
    LEFT = [
        ('C', (200, 446), (166, 442), (148, 436)),     # bottom rim, left
        ('Q', (124, 430), (122, 404)),                 # rounded bottom corner
        ('L', (72, 222)),                              # outer side, leaning out a little
        ('Q', (66, 186), (96, 206)),                  # left tip
        ('C', (126, 232), (156, 256), (178, 264)),     # inner edge, bowed in
        ('Q', (192, 268), (199, 254)),                 # valley
        ('C', (219, 228), (234, 188), (243, 154)),     # middle peak, left edge
        ('Q', (256, 116), (269, 154)),                 # apex (its end is the mirror of the start)
    ]


    def mirror(p):
        return (512 - p[0], p[1])


    def crown_path():
        d = [f'M{P(START)}']
        pts = [START]
        for s in LEFT:
            d.append(s[0] + P(*s[1:]))
            pts.append(s[-1])
        # right half: walk the left segments backwards, mirrored (the apex Q already ends on the right)
        for i in range(len(LEFT) - 2, -1, -1):
            s, a = LEFT[i], pts[i]
            if s[0] == 'C':
                d.append('C' + P(mirror(s[2]), mirror(s[1]), mirror(a)))
            elif s[0] == 'Q':
                d.append('Q' + P(mirror(s[1]), mirror(a)))
            else:
                d.append('L' + P(mirror(a)))
        return ''.join(d) + 'Z'


    BODY = crown_path()
    # violet band: a gentle smile, clipped to the body
    BAND = 'M60 350Q256 398 452 350L452 380Q256 428 60 380Z'
    RIM = 'M60 380Q256 428 452 380L452 470L60 470Z'
    GEMS = [(256, 104, 37, 43), (66, 180, 30, 35), (446, 180, 30, 35)]


    def gem(cx, cy, w, h, tones=('#FFF1A8', '#FFD60A', '#FFC300', '#FFB000', '#FFE45C')):
        t, r, b, l, c = (cx, cy - h), (cx + w, cy), (cx, cy + h), (cx - w, cy), (cx, cy)
        k = 0.42                                        # the central table facet
        tt, tr_, tb, tl = (cx, cy - h * k), (cx + w * k, cy), (cx, cy + h * k), (cx - w * k, cy)
        tri = lambda col, *p: f'<path fill="{col}" d="M{P(*p)}Z"/>'
        return (tri(tones[0], l, t, c) + tri(tones[1], t, r, c) + tri(tones[2], l, c, b) + tri(tones[3], c, r, b)
                + tri(tones[4], tt, tr_, tb, tl))


    def gem_outline(cx, cy, w, h, grow=0):
        w, h = w + grow, h + grow * 1.2
        return f'M{P((cx, cy - h), (cx + w, cy), (cx, cy + h), (cx - w, cy))}Z'


    def crown_color(p=''):
        defs = (f'<clipPath id="{p}k"><path d="{BODY}"/></clipPath>'
                # layer 1: body base, white to lavender
                f'<linearGradient id="{p}b" x1="0" y1="0" x2="1" y2="1"><stop offset=".15" stop-color="#fff"/><stop offset=".6" stop-color="#EEEAFF"/><stop offset="1" stop-color="#BDB1F5"/></linearGradient>'
                # layer 2: shade towards the band and the right face
                f'<linearGradient id="{p}s" x1="0" y1="0" x2="0" y2="1"><stop offset=".45" stop-color="#7D6BEB" stop-opacity="0"/><stop offset=".78" stop-color="#7D6BEB" stop-opacity=".28"/></linearGradient>'
                # layer 3: gloss on the left face
                f'<linearGradient id="{p}g" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#fff" stop-opacity=".95"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>'
                f'<linearGradient id="{p}v" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#5B3FE0"/><stop offset=".5" stop-color="#7C5CFF"/><stop offset="1" stop-color="#5B3FE0"/></linearGradient>'
                f'<linearGradient id="{p}r" x1="0" y1="0" x2="0" y2="1"><stop offset=".3" stop-color="#fff"/><stop offset="1" stop-color="#CFC6F7"/></linearGradient>')
        g = (f'<g clip-path="url(#{p}k)"><path fill="url(#{p}b)" d="{BODY}"/><path fill="url(#{p}s)" d="{BODY}"/>'
             f'<path fill="url(#{p}g)" d="M80 222Q140 256 186 272Q160 300 154 372L122 372Z"/>'
             f'<path fill="url(#{p}g)" opacity=".6" d="M256 150Q238 220 206 268Q236 258 256 246Z"/>'
             f'<path fill="url(#{p}r)" d="{RIM}"/>'
             f'<path fill="url(#{p}v)" d="{BAND}"/>'
             f'<path fill="#A996FF" opacity=".55" d="M60 352Q256 400 452 352L452 358Q256 406 60 358Z"/></g>')
        g += ''.join(gem(*x) for x in GEMS)
        return defs, g


    def crown_mono(color, p='m'):
        """Flat crown for small sizes: one colour, band and gem gaps cut out with a mask."""
        cut = ''.join(f'<path d="{gem_outline(*x, grow=13)}"/>' for x in GEMS)
        defs = (f'<mask id="{p}" maskUnits="userSpaceOnUse" x="0" y="0" width="512" height="512">'
                f'<rect width="512" height="512" fill="#fff"/><g fill="black"><path d="{BAND}"/>{cut}</g></mask>')
        gems = ''.join(f'<path d="{gem_outline(*x, grow=2)}"/>' for x in GEMS)
        return defs, f'<g fill="{color}"><path mask="url(#{p})" d="{BODY}"/>{gems}</g>'


    def svg(w, h, defs, body, title):
        return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {n(w)} {n(h)}" width="{n(w)}" height="{n(h)}">'
                f'<title>{title}</title>\n<defs>{defs}</defs>\n{body}\n</svg>\n')


    def write(name, text):
        with open(os.path.join(OUT, name), 'w') as f:
            f.write(text)
        print(name, len(text.encode()), 'bytes')


    # ---- wordmark: "Lord" ExtraBold + "OS" Medium (WORD_BOLD, WORD_REGULAR), outlined ----
    FONTS = os.path.join(REPO, 'assets/fonts')


    def outline(font, text, x, baseline, size, track=TRACK):
        f = TTFont(os.path.join(FONTS, font))
        cmap, gs, hmtx = f.getBestCmap(), f.getGlyphSet(), f['hmtx']
        sc = size / f['head'].unitsPerEm
        out = ''
        for ch in text:
            gn = cmap[ord(ch)]
            pen = SVGPathPen(gs, ntos=n)
            gs[gn].draw(TransformPen(pen, (sc, 0, 0, -sc, x, baseline)))
            out += pen.getCommands()
            x += hmtx[gn][0] * sc + track * size
        return out, x


    def lockup(name, ink, mono=None):
        # crown 1.6x the cap height, centred on the capitals, as in the original lockup
        size = 150
        a, x = outline(WORD_BOLD, 'Lord', 238, 184, size)
        b, x = outline(WORD_REGULAR, 'OS', x, 184, size)
        d, body = crown_mono(mono) if mono else crown_color()
        g = f'<g transform="matrix(.437 0 0 .437 -10 19)">{body}</g>\n<path fill="{ink}" d="{a}{b}"/>'
        write(name, svg(round(x + 16), 240, d, g, 'LordOS'))
        return d, g, round(x + 16)


    ICON_BG = ('<linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#B46CFF"/><stop offset="1" stop-color="#4F46E5"/></linearGradient>'
               '<radialGradient id="gl" cx=".22" cy=".12" r=".75"><stop offset="0" stop-color="#fff" stop-opacity=".25"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient>'
               '<radialGradient id="sh" cx=".5" cy=".5" r=".5"><stop offset="0" stop-color="#1E1660" stop-opacity=".5"/><stop offset="1" stop-color="#1E1660" stop-opacity="0"/></radialGradient>')
    d, body = crown_color()
    write('crown.svg', svg(512, 512, d, body, 'LordOS crown'))
    d, body = crown_mono('#000')
    write('crown-mono.svg', svg(512, 512, d, body, 'LordOS crown, one colour'))

    d, body = crown_color('c')
    icon = ('<rect width="512" height="512" rx="113" fill="url(#bg)"/><rect width="512" height="512" rx="113" fill="url(#gl)"/>'
            '<ellipse cx="256" cy="416" rx="160" ry="30" fill="url(#sh)"/>'
            f'<g transform="matrix(.72 0 0 .72 71.7 84)">{body}</g>')
    write('app-icon.svg', svg(512, 512, ICON_BG + d, icon, 'LordOS'))
    lockup('logo-horizontal-light.svg', '#12122A')
    lockup('logo-horizontal-dark.svg', '#fff')
    lockup('logo-mono-white.svg', '#fff', mono='#fff')
    lockup('logo-mono-black.svg', '#000', mono='#000')

    # ---- social card 1200 x 630: the dark lockup on the icon's violet gradient ----
    d, g, w = lockup('logo-horizontal-dark.svg', '#fff')
    k = 820 / w                                    # the lockup 820 px wide, centred
    card = ('<rect width="1200" height="630" fill="url(#bg)"/><rect width="1200" height="630" fill="url(#gl)"/>'
            f'<g transform="matrix({n(k)} 0 0 {n(k)} {n((1200 - 820) / 2)} {n(315 - 128 * k)})">{g}</g>')
    write('social-card.svg', svg(1200, 630, ICON_BG + d, card, 'LordOS'))

    # ---- small sizes: pixel-grid geometry (32 grid for the icon, 16 grid for the menu bar glyph) ----
    small_icon = (
        '<rect width="32" height="32" rx="7" fill="url(#bg)"/>'
        '<path fill="#F4F1FF" d="M7 11L12 15L16 9L20 15L25 11L24 23L8 23Z"/>'
        '<path fill="#fff" d="M7 11L12 15L11 23L8 23Z"/>'
        '<rect x="8" y="20" width="16" height="2" fill="#6A4CF0"/>'
        '<path fill="#DCD5FA" d="M8 22H24V24Q24 25 23 25H9Q8 25 8 24Z"/>'
        '<path fill="#FFD60A" d="M16 4L18 6.5L16 9L14 6.5Z"/>'
        '<path fill="#FFC300" d="M7 7.5L8.5 9.5L7 11.5L5.5 9.5ZM25 7.5L26.5 9.5L25 11.5L23.5 9.5Z"/>')
    write('app-icon-small.svg', svg(32, 32, ICON_BG.split('<radialGradient')[0], small_icon, 'LordOS (16 to 32 px)'))
    small_mono = ('<path d="M1.5 5.5L5 9L8 5L11 9L14.5 5.5L12.5 11.5H3.5Z"/><rect x="3.5" y="12.5" width="9" height="2" rx=".5"/>'
                  '<path d="M8 1L9.5 2.75L8 4.5L6.5 2.75Z"/><path d="M1.5 2.5L2.75 3.75L1.5 5L.25 3.75ZM14.5 2.5L15.75 3.75L14.5 5L13.25 3.75Z"/>')
    write('crown-mono-small.svg', svg(16, 16, '', f'<g fill="#000">{small_mono}</g>', 'LordOS crown, one colour (16 px)'))


# PNG renders: (output in assets/icons, source svg, width, recolour)
RENDERS = [
    ('lordos-crown.png', 'crown.svg', 256, None),          # existing consumers (tools/demo, ...)
    ('lordos-crown-512.png', 'crown.svg', 512, None),
    *[(f'lordos-icon-{s}.png', 'app-icon-small.svg' if s <= 32 else 'app-icon.svg', s, None)
      for s in (16, 32, 64, 128, 256, 512, 1024)],
    # menu bar glyph, flat, in the two menu bar text colours (the UI can't tint a PNG)
    *[(f'lordos-glyph-{tone}-{s}.png', 'crown-mono-small.svg' if s <= 16 else 'crown-mono.svg', s, col)
      for tone, col in (('light', '#f5f5f7'), ('dark', '#1d1d1f')) for s in (16, 32)],
]


KIT = os.path.join(REPO, 'docs/brand')
# the brand kit for web pages and other uses (docs/brand/README.md): (file in docs/brand, source, width)
KIT_PNG = [
    *[(f'png/app-icon-{s}.png', 'app-icon-small.svg' if s <= 32 else 'app-icon.svg', s) for s in (16, 32, 64, 128, 256, 512, 1024)],
    ('png/crown-256.png', 'crown.svg', 256), ('png/crown-512.png', 'crown.svg', 512),
    *[(f'png/logo-horizontal-{t}-{w}.png', f'logo-horizontal-{t}.svg', w) for t in ('light', 'dark') for w in (400, 800)],
    *[(f'png/logo-mono-{t}-800.png', f'logo-mono-{t}.svg', 800) for t in ('white', 'black')],
    ('png/social-card-1200x630.png', 'social-card.svg', 1200),
    ('favicon/favicon-32.png', 'app-icon-small.svg', 32),
    ('favicon/apple-touch-icon-180.png', 'app-icon.svg', 180),
    ('favicon/favicon-512.png', 'app-icon.svg', 512),
]
KIT_SVG = ['crown.svg', 'crown-mono.svg', 'crown-mono-small.svg', 'app-icon.svg', 'app-icon-small.svg',
           'logo-horizontal-light.svg', 'logo-horizontal-dark.svg', 'logo-mono-white.svg', 'logo-mono-black.svg',
           'social-card.svg']


# colour chips for docs/LORDOS-BRAND.md: (file, fill or two-stop gradient)
SWATCHES = [('violet-light', '#B46CFF'), ('indigo', '#4F46E5'), ('icon-gradient', ('#B46CFF', '#4F46E5')),
            ('band-dark', '#5B3FE0'), ('band-light', '#7C5CFF'), ('gem-yellow', '#FFD60A'), ('gem-amber', '#FFC300'),
            ('gem-orange', '#FFB000'), ('gem-highlight', '#FFF1A8'), ('crown-lavender', '#BDB1F5'),
            ('navy', '#12122A'), ('white', '#FFFFFF')]


def swatches():
    os.makedirs(os.path.join(KIT, 'swatches'), exist_ok=True)
    for name, fill in SWATCHES:
        if isinstance(fill, tuple):
            defs = (f'<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{fill[0]}"/>'
                    f'<stop offset="1" stop-color="{fill[1]}"/></linearGradient></defs>')
            fill = 'url(#g)'
        else:
            defs = ''
        with open(os.path.join(KIT, 'swatches', name + '.svg'), 'w') as f:
            f.write(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 32" width="64" height="32">{defs}'
                    f'<rect x=".5" y=".5" width="63" height="31" rx="6" fill="{fill}" stroke="#8884" /></svg>\n')
    print('docs/brand/swatches/*.svg')


def render():
    rsvg = shutil.which('rsvg-convert')
    if not rsvg:
        sys.exit('gen-brand: rsvg-convert not found (brew install librsvg / apt install librsvg2-bin)')
    jobs = [(os.path.join(ICONS, out), src, w, col) for out, src, w, col in RENDERS]
    jobs += [(os.path.join(KIT, out), src, w, None) for out, src, w in KIT_PNG]
    for path, src, w, col in jobs:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        svg = open(os.path.join(OUT, src), 'rb').read()
        if col:
            svg = svg.replace(b'fill="#000"', f'fill="{col}"'.encode())
        size = ['-w', str(w)] if w else ['-h', '56']
        subprocess.run([rsvg, *size, '-o', path], input=svg, check=True)
        print(os.path.relpath(path, REPO))
    # vector copies for the kit (favicon.svg is the pixel-grid icon: crisp in a browser tab)
    os.makedirs(os.path.join(KIT, 'svg'), exist_ok=True)
    for name in KIT_SVG:
        shutil.copyfile(os.path.join(OUT, name), os.path.join(KIT, 'svg', name))
    shutil.copyfile(os.path.join(OUT, 'app-icon-small.svg'), os.path.join(KIT, 'favicon', 'favicon.svg'))
    print('docs/brand/svg/*.svg, docs/brand/favicon/favicon.svg')
    swatches()


if __name__ == '__main__':
    if '--svg' in sys.argv:
        author()
    render()
