#!/usr/bin/env python3
# gen-mascot.py — Lumo, the LordOS mascot (assets/brand/mascot/*.svg) and its PNG renders.
#
#   python3 tools/gen-mascot.py           assemble every view and pose from the parts file, render the PNGs
#   python3 tools/gen-mascot.py --parts   first rewrite the parts file from the geometry in this script
#   python3 tools/gen-mascot.py --svg     only the SVGs (no PNGs)
#
# The parts (head, ears, eyes, crown, cape, paws, props...) are reusable <g id="..."> groups in
# assets/brand/mascot/src/lumo-parts.svg, drawn on the front view's 512 x 512 canvas. Every output
# is assembled from those groups with transforms, so the character stays the same everywhere; the
# output keeps only the gradients it uses. Hand edits to the parts file are kept unless you run
# --parts (which re-authors it from PARTS below). Paths marked data-mirror="256" hold the left half
# of a symmetric shape and are completed here, so they are exactly symmetric.
#
# The crown is assets/brand/crown.svg (same geometry, gradients and gem facets) with an outline.
# PNGs (needs rsvg-convert: `brew install librsvg` or `apt install librsvg2-bin`):
#   docs/brand/mascot/png/*.png   512 and 1024 for the views and poses; 64/128/256 for the avatar
#   assets/mascot/*.png           the sizes the UI draws (sdk/mascot.js)
# See docs/brand/README.md ("Lumo").
import os, re, shutil, subprocess, sys
import xml.etree.ElementTree as ET

REPO = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
OUT = os.path.join(REPO, 'assets/brand/mascot')
SRC = os.path.join(OUT, 'src/lumo-parts.svg')
DOCS = os.path.join(REPO, 'docs/brand/mascot')
UI = os.path.join(REPO, 'assets/mascot')
INK = '#2A2366'
SVGNS = 'http://www.w3.org/2000/svg'
ET.register_namespace('', SVGNS)


def n(v):
    s = ('%.2f' % v).rstrip('0').rstrip('.')
    return '0' if s == '-0' else s


def P(*pts):
    return ' '.join(f'{n(x)} {n(y)}' for x, y in pts)


# ---------------------------------------------------------------- path helpers
TOK = re.compile(r'[MCQLZ]|-?\d*\.?\d+(?:e-?\d+)?')


def parse(d):
    """Absolute M/C/Q/L/Z only -> [(cmd, [points])]."""
    toks, out, i = TOK.findall(d), [], 0
    want = {'M': 1, 'L': 1, 'Q': 2, 'C': 3, 'Z': 0}
    cmd = None
    while i < len(toks):
        t = toks[i]
        if t in want:
            cmd = t
            i += 1
            if cmd == 'Z':
                out.append(('Z', []))
                continue
        k = want[cmd]
        pts = [(float(toks[i + 2 * j]), float(toks[i + 2 * j + 1])) for j in range(k)]
        i += 2 * k
        out.append((cmd, pts))
        if cmd == 'M':
            cmd = 'L'
    return out


def fmt(segs):
    return ''.join(c + (P(*p) if p else '') for c, p in segs)


def mirror_half(d, axis=256.0):
    """Left half from the axis round to the axis -> the whole symmetric closed path."""
    segs = parse(d)
    segs = [s for s in segs if s[0] != 'Z']
    m = lambda p: (2 * axis - p[0], p[1])
    ends = [segs[0][1][0]] + [s[1][-1] for s in segs[1:]]
    back = []
    for i in range(len(segs) - 1, 0, -1):
        c, p = segs[i]
        a = m(ends[i - 1])
        if c == 'C':
            back.append(('C', [m(p[1]), m(p[0]), a]))
        elif c == 'Q':
            back.append(('Q', [m(p[0]), a]))
        else:
            back.append(('L', [a]))
    return fmt(segs + back) + 'Z'


def mirror_all(d, axis=256.0):
    return fmt([(c, [(2 * axis - x, y) for x, y in p]) for c, p in parse(d)])


# ---------------------------------------------------------------- the brand crown
CROWN = ('M256 446C200 446 166 442 148 436Q124 430 122 404L72 222Q66 186 96 206C126 232 156 256 178 264Q192 268 199 254'
         'C219 228 234 188 243 154Q256 116 269 154C278 188 293 228 313 254Q320 268 334 264C356 256 386 232 416 206'
         'Q446 186 440 222L390 404Q388 430 364 436C346 442 312 446 256 446Z')
GEMS = [(256, 104, 37, 43), (66, 180, 30, 35), (446, 180, 30, 35)]


def gem(cx, cy, w, h, edge=None, sw=1.6):
    t, r, b, l, c = (cx, cy - h), (cx + w, cy), (cx, cy + h), (cx - w, cy), (cx, cy)
    k = 0.42
    tt, tr_, tb, tl = (cx, cy - h * k), (cx + w * k, cy), (cx, cy + h * k), (cx - w * k, cy)
    tri = lambda col, *p: f'<path fill="{col}" d="M{P(*p)}Z"/>'
    o = f'<path fill="none" stroke="{edge}" stroke-width="{n(sw)}" stroke-linejoin="round" d="M{P(t, r, b, l)}Z"/>' if edge else ''
    return (o + tri('#FFF1A8', l, t, c) + tri('#FFD60A', t, r, c) + tri('#FFC300', l, c, b) + tri('#FFB000', c, r, b)
            + tri('#FFE45C', tt, tr_, tb, tl))


