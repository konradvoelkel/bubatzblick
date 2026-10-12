"""Referenz: Punktsampling, unabhaengig vom Produktionspfad.

NUR fuer Tests/Sandwich (Abschnitt 3.4), nicht Teil der Produktionspipeline.
Implementiert das Beobachterpunkt-Verfahren (Beobachterpunkte entlang
der Kontur, Vereinigung der Einzelsichtfelder) mit einem eigenstaendigen
Algorithmus (Radial-/Ereignis-Sichtbarkeitspolygon per Winkel-Ray-Casting),
damit der Sandwich-Test (test_sandwich.py) die Schattenkegel-Konstruktion
gegen eine unabhaengige Implementierung prueft und nicht nur gegen sich
selbst.

Kontrakt: fuer jeden einzelnen Beobachterpunkt liefert
``visibility_polygon`` das EXAKTE Sichtbarkeitspolygon bzgl. gerader
Hinderniskanten (bis auf einseitige Kreisbogen-Sehnenkorrektur bei
fehlendem Hindernis — das macht das Ergebnis dort etwas kleiner, nie
groesser, als die wahre Kreisbogen-Grenze). Die Vereinigung ueber endlich
viele Punkte der Quellkontur ist damit eine echte Teilmenge der wahren
(kontinuierlichen) Sichtbarkeit V(S) — das ist die untere Schranke der
Sandwich-Ungleichung V_sample(N) ⊆ V(S) ⊆ V_hat(S).
"""

from __future__ import annotations

import math

import numpy as np
from numpy.typing import NDArray
from shapely import (
    GeometryCollection,
    LineString,
    MultiLineString,
    MultiPoint,
    MultiPolygon,
    Point,
    Polygon,
    STRtree,
    make_valid,
    union_all,
)
from shapely.geometry.base import BaseGeometry


def _segments(poly: Polygon) -> list[LineString]:
    segs: list[LineString] = []
    rings = [poly.exterior, *poly.interiors]
    for ring in rings:
        coords = list(ring.coords)
        segs.extend(LineString([coords[i], coords[i + 1]]) for i in range(len(coords) - 1))
    return segs


def _ray_hit_distance(
    origin: tuple[float, float],
    angle: float,
    max_len: float,
    segment_tree: STRtree,
) -> float:
    """Kuerzeste Distanz von origin entlang angle bis zur ersten
    Hinderniskante, oder max_len falls keine Kante getroffen wird.

    ``segment_tree`` statt einer rohen Segmentliste: ein naiver linearer
    Scan ueber alle Hinderniskanten pro Strahl ist O(Winkel x Kanten) und
    wird auf echten OSM-Gebaeudeclustern (hunderte bis tausende Kanten pro
    Verbotsobjekt, siehe M0-Kalibrierung) unbrauchbar langsam -- ein
    einzelner Sandwich-Vergleich auf einem realen Duesseldorfer Objekt hing
    dabei mehrere Minuten. Der STRtree filtert pro Strahl auf Kandidaten,
    deren Bounding Box den Strahl ueberhaupt schneiden kann.
    """
    dx, dy = math.cos(angle), math.sin(angle)
    far = (origin[0] + dx * max_len, origin[1] + dy * max_len)
    ray = LineString([origin, far])
    best = max_len
    candidate_idx = segment_tree.query(ray, predicate="intersects")
    segments = segment_tree.geometries
    for idx in candidate_idx:
        seg = segments[idx]
        inter = ray.intersection(seg)
        for pt in _iter_points(inter):
            d = math.hypot(pt.x - origin[0], pt.y - origin[1])
            if d < best:
                best = d
    return best


