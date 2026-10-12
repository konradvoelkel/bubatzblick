"""Cross-Validierung der numpy-Vektorisierung in sampling_ref.py:
`visibility_polygon()` (vektorisiert) gegen
`_visibility_polygon_reference()` (skalar, STRtree + Shapely-Einzelabfrage)
-- identischer Algorithmus, zwei unabhaengige Implementierungen desselben
Verfahrens im selben Modul.

Wichtig, WARUM diese Datei existiert: `sampling_ref.py` ist selbst die
unabhaengige Testreferenz fuer den Sandwich-Test gegen den Produktionspfad.
Eine Vektorisierung von TESTREFERENZ-Code darf also nicht nur "genauso
schnell, aber neu" sein, sondern muss exakt dasselbe Ergebnis liefern wie
die gehaertete skalare Fassung -- sonst verliert der Sandwich-Test seine
Aussagekraft.
Diese Datei ist der permanente Beleg dafuer, nicht nur eine einmalige
Migrationspruefung.
"""

from __future__ import annotations

import math
import random

import pytest
from hypothesis import given, settings
from hypothesis import strategies as st
from shapely import Polygon

from bubatzblick.geometry import convex_decompose
from bubatzblick.sampling_ref import _visibility_polygon_reference, visibility_polygon

REACH = 100.0

# Kalibriert (nicht frei erfunden): eine erste Fassung mit 1e-6 schlug auf
# zufaelligen Faellen fehl (bis zu 0.0035 m^2 ueber 3000 Stichproben)
# -- Ursache ist float-Rundung beim atan2->cos/sin-
# Roundtrip fuer Winkel exakt auf einer Hindernisecke: `far = origin +
# cos(angle)*max_len` weicht dadurch um ~1e-13..1e-16 von der exakten
# Zielrichtung ab, `max_len` (bis zu 2*reach) verstaerkt das linear, und
# GEOS' Liniensegment-Schnitt (keine Toleranz) kann eine Kante dadurch
# knapp verfehlen, die die vektorisierte Rechnung (dieselbe Rundung, aber
# eigene Toleranzbehandlung) noch trifft -- oder umgekehrt. Exakt dasselbe
# Phaenomen wie test_sandwich.py's OBSTACLE_BOUNDARY_MARGIN_M ("numerisch
# unterschiedliche Entscheidungen an exakt tangential gestreiften
# Hinderniskanten"), hier zwischen zwei sampling_ref.py-Implementierungen
# statt zwischen sampling_ref.py und dem Produktionspfad. Deshalb per Design
# unvermeidbar (die `eps`-verschobenen Nachbarwinkel in
# `_candidate_angles` existieren genau deswegen) und kein Zeichen fuer
# einen Logikfehler. 0.01 m^2 haelt > 10x Sicherheitsabstand zum
# schlimmsten beobachteten Fall (synthetisch: 0.0035 m^2 ueber 3000
# Stichproben; echte, dicht bebaute Golden-Master-Objekte: 0.0004-0.0009 m^2
# -- kleiner als die synthetischen Faelle, nicht groesser).
AREA_DIFF_TOLERANCE_M2 = 0.01


def _random_box(cx: float, cy: float, half_w: float, half_h: float) -> Polygon:
    return Polygon(
        [
            (cx - half_w, cy - half_h),
            (cx + half_w, cy - half_h),
            (cx + half_w, cy + half_h),
            (cx - half_w, cy + half_h),
        ]
    )


box_strategy = st.builds(
    _random_box,
    cx=st.floats(min_value=-60, max_value=60, allow_nan=False).filter(lambda v: abs(v) > 2),
    cy=st.floats(min_value=-60, max_value=60, allow_nan=False).filter(lambda v: abs(v) > 2),
    half_w=st.floats(min_value=0.5, max_value=8, allow_nan=False),
    half_h=st.floats(min_value=0.5, max_value=8, allow_nan=False),
)


@given(obstacles=st.lists(box_strategy, min_size=0, max_size=6))
@settings(max_examples=100, deadline=None)
def test_vectorized_matches_scalar_reference_random_boxes(obstacles: list[Polygon]) -> None:
    origin = (0.0, 0.0)
    fast = visibility_polygon(origin, obstacles, REACH, circle_arc_segments=32)
    ref = _visibility_polygon_reference(origin, obstacles, REACH, circle_arc_segments=32)
    diff = fast.symmetric_difference(ref).area
    assert diff < AREA_DIFF_TOLERANCE_M2, f"vektorisiert vs. skalar weichen um {diff} m^2 ab"


