#!/usr/bin/env python3
"""Regenerate Axia's tiny original COLR font (optional: pip install fonttools).

The generated font is committed; neither the RPM build nor the session needs
fonttools. VS Code's documented product-icon API consumes it without injected CSS.
"""
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.colorLib.builder import buildCOLR, buildCPAL

OUT = Path(__file__).resolve().parents[2] / 'assets/vscode/axia-window-controls/controls.ttf'


def main():
    fb = FontBuilder(1024, isTTF=True)
    names = ['.notdef', 'disc', 'close', 'minimize', 'maximize', 'restore']
    fb.setupGlyphOrder(names)
    glyphs = {}
    for name in names:
        pen = TTGlyphPen(None)
        if name == 'disc':
            # A 13 px circle in VS Code's normal 16 px icon cell. Quadratic
            # curves avoid another image, effect, animation or GPU surface.
            cx, cy, r = 512, 320, 416
            pen.moveTo((cx+r, cy))
            pen.qCurveTo((cx+r, cy+r), (cx, cy+r))
            pen.qCurveTo((cx-r, cy+r), (cx-r, cy))
            pen.qCurveTo((cx-r, cy-r), (cx, cy-r))
            pen.qCurveTo((cx+r, cy-r), (cx+r, cy))
            pen.closePath()
        elif name != '.notdef':
            # Monochrome fallback keeps controls recognizable if a renderer
            # cannot display the optional color layers. Normal COLR rendering
            # still uses only the disc, so these outlines cost no extra paint.
            def polygon(points):
                pen.moveTo(points[0])
                for point in points[1:]:
                    pen.lineTo(point)
                pen.closePath()
            if name == 'close':
                polygon([(240, 544), (288, 592), (784, 96), (736, 48)])
                polygon([(240, 96), (288, 48), (784, 544), (736, 592)])
            elif name == 'minimize':
                polygon([(224, 288), (800, 288), (800, 352), (224, 352)])
            else:
                for x0, y0, x1, y1 in [(224, 32, 800, 96), (224, 544, 800, 608),
                                      (224, 96, 288, 544), (736, 96, 800, 544)]:
                    polygon([(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
        glyphs[name] = pen.glyph()
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics({name: (1024, 0) for name in names})
    fb.setupHorizontalHeader(ascent=832, descent=-192)
    fb.setupCharacterMap({0xe001+i: name for i, name in enumerate(names[2:])})
    fb.setupNameTable({'familyName': 'Axia Window Controls', 'styleName': 'Regular',
                      'uniqueFontIdentifier': 'AxiaWindowControls-Regular-1',
                      'fullName': 'Axia Window Controls', 'psName': 'AxiaWindowControls-Regular',
                      'copyright': 'Original Axia DE artwork; MIT license.'})
    fb.setupOS2(sTypoAscender=832, sTypoDescender=-192, usWinAscent=832, usWinDescent=192)
    fb.setupPost()
    fb.setupMaxp()
    fb.font['COLR'] = buildCOLR({name: [('disc', color)] for name, color in
                               [('close', 0), ('minimize', 1), ('maximize', 2), ('restore', 2)]})
    fb.font['CPAL'] = buildCPAL([[(255/255, 95/255, 87/255, 1),
                                (254/255, 188/255, 46/255, 1),
                                (40/255, 200/255, 64/255, 1)]])
    # Stable timestamp: a regeneration must not change the font just for time.
    fb.font['head'].created = fb.font['head'].modified = 3800000000
    fb.font.recalcTimestamp = False
    OUT.parent.mkdir(parents=True, exist_ok=True)
    fb.save(OUT)
    print(OUT)


if __name__ == '__main__':
    main()
