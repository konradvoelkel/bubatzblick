"""Konvexe Zerlegung (Hertel-Mehlhorn ueber Delaunay-Dreiecke) -- nur noch fuer
die Testreferenz ``sampling_ref.py`` (``tests/test_sampling_ref.py``). Der Rest
von geometry.py (CRS, Verschmelzen, Vereinfachung, Hindernisauswahl) liegt in
``rust/bubatz/src/{extract,obstacles,simplify}.rs``."""

from __future__ import annotations

import shapely
from shapely import Polygon, STRtree, constrained_delaunay_triangles

MAX_TRIANGLES_FOR_MERGE = 600


def _is_convex(poly: Polygon, rel_tol: float = 1e-9) -> bool:
    """Toleranzbasierte Konvexitaetspruefung.

    ``poly.equals(poly.convex_hull)`` scheitert in der Praxis fast immer an
    winzigem Floating-Point-Rauschen aus CRS-Transformation und
    ``make_valid`` (siehe M0-Kalibrierungslauf: reale OSM-Gebaeude, die
    visuell exakte Rechtecke sind, bestehen den exakten Gleichheitstest
    kaum). Die Toleranz ist relativ zur Hindernisflaeche, damit sie sowohl
    fuer kleine Garagen als auch fuer grosse Schulgebaeude sinnvoll ist.

    Sicherheitsrichtung: wird
    ein Polygon mit einer Kerbe unterhalb der Toleranz faelschlich als
    konvex behandelt, blockiert der daraus gebildete Kegel geringfuegig
    weniger als geometrisch maximal moeglich — das macht V_hat hoechstens
    unnoetig etwas groesser, nie kleiner. Die Konservativitaetsgarantie
    bleibt also erhalten.
    """
    hull = poly.convex_hull
    if hull.area <= 0:
        return True
    return (hull.area - poly.area) <= rel_tol * hull.area


def _share_edge(a: Polygon, b: Polygon) -> bool:
    """Teilen zwei innerlich disjunkte konvexe Teile eine Kante (nicht nur
    einen Punkt)? DE-9IM: Rand ∩ Rand ist eindimensional.
    ``relate_pattern`` statt ``a.intersection(b)`` + Typ-/Laengenpruefung --
    dasselbe Praedikat (zwei konvexe Teile mit disjunkten Innenraeumen
    schneiden sich in einer Strecke oder einem Punkt, nie in beidem), aber
    ohne die Schnittgeometrie tatsaechlich zu konstruieren; im Profil der
    dichtesten NRW-Zelle die teuerste Einzeloperation des Merge-Schritts."""
    return bool(shapely.relate_pattern(a, b, "****1****"))


def _greedy_merge_convex(pieces: list[Polygon], max_passes: int = 4) -> list[Polygon]:
    """Nach Hertel-Mehlhorn-Prinzip: benachbarte Dreiecke/Teile zu
    groesseren konvexen Teilen verschmelzen, wie das Ergebnis konvex
    bleibt. Keine minimale Zerlegung, aber deutlich groebere (und damit
    schnellere und schaerfere, Abschnitt 3.3) Teile als rohe Triangulierung.

    STRtree-gestuetzte Nachbarschaftssuche statt paarweisem O(n^2)-Scan:
    ein naiver Scan wiederholt bei jedem Merge neu (das urspruengliche
    Design in diesem Modul brauchte auf Gebaeuden mit mehreren tausend
    Dreiecken effektiv O(n^3) und blieb im M0-Kalibrierungslauf haengen).
    Mehrere begrenzte Durchlaeufe statt Konvergenz bis zum Fixpunkt: nicht
    die maximal groebste Zerlegung, aber eine mit garantiert linearem statt
    kubischem Aufwand pro Durchlauf.
    """
    pieces = list(pieces)
    for _ in range(max_passes):
        if len(pieces) <= 1:
            break
        tree = STRtree(pieces)
        used = [False] * len(pieces)
        new_pieces: list[Polygon] = []
        merged_any = False
        for i, a in enumerate(pieces):
            if used[i]:
                continue
            used[i] = True
            current = a
            for j in tree.query(current, predicate="intersects"):
                j = int(j)
                if used[j]:
                    continue
                b = pieces[j]
                if not _share_edge(current, b):
                    continue
                union = current.union(b)
                if isinstance(union, Polygon) and _is_convex(union):
                    current = union
                    used[j] = True
                    merged_any = True
            new_pieces.append(current)
        pieces = new_pieces
        if not merged_any:
            break
    return pieces


def convex_decompose(poly: Polygon) -> list[Polygon]:
    """Zerlegung eines (ggf. nicht-konvexen) Polygons in konvexe Teile.

    Kein CGAL: Basis ist ``shapely.constrained_delaunay_triangles``
    (Dreiecke sind stets konvex), gefolgt von einem greedy
    Hertel-Mehlhorn-Merge benachbarter Teile zu groesseren konvexen
    Stuecken. Das ist NICHT die minimale konvexe Zerlegung, reduziert die
    Teilezahl aber typischerweise um eine Groessenordnung gegenueber roher
    Triangulierung — laut Abschnitt 3.3 ("groebere Zerlegungen sind
    schaerfer") sowohl schneller als auch eine straffere obere Schranke.
    Die Konservativitaetsgarantie haengt nicht von
    der Wahl der Zerlegung ab, nur ihre Straffheit.

    Bereits (naeherungsweise) konvexe Polygone werden unveraendert
    zurueckgegeben (schaerfer und billiger als jede Triangulierung).
    """
    if _is_convex(poly):
        return [poly]
    triangles = constrained_delaunay_triangles(poly)
    out: list[Polygon] = [t for t in triangles.geoms if isinstance(t, Polygon) and t.area > 0]
    if not out:
        raise ValueError("convex_decompose: Triangulierung lieferte keine Flaeche")
    if len(out) > MAX_TRIANGLES_FOR_MERGE:
        # Sicherheitsventil gegen pathologische Faelle (sehr grosse/komplexe
        # verschmolzene Gebaeudekomplexe): der Merge-Schritt ist bewusst nur
        # eine Optimierung, keine Korrektheitsvoraussetzung. Ohne ihn bleibt
        # das Ergebnis weiterhin eine korrekte (nur weniger straffe)
        # konvexe Zerlegung -- siehe Vorzeichendisziplin, Abschnitt 2.3.
        return out
    return _greedy_merge_convex(out)


# Ab dieser Toleranz reicht ein groeberer Kreisbogen fuer den Korrektur-
# puffer (Sehnenfehler bei quad_segs=2 und r=25 m: 1,9 m, weit unter einem
# z12-Pixel von 38 m); darunter bleibt quad_segs=8, z14 bleibt damit
# unveraendert.
COARSE_TOLERANCE_M = 25.0
# Vertrag von one_sided_simplify(): Original ⊆ Ergebnis ⊆ Original.buffer(
# OUTWARD_BOUND_FACTOR * tolerance_m). Herleitung: Douglas-Peucker mit
# preserve_topology verschiebt keinen Punkt weiter als tolerance_m; der
# Korrekturpuffer betraegt tolerance_m, im Wiederholungsfall 2*tolerance_m;
# der Rueckfall ist Original.buffer(tolerance_m). Summe hoechstens 3.
OUTWARD_BOUND_FACTOR = 3.0