def crown_group(scale=0.34, base=(256, 150)):
    """The brand crown, its base centre at `base`, with a dark outline drawn behind it."""
    tx, ty = base[0] - 256 * scale, base[1] - 446 * scale
    sw = 3.2 / scale
    body = (f'<path fill="none" stroke="{INK}" stroke-width="{n(sw)}" stroke-linejoin="round" d="{CROWN}"/>'
            f'<g clip-path="url(#l-ck)"><path fill="url(#l-cb)" d="{CROWN}"/><path fill="url(#l-cs)" d="{CROWN}"/>'
            '<path fill="url(#l-cg)" d="M80 222Q140 256 186 272Q160 300 154 372L122 372Z"/>'
            '<path fill="url(#l-cg)" opacity=".6" d="M256 150Q238 220 206 268Q236 258 256 246Z"/>'
            '<path fill="url(#l-cr)" d="M60 380Q256 428 452 380L452 470L60 470Z"/>'
            '<path fill="url(#l-cv)" d="M60 350Q256 398 452 350L452 380Q256 428 60 380Z"/>'
            '<path fill="#A996FF" opacity=".55" d="M60 352Q256 400 452 352L452 358Q256 406 60 358Z"/></g>')
    body += ''.join(gem(*g, edge='#C27C00', sw=2.4 / scale) for g in GEMS)
    return f'<g transform="matrix({n(scale)} 0 0 {n(scale)} {n(tx)} {n(ty)})">{body}</g>'


DEFS = '''
<radialGradient id="l-fur" gradientUnits="userSpaceOnUse" cx="256" cy="236" r="206"><stop offset="0" stop-color="#E4E3FF"/><stop offset=".52" stop-color="#C2C0FE"/><stop offset=".78" stop-color="#A09EF8"/><stop offset="1" stop-color="#7C78EE"/></radialGradient>
<linearGradient id="l-ear" gradientUnits="userSpaceOnUse" x1="70" y1="96" x2="150" y2="214"><stop offset="0" stop-color="#7C5CFF"/><stop offset=".4" stop-color="#9C98FA"/><stop offset="1" stop-color="#C9C8FF"/></linearGradient>
<linearGradient id="l-earIn" gradientUnits="userSpaceOnUse" x1="88" y1="118" x2="140" y2="200"><stop offset="0" stop-color="#6234EA"/><stop offset=".55" stop-color="#9579FF"/><stop offset="1" stop-color="#CFC6FF"/></linearGradient>
<radialGradient id="l-face" gradientUnits="userSpaceOnUse" cx="256" cy="250" r="142"><stop offset="0" stop-color="#fff"/><stop offset=".8" stop-color="#fff"/><stop offset="1" stop-color="#EEEAFF"/></radialGradient>
<radialGradient id="l-body" gradientUnits="userSpaceOnUse" cx="250" cy="392" r="108"><stop offset="0" stop-color="#fff"/><stop offset=".62" stop-color="#fff"/><stop offset=".88" stop-color="#ECE8FF"/><stop offset="1" stop-color="#D3CCFF"/></radialGradient>
<linearGradient id="l-tail" gradientUnits="userSpaceOnUse" x1="168" y1="452" x2="66" y2="354"><stop offset="0" stop-color="#F4F2FF"/><stop offset=".5" stop-color="#C6C5FD"/><stop offset=".82" stop-color="#9E97F7"/><stop offset="1" stop-color="#7C5CFF"/></linearGradient>
<linearGradient id="l-hi" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".8"/><stop offset="1" stop-color="#fff" stop-opacity=".1"/></linearGradient>
<linearGradient id="l-iris" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#0B0833"/><stop offset=".55" stop-color="#191262"/><stop offset="1" stop-color="#3A2BA0"/></linearGradient>
<linearGradient id="l-rim" x1="0" y1="0" x2="0" y2="1"><stop offset=".5" stop-color="#4A33C8"/><stop offset="1" stop-color="#A28DFF"/></linearGradient>
<radialGradient id="l-blush"><stop offset="0" stop-color="#FF8FAB" stop-opacity=".85"/><stop offset=".6" stop-color="#FFA3BA" stop-opacity=".5"/><stop offset="1" stop-color="#FFB8CA" stop-opacity="0"/></radialGradient>
<linearGradient id="l-mouth" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#7E1A40"/><stop offset="1" stop-color="#B42E55"/></linearGradient>
<linearGradient id="l-arm" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#E4E2FF"/><stop offset=".55" stop-color="#C4C2FD"/><stop offset="1" stop-color="#9E9CF7"/></linearGradient>
<linearGradient id="l-paw" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#5550C8"/><stop offset="1" stop-color="#2E2984"/></linearGradient>
<linearGradient id="l-cape" x1="0" y1="0" x2=".35" y2="1"><stop offset="0" stop-color="#7C5CFF"/><stop offset="1" stop-color="#5B3FE0"/></linearGradient>
<linearGradient id="l-capeIn" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#4C31C9"/><stop offset="1" stop-color="#3B24A6"/></linearGradient>
<linearGradient id="l-capeHi" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#C2B3FF" stop-opacity=".95"/><stop offset="1" stop-color="#C2B3FF" stop-opacity="0"/></linearGradient>
<linearGradient id="l-collar" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#9077FF"/><stop offset="1" stop-color="#5B3FE0"/></linearGradient>
<radialGradient id="l-glow"><stop offset="0" stop-color="#E6DEFF" stop-opacity=".8"/><stop offset="1" stop-color="#E6DEFF" stop-opacity="0"/></radialGradient>
<radialGradient id="l-shadow"><stop offset="0" stop-color="#6F5FE0" stop-opacity=".32"/><stop offset=".7" stop-color="#8B7CF6" stop-opacity=".14"/><stop offset="1" stop-color="#8B7CF6" stop-opacity="0"/></radialGradient>
<clipPath id="l-ck"><path d="''' + CROWN + '''"/></clipPath>
<linearGradient id="l-cb" x1="0" y1="0" x2="1" y2="1"><stop offset=".15" stop-color="#fff"/><stop offset=".6" stop-color="#EEEAFF"/><stop offset="1" stop-color="#BDB1F5"/></linearGradient>
<linearGradient id="l-cs" x1="0" y1="0" x2="0" y2="1"><stop offset=".45" stop-color="#7D6BEB" stop-opacity="0"/><stop offset=".78" stop-color="#7D6BEB" stop-opacity=".28"/></linearGradient>
<linearGradient id="l-cg" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#fff" stop-opacity=".95"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
<linearGradient id="l-cv" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#5B3FE0"/><stop offset=".5" stop-color="#7C5CFF"/><stop offset="1" stop-color="#5B3FE0"/></linearGradient>
<linearGradient id="l-cr" x1="0" y1="0" x2="0" y2="1"><stop offset=".3" stop-color="#fff"/><stop offset="1" stop-color="#CFC6F7"/></linearGradient>
'''

