"""Zentrale Pipelinekonfiguration (Abschnitt 8: "ein Ort fuer alle Parameter").

Liest config/pipeline.toml. pydantic-settings erlaubt Ueberschreibung per
Umgebungsvariable (Praefix BUBATZBLICK_) fuer CI/Betrieb, ohne dass die
Datei die einzige Quelle bleiben muss.
"""

from __future__ import annotations

import tomllib
from pathlib import Path

from pydantic import BaseModel
from pydantic_settings import BaseSettings, SettingsConfigDict

DEFAULT_PIPELINE_TOML = Path(__file__).resolve().parents[2] / "config" / "pipeline.toml"


class CrsConfig(BaseModel):
    working: str = "EPSG:25832"
    output: str = "EPSG:4326"


class ReachConfig(BaseModel):
    # v7: nur die gesetzliche Sichtweite 100 m, ein Band.
    r_max_m: float = 100.0
    bands_m: list[int] = [100]
    data_quality_margin_m: float = 2.0


class GridConfig(BaseModel):
    # 2 km statt 10 km: Lastverteilung bei dichten Staedten, siehe den
    # Kommentar in config/pipeline.toml.
    cell_size_m: float = 2_000.0


class VisibilityConfig(BaseModel):
    """Abtastung des Sichtfelds (siehe config/pipeline.toml, `[visibility]`)."""

    spacing_m: float = 0.5
    margin_m: float = 0.5
    min_points: int = 32
    # Der Deckel setzt bei grossen Objekten `spacing_m` ausser Kraft, und
    # jeder Abstand ueber `spacing_m` reisst Luecken: auf Landesebene
    # gemessen, dass bei 1,67 m Abstand ein 9,8 m breiter Sichtkorridor
    # fehlte (osm_id 437966337, Deckel 4096) und bei 2-3 m Abstand 77 von
    # 101 Objekten Luecken bis 4,3 m hatten -- Schluessellöcher, die keine
    # Marge schliesst. Der Deckel ist deshalb so hoch, dass ihn KEIN
    # bekanntes Objekt erreicht (groesstes national: 13.185 m Umfang =
    # 26.369 Punkte); der Test test_config_point_cap_keeps_spacing sichert
    # das. Kosten gegenueber 4096: +0,22 % Punkte national.
    max_points: int = 32_768
    # Sweep, Vereinigung und Abschluss (Marge, Quelle, Freikreis) laufen im
    # Kern `bubatzcore`, ebenso Hindernisverschmelzung, Kreis-Puffer und
    # Baender. Ohne gebauten Kern bricht der Lauf ab (gewollt): `make core`
    # bzw. `uv run maturin develop --release -m rust/bubatzcore-py/Cargo.toml`.


class SimplifyConfig(BaseModel):
    tolerances_m: dict[str, float] = {"z8": 200, "z10": 80, "z12": 25, "z14": 0.5}
    # Bis zu diesem Zoom werden die Baender eines Objekts zu EINEM Merkmal
    # vereinigt (siehe tiles.rs); -1 = nie. z11 = 75 m/Pixel: die 25-50 m
    # breiten Baender sind dort unter einem Pixel.
    merge_bands_up_to_zoom: int = 11
    # Baender mit Toleranz <= diesem Wert bekommen die opaken Gebaeudegrund-
    # risse abgezogen (tiles.rs; nur mit --buildings-path); -1 = nie.
    subtract_buildings_max_tolerance_m: float = 8.0


class PipelineConfig(BaseSettings):
    """Nur die Abschnitte, die der Code tatsaechlich liest; unbekannte
    Abschnitte in pipeline.toml werden ignoriert."""

    model_config = SettingsConfigDict(
        env_prefix="BUBATZBLICK_", env_nested_delimiter="__", extra="ignore"
    )

    crs: CrsConfig = CrsConfig()
    reach: ReachConfig = ReachConfig()
    grid: GridConfig = GridConfig()
    visibility: VisibilityConfig = VisibilityConfig()
    simplify: SimplifyConfig = SimplifyConfig()

    @property
    def effective_reach_m(self) -> float:
        """r + Datenqualitaetszuschlag (E2). Das ist die einzige additive
        Vergroesserung des Radius im gesamten System."""
        return self.reach.r_max_m + self.reach.data_quality_margin_m


def load_config(path: Path = DEFAULT_PIPELINE_TOML) -> PipelineConfig:
    with path.open("rb") as f:
        raw = tomllib.load(f)
    return PipelineConfig(**raw)
