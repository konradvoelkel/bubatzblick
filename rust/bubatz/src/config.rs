//! `config/pipeline.toml` -- dieselben Abschnitte und Defaults wie
//! `bubatzblick.config` (nur die gelesenen Felder).

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Reach {
    pub r_max_m: f64,
    pub bands_m: Vec<i64>,
    pub data_quality_margin_m: f64,
}
impl Default for Reach {
    fn default() -> Self {
        Reach {
            r_max_m: 100.0,
            bands_m: vec![100],
            data_quality_margin_m: 2.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Grid {
    pub cell_size_m: f64,
}
impl Default for Grid {
    fn default() -> Self {
        Grid {
            cell_size_m: 2000.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Visibility {
    pub spacing_m: f64,
    pub margin_m: f64,
    pub min_points: usize,
    pub max_points: usize,
}
impl Default for Visibility {
    fn default() -> Self {
        Visibility {
            spacing_m: 0.5,
            margin_m: 0.5,
            min_points: 32,
            max_points: 32_768,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Crs {
    pub working: String,
    pub output: String,
}
impl Default for Crs {
    fn default() -> Self {
        Crs {
            working: "EPSG:25832".into(),
            output: "EPSG:4326".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Simplify {
    pub tolerances_m: std::collections::BTreeMap<String, f64>,
    pub merge_bands_up_to_zoom: i32,
    pub subtract_buildings_max_tolerance_m: f64,
}
impl Default for Simplify {
    fn default() -> Self {
        Simplify {
            tolerances_m: [("z8", 200.0), ("z10", 80.0), ("z12", 25.0), ("z14", 0.5)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            merge_bands_up_to_zoom: 11,
            subtract_buildings_max_tolerance_m: 8.0,
        }
    }
}

/// Siehe [`Config::param_version`].
pub const PARAM_VERSION: &str = "v1";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub crs: Crs,
    pub reach: Reach,
    pub grid: Grid,
    pub visibility: Visibility,
    pub simplify: Simplify,
}

impl Config {
    pub fn effective_reach_m(&self) -> f64 {
        self.reach.r_max_m + self.reach.data_quality_margin_m
    }

    /// Parameterversion, Teil jedes `cache_key` und des Zellen-Fingerabdrucks:
    /// aendert sie sich, rechnet `zones` alle Objekte neu. `v7` = Reichweite
    /// 100 m, ein Band; `rust` = Geometrie im Kern `bubatzcore`. Wer am
    /// Verfahren etwas aendert, das die Zonengeometrie beruehrt, muss diesen
    /// Wert mitziehen -- sonst werden Baender aus einem fremden Verfahren
    /// weiterverwendet.
    pub fn param_version(&self) -> &'static str {
        PARAM_VERSION
    }
}

pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Konfiguration {} lesen", path.display()))?;
    let cfg: Config = toml::from_str(&text)
        .with_context(|| format!("Konfiguration {} parsen", path.display()))?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Parameterversion steckt in jedem `cache_key` und in jedem
    /// Zellen-Fingerabdruck. Aendert sie sich, rechnet der naechste Bundeslauf
    /// ~500 000 Objekte neu statt sie zu uebernehmen -- also pinnen, damit das
    /// eine Entscheidung bleibt und kein Nebeneffekt eines Refactorings.
    #[test]
    fn param_version_is_pinned() {
        assert_eq!(Config::default().param_version(), "v1");
    }

    /// Eine `pipeline.toml` aus einem aelteren Stand traegt noch
    /// `method`/`engine`/`union`/`finish`. Die Felder gibt es nicht mehr; die
    /// Datei muss trotzdem laden (serde ignoriert Unbekanntes), und die
    /// gelesenen Werte muessen stimmen.
    #[test]
    fn unknown_keys_from_an_older_config_are_ignored() {
        let cfg: Config = toml::from_str(
            r#"
            [visibility]
            method = "cones"
            engine = "alt"
            union = "geos"
            finish = "geos"
            spacing_m = 0.25
            "#,
        )
        .expect("aeltere Konfiguration muss laden");
        assert_eq!(cfg.visibility.spacing_m, 0.25);
        assert_eq!(cfg.visibility.margin_m, 0.5, "Vorgabe bleibt");
        assert_eq!(cfg.param_version(), "v1", "keine Engine-Wahl mehr");
    }

    /// `config/pipeline.toml` ist die Datei, mit der die Pipeline laeuft.
    #[test]
    fn the_shipped_config_loads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/pipeline.toml");
        let cfg = load(&path).expect("config/pipeline.toml muss laden");
        assert_eq!(cfg.reach.r_max_m, 100.0);
        assert_eq!(cfg.visibility.spacing_m, 0.5);
        assert_eq!(cfg.effective_reach_m(), 102.0);
    }
}