SIL = f'stroke="{INK}" stroke-width="3.4" stroke-linejoin="round" stroke-linecap="round"'
LINE = f'stroke="{INK}" stroke-width="2.2" stroke-linejoin="round" stroke-linecap="round"'
LINE26 = LINE.replace('2.2', '2.6')


def sym(d, attrs):
    """A symmetric shape given by its left half."""
    return f'<path data-mirror="256" {attrs} d="{d}"/>'


def pair(d, attrs):
    """A left shape and its mirror image."""
    return f'<path {attrs} d="{d}"/><path {attrs} d="{mirror_all(d)}"/>'


# ---------------------------------------------------------------- parts (front view, 512 canvas)
PARTS = {}

PARTS['shadow'] = '<ellipse cx="256" cy="490" rx="132" ry="13" fill="url(#l-shadow)"/>'

PARTS['tail'] = (
    f'<path fill="url(#l-tail)" {SIL} d="M172 458C140 466 104 454 88 428C78 410 70 388 66 354C80 368 92 384 100 398'
    'C100 390 102 380 106 368C116 384 124 398 134 410C136 404 138 398 142 390C150 406 160 420 178 428Z"/>'
    '<path fill="url(#l-hi)" opacity=".8" d="M160 452C128 456 102 442 90 418Q114 438 150 442Z"/>'
    '<path fill="#5B3FE0" opacity=".35" d="M66 354C80 368 92 384 100 398C90 394 80 386 74 376Z"/>')

PARTS['ear'] = (   # the left ear; the right one is its mirror
    f'<path fill="url(#l-ear)" {SIL} d="M170 152C142 122 106 103 72 96C62 126 61 170 76 216L156 200Z"/>'
    '<path fill="url(#l-earIn)" d="M152 158C132 138 110 124 90 118C84 142 86 174 96 204C114 192 134 176 152 158Z"/>'
    '<path fill="#D9D7FF" d="M98 214Q96 186 100 162Q108 182 118 192Q118 172 124 158Q132 178 136 192Q140 180 148 172Q150 190 146 208Z"/>'
    '<path fill="#fff" opacity=".55" d="M106 206Q104 190 106 176Q112 190 120 198Z"/>')

HEAD_D = ('M256 118C212 118 172 128 146 150C126 166 110 186 98 204Q74 212 44 222Q66 238 84 248'
          'Q64 268 46 290Q76 296 96 300Q88 318 88 340Q108 330 122 336Q138 352 156 352Q166 364 184 360Q198 372 214 366Q232 376 256 372')
FACE_D = ('M256 196C246 175 228 160 204 160C168 160 136 176 122 206C114 222 112 240 112 254'
          'Q96 262 76 270Q98 280 112 284Q102 302 94 320Q116 318 132 324C164 346 210 354 256 354')
PARTS['head'] = (
    sym(HEAD_D, f'fill="url(#l-fur)" {SIL}')
    # lighter strands on the side tufts
    + pair('M118 206Q88 216 62 228Q90 230 112 236Z', 'fill="url(#l-hi)" opacity=".7"')
    + pair('M104 262Q84 278 66 290Q92 288 112 282Z', 'fill="url(#l-hi)" opacity=".7"')
    + pair('M112 304Q104 318 104 332Q116 322 126 318Z', 'fill="url(#l-hi)" opacity=".6"')
    + pair('M160 148Q140 162 128 180Q146 168 168 160Z', 'fill="url(#l-hi)" opacity=".6"'))
PARTS['face'] = (
    sym(FACE_D, 'fill="url(#l-face)"')
    # soft lavender tufts where the white face meets the fur
    + pair('M130 250Q114 258 100 262Q116 266 128 268Z', 'fill="#E4E0FF"')
    + pair('M126 290Q114 302 108 312Q122 308 134 306Z', 'fill="#E4E0FF"')
    + pair('M150 188Q144 198 140 210Q150 202 160 196Z', 'fill="#EEEBFF"'))

PARTS['forelock'] = sym('M256 118C250 126 246 134 246 144Q240 136 232 132C230 142 234 152 242 158C248 160 252 160 256 160',
                        f'fill="url(#l-fur)" stroke="{INK}" stroke-width="2" stroke-linejoin="round"')


def eye(cx, cy, rx=28, ry=35):
    return (f'<ellipse cx="{n(cx)}" cy="{n(cy)}" rx="{n(rx)}" ry="{n(ry)}" fill="url(#l-rim)" {LINE}/>'
            f'<ellipse cx="{n(cx)}" cy="{n(cy - 4)}" rx="{n(rx - 2.4)}" ry="{n(ry - 6)}" fill="url(#l-iris)"/>'
            f'<circle cx="{n(cx - 7)}" cy="{n(cy - 15)}" r="{n(rx * .37)}" fill="#fff"/>'
            f'<circle cx="{n(cx + 9)}" cy="{n(cy + 12)}" r="{n(rx * .16)}" fill="#fff"/>'
            f'<circle cx="{n(cx - 12)}" cy="{n(cy + 6)}" r="{n(rx * .07)}" fill="#fff" opacity=".8"/>')


PARTS['eyes'] = eye(188, 246, 30, 37) + eye(324, 246, 30, 37)
PARTS['eyes-happy'] = pair('M166 250Q190 222 214 250', f'fill="none" stroke="{INK}" stroke-width="5" stroke-linecap="round"')
PARTS['eyes-closed'] = pair('M168 248Q190 266 212 248', f'fill="none" stroke="{INK}" stroke-width="4.4" stroke-linecap="round"')
PARTS['brows'] = pair('M178 186Q189 179 201 183', 'fill="none" stroke="#6C61D9" stroke-width="4.6" stroke-linecap="round"')
PARTS['blush'] = '<ellipse cx="146" cy="298" rx="22" ry="13" fill="url(#l-blush)"/><ellipse cx="366" cy="298" rx="22" ry="13" fill="url(#l-blush)"/>'
PARTS['nose'] = f'<path fill="{INK}" d="M250 263Q256 259 262 263Q260 269 256 269Q252 269 250 263Z"/>'
MOUTH_D = 'M238 279Q247 287 256 280Q265 287 274 279C273 297 265 307 256 307C247 307 239 297 238 279Z'
PARTS['mouth-open'] = (
    f'<path fill="url(#l-mouth)" d="{MOUTH_D}"/>'
    '<path fill="#FF8098" d="M244 299C248 293 264 293 268 299C264 306 248 306 244 299Z"/>'
    f'<path fill="none" {LINE} d="{MOUTH_D}"/>')
