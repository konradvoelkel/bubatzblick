"""Die Python-Gegenprobe: der Rust-Sweep
(``bubatzcore`` ueber ``visibility_rust``) gegen ``sampling_ref.py``, die
unabhaengige shapely/numpy-Referenz (Winkel-Ereignis-Ray-Casting, anderer
Algorithmus, andere Sprache). Alles andere -- Vertrag der Zone, Marge,
Monotonie, Determinismus, starre Bewegung, Anker -- prueft
``rust/bubatz/src/contract_tests.rs`` in Rust.

Warum diese Datei bleibt: ``bubatzcore::reference`` ist ein Port der
Referenz durch dieselbe Hand wie der schnelle Kern. Diese Gegenprobe hier
ist die einzige, die nicht aus demselben Code stammt.
"""

from __future__ import annotations

import math
from pathlib import Path

import geopandas as gpd
import numpy as np
import pytest
import shapely
from shapely import MultiPolygon, Point, Polygon, union_all

from bubatzblick import sampling_ref as ref
from bubatzblick.contour import sample_contour
from bubatzblick.visibility_rust import RustScene, free_circle, visible_region_all_rust

FIXTURES_DIR = Path(__file__).parent / "fixtures"
REACH = 102.0  # config.effective_reach_m unter v7


def _rotated_box(cx: float, cy: float, w: float, h: float, rot: float) -> Polygon:
    base = np.array([[-w / 2, -h / 2], [w / 2, -h / 2], [w / 2, h / 2], [-w / 2, h / 2]])
    r = np.array([[math.cos(rot), -math.sin(rot)], [math.sin(rot), math.cos(rot)]])
    return Polygon(base @ r.T + [cx, cy])


def _random_scene(rng: np.random.Generator) -> list[Polygon]:
    # Einzeln statt `*rng.uniform(..., 2)`: die Laenge eines numpy-Arrays
    # steht nicht im Typ, und entpackt sieht mypy beliebig viele Argumente.
    def box() -> Polygon:
        cx, cy = (float(v) for v in rng.uniform(-80, 80, 2))
        w, h = (float(v) for v in rng.uniform(3, 25, 2))
        return _rotated_box(cx, cy, w, h, float(rng.uniform(0, math.pi)))

    return [box() for _ in range(int(rng.integers(1, 12)))]


def _assert_close(
    expected: Polygon | MultiPolygon, got: Polygon, origin: tuple[float, float]
) -> None:
    """Gemessen: zufaellige Szenen relativ 1,3e-5 / Hausdorff 1,5 cm
    (Bogendiskretisierung), echte Szenen bis 1,3e-4 / 0,47 m -- Letzteres sind
    Splitter an streifenden Strahlen ueber Hindernisecken (Flaeche ~0, Laenge
    bis zum Reichweitenkreis). Schranken mit Luft: 5e-4 und 1 m."""
    assert got.is_valid, origin
    diff = expected.symmetric_difference(got).area
    assert diff < 5e-4 * expected.area, (origin, diff, expected.area)
    assert shapely.hausdorff_distance(expected, got) < 1.0, origin


def _fixture(name: str) -> tuple[Polygon | MultiPolygon, list[Polygon]]:
    """Hindernisse OHNE Loecher: ``sampling_ref`` erzeugt fuer Lochecken keine
    Kandidatenwinkel und sieht deshalb in Innenhoefe hinein (bekannte Grenze
    der Referenz); der Kern behandelt Lochkanten als
    Hindernis (Kern-Test ``region_of_points_inside_a_wall_ring_is_the_inner_disc``).
    Mit Loechern: ``mittel`` (53 Hoefe) weicht 0,66 % ab, ohne 2e-5."""
    gdf = gpd.read_parquet(FIXTURES_DIR / f"duesseldorf_regbez_{name}.parquet")
    source = gdf.loc[gdf["role"] == "source"].geometry.iloc[0]
    # geopandas gibt BaseGeometry zurueck; eine Quelle ist eine Flaeche. Faellt
    # das aus, stimmt die Fixture nicht und der Test soll es sagen.
    assert isinstance(source, Polygon | MultiPolygon), type(source)
    obstacles = [
        Polygon(g.exterior)
        for g in gdf.loc[gdf["role"] == "obstacle"].geometry
        if isinstance(g, Polygon)
    ]
    return source, obstacles


def test_rust_sweep_matches_reference_on_random_scenes() -> None:
    """Punktpolygon des Kerns gegen die Referenz, ohne ``drop_collinear``.
    Die freien Kreisboegen sind verschieden diskretisiert (Referenz 180
    Segmente je Vollkreis in absoluten Winkeln, Kern eigener Schritt), deshalb
    keine Bit-Gleichheit, sondern Schranken nach Messung (``_assert_close``)."""
    rng = np.random.default_rng(20260914)
    checked = 0
    for _ in range(25):
        obstacles = _random_scene(rng)
        scene = RustScene(obstacles, 100.0)
        for _ in range(3):
            origin = (float(rng.uniform(-90, 90)), float(rng.uniform(-90, 90)))
            if any(o.buffer(0.5).contains(Point(origin)) for o in obstacles):
                continue
            expected = ref.visibility_polygon(origin, obstacles, 100.0)
            got = scene.polygon(origin, drop_collinear=False)
            _assert_close(expected, got, origin)
            checked += 1
    assert checked > 40, "zu wenige gueltige Faelle geprueft"


@pytest.mark.parametrize("name", ["gross", "anker"])
def test_rust_sweep_matches_reference_on_real_scenes(name: str) -> None:
    source, obstacles = _fixture(name)
    scene = RustScene(obstacles, REACH)
    for origin in ref.sample_source_points(source, 8):
        expected = ref.visibility_polygon(origin, obstacles, REACH)
        got = scene.polygon(origin, drop_collinear=False)
        _assert_close(expected, got, origin)


@pytest.mark.parametrize("name", ["mittel", "anker"])
def test_rust_zone_covers_the_reference_union_of_the_same_points(name: str) -> None:
    """Zwei Aussagen. (1) Die Vereinigung der Kern-Punktpolygone mit
    Strahllaenge = Reichweite ist die Vereinigung der Referenzpolygone
    derselben Punkte (relativ 1e-4, Bogendiskretisierung). (2) Die
    Produktionszone (ein Kernaufruf, Marge 0, Strahllaenge = Reichweite +
    Quellendurchmesser, Schnitt mit dem Freikreis) enthaelt
    diese Vereinigung; sie ist GROESSER, weil ein Beobachter auf der fernen
    Seite der Quelle bis an den Freikreis sieht (Anker: 8.550 statt 8.312 m2),
    und das ist Absicht."""
    source, obstacles = _fixture(name)
    points = sample_contour(source, 0.5, 32, 4096)
    free = free_circle(source, REACH)
    expected = union_all([ref.visibility_polygon(p, obstacles, REACH) for p in points] + [source])
    expected = expected.intersection(free)
    scene = RustScene(obstacles, REACH)
    per_point = union_all([scene.polygon(p) for p in points] + [source]).intersection(free)
    assert per_point.symmetric_difference(expected).area < 1e-4 * expected.area
    minx, miny, maxx, maxy = source.bounds
    max_len = REACH + math.hypot(maxx - minx, maxy - miny)
    zone = visible_region_all_rust(source, obstacles, REACH, max_len, points, free, 0.0)
    assert zone.is_valid
    assert expected.difference(zone).area < 1e-4 * expected.area, "Referenz ragt aus der Zone"
    assert zone.area >= expected.area
    assert zone.difference(free).area < 1e-6
