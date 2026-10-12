"""Beobachterpunkte entlang der Quellkontur -- Python-Fassung von
``rust/bubatz/src/sample.rs`` fuer die Gegenprobe des Rust-Sweeps gegen
``sampling_ref.py`` (``tests/test_rust_vs_reference.py``)."""

from __future__ import annotations

import math

from shapely import MultiPolygon, Polygon


def sample_contour(
    source: Polygon | MultiPolygon, spacing_m: float, min_points: int, max_points: int
) -> list[tuple[float, float]]:
    """Punkte in festem Abstand entlang aller Aussenringe; ``min_points``/
    ``max_points`` deckeln die Anzahl nach unten und oben."""
    rings = [g.exterior for g in (source.geoms if isinstance(source, MultiPolygon) else [source])]
    total = sum(r.length for r in rings)
    if total <= 0:
        c = source.centroid
        return [(c.x, c.y)]
    n_total = int(min(max(math.ceil(total / spacing_m), min_points), max_points))
    points: list[tuple[float, float]] = []
    for ring in rings:
        n = max(3, round(n_total * ring.length / total))
        for i in range(n):
            p = ring.interpolate(ring.length * i / n)
            points.append((p.x, p.y))
    return points


def contour_spacing(source: Polygon | MultiPolygon, n_points: int) -> float:
    """Tatsaechlich erreichter Punktabstand (die Deckel koennen ``spacing_m``
    ausser Kraft setzen)."""
    rings = [g.exterior for g in (source.geoms if isinstance(source, MultiPolygon) else [source])]
    total = sum(r.length for r in rings)
    return total / n_points if n_points > 0 else 0.0