GRIN_D = 'M232 276Q244 286 256 278Q268 286 280 276C280 304 268 318 256 318C244 318 232 304 232 276Z'
PARTS['mouth-grin'] = (
    f'<path fill="url(#l-mouth)" d="{GRIN_D}"/>'
    '<path fill="#FF8098" d="M240 308C246 298 266 298 272 308C266 318 246 318 240 308Z"/>'
    f'<path fill="none" {LINE} d="{GRIN_D}"/>')
PARTS['mouth-cat'] = f'<path fill="none" {LINE26} d="M238 278Q247 287 256 279Q265 287 274 278"/>'
PARTS['mouth-wavy'] = f'<path fill="none" {LINE26} d="M240 286Q245 279 250 285Q256 291 262 285Q267 279 272 286"/>'

PARTS['crown'] = crown_group(0.32, (256, 142))

BODY_D = 'M256 316C200 316 166 340 158 382C150 424 158 460 186 474Q222 488 256 480'
PARTS['body'] = (sym(BODY_D, f'fill="url(#l-body)" {SIL}')
                 + pair('M224 404Q234 412 244 408', f'fill="none" stroke="#D2CCFA" stroke-width="2.4" stroke-linecap="round"')
                 + pair('M232 430Q240 436 248 432', f'fill="none" stroke="#DCD6FC" stroke-width="2.2" stroke-linecap="round"'))

FOOT_D = 'M178 484C176 470 188 462 202 462C217 462 228 470 226 484C221 492 183 492 178 484Z'
PARTS['foot'] = (f'<path fill="url(#l-paw)" {SIL} d="{FOOT_D}"/>'
                 '<path fill="none" stroke="#8A82F0" stroke-width="2.4" stroke-linecap="round" d="M195 482Q194 477 196 472M209 482Q210 477 208 472"/>'
                 '<path fill="#fff" opacity=".22" d="M186 472Q194 466 206 466Q194 469 189 476Z"/>')

FLAP_D = ('M248 382C220 372 184 356 140 352C120 360 112 376 108 392C96 420 84 448 72 474'
          'Q92 484 112 478Q130 486 148 478Q156 482 164 476C162 450 168 424 184 408C204 394 228 388 248 382Z')
PARTS['flap'] = (   # the left side of the capelet, seen from the front; the right one is its mirror
    f'<path fill="url(#l-cape)" d="{FLAP_D}"/>'
    '<path fill="url(#l-capeIn)" d="M164 476C162 450 168 424 184 408C176 430 172 452 174 478Z"/>'
    '<path fill="#9B6BFF" opacity=".55" d="M142 356C176 362 210 372 242 382C206 382 172 374 138 368Z"/>'
    '<path fill="#3B22A8" opacity=".35" d="M122 404Q104 438 94 474Q112 444 134 412Z"/>'
    '<path fill="#3B22A8" opacity=".25" d="M150 410Q138 444 136 480Q146 450 160 418Z"/>'
    '<path fill="url(#l-capeHi)" d="M112 384C100 414 88 444 78 470Q84 471 88 467C98 440 110 412 122 390Z"/>'
    f'<path fill="none" {SIL} d="{FLAP_D}"/>')

KNOT_D = 'M222 380C220 370 232 364 242 370C250 376 248 388 238 390C230 392 224 388 222 380Z'
PARTS['knots'] = (pair(KNOT_D, 'fill="#8467FF"') + pair('M228 376Q234 370 240 374', 'fill="none" stroke="#B9A8FF" stroke-width="2" stroke-linecap="round"')
                  + pair(KNOT_D, f'fill="none" {LINE}'))

PARTS['clasp'] = gem(256, 386, 16, 20, edge=INK, sw=2.2)


def load_parts():
    """The parts file -> {id: inner svg text} (data-mirror paths completed)."""
    tree = ET.parse(SRC)
    parts = {}
    for g in tree.getroot():
        if g.tag == f'{{{SVGNS}}}g' and g.get('id'):
            for el in g.iter(f'{{{SVGNS}}}path'):
                ax = el.attrib.pop('data-mirror', None)
                if ax is not None:
                    el.set('d', mirror_half(el.get('d'), float(ax)))
            inner = ''.join(ET.tostring(c, encoding='unicode') for c in g)
            parts[g.get('id')] = inner.replace(f' xmlns="{SVGNS}"', '')
    defs = tree.getroot().find(f'{{{SVGNS}}}defs')
    alldefs = {d.get('id'): ET.tostring(d, encoding='unicode').replace(f' xmlns="{SVGNS}"', '') for d in defs}
    return parts, alldefs


def write_parts():
    os.makedirs(os.path.dirname(SRC), exist_ok=True)
    head = open(__file__).read().split('\n')[:0]
    groups = '\n'.join(f'<g id="{k}">{v}</g>' for k, v in PARTS.items())
    text = ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">\n'
            '<title>Lumo parts (source of tools/gen-mascot.py)</title>\n'
            '<!-- Lumo, the LordOS guardian: the shared parts every view and pose is assembled from.\n'
            '     Front-view coordinates on a 512 x 512 canvas, centred on x = 256, feet on y = 490.\n'
            '     Each top-level <g id> is a part; edit it here and run `python3 tools/gen-mascot.py`.\n'
            '     data-mirror="256": the path is the left half of a symmetric shape (completed by the script).\n'
            '     Outline #2A2366: 3.4 on silhouettes, 2.2 inside. -->\n'
            f'<defs>{DEFS}</defs>\n{groups}\n</svg>\n')
    open(SRC, 'w').write(text)
    print('parts', len(text.encode()), 'bytes')


