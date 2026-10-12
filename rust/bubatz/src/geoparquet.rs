//! Zellendatei schreiben wie `zones.zones_to_gdf` + `parquet_io.atomic_write_parquet`
//! (geopandas `to_parquet`): Spalten osm_id, category, band_max_m, geometry
//! (WKB), cache_key; GeoParquet-Metadaten `geo` (Version 1.0.0, WKB, CRS
//! EPSG:25832 als PROJJSON, geometry_types, bbox) und GeoArrow-Feldmetadaten
//! wie geopandas 1.1.4 sie schreibt; Snappy; atomar ueber Geschwisterdatei.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow::array::{ArrayRef, BinaryBuilder, Int64Builder, LargeStringBuilder};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::metadata::KeyValue;
use parquet::file::properties::WriterProperties;

const GEO_TEMPLATE: &str = include_str!("geoparquet/geo_metadata_zones.json");
const GEOARROW_EXT: &str = include_str!("geoparquet/geoarrow_extension_metadata.json");

pub struct ZoneRow {
    pub osm_id: i64,
    /// "way", "relation" oder "node" (aus `protected.parquet` durchgereicht).
    pub osm_type: String,
    pub category: String,
    pub band_max_m: i64,
    pub wkb: Vec<u8>,
    pub is_multi: bool,
    pub bounds: [f64; 4],
    pub cache_key: String,
}

fn geo_metadata(rows: &[ZoneRow]) -> Result<String> {
    let mut geo: serde_json::Value = serde_json::from_str(GEO_TEMPLATE)?;
    let col = &mut geo["columns"]["geometry"];
    let mut types: Vec<&str> = Vec::new();
    if rows.iter().any(|r| r.is_multi) {
        types.push("MultiPolygon");
    }
    if rows.iter().any(|r| !r.is_multi) {
        types.push("Polygon");
    }
    col["geometry_types"] = serde_json::json!(types);
    if rows.is_empty() {
        col.as_object_mut().unwrap().remove("bbox");
    } else {
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for r in rows {
            b[0] = b[0].min(r.bounds[0]);
            b[1] = b[1].min(r.bounds[1]);
            b[2] = b[2].max(r.bounds[2]);
            b[3] = b[3].max(r.bounds[3]);
        }
        col["bbox"] = serde_json::json!(b);
    }
    Ok(serde_json::to_string(&geo)?)
}

