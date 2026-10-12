"""Rust-Engine fuer den Rotationssweep: ``bubatzcore.Scene`` rechnet den
Sichtbarkeitsring je Beobachterpunkt, mit denselben Formeln in derselben
Reihenfolge wie die Python-Referenz --
Kandidatenwinkel, Winkelbereich je Kante, Verdeckungs-Kullung, 2x2-Schnitt,
kollinearer Grenzfall, Ausduennung.

**Vertrag: Toleranz, nicht Bit-Identitaet.** ``atan2/sin/cos/hypot`` laufen
in Rust ueber die System-libm, in Python ueber libm bzw. numpy; gleiche glibc
heisst gleiche Bits, aber das ist eine Plattform-, keine Sprachgarantie.
Abnahme (``tests/test_rust_vs_reference.py``): je Punktpolygon gegen die Referenz
Hausdorff < 1e-6 m und symmetrische Differenz < 1e-6 m2; ``equals_exact``
wird gezaehlt und protokolliert, nicht verlangt. Deshalb gehoert die Engine
zur Parameterversion: ein Wechsel der Engine invalidiert die `cache_key`.
"""

from __future__ import annotations

import numpy as np
from numpy.typing import NDArray
from shapely import MultiPolygon, Polygon
from shapely.geometry.base import BaseGeometry

# Konvertierung Shapely <-> Kern


def _polygon_parts(geom: BaseGeometry) -> list[Polygon]:
    """Nichtleere Polygon-Teile einer beliebigen Geometrie. ``Scene.polygon``
    repariert ein selbstberuehrendes Sweep-Polygon per ``buffer(0)``, und
    das liefert gelegentlich ein MultiPolygon -- selten genug, dass kleine
    Gebiete den Fall nie zeigen, auf Landesebene aber sicher. GEOS
    (``union_all``) nimmt jede Geometrie; dem Rust-Kern werden nur Ringe
    einzelner Polygone uebergeben, also vorher zerlegen."""
    if geom.is_empty:
        return []
    if isinstance(geom, Polygon):
        return [geom]
    if hasattr(geom, "geoms"):
        return [part for g in geom.geoms for part in _polygon_parts(g)]
    return []


def _shapes_of(geom: BaseGeometry) -> list[list[NDArray[np.float64]]]:
    """(Multi)Polygon -> Shape-Liste fuer den Kern: je Teil Aussenring zuerst
    (CCW), dann Loecher (CW), Ringe offen. Absolute Koordinaten -- der Kern
    skaliert intern auf das umschliessende Rechteck (i64)."""
    from shapely.geometry.polygon import orient

    out = []
    for part in _polygon_parts(geom):
        part = orient(part, 1.0)
        out.append(
            [np.ascontiguousarray(part.exterior.coords)[:-1]]
            + [np.ascontiguousarray(r.coords)[:-1] for r in part.interiors]
        )
    return out


def _geom_of(shapes: list[list[NDArray[np.float64]]]) -> BaseGeometry:
    """Shape-Liste des Kerns -> (Multi)Polygon, OHNE ``buffer(0)``: i_overlay
    liefert gueltige, konsistent orientierte Shapes (Vertrag P8 prueft es)."""
    # Entartete Ringe (< 3 Punkte) verwerfen -- i_overlay kann sie als Loch
    # ausgeben, Shapely nennt das Polygon dann ungueltig ("Too few points").
    # Sich selbst beruehrende Ringe (Loch beruehrt Aussenring in einem Punkt)
    # sind fuer i_overlay gueltig, fuer OGC nicht; der Aufrufer (`zones`)
    # repariert je Band per make_valid, das anders als buffer(0) nichts
    # fuellt -- `buffer(0)` fuellte damit Hoefe.
    parts = [
        Polygon(s[0], holes=[h for h in s[1:] if len(h) >= 3]) for s in shapes if len(s[0]) >= 3
    ]
    if not parts:
        return Polygon()
    return parts[0] if len(parts) == 1 else MultiPolygon(parts)


