//! Tag-Matching -> Kategorie (liest `config/tags.v<N>.json`): erste passende
//! Kategorie in Deklarationsreihenfolge, Opazitaet, Plausibilitaetsgrenze.
//! Die JSON-Datei ist zugleich die Quelle der Ueber-Seite (title/description/
//! comment, `deploy/bin/build_about.py`); unbekannte Felder ignoriert serde.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::pbf::Tags;

#[derive(Debug, Deserialize)]
struct RuleJson {
    key: String,
    values: Vec<String>,
    /// Wert an ';' teilen und jedes Element pruefen (Mehrfachwerte wie
    /// `social_facility:for=child;juvenile`).
    #[serde(default)]
    split: bool,
}

#[derive(Debug, Deserialize)]
struct NameMatchJson {
    when: WhenJson,
    keys: Vec<String>,
    contains: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WhenJson {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct ExcludeJson {
    when: Option<WhenJson>,
    key: String,
    values: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PublicOnlyJson {
    tags: Vec<RuleJson>,
    public_access: Vec<String>,
    min_area_m2_if_untagged: f64,
}

#[derive(Debug, Deserialize)]
struct CategoryJson {
    osm_tags: Vec<RuleJson>,
    #[serde(default)]
    exclude: Vec<ExcludeJson>,
    #[serde(default)]
    public_only: Vec<PublicOnlyJson>,
    #[serde(default)]
    name_match: Vec<NameMatchJson>,
}

#[derive(Debug, Deserialize)]
struct NonOpaqueJson {
    building_values: Vec<String>,
    building_levels_zero: bool,
}

#[derive(Debug, Deserialize)]
struct TagsJson {
    version: u32,
    // serde_json::Map bewahrt die Reihenfolge nur mit dem Feature preserve_order;
    // die Reihenfolge der Kategorien ist Vertrag -> Vec aus dem Rohtext lesen.
    categories: serde_json::Value,
    non_opaque_exclusion: NonOpaqueJson,
    plausibility_limits_m2: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct TagRule {
    pub key: String,
    pub values: HashSet<String>,
    pub split: bool,
}

impl TagRule {
    pub fn hits(&self, tags: &Tags) -> bool {
        let Some(v) = tags.get(&self.key) else {
            return false;
        };
        if self.split {
            v.split(';').any(|p| self.values.contains(p.trim()))
        } else {
            self.values.contains(v)
        }
    }
}

/// `name_match`: unter `when` (Tag = Wert) passt das Objekt, wenn eines der
/// Felder `keys` (z. B. name, operator) einen der Teilstrings `contains`
/// enthaelt (ohne Gross-/Kleinschreibung). Nur `when.key` ist relevanter
/// Schluessel -- `name` selbst darf es nicht sein, sonst hielte `pbf::read`
/// jeden benannten Way mit allen Tags im Speicher.
#[derive(Debug, Clone)]
pub struct NameMatchRule {
    pub when: (String, String),
    pub keys: Vec<String>,
    pub contains: Vec<String>,
}

impl NameMatchRule {
    pub fn hits(&self, tags: &Tags) -> bool {
        tags.get(&self.when.0) == Some(self.when.1.as_str())
            && self.keys.iter().any(|k| {
                tags.get(k).is_some_and(|v| {
                    let v = v.to_lowercase();
                    self.contains.iter().any(|c| v.contains(c.as_str()))
                })
            })
    }
}

/// `exclude`: Objekt passt NICHT zur Kategorie, wenn (ggf. unter `when`)
/// `key` einen der `values` traegt.
#[derive(Debug, Clone)]
pub struct ExcludeRule {
    pub when: Option<(String, String)>,
    pub key: String,
    pub values: HashSet<String>,
}

/// `public_only`: Objekte mit einem dieser Tags zaehlen nur mit `access` aus
/// `public_access`, oder ohne `access`-Tag ab `min_area_m2_if_untagged`.
#[derive(Debug, Clone)]
pub struct PublicOnlyRule {
    pub tags: Vec<TagRule>,
    pub public_access: HashSet<String>,
    pub min_area_m2_if_untagged: f64,
}

#[derive(Debug, Clone)]
pub struct Category {
    pub name: String,
    pub rules: Vec<TagRule>,
    pub exclude: Vec<ExcludeRule>,
    pub public_only: Vec<PublicOnlyRule>,
    pub name_match: Vec<NameMatchRule>,
}

/// Ergebnis des Tag-Matchings: Kategorie und ggf. eine Mindestflaeche, die
/// der Aufrufer nach der Geometrie prueft (Punkt-Fallbacks haben keine echte
/// Flaeche und fallen dann weg).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Match<'a> {
    pub category: &'a str,
    pub min_area_m2: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct TagSpec {
    pub version: u32,
    /// Kategorien in Deklarationsreihenfolge.
    pub categories: Vec<Category>,
    pub non_opaque_building_values: HashSet<String>,
    pub non_opaque_levels_zero: bool,
    pub plausibility_limits_m2: HashMap<String, f64>,
    pub relevant_keys: HashSet<String>,
    /// Alle Schluessel, die diese Spezifikation (und `extract`) an einem
    /// Objekt liest: relevante Schluessel, `exclude`/`name_match`-Felder,
    /// `access`, `building:levels`, `area`, Plausibilitaetsschluessel. Nur
    /// diese Tags haelt `pbf::read` im Speicher.
    pub read_keys: HashSet<String>,
}

impl TagSpec {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("{} lesen", path.display()))?;
        let raw: TagsJson = serde_json::from_str(&text)?;
        // Kategorien in Dateireihenfolge: die Schluessel aus dem Text ziehen.
        let obj = raw.categories.as_object().context("categories")?;
        let order = category_order(&text)?;
        let mut categories = Vec::new();
        for name in order {
            let c: CategoryJson = serde_json::from_value(obj[&name].clone())?;
            let to_rule = |r: RuleJson| TagRule {
                key: r.key,
                values: r.values.into_iter().collect(),
                split: r.split,
            };
            let rules: Vec<TagRule> = c.osm_tags.into_iter().map(to_rule).collect();
            let exclude = c
                .exclude
                .into_iter()
                .map(|e| ExcludeRule {
                    when: e.when.map(|w| (w.key, w.value)),
                    key: e.key,
                    values: e.values.into_iter().collect(),
                })
                .collect();
            let public_only = c
                .public_only
                .into_iter()
                .map(|p| PublicOnlyRule {
                    tags: p.tags.into_iter().map(to_rule).collect(),
                    public_access: p.public_access.into_iter().collect(),
                    min_area_m2_if_untagged: p.min_area_m2_if_untagged,
                })
                .collect();
            let name_match = c
                .name_match
                .into_iter()
                .map(|n| NameMatchRule {
                    when: (n.when.key, n.when.value),
                    keys: n.keys,
                    contains: n.contains.into_iter().map(|c| c.to_lowercase()).collect(),
                })
                .collect();
            categories.push(Category {
                name,
                rules,
                exclude,
                public_only,
                name_match,
            });
        }
        let mut limits = HashMap::new();
        for (k, v) in raw
            .plausibility_limits_m2
            .as_object()
            .context("plausibility_limits_m2")?
        {
            if let Some(f) = v.as_f64() {
                limits.insert(k.clone(), f);
            }
        }
        let mut relevant_keys: HashSet<String> = categories
            .iter()
            .flat_map(|c| c.rules.iter().map(|r| r.key.clone()))
            .collect();
        for c in &categories {
            for e in &c.exclude {
                relevant_keys.insert(e.key.clone());
                if let Some((k, _)) = &e.when {
                    relevant_keys.insert(k.clone());
                }
            }
            for p in &c.public_only {
                relevant_keys.insert("access".to_string());
                relevant_keys.extend(p.tags.iter().map(|r| r.key.clone()));
            }
            for n in &c.name_match {
                relevant_keys.insert(n.when.0.clone());
            }
        }
        relevant_keys.insert("building".to_string());
        let mut read_keys = relevant_keys.clone();
        for c in &categories {
            for e in &c.exclude {
                read_keys.insert(e.key.clone());
                if let Some((k, _)) = &e.when {
                    read_keys.insert(k.clone());
                }
            }
            for n in &c.name_match {
                read_keys.insert(n.when.0.clone());
                read_keys.extend(n.keys.iter().cloned());
            }
            for p in &c.public_only {
                read_keys.extend(p.tags.iter().map(|r| r.key.clone()));
            }
        }
        for k in [
            "access",
            "building",
            "building:levels",
            "area",
            "leisure",
            "amenity",
        ] {
            read_keys.insert(k.to_string());
        }
        for k in limits.keys() {
            if let Some((key, _)) = k.split_once('=') {
                read_keys.insert(key.to_string());
            }
        }
        Ok(TagSpec {
            version: raw.version,
            categories,
            non_opaque_building_values: raw
                .non_opaque_exclusion
                .building_values
                .into_iter()
                .collect(),
            non_opaque_levels_zero: raw.non_opaque_exclusion.building_levels_zero,
            plausibility_limits_m2: limits,
            relevant_keys,
            read_keys,
        })
    }

    /// Erste passende Kategorie in Deklarationsreihenfolge; `None`, wenn
    /// keine passt oder eine `exclude`-/`public_only`-Regel das Objekt
    /// ausschliesst. Eine Mindestflaeche (`public_only` ohne `access`, das
    /// Maximum aller zutreffenden Regeln) prueft der Aufrufer; 0 heisst
    /// "zaehlt auch als Punkt".
    pub fn match_category(&self, tags: &Tags) -> Option<Match<'_>> {
        'cats: for c in &self.categories {
            let hit = c.rules.iter().any(|rule| rule.hits(tags))
                || c.name_match.iter().any(|n| n.hits(tags));
            if !hit {
                continue;
            }
            for e in &c.exclude {
                let when_ok = match &e.when {
                    Some((k, v)) => tags.get(k) == Some(v.as_str()),
                    None => true,
                };
                if when_ok && tags.get(&e.key).is_some_and(|v| e.values.contains(v)) {
                    continue 'cats;
                }
            }
            let mut min_area: Option<f64> = None;
            for p in &c.public_only {
                let applies = p.tags.iter().any(|rule| rule.hits(tags));
                if !applies {
                    continue;
                }
                match tags.get("access") {
                    Some(a) if p.public_access.contains(a) => {}
                    Some(_) => continue 'cats,
                    None => {
                        let m = p.min_area_m2_if_untagged;
                        min_area = Some(min_area.map_or(m, |x: f64| x.max(m)));
                    }
                }
            }
            return Some(Match {
                category: &c.name,
                min_area_m2: min_area,
            });
        }
        None
    }

