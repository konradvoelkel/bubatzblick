//! GeoParquet lesen (WKB-Spalte + Attribute) mit arrow/parquet.

use std::path::Path;

use anyhow::{Context, Result, bail};
use arrow::array::{Array, AsArray};
use arrow::datatypes::DataType;
use parquet::arrow::ProjectionMask;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

pub struct ProtectedRow {
    pub osm_id: i64,
    /// "way", "relation" oder "node" -- zusammen mit `osm_id` die Kennung des
    /// Objekts in OpenStreetMap. Reicht bis in den Kachelvertrag durch, damit
    /// das Popup auf die Quelle verlinken kann.
    pub osm_type: String,
    pub category: String,
    pub wkb: Vec<u8>,
}

pub(crate) fn string_at(col: &arrow::array::ArrayRef, i: usize) -> Result<String> {
    Ok(match col.data_type() {
        DataType::Utf8 => col.as_string::<i32>().value(i).to_string(),
        DataType::LargeUtf8 => col.as_string::<i64>().value(i).to_string(),
        other => bail!("String-Spalte erwartet, {other} gefunden"),
    })
}

pub(crate) fn binary_at(col: &arrow::array::ArrayRef, i: usize) -> Result<Vec<u8>> {
    Ok(match col.data_type() {
        DataType::Binary => col.as_binary::<i32>().value(i).to_vec(),
        DataType::LargeBinary => col.as_binary::<i64>().value(i).to_vec(),
        other => bail!("Binary-Spalte erwartet, {other} gefunden"),
    })
}