# ---------------------------------------------------------------- assembly
MIRROR = 'matrix(-1 0 0 1 512 0)'


class Pose:
    def __init__(self, parts):
        self.parts, self.out = parts, []

    def add(self, name, transform=None, **attrs):
        a = ''.join(f' {k.replace("_", "-")}="{v}"' for k, v in attrs.items())
        body = self.parts[name]
        if transform or a:
            self.out.append(f'<g{" transform=" + chr(34) + transform + chr(34) if transform else ""}{a}>{body}</g>')
        else:
            self.out.append(body)
        return self

    def raw(self, text):
        self.out.append(re.sub(r'<path data-mirror="([\d.]+)" (.*?)d="([^"]+)"/>',
                               lambda m: f'<path {m.group(2)}d="{mirror_half(m.group(3), float(m.group(1)))}"/>', text))
        return self

    def group(self, transform, fn):
        mark = len(self.out)
        fn(self)
        inner = ''.join(self.out[mark:])
        del self.out[mark:]
        self.out.append(f'<g transform="{transform}">{inner}</g>')
        return self


def svg(title, desc, body, alldefs, view='0 0 512 512'):
    used, todo = [], set(re.findall(r'url\(#([\w-]+)\)', body))
    while todo:                                   # gradients may reference others (none today, but be safe)
        k = todo.pop()
        if k in alldefs and k not in used:
            used.append(k)
            todo |= set(re.findall(r'url\(#([\w-]+)\)|href="#([\w-]+)"', alldefs[k])[0:0])
    defs = ''.join(alldefs[k] for k in sorted(used))
    w, h = view.split()[2:]
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}" width="{w}" height="{h}" role="img">'
            f'<title>{title}</title><desc>{desc}</desc>\n<defs>{defs}</defs>\n{body}\n</svg>\n')


# ---- three-quarter view: the face turned to the viewer's left
PARTS['eyes-tq'] = eye(160, 248, 23, 35) + eye(290, 246, 30, 37)
PARTS['brows-tq'] = (f'<path fill="none" stroke="#6C61D9" stroke-width="4.6" stroke-linecap="round" d="M150 188Q159 182 170 185"/>'
                     f'<path fill="none" stroke="#6C61D9" stroke-width="4.6" stroke-linecap="round" d="M278 184Q290 177 303 181"/>')
PARTS['blush-tq'] = '<ellipse cx="122" cy="300" rx="15" ry="12" fill="url(#l-blush)"/><ellipse cx="338" cy="298" rx="22" ry="13" fill="url(#l-blush)"/>'

# ---- back view
PARTS['ear-back'] = (f'<path fill="url(#l-ear)" {SIL} d="M170 152C142 122 106 103 72 96C62 126 61 170 76 216L156 200Z"/>'
                     '<path fill="#fff" opacity=".35" d="M150 160C126 140 104 126 84 120Q100 140 104 176Q122 160 150 160Z"/>')
BACKFLUFF_D = ('M256 148C236 138 214 140 200 152C184 144 160 150 152 168C134 166 116 182 118 202C100 208 92 230 102 248'
               'C88 262 90 286 106 296C98 314 108 336 128 340C138 358 162 364 180 356C196 370 226 372 240 362C246 366 252 368 256 368')
PARTS['backfluff'] = (sym(BACKFLUFF_D, 'fill="url(#l-face)" stroke="#C9C4FF" stroke-width="2"')
                      + pair('M176 196Q196 186 214 194Q196 196 184 206Z', 'fill="#E9E5FF"')
                      + pair('M140 262Q158 252 174 262Q156 264 146 276Z', 'fill="#E9E5FF"')
                      + pair('M196 320Q214 310 230 322Q212 322 202 334Z', 'fill="#E9E5FF"'))
REAR_D = ('M256 354C214 354 170 352 136 360C110 392 90 434 74 474Q96 486 118 478Q140 488 162 480'
          'Q186 490 210 482Q234 490 256 486')
STAR = lambda cx, cy, r: (f'M{P((cx, cy - r))}Q{P((cx + r * .14, cy - r * .14), (cx + r, cy))}Q{P((cx + r * .14, cy + r * .14), (cx, cy + r))}'
                          f'Q{P((cx - r * .14, cy + r * .14), (cx - r, cy))}Q{P((cx - r * .14, cy - r * .14), (cx, cy - r))}Z')
PARTS['cape-rear'] = (
    sym(REAR_D, 'fill="url(#l-cape)"')
    + pair('M150 380Q122 430 112 478Q128 440 160 392Z', 'fill="#3B22A8" opacity=".35"')
    + pair('M200 392Q186 440 186 484Q198 444 214 400Z', 'fill="#3B22A8" opacity=".28"')
    + pair('M134 366C110 398 92 432 80 468Q86 469 90 465C102 432 120 400 142 372Z', 'fill="url(#l-capeHi)"')
    + sym(REAR_D, f'fill="none" {SIL}')
    + f'<circle cx="256" cy="434" r="52" fill="url(#l-glow)"/>'
    + f'<path fill="#EEE9FF" d="{STAR(256, 434, 34)}"/>'
    + f'<path fill="#7C5CFF" opacity=".6" d="{STAR(256, 434, 9)}"/>'
    + sym('M256 346C214 346 168 344 138 354Q128 368 140 378C180 388 222 392 256 392', 'fill="url(#l-collar)"')
    + pair('M150 360Q186 356 222 364Q186 364 154 368Z', 'fill="#C2B3FF" opacity=".8"')
    + sym('M256 346C214 346 168 344 138 354Q128 368 140 378C180 388 222 392 256 392', f'fill="none" {SIL}'))

