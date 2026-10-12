//! `bubatz extract` auf einer winzigen, von Hand gebauten PBF
//! (`tests/data/tiny.osm.pbf`): erwartete Objekte, Kategorien, Opazitaet und
//! byte-gleiche WKB stehen in `tests/data/tiny_expected.json`.
use std::path::Path;
use std::process::Command;

use arrow::array::{Array, AsArray};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

fn read_column_pairs(path: &Path, key_cols: &[&str]) -> Vec<(String, Vec<u8>)> {
    let f = std::fs::File::open(path).unwrap();
    let reader = ParquetRecordBatchReaderBuilder::try_new(f)
        .unwrap()
        .build()
        .unwrap();
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        let geo = batch.column_by_name("geometry").unwrap().as_binary::<i32>();
        for i in 0..batch.num_rows() {
            let mut key = String::new();
            for (k, c) in key_cols.iter().enumerate() {
                let col = batch.column_by_name(c).unwrap();
                let s = match col.data_type() {
                    arrow::datatypes::DataType::LargeUtf8 => {
                        col.as_string::<i64>().value(i).to_string()
                    }
                    arrow::datatypes::DataType::Int64 => col
                        .as_primitive::<arrow::datatypes::Int64Type>()
                        .value(i)
                        .to_string(),
                    other => panic!("{other}"),
                };
                if k > 0 {
                    key.push('/');
                }
                key.push_str(&s);
            }
            out.push((key, geo.value(i).to_vec()));
        }
    }
    out
}

#[test]
fn extract_matches_python_on_tiny_pbf() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let out = std::env::temp_dir().join(format!("bubatz-extract-tiny-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_bubatz"))
        .args([
            "--config",
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../config/pipeline.toml")
                .to_string_lossy(),
        ])
        .args([
            "extract",
            &data.join("tiny.osm.pbf").to_string_lossy(),
            "--out-dir",
            &out.to_string_lossy(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let expected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(data.join("tiny_expected.json")).unwrap())
            .unwrap();
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let protected = read_column_pairs(&out.join("protected.parquet"), &["osm_type", "osm_id"]);
    let buildings = read_column_pairs(&out.join("buildings.parquet"), &["osm_id"]);
    let exp_p = expected["protected"].as_object().unwrap();
    let exp_b = expected["buildings"].as_object().unwrap();
    assert_eq!(protected.len(), exp_p.len(), "Verbotsobjekte");
    assert_eq!(buildings.len(), exp_b.len(), "Gebaeude");
    for (k, wkb) in &protected {
        assert_eq!(hex(wkb), exp_p[k].as_str().unwrap(), "WKB {k}");
    }
    for (k, wkb) in &buildings {
        assert_eq!(hex(wkb), exp_b[k].as_str().unwrap(), "WKB Gebaeude {k}");
    }
    let summary: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("extract_summary.json")).unwrap())
            .unwrap();
    assert_eq!(summary["n_protected"], 3);
    assert_eq!(summary["n_buildings"], 3);
    let _ = std::fs::remove_dir_all(&out);
}