def _ray_hit_distances_batch(
    origin: tuple[float, float],
    angles: NDArray[np.float64],
    max_len: float,
    seg_p0: NDArray[np.float64],
    seg_p1: NDArray[np.float64],
) -> NDArray[np.float64]:
    """Vektorisierte Fassung von ``_ray_hit_distance`` fuer ALLE Winkel
    gegen ALLE Kanten gleichzeitig: der
    urspruengliche Weg (ein STRtree-Query + eine Shapely-Intersection je
    (Winkel, Kandidatenkante)-Paar) ist fuer die Anzahl Winkel/Kanten
    dichter echter Objekte der dominante Engpass (~80 s fuer N=60 Punkte)
    -- reiner Python-/Shapely-Overhead je
    Paar, nicht die eigentliche Geometrie. Kein raeumlicher Index noetig:
    typische Kantenzahlen (hunderte bis niedrige Tausende je Objekt) sind
    fuer numpy-Broadcasting ueber ALLE Paare trivial schnell, ganz ohne
    STRtree-Vorfilterung.

    Zwei Faelle je (Winkel, Kante)-Paar, wie bei der Shapely-Referenz:
      1. Normalfall (Strahl und Kante nicht parallel): Standard-
         Geradenschnitt ueber ein 2x2-Gleichungssystem (Kreuzprodukt-Form).
      2. Kollinearer Grenzfall (Strahl und Kante (fast) parallel UND auf
         derselben Traeger-Geraden): ``_iter_points`` liefert fuer eine
         ueberlappende Shapely-Intersection die Kanten-Endpunkte als
         Kandidaten -- hier nachgebildet durch Projektion beider Endpunkte
         auf den Strahl, wenn ihr senkrechter Abstand zur Strahl-Geraden
         vernachlaessigbar ist.
    """
    ox, oy = origin
    n_angles = angles.shape[0]
    n_segs = seg_p0.shape[0]
    if n_segs == 0:
        return np.full(n_angles, max_len)

    dx = np.cos(angles)  # (A,)
    dy = np.sin(angles)
    sx = seg_p1[:, 0] - seg_p0[:, 0]  # (M,)
    sy = seg_p1[:, 1] - seg_p0[:, 1]
    qx = seg_p0[:, 0] - ox  # (M,)
    qy = seg_p0[:, 1] - oy

    denom = dx[:, None] * sy[None, :] - dy[:, None] * sx[None, :]  # (A, M)
    tol = 1e-9
    parallel = np.abs(denom) <= tol

    with np.errstate(divide="ignore", invalid="ignore"):
        t = (qx[None, :] * sy[None, :] - qy[None, :] * sx[None, :]) / denom
        s = (qx[None, :] * dy[:, None] - qy[None, :] * dx[:, None]) / denom
    normal_hit = (
        ~parallel & np.isfinite(t) & (t >= 0.0) & (t <= max_len) & (s >= -1e-9) & (s <= 1 + 1e-9)
    )
    t_normal = np.where(normal_hit, t, np.inf)

    # Kollinearer Fall: senkrechter Abstand jedes Kanten-Endpunkts zur
    # Strahl-Geraden durch origin in Richtung (dx,dy); Projektion (t) nur
    # als Kandidat gewertet, wenn dieser Abstand vernachlaessigbar ist.
    perp0 = qx[None, :] * dy[:, None] - qy[None, :] * dx[:, None]  # (A, M)
    q1x = seg_p1[:, 0] - ox
    q1y = seg_p1[:, 1] - oy
    perp1 = q1x[None, :] * dy[:, None] - q1y[None, :] * dx[:, None]
    t0 = qx[None, :] * dx[:, None] + qy[None, :] * dy[:, None]
    t1 = q1x[None, :] * dx[:, None] + q1y[None, :] * dy[:, None]
    collinear0 = parallel & (np.abs(perp0) <= tol) & (t0 >= 0.0) & (t0 <= max_len)
    collinear1 = parallel & (np.abs(perp1) <= tol) & (t1 >= 0.0) & (t1 <= max_len)
    t_col = np.where(collinear0, t0, np.inf)
    t_col = np.minimum(t_col, np.where(collinear1, t1, np.inf))

    best = np.minimum(t_normal, t_col).min(axis=1)
    return np.where(np.isfinite(best), best, max_len)


def _iter_points(geom: BaseGeometry) -> list[Point]:
    if geom.is_empty:
        return []
    if isinstance(geom, Point):
        return [geom]
    if isinstance(geom, MultiPoint):
        return list(geom.geoms)
    if isinstance(geom, (LineString, MultiLineString)):
        # Ueberlappung ray/Kante (kollinear): alle Endpunkte als Kandidaten.
        coords: list[tuple[float, float]] = []
        line_geoms: list[LineString] = (
            list(geom.geoms) if isinstance(geom, MultiLineString) else [geom]
        )
        for g in line_geoms:
            coords.extend((x, y) for x, y, *_ in g.coords)
        return [Point(c) for c in coords]
    if isinstance(geom, GeometryCollection):
        out: list[Point] = []
        for g in geom.geoms:
            out.extend(_iter_points(g))
        return out
    return []


def _nearby_obstacles(
    origin: tuple[float, float], obstacles: list[Polygon], reach: float
) -> list[Polygon]:
    search = Point(origin).buffer(reach)
    return [o for o in obstacles if o.intersects(search)]