def obstacle_rings(obstacles: list[Polygon]) -> list[NDArray[np.float64]]:
    """Alle Ringe (Aussenring und Loecher je Hindernis, offen) in derselben
    Reihenfolge wie der Kern die Kanten erwartet."""
    rings: list[NDArray[np.float64]] = []
    for o in obstacles:
        parts = list(o.geoms) if isinstance(o, MultiPolygon) else [o]
        for p in parts:
            if not isinstance(p, Polygon) or p.is_empty:
                continue
            for ring in (p.exterior, *p.interiors):
                coords = np.ascontiguousarray(ring.coords, dtype=np.float64)[:-1, :2]
                if len(coords) >= 2:
                    rings.append(np.ascontiguousarray(coords))
    return rings


def free_circle(source: Polygon | MultiPolygon, reach: float) -> BaseGeometry:
    """``D(source, reach)`` im Kern: exakte Minkowski-Summe mit 200
    Segmenten je Vollkreis (statt GEOS ``buffer(reach, quad_segs=128)``, das
    die Eingabe um 1 % des Radius vereinfacht) -- derselbe Kreis wie in den
    Baendern (``bands.decompose_into_bands(engine="rust")``)."""
    import bubatzcore

    return _geom_of(bubatzcore.free_circle(_shapes_of(source), reach))


def visible_region_all_rust(
    source: Polygon | MultiPolygon,
    obstacles: list[Polygon],
    reach: float,
    max_len: float,
    points: list[tuple[float, float]],
    free: BaseGeometry,
    margin_m: float,
    chunk: int = 64,
) -> BaseGeometry:
    """Die ganze Zone in EINEM Kernaufruf: Szene, Sweep aller Punkte,
    streamende Kaskade, Versatz um ``margin_m``, Vereinigung mit der Quelle,
    Schnitt mit ``free``.
    ``margin_m <= 0`` heisst ohne Marge."""
    import bubatzcore

    pts = np.ascontiguousarray(np.asarray(points, dtype=np.float64).reshape(-1, 2))
    shapes = bubatzcore.visible_region(
        obstacle_rings(obstacles),
        reach,
        max_len,
        pts,
        _shapes_of(source),
        _shapes_of(free),
        max(margin_m, 0.0),
        bubatzcore.ROUND_STEP_QUAD_SEGS_16,
        chunk,
    )
    return _geom_of(shapes)


class RustScene:
    """Szene des Rust-Kerns hinter einem Shapely-Interface."""

    __slots__ = ("_scene", "max_len", "reach")

    def __init__(
        self, obstacles: list[Polygon], reach: float, max_len: float | None = None
    ) -> None:
        import bubatzcore

        self.max_len = max(max_len if max_len is not None else reach, reach)
        self._scene = bubatzcore.Scene(obstacle_rings(obstacles), reach, self.max_len)
        self.reach = reach

    @property
    def n_segments(self) -> int:
        return int(self._scene.n_segments)

    def hit_distances(
        self, origin: tuple[float, float], angles: NDArray[np.float64], max_len: float
    ) -> NDArray[np.float64]:
        if self.n_segments == 0 or len(angles) == 0:
            return np.full(len(angles), max_len)
        return np.asarray(
            self._scene.hit_distances(
                float(origin[0]), float(origin[1]), np.ascontiguousarray(angles), max_len
            )
        )

    def ring(self, origin: tuple[float, float], drop_collinear: bool = True) -> NDArray[np.float64]:
        return np.asarray(self._scene.ring(float(origin[0]), float(origin[1]), drop_collinear))

    def region(self, points: list[tuple[float, float]], chunk: int = 64) -> BaseGeometry:
        """Vereinigung der Sichtbarkeitsringe aller ``points`` im Kern:
        kein Shapely je Punkt, kein ``buffer(0)`` -- die Ringe gehen direkt in
        die streamende i_overlay-Kaskade (Abschnitte von ``chunk`` Punkten)."""
        import bubatzcore

        if not points:
            return Polygon()
        pts = np.ascontiguousarray(np.asarray(points, dtype=np.float64).reshape(-1, 2))
        return _geom_of(bubatzcore.visible_region_points(self._scene, pts, chunk))

    def polygon(self, origin: tuple[float, float], drop_collinear: bool = True) -> Polygon:
        poly = Polygon(self.ring(origin, drop_collinear))
        if not poly.is_valid:
            # Reparatur per ``buffer(0)``; entfaellt, sobald die
            # Ringe direkt in die i_overlay-Kaskade gehen.
            poly = poly.buffer(0)
        return poly