/// Eine Zonen-Zellendatei schreiben, mit Schluessel-Wert-Metadaten im
/// Parquet-Kopf (z. B. `bubatz.cell_fp`).
pub fn write_cell_with_meta(path: &Path, rows: &[ZoneRow], extra: &[(&str, &str)]) -> Result<()> {
    let geometry_field = Field::new("geometry", DataType::Binary, true).with_metadata([
        (
            "ARROW:extension:name".to_string(),
            "geoarrow.wkb".to_string(),
        ),
        (
            "ARROW:extension:metadata".to_string(),
            GEOARROW_EXT.to_string(),
        ),
    ]);
    // Schluessel-Wert-Metadaten sowohl im Arrow-Schema (pyarrow/geopandas lesen
    // `geo` von dort) als auch als Parquet-Dateimetadaten (GeoParquet-Vertrag;
    // `parquet_io::kv_metadata`/`geo_bbox` lesen nur diese -- der ArrowWriter
    // legt Schema-Metadaten NUR in `ARROW:schema` ab).
    let kv: Vec<(String, String)> = [("geo".to_string(), geo_metadata(rows)?)]
        .into_iter()
        .chain(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())))
        .collect();
    let schema = Arc::new(
        Schema::new(vec![
            Field::new("osm_id", DataType::Int64, true),
            Field::new("osm_type", DataType::LargeUtf8, true),
            Field::new("category", DataType::LargeUtf8, true),
            Field::new("band_max_m", DataType::Int64, true),
            geometry_field,
            Field::new("cache_key", DataType::LargeUtf8, true),
        ])
        .with_metadata(
            kv.iter()
                .cloned()
                .collect::<std::collections::HashMap<_, _>>(),
        ),
    );
    let mut osm = Int64Builder::with_capacity(rows.len());
    let mut typ = LargeStringBuilder::new();
    let mut cat = LargeStringBuilder::new();
    let mut band = Int64Builder::with_capacity(rows.len());
    let mut geom = BinaryBuilder::new();
    let mut key = LargeStringBuilder::new();
    for r in rows {
        osm.append_value(r.osm_id);
        typ.append_value(&r.osm_type);
        cat.append_value(&r.category);
        band.append_value(r.band_max_m);
        geom.append_value(&r.wkb);
        key.append_value(&r.cache_key);
    }
    let columns: Vec<ArrayRef> = vec![
        Arc::new(osm.finish()),
        Arc::new(typ.finish()),
        Arc::new(cat.finish()),
        Arc::new(band.finish()),
        Arc::new(geom.finish()),
        Arc::new(key.finish()),
    ];
    let batch = RecordBatch::try_new(schema.clone(), columns)?;
    let tmp = path.with_extension(format!("parquet.tmp{}", std::process::id()));
    let file = std::fs::File::create(&tmp).with_context(|| format!("{} anlegen", tmp.display()))?;
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_key_value_metadata(Some(
            kv.iter()
                .map(|(k, v)| KeyValue::new(k.clone(), v.clone()))
                .collect(),
        ))
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("{} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

// --- extract: protected.parquet / buildings.parquet ---------------------------

const GEO_PROTECTED: &str = include_str!("geoparquet/geo_metadata_protected.json");
const GEO_BUILDINGS: &str = include_str!("geoparquet/geo_metadata_buildings.json");

pub struct ProtectedOut {
    pub osm_type: &'static str,
    pub osm_id: i64,
    pub category: String,
    pub is_point_fallback: bool,
    pub area_m2: f64,
    pub plausibility_warning: bool,
    pub wkb: Vec<u8>,
}

pub struct BuildingOut {
    pub osm_id: i64,
    pub is_opaque: bool,
    pub building_tag: String,
    pub wkb: Vec<u8>,
}

fn geometry_field() -> Field {
    Field::new("geometry", DataType::Binary, true).with_metadata([
        (
            "ARROW:extension:name".to_string(),
            "geoarrow.wkb".to_string(),
        ),
        (
            "ARROW:extension:metadata".to_string(),
            GEOARROW_EXT.to_string(),
        ),
    ])
}

/// Streaming-Writer wie `GeoParquetStreamWriter`: ein Row-Group je Block,
/// `geo`-Metadaten ohne bbox, atomar ueber Geschwisterdatei.
pub struct StreamWriter {
    schema: Arc<Schema>,
    writer: Option<ArrowWriter<std::fs::File>>,
    tmp: std::path::PathBuf,
    path: std::path::PathBuf,
    pub n_rows: usize,
}

impl StreamWriter {
    fn open(path: &Path, fields: Vec<Field>, geo: &str) -> Result<Self> {
        let schema =
            Arc::new(Schema::new(fields).with_metadata([("geo".to_string(), geo.to_string())]));
        let tmp = path.with_extension(format!("parquet.tmp{}", std::process::id()));
        let file =
            std::fs::File::create(&tmp).with_context(|| format!("{} anlegen", tmp.display()))?;
        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build();
        let writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;
        Ok(StreamWriter {
            schema,
            writer: Some(writer),
            tmp,
            path: path.to_path_buf(),
            n_rows: 0,
        })
    }

    pub fn protected(path: &Path) -> Result<Self> {
        Self::open(
            path,
            vec![
                Field::new("osm_type", DataType::LargeUtf8, true),
                Field::new("osm_id", DataType::Int64, true),
                Field::new("category", DataType::LargeUtf8, true),
                Field::new("is_point_fallback", DataType::Boolean, true),
                Field::new("area_m2", DataType::Float64, true),
                Field::new("plausibility_warning", DataType::Boolean, true),
                geometry_field(),
            ],
            GEO_PROTECTED,
        )
    }

    pub fn buildings(path: &Path) -> Result<Self> {
        Self::open(
            path,
            vec![
                Field::new("osm_id", DataType::Int64, true),
                Field::new("is_opaque", DataType::Boolean, true),
                Field::new("building_tag", DataType::LargeUtf8, true),
                geometry_field(),
            ],
            GEO_BUILDINGS,
        )
    }

    fn write_batch(&mut self, columns: Vec<ArrayRef>) -> Result<()> {
        let n = columns.first().map(|c| c.len()).unwrap_or(0);
        if n == 0 {
            return Ok(());
        }
        let batch = RecordBatch::try_new(self.schema.clone(), columns)?;
        self.writer
            .as_mut()
            .context("Writer geschlossen")?
            .write(&batch)?;
        self.n_rows += n;
        Ok(())
    }

    pub fn write_protected(&mut self, rows: &[ProtectedOut]) -> Result<()> {
        use arrow::array::{BooleanBuilder, Float64Builder};
        let mut t = LargeStringBuilder::new();
        let mut id = Int64Builder::with_capacity(rows.len());
        let mut cat = LargeStringBuilder::new();
        let mut pf = BooleanBuilder::with_capacity(rows.len());
        let mut area = Float64Builder::with_capacity(rows.len());
        let mut warn = BooleanBuilder::with_capacity(rows.len());
        let mut geom = BinaryBuilder::new();
        for r in rows {
            t.append_value(r.osm_type);
            id.append_value(r.osm_id);
            cat.append_value(&r.category);
            pf.append_value(r.is_point_fallback);
            area.append_value(r.area_m2);
            warn.append_value(r.plausibility_warning);
            geom.append_value(&r.wkb);
        }
        self.write_batch(vec![
            Arc::new(t.finish()),
            Arc::new(id.finish()),
            Arc::new(cat.finish()),
            Arc::new(pf.finish()),
            Arc::new(area.finish()),
            Arc::new(warn.finish()),
            Arc::new(geom.finish()),
        ])
    }

    pub fn write_buildings(&mut self, rows: &[BuildingOut]) -> Result<()> {
        use arrow::array::BooleanBuilder;
        let mut id = Int64Builder::with_capacity(rows.len());
        let mut op = BooleanBuilder::with_capacity(rows.len());
        let mut tag = LargeStringBuilder::new();
        let mut geom = BinaryBuilder::new();
        for r in rows {
            id.append_value(r.osm_id);
            op.append_value(r.is_opaque);
            tag.append_value(&r.building_tag);
            geom.append_value(&r.wkb);
        }
        self.write_batch(vec![
            Arc::new(id.finish()),
            Arc::new(op.finish()),
            Arc::new(tag.finish()),
            Arc::new(geom.finish()),
        ])
    }

    pub fn close(mut self) -> Result<()> {
        if let Some(w) = self.writer.take() {
            w.close()?;
        }
        std::fs::rename(&self.tmp, &self.path)
            .with_context(|| format!("{} -> {}", self.tmp.display(), self.path.display()))?;
        Ok(())
    }
}
