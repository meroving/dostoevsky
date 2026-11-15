"""Thicken a TrueType font by stroking every outline and merging it in (monoline pens take this well).

Advances stay as they are, so a connected script's joins still meet. Hinting is dropped.
Needs skia-pathops and fonttools (a venv: `pip install skia-pathops fonttools`).

    embolden_font.py SRC.ttf OUT.ttf UNITS FAMILY STYLE WEIGHT

The app's Bad Script is the upstream BadScript-Regular.ttf (github.com/google/fonts, ofl/badscript)
thickened by 10 units of its 1000-unit em:

    embolden_font.py BadScript-Regular.ttf crates/app/assets/fonts/BadScript-Regular.ttf 10 "Bad Script" Regular 400
"""
import sys
import pathops
from fontTools.ttLib import TTFont
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.areaPen import AreaPen
from fontTools.pens.reverseContourPen import ReverseContourPen
from fontTools.pens.transformPen import TransformPen

src, out, d, family, style, weight = sys.argv[1], sys.argv[2], float(sys.argv[3]), sys.argv[4], sys.argv[5], int(sys.argv[6])
f = TTFont(src)
gs = f.getGlyphSet()
glyf, hmtx = f['glyf'], f['hmtx']
upm = f['head'].unitsPerEm
new = {}
for name in f.getGlyphOrder():
    g = glyf[name]
    adv, lsb = hmtx[name]
    if g.isComposite():
        hmtx[name] = (int(round(adv)), lsb)
        continue
    if g.numberOfContours == 0:
        hmtx[name] = (int(round(adv)), lsb)
        continue
    p = pathops.Path()
    gs[name].draw(p.getPen())
    a = AreaPen(); gs[name].draw(a); sign = a.value
    s = pathops.Path(); gs[name].draw(s.getPen())
    s.stroke(2 * d, pathops.LineCap.ROUND_CAP, pathops.LineJoin.ROUND_JOIN, 4)
    s.convertConicsToQuads()
    res = pathops.op(p, s, pathops.PathOp.UNION)
    a2 = AreaPen(); res.draw(a2)
    tt = TTGlyphPen(None)
    pen = TransformPen(tt, (1, 0, 0, 1, 0, 0))
    if (a2.value > 0) != (sign > 0):
        pen = ReverseContourPen(pen)
    res.draw(Cu2QuPen(pen, max_err=1.0, reverse_direction=False))
    new[name] = tt.glyph()
    hmtx[name] = (int(round(adv)), lsb)
for name, g in new.items():
    glyf[name] = g
for name in f.getGlyphOrder():
    g = glyf[name]
    g.recalcBounds(glyf)
    hmtx[name] = (hmtx[name][0], getattr(g, 'xMin', 0))
for t in ('hdmx', 'LTSH', 'VDMX'):
    if t in f: del f[t]
if 'fpgm' in f or 'prep' in f:
    for t in ('fpgm', 'prep', 'cvt '):
        if t in f: del f[t]
    for name in f.getGlyphOrder():
        glyf[name].removeHinting()
nm = f['name']
for nid in (1, 2, 3, 4, 6, 16, 17):
    nm.removeNames(nameID=nid)
ps = family.replace(' ', '') + '-' + style.replace(' ', '')
for plat, enc, lang in ((3, 1, 0x409), (1, 0, 0)):
    nm.setName(family, 1, plat, enc, lang); nm.setName(style, 2, plat, enc, lang)
    nm.setName(f"{ps};thickened {d:g} (Dostoevsky)", 3, plat, enc, lang)
    nm.setName(family if style == 'Regular' else f"{family} {style}", 4, plat, enc, lang)
    nm.setName(ps, 6, plat, enc, lang)
f['OS/2'].usWeightClass = weight
f.save(out)
print(out, 'upm', upm)
