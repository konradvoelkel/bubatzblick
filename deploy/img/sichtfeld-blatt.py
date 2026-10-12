import math
import random
from pathlib import Path

from shapely.geometry import LineString, Point, Polygon
from shapely.ops import unary_union

C = 256.0
R = 225.0


def P(r, a):
    a = math.radians(a)
    return (C + r * math.sin(a), C - r * math.cos(a))


# --- leaf as one polygon: r(theta) = max over leaflets ---
LF = [
    (0, 1.0, 13.5),
    (37, 0.93, 12.5),
    (-37, 0.93, 12.5),
    (73, 0.80, 11.5),
    (-73, 0.80, 11.5),
    (110, 0.62, 10),
    (-110, 0.62, 10),
]


def rleaf(th):
    best = 0.09 * R
    for a, L, s in LF:
        d = (th - a + 180) % 360 - 180
        if abs(d) > 3.2 * s:
            continue
        base = L * R * math.exp(-((d / s) ** 2))
        # serration: teeth get finer toward the tip
        # serration: triangle wave along the flank, ~6 teeth per side, none on the axis
        k = abs(d) / s
        ph = (k * 6.5) % 1.0
        tri = 1 - abs(2 * ph - 1)
        tooth = 1 + 0.13 * tri * min(1, k * 2.2) * (1 if k < 2.6 else 0)
        best = max(best, base * tooth)
    # stem
    d = (th - 180 + 180) % 360 - 180
    if abs(d) < 4:
        best = max(best, 0.58 * R * math.exp(-((d / 1.6) ** 2)))
    return best


leaf = Polygon([P(rleaf(t / 2), t / 2) for t in range(720)]).buffer(0)
# --- roads along the leaflet axes + stem ---
roads = [LineString([P(0.11 * R, a), P(1.2 * R, a)]) for a, _, _ in LF] + [
    LineString([P(0.11 * R, 180), P(1.2 * R, 180)])
]
roadbuf = unary_union([r.buffer(11) for r in roads])
keepout = unary_union([leaf.buffer(7), roadbuf, Point(C, C).buffer(0.2 * R)])
# --- buildings: annular rings, cut by keep-out, split into blocks ---
random.seed(3)
blds = []
r = 0.2 * R
while r < 1.12 * R:
    h = random.uniform(28, 44)
    g = random.uniform(4, 7)
    ring = Point(C, C).buffer(r + h, 128).difference(Point(C, C).buffer(r, 128))
    # cut the ring into angular pieces of random width (streets between them)
    a = random.uniform(0, 20)
    while a < 360:
        w = random.uniform(16, 34)
        sec = Polygon([P(0, 0)] + [P(1.3 * R, a + w * i / 8) for i in range(9)])
        piece = ring.intersection(sec).difference(keepout)
        for g_ in piece.geoms if piece.geom_type == "MultiPolygon" else [piece]:
            if g_.area > 220 and g_.buffer(-4).area > 0:
                # shrink slightly for gaps between neighbours, drop slivers
                g2 = g_.buffer(-2.2).simplify(0.6)
                if g2.geom_type == "Polygon" and g2.area > 180:
                    blds.append(g2)
                elif g2.geom_type == "MultiPolygon":
                    blds += [q for q in g2.geoms if q.area > 180]
        a += w + random.uniform(3, 6)
    r += h + g


def path(poly):
    ext = list(poly.exterior.coords)
    return "M" + " L".join(f"{px:.1f},{py:.1f}" for px, py in ext) + "Z"


casing = "".join(
    f'<line x1="{x1:.1f}" y1="{y1:.1f}" x2="{x2:.1f}" y2="{y2:.1f}"/>'
    for (x1, y1), (x2, y2) in (rd.coords for rd in roads)
)
building_paths = "".join(f'<path d="{path(b)}"/>' for b in blds)
G_BLD = 'fill="#d9d0c9" stroke="#b5a99f" stroke-width="1.1" stroke-linejoin="round"'
CIRCLE = 'fill="none" stroke="#7b241c" stroke-width="1.3" stroke-dasharray="5 6" opacity=".45"'
LEAF = (
    'fill="#c0392b" fill-opacity=".45" stroke="#7b241c" stroke-width="1.4" stroke-linejoin="round"'
)
PIN = (
    'stroke="#2e7d32" stroke-width="2.6" fill="none" stroke-linecap="round" stroke-linejoin="round"'
)
pin_paths = (
    f'<path d="M{C - 12},{C + 9} L{C - 8},{C - 8} L{C + 8},{C - 8} L{C + 12},{C + 9}"/>'
    f'<path d="M{C - 3.5},{C - 8} L{C - 3.5},{C + 5} M{C + 3.5},{C - 8} L{C + 3.5},{C + 5}"/>'
    f'<path d="M{C - 7},{C + 5} L{C + 7},{C + 5}"/>'
)
svg = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
<rect width="512" height="512" fill="#f2efe9"/>
<g stroke="#c9c2b8" stroke-width="12" stroke-linecap="round">{casing}</g>
<g stroke="#ffffff" stroke-width="8" stroke-linecap="round">{casing}</g>
<g {G_BLD}>{building_paths}</g>
<circle cx="{C}" cy="{C}" r="{R}" {CIRCLE}/>
<path d="{path(leaf)}" {LEAF}/>
<circle cx="{C}" cy="{C}" r="22" fill="#dffce2" stroke="#8fcf9a" stroke-width="1.5"/>
<g {PIN}>
 {pin_paths}
</g>
</svg>'''
Path("leafmap.svg").write_text(svg)
print(len(blds), "buildings")