def _candidate_angles(
    origin: tuple[float, float], nearby: list[Polygon], circle_arc_segments: int
) -> list[float]:
    """Kandidatenwinkel: alle Hindernisecken-Winkel (je mit einer winzigen
    +/- Stoerung, um Tangentialfaelle korrekt aufzuloesen), ergaenzt um
    ``circle_arc_segments`` gleichverteilte Zusatzwinkel als Sehnenstuetz-
    punkte fuer freie Kreisbogenabschnitte. Gemeinsam fuer den vektorisierten
    Pfad (``visibility_polygon``) und die skalare Referenz
    (``_visibility_polygon_reference``) -- beide muessen dieselben Winkel
    testen, damit ein Vergleich beider Ergebnisse aussagekraeftig ist.
    """
    angles: set[float] = set()
    eps = 1e-7
    for o in nearby:
        for x, y in list(o.exterior.coords)[:-1]:
            base = math.atan2(y - origin[1], x - origin[0])
            angles.add(base)
            angles.add(base + eps)
            angles.add(base - eps)
    # Wichtig: im selben Wertebereich wie atan2 ((-pi, pi]) erzeugen, sonst
    # sortiert `sorted(angles)` die Kreisbogen-Stuetzpunkte nicht in
    # tatsaechlicher Kreisreihenfolge relativ zu den Hindernis-Winkeln, was
    # sich kreuzende Kanten (Selbstueberschneidung) im rekonstruierten
    # Polygon erzeugt.
    # Kleiner, deterministischer Versatz (nicht 0): ohne ihn faellt der
    # erste Kreisbogen-Stuetzpunkt exakt auf -pi, also exakt auf die
    # negative x-Achse -- trifft eine achsparallele Hinderniskante (in
    # synthetischen/rasterartigen Layouts nicht selten) dann exakt
    # kollinear. Der `atan2`->`cos`/`sin`-Rundungsfehler an dieser Stelle
    # ist mit ~1e-16 winzig, aber genau das macht ihn gefaehrlich: anders
    # als beim eps-Versatz der Hindernisecken-Winkel (siehe den
    # `AREA_DIFF_TOLERANCE_M2`-Kommentar) betrifft eine falsche
    # Kollinearitaets-Entscheidung hier einen GANZEN Kreisbogen-Sektor
    # (Winkelabstand `2*pi/circle_arc_segments`, keine eng benachbarten
    # Stuetzpunkte, die den Fehler zu einem schmalen Splitter zusammen-
    # druecken) -- das kann eine spuerbare Flaeche falsch zuordnen, nicht
    # nur eine vernachlaessigbare. Der Versatz ist klein genug, die
    # Approximationsguete nicht zu beeintraechtigen (per min() auf
    # hoechstens 10 % des Stuetzpunkt-Abstands begrenzt), aber viele
    # Groessenordnungen groesser als Gleitkommarauschen, sodass exakte
    # Achsen-Ausrichtung praktisch ausgeschlossen ist.
    arc_offset = min(1e-4, 0.1 * (2.0 * math.pi / circle_arc_segments))
    for i in range(circle_arc_segments):
        angles.add(-math.pi + arc_offset + 2.0 * math.pi * i / circle_arc_segments)
    return sorted(angles)


def visibility_polygon(
    origin: tuple[float, float],
    obstacles: list[Polygon],
    reach: float,
    circle_arc_segments: int = 180,
) -> Polygon | MultiPolygon:
    """Exaktes (bis auf Sehnenkorrektur der freien Kreisboegen) Sichtbarkeits-
    polygon eines einzelnen Punktes ``origin`` bzgl. gerader Hinderniskanten,
    per Winkel-Ereignis-Ray-Casting.

    Numpy-vektorisiert -- siehe
    ``_ray_hit_distances_batch`` fuer die vektorisierte Kernberechnung.
    Gegen die skalare Referenzimplementierung (``_visibility_polygon_reference``,
    identischer Algorithmus, bewusst als STRtree+Shapely-Einzelabfrage-Pfad
    erhalten) erschoepfend auf zufaelligen und echten Golden-Master-Objekten
    verifiziert (siehe ``tests/test_sampling_ref.py``) -- diese Funktion ist
    selbst eine Testreferenz (Sandwich-Test), ihre eigene Korrektheit haengt
    also nicht nur vom internen numpy-Code ab, sondern von diesem externen
    Abgleich gegen eine unabhaengige Implementierung desselben Algorithmus.
    """
    nearby = _nearby_obstacles(origin, obstacles, reach)
    segments: list[LineString] = []
    for o in nearby:
        segments.extend(_segments(o))
    if segments:
        seg_p0 = np.array([s.coords[0] for s in segments], dtype=np.float64)
        seg_p1 = np.array([s.coords[1] for s in segments], dtype=np.float64)
    else:
        seg_p0 = np.zeros((0, 2))
        seg_p1 = np.zeros((0, 2))

    max_len = reach * 2.0 + 1.0
    ordered = np.array(_candidate_angles(origin, nearby, circle_arc_segments), dtype=np.float64)
    distances = np.minimum(
        _ray_hit_distances_batch(origin, ordered, max_len, seg_p0, seg_p1), reach
    )
    xs = origin[0] + distances * np.cos(ordered)
    ys = origin[1] + distances * np.sin(ordered)
    points = list(zip(xs.tolist(), ys.tolist(), strict=True))

    return _ring_to_polygon(points)