# ---- arms, for the poses (pivot at the shoulder, pointing up)
ARM_D = 'M-25 16C-31 -6 -27 -30 -18 -46Q0 -56 18 -46C27 -30 31 -6 25 16Z'
PARTS['arm'] = (f'<path fill="url(#l-arm)" {SIL} d="{ARM_D}"/>'
                '<path fill="#fff" opacity=".6" d="M-14 0C-18 -16 -16 -32 -10 -42Q-8 -24 -6 -2Z"/>'
                f'<ellipse cx="0" cy="-60" rx="22" ry="19" fill="url(#l-paw)" {SIL}/>'
                '<path fill="none" stroke="#8A82F0" stroke-width="2.4" stroke-linecap="round" d="M-8 -72Q-8 -67 -6 -63M0 -74V-65M8 -72Q8 -67 6 -63"/>'
                '<ellipse cx="-6" cy="-68" rx="7" ry="4" fill="#fff" opacity=".2"/>')
PARTS['paw'] = (f'<ellipse cx="0" cy="0" rx="20" ry="17" fill="url(#l-paw)" {SIL}/>'
                '<path fill="none" stroke="#8A82F0" stroke-width="2.4" stroke-linecap="round" d="M-8 -9Q-8 -4 -6 0M0 -11V-2M8 -9Q8 -4 6 0"/>')

# ---- props
SHIELD_D = 'M256 318C228 332 196 338 166 334C164 400 182 452 256 494'
PARTS['shield'] = (
    sym(SHIELD_D, f'fill="#F4F1FF" {SIL}')
    + f'<g transform="matrix(.84 0 0 .84 40.96 66.4)">' + sym(SHIELD_D, 'fill="url(#l-cape)"')
    + pair('M166 334C164 400 182 452 256 494C210 450 196 400 200 340Z', 'fill="#3B22A8" opacity=".25"') + '</g>'
    + sym('M256 340C232 352 206 356 184 354Q186 372 190 388C214 380 236 372 256 362', 'fill="#fff" opacity=".18"')
    + f'<circle cx="256" cy="404" r="46" fill="url(#l-glow)" opacity=".7"/>'
    + f'<path fill="#fff" d="{STAR(256, 404, 38)}"/>'
    + f'<path fill="#7C5CFF" opacity=".55" d="{STAR(256, 404, 10)}"/>')

ZZ = lambda x, y, s: f'M{P((x, y), (x + s, y), (x, y + s), (x + s, y + s))}'
PARTS['zzz'] = (f'<path fill="none" stroke="#7C5CFF" stroke-width="7" stroke-linejoin="round" stroke-linecap="round" d="{ZZ(318, 214, 22)}"/>'
                f'<path fill="none" stroke="#7C5CFF" stroke-width="8" stroke-linejoin="round" stroke-linecap="round" d="{ZZ(356, 162, 30)}"/>'
                f'<path fill="none" stroke="#6A48F2" stroke-width="9" stroke-linejoin="round" stroke-linecap="round" d="{ZZ(404, 96, 40)}"/>')
LID_D = 'M262 268L462 250Q474 249 474 262L470 432Q470 444 458 444L270 448Q258 448 258 436L252 282Q252 270 262 268Z'
PARTS['laptop'] = (
    '<linearGradient id="l-lid" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#A9A6C8"/><stop offset="1" stop-color="#77739A"/></linearGradient>'
    f'<path fill="url(#l-lid)" {SIL} d="{LID_D}"/>'
    '<path fill="#fff" opacity=".18" d="M262 274L460 256L300 440L268 440Z"/>'
    f'<path fill="#C9C6E0" {SIL} d="M196 446L478 440Q490 440 488 452L486 464Q484 474 472 474L206 478Q192 478 190 466L188 456Q188 446 196 446Z"/>'
    '<path fill="#9C98BA" d="M190 462L488 456L486 464Q484 474 472 474L206 478Q192 478 190 466Z"/>'
    '<g transform="matrix(.16 0 0 .16 322 300)" fill="#fff" opacity=".9"><path d="' + CROWN + '"/></g>')

QM_D = 'M410 132C410 104 432 88 456 88C482 88 500 104 500 126C500 146 488 156 472 166C462 172 458 180 458 192'
PARTS['qmark'] = (f'<path fill="none" stroke="#7C5CFF" stroke-width="15" stroke-linecap="round" d="{QM_D}"/>'
                  '<circle cx="458" cy="222" r="10" fill="#7C5CFF"/>')
CONF = [(60, 150, 18, 9, 30, '#FFD60A'), (120, 70, 14, 8, -20, '#7C5CFF'), (420, 60, 16, 9, 40, '#FF8FAB'),
        (470, 160, 18, 8, -30, '#5AC8FA'), (40, 270, 14, 8, 60, '#FF8FAB'), (480, 280, 16, 9, 15, '#FFD60A'),
        (180, 30, 12, 7, 70, '#5AC8FA'), (340, 24, 14, 8, -50, '#FFD60A'), (30, 380, 12, 7, -40, '#7C5CFF'),
        (490, 390, 12, 7, 35, '#7C5CFF'), (90, 220, 8, 8, 0, '#9C86FF'), (440, 220, 8, 8, 0, '#FFC300')]
PARTS['confetti'] = ''.join(f'<rect x="{n(x - w / 2)}" y="{n(y - h / 2)}" width="{n(w)}" height="{n(h)}" rx="2.5" fill="{c}" transform="rotate({r} {x} {y})"/>' for x, y, w, h, r, c in CONF)
PARTS['speed'] = ''.join(f'<path fill="none" stroke="{c}" stroke-width="{w}" stroke-linecap="round" opacity="{o}" d="M{P((x, y), (x + l, y))}"/>'
                         for x, y, l, w, c, o in [(18, 250, 70, 8, '#8C7BFF', .7), (40, 300, 60, 7, '#A08FFF', .6), (8, 350, 86, 9, '#7C5CFF', .7),
                                                  (36, 400, 56, 7, '#A08FFF', .6), (14, 446, 66, 8, '#8C7BFF', .5)])