    pub fn is_opaque_building(&self, tags: &Tags) -> bool {
        if let Some(b) = tags.get("building")
            && self.non_opaque_building_values.contains(b)
        {
            return false;
        }
        !(self.non_opaque_levels_zero && tags.get("building:levels") == Some("0"))
    }

    pub fn plausibility_limit_m2(&self, tags: &Tags) -> f64 {
        for key in ["leisure", "amenity", "building"] {
            if let Some(v) = tags.get(key)
                && let Some(l) = self.plausibility_limits_m2.get(&format!("{key}={v}"))
            {
                return *l;
            }
        }
        self.plausibility_limits_m2["default"]
    }
}

/// Schluessel des Objekts `"categories"` in Textreihenfolge (ohne Tolerierung
/// exotischer Formatierung: der Datei-Vertrag ist eine Kategorie je `"name": {`).
fn category_order(text: &str) -> Result<Vec<String>> {
    let start = text.find("\"categories\"").context("categories")?;
    let rest = &text[start..];
    let open = rest.find('{').context("{")? + 1;
    let mut depth = 1usize;
    let mut i = open;
    let bytes = rest.as_bytes();
    let mut names = Vec::new();
    while i < bytes.len() && depth > 0 {
        match bytes[i] as char {
            '{' => depth += 1,
            '}' => depth -= 1,
            '"' if depth == 1 => {
                let end = rest[i + 1..].find('"').context("\"")? + i + 1;
                names.push(rest[i + 1..end].to_string());
                i = end;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> TagSpec {
        TagSpec::load(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../config/tags.json"
        )))
        .unwrap()
    }

    #[test]
    fn v5_schools_sports_access_and_youth() {
        let s = spec();
        assert_eq!(s.version, 5);
        let m = |pairs: &[(&str, &str)]| s.match_category(&tags(pairs));
        let cat = |pairs: &[(&str, &str)]| m(pairs).map(|x| x.category);
        // Nr. 1: nur amenity=school; Fahr-/Sprachschulen weg, Musikschule -> youth
        assert_eq!(cat(&[("amenity", "school")]), Some("school"));
        assert_eq!(m(&[("amenity", "driving_school")]), None);
        assert_eq!(m(&[("amenity", "language_school")]), None);
        assert_eq!(cat(&[("amenity", "music_school")]), Some("youth"));
        // Nr. 2: Spielplaetze auch privat, Indoorspielplatz
        assert_eq!(
            cat(&[("leisure", "playground"), ("access", "private")]),
            Some("playground")
        );
        assert_eq!(cat(&[("leisure", "indoor_play")]), Some("playground"));
        // Nr. 4: Zugangsregel fuer alle Sportstaetten; ohne access zaehlt auch ein Punkt
        let track = m(&[("leisure", "track"), ("sport", "running")]).unwrap();
        assert_eq!((track.category, track.min_area_m2), ("sports", Some(0.0)));
        assert_eq!(
            m(&[("leisure", "horse_riding"), ("access", "private")]),
            None
        );
        assert_eq!(m(&[("leisure", "golf_course"), ("access", "no")]), None);
        assert_eq!(
            cat(&[("leisure", "golf_course"), ("access", "customers")]),
            Some("sports")
        );
        // Schwimmbecken: Maximum der Mindestflaechen (0 und 100) -> 100
        let pool = m(&[("leisure", "swimming_pool")]).unwrap();
        assert_eq!((pool.category, pool.min_area_m2), ("sports", Some(100.0)));
        assert_eq!(
            m(&[("leisure", "swimming_pool"), ("access", "private")]),
            None
        );
        // Tische bleiben Geraete
        assert_eq!(m(&[("leisure", "pitch"), ("sport", "table_tennis")]), None);
        assert_eq!(
            cat(&[("leisure", "sports_centre"), ("sport", "table_tennis")]),
            Some("sports")
        );
        // Nr. 3: social_facility nur mit :for Kind/Jugend (auch als Liste)
        assert_eq!(m(&[("amenity", "social_facility")]), None);
        assert_eq!(
            m(&[
                ("amenity", "social_facility"),
                ("social_facility", "nursing_home"),
                ("social_facility:for", "senior")
            ]),
            None
        );
        assert_eq!(
            cat(&[
                ("amenity", "social_facility"),
                ("social_facility:for", "child;juvenile")
            ]),
            Some("youth")
        );
        assert_eq!(
            cat(&[
                ("amenity", "social_facility"),
                ("social_facility:for", "disabled; juvenile")
            ]),
            Some("youth")
        );
        assert_eq!(
            cat(&[
                ("amenity", "community_centre"),
                ("community_centre", "youth_centre")
            ]),
            Some("youth")
        );
        assert_eq!(
            m(&[
                ("amenity", "community_centre"),
                ("community_centre", "village_hall")
            ]),
            None
        );
        assert_eq!(m(&[("amenity", "youth_centre")]), None);
        // Jugendherbergen ueber Name/Betreiber, nur bei tourism=hostel
        assert_eq!(
            cat(&[("tourism", "hostel"), ("name", "Jugendherberge Bonn")]),
            Some("youth")
        );
        assert_eq!(
            cat(&[
                ("tourism", "hostel"),
                ("name", "Haus Sonnenschein"),
                ("operator", "DJH Rheinland")
            ]),
            Some("youth")
        );
        assert_eq!(
            m(&[("tourism", "hostel"), ("name", "Backpacker Inn")]),
            None
        );
        assert_eq!(m(&[("name", "Jugendherberge (ehemalig)")]), None);
        assert!(s.relevant_keys.contains("tourism") && !s.relevant_keys.contains("name"));
        assert!(s.relevant_keys.contains("access") && s.relevant_keys.contains("sport"));
        assert!(s.relevant_keys.contains("social_facility:for"));
    }

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        Tags(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn categories_in_file_order_and_first_match_wins() {
        let s = spec();
        let names: Vec<&str> = s.categories.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "school",
                "kindergarten",
                "childcare",
                "playground",
                "sports",
                "youth",
                "cannabis_club"
            ]
        );
        let cat = |t: &Tags| s.match_category(t).map(|m| m.category.to_string());
        assert_eq!(
            cat(&tags(&[("amenity", "school")])).as_deref(),
            Some("school")
        );
        assert_eq!(
            cat(&tags(&[("leisure", "pitch"), ("amenity", "school")])).as_deref(),
            Some("school")
        );
        assert_eq!(cat(&tags(&[("building", "yes")])), None);
        assert!(s.relevant_keys.contains("building") && s.relevant_keys.contains("leisure"));
    }

    #[test]
    fn opacity_and_plausibility() {
        let s = spec();
        assert!(s.is_opaque_building(&tags(&[("building", "yes")])));
        assert!(!s.is_opaque_building(&tags(&[("building", "roof")])));
        assert!(!s.is_opaque_building(&tags(&[("building", "yes"), ("building:levels", "0")])));
        assert_eq!(
            s.plausibility_limit_m2(&tags(&[("amenity", "school")])),
            150_000.0
        );
        assert_eq!(
            s.plausibility_limit_m2(&tags(&[("leisure", "playground")])),
            50_000.0
        );
    }
}