@given(
    origin_angle=st.floats(min_value=-math.pi, max_value=math.pi, allow_nan=False),
    origin_dist=st.floats(min_value=5, max_value=40, allow_nan=False),
)
@settings(max_examples=40, deadline=None)
def test_vectorized_matches_scalar_reference_varying_origin(
    origin_angle: float, origin_dist: float
) -> None:
    """Wie oben, aber mit variierendem Beobachterpunkt statt fixem Ursprung
    -- deckt mehr relative Winkel/Distanzen zwischen origin und Hindernis-
    kanten ab (u.a. Faelle nahe an Kollinearitaet, siehe
    `_ray_hit_distances_batch`-Docstring)."""
    origin = (origin_dist * math.cos(origin_angle), origin_dist * math.sin(origin_angle))
    rng = random.Random(hash((origin_angle, origin_dist)) & 0xFFFFFFFF)
    obstacles = [
        _random_box(
            rng.uniform(-50, 50),
            rng.uniform(-50, 50),
            rng.uniform(1, 6),
            rng.uniform(1, 6),
        )
        for _ in range(rng.randint(0, 5))
    ]
    fast = visibility_polygon(origin, obstacles, REACH, circle_arc_segments=32)
    ref = _visibility_polygon_reference(origin, obstacles, REACH, circle_arc_segments=32)
    diff = fast.symmetric_difference(ref).area
    assert diff < AREA_DIFF_TOLERANCE_M2, f"vektorisiert vs. skalar weichen um {diff} m^2 ab"


def test_vectorized_matches_scalar_reference_collinear_edge() -> None:
    """Gezielter Grenzfall: eine Hinderniskante liegt exakt radial vom
    Beobachterpunkt aus gesehen (Strahl und Kante kollinear) -- genau der
    Fall, den `_ray_hit_distances_batch` separat behandeln muss (siehe
    Docstring, "kollinearer Grenzfall")."""
    origin = (0.0, 0.0)
    # Kante von (10,0) nach (20,0) -- exakt auf dem Strahl bei Winkel 0.
    obstacle = Polygon([(10, 0), (20, 0), (20, 5), (10, 5)])
    fast = visibility_polygon(origin, [obstacle], REACH, circle_arc_segments=64)
    ref = _visibility_polygon_reference(origin, [obstacle], REACH, circle_arc_segments=64)
    diff = fast.symmetric_difference(ref).area
    assert diff < AREA_DIFF_TOLERANCE_M2, (
        f"vektorisiert vs. skalar weichen um {diff} m^2 ab (kollinearer Fall)"
    )


def test_vectorized_no_obstacles_is_full_circle() -> None:
    origin = (0.0, 0.0)
    poly = visibility_polygon(origin, [], REACH, circle_arc_segments=64)
    expected_area = math.pi * REACH**2
    assert abs(poly.area - expected_area) / expected_area < 0.01


def _load_golden_master(name: str) -> tuple[Polygon, list[Polygon]]:
    import geopandas as gpd

    path = f"tests/fixtures/duesseldorf_regbez_{name}.parquet"
    gdf = gpd.read_parquet(path)
    source = gdf.loc[gdf["role"] == "source", "geometry"].iloc[0]
    if source.geom_type == "MultiPolygon":
        source = max(source.geoms, key=lambda g: g.area)
    obstacles_raw = list(gdf.loc[gdf["role"] == "obstacle", "geometry"])
    obstacles = [p for b in obstacles_raw for p in convex_decompose(b)]
    return source, obstacles


@pytest.mark.slow
@pytest.mark.parametrize("fixture_name", ["mittel", "gross"])
def test_vectorized_matches_scalar_reference_real_fixture(fixture_name: str) -> None:
    """Dieselbe Gegenprobe auf beiden echten, dicht bebauten Golden-Master-
    Objekten (nicht nur synthetischen Boxen) -- mit deutlich mehr
    Hinderniskanten als die synthetischen Faelle oben, also der
    realistischste verfuegbare Stresstest fuer die Vektorisierung."""
    source, obstacles = _load_golden_master(fixture_name)
    origin = (source.exterior.coords[0][0], source.exterior.coords[0][1])
    fast = visibility_polygon(origin, obstacles, REACH, circle_arc_segments=32)
    ref = _visibility_polygon_reference(origin, obstacles, REACH, circle_arc_segments=32)
    diff = fast.symmetric_difference(ref).area
    assert diff < AREA_DIFF_TOLERANCE_M2, (
        f"vektorisiert vs. skalar weichen um {diff} m^2 ab ({fixture_name})"
    )


def test_bowtie_ring_keeps_both_lobes() -> None:
    """Ein sich selbst beruehrender Sweep-Ring darf keine Flaeche verlieren.

    `buffer(0)` behaelt nur einen der beiden Lappen -- fuer eine konservative
    Obermenge ist das der falsche Fehler. Der Rust-Kern behaelt beide
    (rust/bubatzcore/src/region.rs), das Orakel muss dazu passen.
    """
    from shapely import Polygon as _P

    from bubatzblick.sampling_ref import _ring_to_polygon

    bowtie = [(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)]
    roh = _P(bowtie)
    assert not roh.is_valid

    verloren = roh.buffer(0)
    behalten = _ring_to_polygon(bowtie)

    # beide Lappen sind je 25 Flaecheneinheiten gross
    assert verloren.area == pytest.approx(25.0)
    assert behalten.area == pytest.approx(50.0)
    assert behalten.is_valid