PARTS['cheer'] = (f'<path fill="none" stroke="#FFC300" stroke-width="7" stroke-linecap="round" d="M418 150L432 120M440 166L470 150M444 196L474 196"/>')
CAPEFLY_D = 'M300 384C236 368 150 352 40 366Q60 390 40 414Q72 420 80 446Q104 430 122 450Q142 436 160 456C200 460 240 456 290 444Z'
PARTS['cape-fly'] = (f'<path fill="url(#l-cape)" d="{CAPEFLY_D}"/>'
                     '<path fill="#3B22A8" opacity=".3" d="M250 420Q160 400 70 404Q160 414 246 432Z"/>'
                     '<path fill="url(#l-capeHi)" d="M290 386C230 372 150 360 52 370Q150 372 286 396Z"/>'
                     f'<path fill="none" {SIL} d="{CAPEFLY_D}"/>')


def front(p, eyes='eyes', mouth='mouth-open', tail=True, arms=(), head=None, feet=True):
    p.add('shadow')
    if tail:
        p.add('tail', 'translate(-30 -2)')
    p.add('body')
    if feet:
        p.add('foot').add('foot', MIRROR)
    if head:
        p.group(head, lambda q: face(q, eyes, mouth))
    else:
        face(p, eyes, mouth)
    for a in arms:
        p.add('arm', a)
    p.add('flap').add('flap', MIRROR).add('knots').add('clasp')


def face(p, eyes='eyes', mouth='mouth-open'):
    p.add('ear').add('ear', MIRROR).add('head').add('face').add('blush').add(eyes)
    if eyes == 'eyes':
        p.add('brows')
    p.add('nose').add(mouth).add('crown').add('forelock')


def tq(p, eyes='eyes-tq', mouth='mouth-open', feet=True, shadow=True, flaps=True, tail=True):
    if shadow:
        p.add('shadow')
    if tail:
        p.add('tail', 'matrix(-1 0 0 1 548 -6)')
    p.add('body', 'translate(8 0)')
    if feet:
        p.add('foot', 'translate(4 0)').add('foot', 'matrix(-1 0 0 1 524 0)')
    p.add('ear', 'matrix(.84 0 0 .9 44 18)').add('ear', 'matrix(-1.06 0 0 1.04 530 -6)')
    p.add('head', 'matrix(.97 0 0 1 4 0)').add('face', 'matrix(.93 0 0 1 -4 0)')
    p.add('blush-tq').add(eyes)
    if eyes == 'eyes-tq':
        p.add('brows-tq')
    p.add('nose', 'translate(-30 0)').add(mouth, 'translate(-30 0)')
    p.add('crown', 'rotate(-5 256 142) translate(-14 0)').add('forelock', 'translate(-20 2)')
    if flaps:
        p.add('flap', 'matrix(.74 0 0 1 50 0)').add('flap', 'matrix(-1.12 0 0 1 532 0)')
    p.add('knots', 'translate(-22 0)').add('clasp', 'translate(-22 0)')


VIEWS = {}
POSE = 'matrix(.86 0 0 .86 35.84 66)'      # every pose draws Lumo at the same, slightly smaller scale


def view(name, title, desc):
    def deco(fn):
        VIEWS[name] = (title, desc, fn)
        return fn
    return deco


DESC = 'Lumo, the LordOS guardian: a small celestial fox with white fur, the LordOS crown and a violet cape'


@view('lumo-front', 'Lumo', DESC + ', front view.')
def v_front(p):
    front(p)


@view('lumo-three-quarter', 'Lumo', DESC + ', three-quarter view.')
def v_tq(p):
    tq(p)


@view('lumo-back', 'Lumo', DESC + ', seen from the back, with the star on the cape.')
def v_back(p):
    p.add('shadow').add('tail', 'matrix(-1 0 0 1 560 -10)').add('foot').add('foot', MIRROR)
    p.add('cape-rear').add('ear-back').add('ear-back', MIRROR).add('head').add('backfluff').add('crown')


@view('lumo-wave', 'Lumo waving', DESC + ', waving hello.')
def v_wave(p):
    p.group(POSE, lambda q: front(q, 'eyes-happy', 'mouth-grin', arms=['translate(146 392) rotate(-26) scale(1 1.5)']))
    p.add('cheer')


@view('lumo-shield', 'Lumo with a shield', DESC + ', holding a shield with a star.')
def v_shield(p):
    def body(q):
        front(q, 'eyes', 'mouth-cat')
        q.add('shield', 'translate(58 8) scale(.84) translate(48.8 60)')
        q.add('paw', 'translate(226 418)')
    p.group(POSE, body)


@view('lumo-run', 'Lumo running', DESC + ', running fast.')
def v_run(p):
    p.add('speed').raw('<ellipse cx="270" cy="494" rx="110" ry="11" fill="url(#l-shadow)"/>')

    def body(q):
        q.add('tail', 'translate(-36 -6)').add('cape-fly')
        q.add('foot', 'rotate(-34 200 476) translate(-30 -6)')
        q.group('matrix(-1 0 0 1 512 0)', lambda r: tq(r, feet=False, shadow=False, flaps=False, tail=False))
        q.add('foot', 'matrix(-1 0 0 1 560 -14)')
        q.add('arm', 'translate(338 404) rotate(62) scale(.9)')
    p.group(POSE + ' translate(0 -22) rotate(12 256 480)', body)


@view('lumo-sleep', 'Lumo sleeping', DESC + ', asleep on a laptop.')
def v_sleep(p):
    def body(q):
        q.raw('<ellipse cx="256" cy="476" rx="230" ry="16" fill="url(#l-shadow)"/>')
        q.raw(f'<path fill="url(#l-body)" {SIL} d="M60 470C40 430 60 380 120 372C170 366 230 380 250 420L258 470Z"/>')
        q.raw(f'<path fill="url(#l-cape)" {SIL} d="M74 404C96 372 160 360 214 378C236 390 248 410 254 440Q230 452 206 446Q180 458 150 448Q120 460 92 450Q70 446 62 430Z"/>')
        q.raw('<path fill="url(#l-capeHi)" d="M90 396C120 376 170 370 208 384Q160 380 96 404Z"/>')
        q.group('translate(-42 128) rotate(-8 256 300) scale(.82) translate(56 30)', lambda r: (face(r, 'eyes-closed', 'mouth-cat')))
        q.add('laptop', 'translate(80 70) scale(.86)')
        q.add('paw', 'translate(236 456) scale(.9 .8)').add('paw', 'translate(186 460) scale(.9 .8)')
    p.group('translate(0 10)', body)
    p.add('zzz')