/// `protected.parquet`: osm_id, osm_type, category, geometry (Zeilenreihenfolge wie Datei).
pub fn read_protected(path: &Path) -> Result<Vec<ProtectedRow>> {
    let file = std::fs::File::open(path).with_context(|| format!("{} oeffnen", path.display()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.parquet_schema();
    let idx = |name: &str| -> Result<usize> {
        schema
            .columns()
            .iter()
            .position(|c| c.name() == name)
            .with_context(|| format!("Spalte {name} fehlt in {}", path.display()))
    };
    let mask = ProjectionMask::leaves(
        schema,
        [
            idx("osm_id")?,
            idx("osm_type")?,
            idx("category")?,
            idx("geometry")?,
        ],
    );
    let reader = builder
        .with_projection(mask)
        .with_batch_size(65_536)
        .build()?;
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch?;
        let osm = batch
            .column_by_name("osm_id")
            .context("osm_id")?
            .as_primitive::<arrow::datatypes::Int64Type>();
        let typ = batch
            .column_by_name("osm_type")
            .context("osm_type")?
            .clone();
        let cat = batch
            .column_by_name("category")
            .context("category")?
            .clone();
        let geo = batch
            .column_by_name("geometry")
            .context("geometry")?
            .clone();
        for i in 0..batch.num_rows() {
            rows.push(ProtectedRow {
                osm_id: osm.value(i),
                osm_type: string_at(&typ, i)?,
                category: string_at(&cat, i)?,
                wkb: binary_at(&geo, i)?,
            });
        }
    }
    Ok(rows)
}

/// Opake Gebaeude aus `buildings.parquet` der Reihe nach: `f(Zeilenindex, WKB)`
/// je opakem Gebaeude, blockweise gelesen, nichts wird gehalten (der
/// Gebaeudespeicher `bstore` baut daraus seine Kacheln).
pub fn for_each_opaque_building(
    path: &Path,
    mut f: impl FnMut(u32, &[u8]) -> Result<()>,
) -> Result<()> {
    let file = std::fs::File::open(path).with_context(|| format!("{} oeffnen", path.display()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.parquet_schema();
    let idx = |name: &str| -> Result<usize> {
        schema
            .columns()
            .iter()
            .position(|c| c.name() == name)
            .with_context(|| format!("Spalte {name} fehlt in {}", path.display()))
    };
    let mask = ProjectionMask::leaves(schema, [idx("is_opaque")?, idx("geometry")?]);
    let reader = builder
        .with_projection(mask)
        .with_batch_size(65_536)
        .build()?;
    let mut row = 0u32;
    for batch in reader {
        let batch = batch?;
        let opaque = batch
            .column_by_name("is_opaque")
            .context("is_opaque")?
            .as_boolean();
        let geo = batch
            .column_by_name("geometry")
            .context("geometry")?
            .clone();
        for i in 0..batch.num_rows() {
            if opaque.value(i) {
                match geo.data_type() {
                    DataType::Binary => f(row, geo.as_binary::<i32>().value(i))?,
                    DataType::LargeBinary => f(row, geo.as_binary::<i64>().value(i))?,
                    other => bail!("Binary-Spalte erwartet, {other} gefunden"),
                }
            }
            row += 1;
        }
    }
    Ok(())
}

/// Baender einer Zellendatei eines Vorlaufs (`PreviousZones`): je Zeile
/// (band_max_m, cache_key, WKB). Fehlt die Datei, leer.
pub struct PreviousBand {
    pub osm_id: i64,
    pub band_max_m: i64,
    pub cache_key: String,
    pub wkb: Vec<u8>,
}

pub fn read_previous_cell(path: &Path) -> Result<Vec<PreviousBand>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(path).with_context(|| format!("{} oeffnen", path.display()))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.parquet_schema();
    let idx = |name: &str| -> Result<usize> {
        schema
            .columns()
            .iter()
            .position(|c| c.name() == name)
            .with_context(|| format!("Spalte {name} fehlt in {}", path.display()))
    };
    let mask = ProjectionMask::leaves(
        schema,
        [
            idx("osm_id")?,
            idx("band_max_m")?,
            idx("cache_key")?,
            idx("geometry")?,
        ],
    );
    let reader = builder
        .with_projection(mask)
        .with_batch_size(65_536)
        .build()?;
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch?;
        let osm = batch
            .column_by_name("osm_id")
            .context("osm_id")?
            .as_primitive::<arrow::datatypes::Int64Type>();
        let band = batch
            .column_by_name("band_max_m")
            .context("band_max_m")?
            .as_primitive::<arrow::datatypes::Int64Type>();
        let key = batch
            .column_by_name("cache_key")
            .context("cache_key")?
            .clone();
        let geo = batch
            .column_by_name("geometry")
            .context("geometry")?
            .clone();
        for i in 0..batch.num_rows() {
            rows.push(PreviousBand {
                osm_id: osm.value(i),
                band_max_m: band.value(i),
                cache_key: string_at(&key, i)?,
                wkb: binary_at(&geo, i)?,
            });
        }
    }
    Ok(rows)
}

/// `protected.parquet` mit allen Spalten (fuer `clip`).
pub fn read_protected_full(path: &Path) -> Result<Vec<crate::geoparquet::ProtectedOut>> {
    let file = std::fs::File::open(path).with_context(|| format!("{} oeffnen", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?
        .with_batch_size(65_536)
        .build()?;
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch?;
        let t = batch
            .column_by_name("osm_type")
            .context("osm_type")?
            .clone();
        let osm = batch
            .column_by_name("osm_id")
            .context("osm_id")?
            .as_primitive::<arrow::datatypes::Int64Type>();
        let cat = batch
            .column_by_name("category")
            .context("category")?
            .clone();
        let pf = batch
            .column_by_name("is_point_fallback")
            .context("is_point_fallback")?
            .as_boolean();
        let area = batch
            .column_by_name("area_m2")
            .context("area_m2")?
            .as_primitive::<arrow::datatypes::Float64Type>();
        let warn = batch
            .column_by_name("plausibility_warning")
            .context("plausibility_warning")?
            .as_boolean();
        let geo = batch
            .column_by_name("geometry")
            .context("geometry")?
            .clone();
        for i in 0..batch.num_rows() {
            let osm_type: &'static str = match string_at(&t, i)?.as_str() {
                "way" => "way",
                "relation" => "relation",
                "node" => "node",
                other => bail!("osm_type {other}"),
            };
            rows.push(crate::geoparquet::ProtectedOut {
                osm_type,
                osm_id: osm.value(i),
                category: string_at(&cat, i)?,
                is_point_fallback: pf.value(i),
                area_m2: area.value(i),
                plausibility_warning: warn.value(i),
                wkb: binary_at(&geo, i)?,
            });
        }
    }
    Ok(rows)
}

pub fn count_rows(path: &Path) -> Result<usize> {
    let file = std::fs::File::open(path)?;
    let r = parquet::file::reader::SerializedFileReader::new(file)?;
    Ok(parquet::file::reader::FileReader::metadata(&r)
        .file_metadata()
        .num_rows() as usize)
}

/// Zeile einer Zonen-Zellendatei fuer `tiles`.
pub struct ZoneCellRow {
    pub osm_id: i64,
    /// "way", "relation" oder "node" -- Teil des Kachelvertrags, damit das
    /// Popup auf die Quelle verlinken kann. Fehlt die Spalte, bricht der Lauf
    /// ab: ein geratener Typ wuerde falsch verlinken, denn ein Weg und eine
    /// Relation koennen dieselbe Zahl tragen.
    pub osm_type: String,
    pub category: String,
    pub band_max_m: i64,
    pub cache_key: String,
    pub wkb: Vec<u8>,
}

pub fn read_zone_cell(path: &Path) -> Result<Vec<ZoneCellRow>> {
    let file = std::fs::File::open(path).with_context(|| format!("{} oeffnen", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?
        .with_batch_size(65_536)
        .build()?;
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch?;
        let osm = batch
            .column_by_name("osm_id")
            .context("osm_id")?
            .as_primitive::<arrow::datatypes::Int64Type>();
        let cat = batch
            .column_by_name("category")
            .context("category")?
            .clone();
        let band = batch
            .column_by_name("band_max_m")
            .context("band_max_m")?
            .as_primitive::<arrow::datatypes::Int64Type>();
        let key = batch
            .column_by_name("cache_key")
            .context("cache_key")?
            .clone();
        let typ = batch
            .column_by_name("osm_type")
            .context("osm_type")?
            .clone();
        let geo = batch
            .column_by_name("geometry")
            .context("geometry")?
            .clone();
        for i in 0..batch.num_rows() {
            rows.push(ZoneCellRow {
                osm_id: osm.value(i),
                osm_type: string_at(&typ, i)?,
                category: string_at(&cat, i)?,
                band_max_m: band.value(i),
                cache_key: string_at(&key, i)?,
                wkb: binary_at(&geo, i)?,
            });
        }
    }
    Ok(rows)
}

/// bbox aus den GeoParquet-Metadaten (`geo.columns.geometry.bbox`), sonst None.
/// Ein Schluessel-Wert-Metadatum aus dem Parquet-Kopf (nur Metadaten lesen).
pub fn kv_metadata(path: &Path, key: &str) -> Result<Option<String>> {
    let file = std::fs::File::open(path)?;
    let r = parquet::file::reader::SerializedFileReader::new(file)?;
    let md = parquet::file::reader::FileReader::metadata(&r);
    Ok(md.file_metadata().key_value_metadata().and_then(|kv| {
        kv.iter()
            .find(|e| e.key == key)
            .and_then(|e| e.value.clone())
    }))
}

pub fn geo_bbox(path: &Path) -> Result<Option<[f64; 4]>> {
    let file = std::fs::File::open(path)?;
    let r = parquet::file::reader::SerializedFileReader::new(file)?;
    let md = parquet::file::reader::FileReader::metadata(&r);
    let kv = md.file_metadata().key_value_metadata();
    if let Some(kv) = kv {
        for e in kv {
            if e.key == "geo"
                && let Some(v) = &e.value
            {
                let g: serde_json::Value = serde_json::from_str(v)?;
                if let Some(b) = g["columns"]["geometry"]["bbox"].as_array()
                    && b.len() == 4
                {
                    return Ok(Some([
                        b[0].as_f64().unwrap_or(f64::NAN),
                        b[1].as_f64().unwrap_or(f64::NAN),
                        b[2].as_f64().unwrap_or(f64::NAN),
                        b[3].as_f64().unwrap_or(f64::NAN),
                    ]));
                }
            }
        }
    }
    Ok(None)
}