def _ring_to_polygon(points: list[tuple[float, float]]) -> Polygon | MultiPolygon:
    """Sweep-Ring -> Flaeche, ohne bei einer Bow-Tie Flaeche zu verlieren.

    Ein Ring aus dem Sweep kann sich in Rundungsfaellen selbst beruehren
    (Bow-Tie). Shapelys ``buffer(0)`` behaelt dann nur EINEN der beiden Lappen
    und wirft den anderen weg -- also genau Flaeche, die konservativ drin
    bleiben muss. ``make_valid`` behaelt beide; der Rust-Kern tut dasselbe
    (``rust/bubatzcore/src/region.rs``), das Orakel muss dazu passen.
    """
    poly = Polygon(points)
    if poly.is_valid:
        return poly
    fixed = make_valid(poly)
    parts = [
        g for g in getattr(fixed, "geoms", [fixed]) if g.geom_type in ("Polygon", "MultiPolygon")
    ]
    if not parts:
        return Polygon()
    return union_all(parts)  # type: ignore[return-value]


def _visibility_polygon_reference(
    origin: tuple[float, float],
    obstacles: list[Polygon],
    reach: float,
    circle_arc_segments: int = 180,
) -> Polygon | MultiPolygon:
    """Skalare Referenz fuer ``visibility_polygon`` (identischer Algorithmus,
    Winkel fuer Winkel ueber ``_ray_hit_distance``/STRtree statt vektorisiert)
    -- absichtlich erhalten, NICHT der Produktionspfad von ``sampling_ref.py``,
    sondern die unabhaengige Gegenprobe fuer ``tests/test_sampling_ref.py``.
    Sie ist die aeltere, skalare Fassung desselben Verfahrens.
    """
    nearby = _nearby_obstacles(origin, obstacles, reach)
    segments: list[LineString] = []
    for o in nearby:
        segments.extend(_segments(o))
    segment_tree = STRtree(segments) if segments else STRtree([])

    max_len = reach * 2.0 + 1.0
    ordered = _candidate_angles(origin, nearby, circle_arc_segments)
    points: list[tuple[float, float]] = []
    for a in ordered:
        d = _ray_hit_distance(origin, a, max_len, segment_tree)
        d = min(d, reach)
        points.append((origin[0] + d * math.cos(a), origin[1] + d * math.sin(a)))

    return _ring_to_polygon(points)


def sample_source_points(source: Polygon | MultiPolygon, n: int) -> list[tuple[float, float]]:
    """``n`` gleichverteilte Punkte entlang des Quellumrings (Approximation
    der Beobachterpunkt-Diskretisierung, hier parametrisiert
    ueber die Punktzahl statt einen festen Abstand).

    OSM-Areas (auch von einzelnen geschlossenen Ways) kommen aus der
    Extraktion durchgehend als MultiPolygon (libosmium modelliert Areas
    generisch als Multipolygon, siehe extract.rs) -- Sampling wird
    proportional zur Aussenring-Laenge auf alle Teilflaechen verteilt.
    """
    if n <= 0:
        raise ValueError("sample_source_points: n muss > 0 sein")
    parts: list[Polygon] = list(source.geoms) if isinstance(source, MultiPolygon) else [source]
    lengths = [p.exterior.length for p in parts]
    total_length = sum(lengths)
    if total_length <= 0:
        raise ValueError("sample_source_points: Quelle hat keinen Umring")

    points: list[tuple[float, float]] = []
    remaining_n = n
    for k, (part, length) in enumerate(zip(parts, lengths, strict=True)):
        is_last = k == len(parts) - 1
        n_part = remaining_n if is_last else round(n * length / total_length)
        n_part = max(1, min(n_part, remaining_n))
        remaining_n -= n_part
        boundary = part.exterior
        points.extend(
            (
                boundary.interpolate(i / n_part * length).x,
                boundary.interpolate(i / n_part * length).y,
            )
            for i in range(n_part)
        )
    return points


def visible_region_sampled(
    source: Polygon | MultiPolygon, obstacles: list[Polygon], reach: float, n_points: int
) -> BaseGeometry:
    """V_sample(N): Vereinigung der Sichtbarkeitspolygone von ``n_points``
    Beobachterpunkten entlang der Quellkontur. Referenzimplementierung fuer
    den Sandwich-Test — NICHT fuer die Produktionspipeline (siehe
    rust/bubatzcore fuer die dortige samplingfreie Konstruktion).
    """
    pts = sample_source_points(source, n_points)
    polys = [visibility_polygon(p, obstacles, reach) for p in pts]
    return union_all(polys)
