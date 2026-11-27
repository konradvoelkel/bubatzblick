//! `bubatz pedestrian` auf `tests/data/pedestrian.osm.pbf` (Quelle daneben als
//! `.osm`): ein Flaechen-Way, ein offener und ein geschlossener Linien-Way, eine
//! Multipolygon-Relation aus zwei Aussen-Ways und einem Loch, eine
//! unvollstaendige und eine fremde Relation. Geprueft wird die GeoJSONSeq
//! (`--no-tiles`), nicht tippecanoe.
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn ring(v: &Value) -> Vec<(f64, f64)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
        .collect()
}

/// Schnuerformel in Grad^2: > 0 gegen den Uhrzeigersinn (GeoJSON-Aussenring).
fn signed_area(r: &[(f64, f64)]) -> f64 {
    r.windows(2)
        .map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1)
        .sum::<f64>()
        / 2.0
}

#[test]
fn pedestrian_writes_ways_and_multipolygon_relations() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let out = std::env::temp_dir().join(format!(
        "bubatz-pedestrian-tiny-{}.geojsonl",
        std::process::id()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_bubatz"))
        .args([
            "pedestrian",
            &data.join("pedestrian.osm.pbf").to_string_lossy(),
            "--out-geojson",
            &out.to_string_lossy(),
            "--no-tiles",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("3 Ways -> 1 Flaechen, 2 Linien, 0 unvollstaendig; 2 Relationen -> 1 Flaechen, 1 unvollstaendig, 0 ungueltig"),
        "Zusammenfassung: {stdout}"
    );

    let text = std::fs::read_to_string(&out).unwrap();
    let features: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(features.len(), 4, "{text}");
    let by_key = |t: &str, id: i64| -> Value {
        features
            .iter()
            .find(|f| f["properties"]["osm_type"] == t && f["properties"]["osm_id"] == id)
            .unwrap_or_else(|| panic!("{t} {id} fehlt in {text}"))
            .clone()
    };
    for f in &features {
        assert_eq!(
            f["properties"].as_object().unwrap().len(),
            2,
            "nur osm_id und osm_type"
        );
    }

    // Way 200: geschlossen + area=yes -> Polygon, Ring geschlossen
    let w200 = by_key("way", 200);
    assert_eq!(w200["geometry"]["type"], "Polygon");
    let r = ring(&w200["geometry"]["coordinates"][0]);
    assert_eq!(r.len(), 5);
    assert_eq!(r.first(), r.last());
    assert_eq!(r[0], (6.78, 51.22));

    // Way 201: offen -> LineString; Way 202: geschlossen ohne area=yes -> LineString
    let w201 = by_key("way", 201);
    assert_eq!(w201["geometry"]["type"], "LineString");
    assert_eq!(ring(&w201["geometry"]["coordinates"]).len(), 3);
    let w202 = by_key("way", 202);
    assert_eq!(w202["geometry"]["type"], "LineString");
    assert_eq!(ring(&w202["geometry"]["coordinates"]).len(), 5);

    // Relation 300: zwei Aussen-Ways + Loch -> ein Polygon mit zwei Ringen,
    // aussen gegen den Uhrzeigersinn, Loch mit (RFC 7946), beide geschlossen,
    // Loch innerhalb des Aussenrings.
    let r300 = by_key("relation", 300);
    assert_eq!(r300["geometry"]["type"], "MultiPolygon");
    let polys = r300["geometry"]["coordinates"].as_array().unwrap();
    assert_eq!(polys.len(), 1);
    let rings = polys[0].as_array().unwrap();
    assert_eq!(rings.len(), 2, "Aussenring + Loch");
    let outer = ring(&rings[0]);
    let hole = ring(&rings[1]);
    assert_eq!(outer.len(), 5);
    assert_eq!(hole.len(), 5);
    assert_eq!(outer.first(), outer.last());
    assert_eq!(hole.first(), hole.last());
    assert!(signed_area(&outer) > 0.0, "Aussenring CCW: {outer:?}");
    assert!(signed_area(&hole) < 0.0, "Loch CW: {hole:?}");
    let (xs, ys): (Vec<f64>, Vec<f64>) = outer.iter().cloned().unzip();
    assert_eq!(xs.iter().cloned().fold(f64::MAX, f64::min), 6.79);
    assert_eq!(xs.iter().cloned().fold(f64::MIN, f64::max), 6.792);
    assert_eq!(ys.iter().cloned().fold(f64::MAX, f64::min), 51.23);
    assert_eq!(ys.iter().cloned().fold(f64::MIN, f64::max), 51.232);
    assert!(
        hole.iter()
            .all(|(x, y)| (6.79..6.792).contains(x) && (51.23..51.232).contains(y))
    );

    // Relation 301 (Mitglied fehlt) und 302 (highway=footway) fehlen
    assert!(!features.iter().any(|f| f["properties"]["osm_id"] == 301));
    assert!(!features.iter().any(|f| f["properties"]["osm_id"] == 302));
    let _ = std::fs::remove_file(&out);
}

/// Das Archiv nennt in seinen Metadaten keinen Pfad und keine Kommandozeile
/// (`tiles::scrub_metadata`). Braucht tippecanoe und pmtiles auf dem PATH,
/// sonst wird uebersprungen: die CI hat beide nicht, der pre-push-Hook schon.
#[test]
fn pedestrian_archive_metadata_names_no_path() {
    let on_path = |tool: &str| {
        Command::new(tool)
            .arg("--help")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    };
    if !on_path("tippecanoe") || !on_path("pmtiles") {
        eprintln!("uebersprungen: tippecanoe oder pmtiles fehlt");
        return;
    }
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let dir = std::env::temp_dir().join(format!("bubatz-pedestrian-meta-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let geojson = dir.join("fz.geojsonl");
    let archive = dir.join("fz.pmtiles");
    let output = Command::new(env!("CARGO_BIN_EXE_bubatz"))
        .args([
            "pedestrian",
            &data.join("pedestrian.osm.pbf").to_string_lossy(),
            "--out-geojson",
            &geojson.to_string_lossy(),
            "--out-path",
            &archive.to_string_lossy(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let shown = Command::new("pmtiles")
        .args(["show", "--metadata"])
        .arg(&archive)
        .output()
        .unwrap();
    assert!(
        shown.status.success(),
        "{}",
        String::from_utf8_lossy(&shown.stderr)
    );
    let meta: Value = serde_json::from_slice(&shown.stdout).unwrap();
    let text = meta.to_string();
    assert!(meta.get("generator_options").is_none(), "{text}");
    assert_eq!(meta["name"], "fussgaengerzonen");
    assert_eq!(meta["vector_layers"][0]["id"], "fussgaengerzonen");
    assert!(
        meta["attribution"]
            .as_str()
            .unwrap()
            .contains("OpenStreetMap")
    );
    assert!(
        !text.contains(&*dir.to_string_lossy()),
        "Pfad in den Metadaten: {text}"
    );
    assert!(!text.contains('/'), "Pfadartiges in den Metadaten: {text}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `--extra-geojson`: amtlich belegte Ergaenzungen landen hinter den OSM-Merkmalen;
/// eine Zeile ohne `amtlich: true` bricht den Lauf ab.
#[test]
fn pedestrian_appends_official_extras_and_rejects_others() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let tmp = std::env::temp_dir();
    let pid = std::process::id();
    let extra = tmp.join(format!("bubatz-pedestrian-extra-{pid}.geojsonl"));
    std::fs::write(
        &extra,
        concat!(
            r#"{"type":"Feature","properties":{"amtlich":true,"ort":"Testort"},"#,
            r#""geometry":{"type":"LineString","coordinates":[[6.0,51.0],[6.001,51.001]]}}"#,
            "\n\n"
        ),
    )
    .unwrap();
    let out = tmp.join(format!("bubatz-pedestrian-extra-out-{pid}.geojsonl"));
    let run = |extra: &Path| {
        Command::new(env!("CARGO_BIN_EXE_bubatz"))
            .args([
                "pedestrian",
                &data.join("pedestrian.osm.pbf").to_string_lossy(),
                "--out-geojson",
                &out.to_string_lossy(),
                "--no-tiles",
                "--extra-geojson",
                &extra.to_string_lossy(),
            ])
            .output()
            .unwrap()
    };
    let output = run(&extra);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 amtlich belegte Ergaenzungen"));
    let text = std::fs::read_to_string(&out).unwrap();
    let features: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(features.len(), 5, "{text}");
    assert_eq!(features[4]["properties"]["amtlich"], Value::Bool(true));
    assert_eq!(features[4]["properties"]["ort"], "Testort");

    let falsch = tmp.join(format!("bubatz-pedestrian-extra-falsch-{pid}.geojsonl"));
    std::fs::write(
        &falsch,
        r#"{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[6.0,51.0]}}"#,
    )
    .unwrap();
    let output = run(&falsch);
    assert!(
        !output.status.success(),
        "ohne amtlich = true haette der Lauf abbrechen muessen"
    );
    for p in [&extra, &falsch, &out] {
        let _ = std::fs::remove_file(p);
    }
}