@view('lumo-celebrate', 'Lumo celebrating', DESC + ', celebrating with confetti.')
def v_celebrate(p):
    p.add('confetti')
    p.group(POSE, lambda q: front(q, 'eyes-happy', 'mouth-grin', arms=[
        'translate(150 392) rotate(-30) scale(1 1.6)', 'translate(362 392) rotate(30) scale(1 1.6)']))


@view('lumo-confused', 'Lumo confused', DESC + ', confused, with a question mark.')
def v_confused(p):
    p.group(POSE, lambda q: front(q, 'eyes', 'mouth-wavy', head='rotate(7 256 340)',
                                  arms=['translate(350 390) rotate(22) scale(1 1.9)']))
    p.add('qmark')


@view('lumo-avatar', 'Lumo', 'Lumo, the LordOS guardian: avatar.')
def v_avatar(p):
    p.raw('<radialGradient id="l-av" cx=".35" cy=".25" r=".9"><stop offset="0" stop-color="#9A7DFF"/><stop offset=".6" stop-color="#7C5CFF"/><stop offset="1" stop-color="#5B3FE0"/></radialGradient>'
          '<clipPath id="l-avc"><circle cx="256" cy="256" r="256"/></clipPath>'
          '<circle cx="256" cy="256" r="256" fill="url(#l-av)"/>')

    def head(q):
        q.add('body').add('flap').add('flap', MIRROR)
        face(q)
        q.add('knots').add('clasp')
    p.out.append('<g clip-path="url(#l-avc)">')
    p.group('matrix(1.12 0 0 1.12 -30.7 -2)', head)
    p.out.append('</g>')


@view('lumo-avatar-small', 'Lumo', 'Lumo, the LordOS guardian: small avatar.')
def v_avatar_small(p):
    k = f'stroke="{INK}" stroke-width="16" stroke-linejoin="round"'
    p.raw('<radialGradient id="l-av" cx=".35" cy=".25" r=".9"><stop offset="0" stop-color="#9A7DFF"/><stop offset=".6" stop-color="#7C5CFF"/><stop offset="1" stop-color="#5B3FE0"/></radialGradient>'
          '<clipPath id="l-avc"><circle cx="256" cy="256" r="256"/></clipPath>'
          '<circle cx="256" cy="256" r="256" fill="url(#l-av)"/><g clip-path="url(#l-avc)">'
          + pair('M118 262L74 96Q70 76 90 84L240 168Z', f'fill="#B9B4FF" {k}')
          + pair('M124 220L98 118L196 172Z', 'fill="#7C5CFF"')
          + sym('M256 150C190 150 140 176 112 214L40 250L92 290L36 340L100 370C130 450 190 500 256 500', f'fill="#C9C6FF" {k}')
          + f'<path fill="#fff" d="M256 214C346 196 410 260 404 340C398 420 336 470 256 470C176 470 114 420 108 340C102 260 166 196 256 214Z"/>'
          + ''.join(f'<ellipse cx="{x}" cy="338" rx="44" ry="56" fill="#1B1466"/><circle cx="{x - 12}" cy="316" r="17" fill="#fff"/>' for x in (190, 322))
          + '<ellipse cx="128" cy="404" rx="30" ry="18" fill="#FF9DB4" opacity=".7"/><ellipse cx="384" cy="404" rx="30" ry="18" fill="#FF9DB4" opacity=".7"/>'
          + f'<path fill="none" stroke="{INK}" stroke-width="12" stroke-linecap="round" stroke-linejoin="round" d="M232 412Q244 424 256 414Q268 424 280 412"/>'
          + '</g>')
    p.raw(crown_group(0.4, (256, 176)).replace('stroke-width="8"', 'stroke-width="24"'))


def build(only=None):
    parts, alldefs = load_parts()
    os.makedirs(OUT, exist_ok=True)
    for name, (title, desc, fn) in VIEWS.items():
        if only and name not in only:
            continue
        p = Pose(parts)
        fn(p)
        text = svg(title, desc, ''.join(p.out), alldefs)
        open(os.path.join(OUT, name + '.svg'), 'w').write(text)
        print(f'{name}.svg', len(text.encode()), 'bytes')


AVATARS = ('lumo-avatar', 'lumo-avatar-small')
UI_PNG = {   # what the UI draws (sdk/mascot.js): twice the largest size it is shown at
    'lumo-wave': [256], 'lumo-shield': [256], 'lumo-celebrate': [256], 'lumo-confused': [256], 'lumo-run': [256], 'lumo-sleep': [256],
    'lumo-avatar': [128], 'lumo-avatar-small': [64],
}


def render():
    if not shutil.which('rsvg-convert'):
        sys.exit('rsvg-convert not found: brew install librsvg / apt install librsvg2-bin')
    png = os.path.join(DOCS, 'png')
    os.makedirs(png, exist_ok=True)
    os.makedirs(UI, exist_ok=True)
    run = lambda src, px, out: subprocess.run(['rsvg-convert', '-w', str(px), '-h', str(px), src, '-o', out], check=True)
    for name in VIEWS:
        src = os.path.join(OUT, name + '.svg')
        shutil.copyfile(src, os.path.join(DOCS, name + '.svg'))
        sizes = {'lumo-avatar': (64, 128, 256), 'lumo-avatar-small': (24, 32, 64)}.get(name, (512, 1024))
        for px in sizes:
            run(src, px, os.path.join(png, f'{name}-{px}.png'))
        for px in UI_PNG.get(name, ()):
            run(src, px, os.path.join(UI, f'{name}-{px}.png'))
    print('PNGs in', os.path.relpath(png, REPO), 'and', os.path.relpath(UI, REPO))


if __name__ == '__main__':
    args = sys.argv[1:]
    if '--parts' in args or not os.path.exists(SRC):
        write_parts()
    only = [a for a in args if not a.startswith('--')] or None
    build(only)
    if '--svg' not in args and not only:
        render()
