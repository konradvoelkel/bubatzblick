//! `bubatz serve` -- Vorschau- und Kachelserver in einem Prozess: die
//! eingebettete Viewer-Seite (`viewer/index.html` samt MapLibre und
//! pmtiles.js aus `viewer/vendor/`, kein CDN) und Kacheln aus einem oder
//! mehreren `zonen.pmtiles`-Archiven. Ersetzt `pmtiles serve` je Archiv
//! plus `python3 -m http.server` (eine Einheit statt vier).
//!
//! Ein Archiv wird bei jeder Anfrage per `stat` auf Inode, mtime und
//! Groesse geprueft und nach einem `make tiles` (atomarer `rename` in
//! `tiles.rs`) ohne Neustart neu geoeffnet. Der ETag ist aus derselben
//! Identitaet gebildet, `Cache-Control: no-cache` zwingt den Browser zur
//! Rueckfrage: nach einem Lauf gibt es keine alten Kacheln, dazwischen nur
//! 304er. Kacheln liegen im Archiv bereits gzip-komprimiert (tippecanoe)
//! und gehen unveraendert mit `Content-Encoding` aus dem Archivkopf raus.
//!
//! Routen: `/` (Seite), `/vendor/*`, `/datasets` (JSON), `/healthz`,
//! `/{name}/tiles.json`, `/{name}/summary.json`,
//! `/{name}/zonen/{z}/{x}/{y}.mvt`; `/zonen/{z}/{x}/{y}.mvt` = erstes Archiv.

use std::net::SocketAddr;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::body::Body;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use clap::Args;
use pmtiles::{AsyncPmTilesReader, MmapBackend, TileCoord};
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{info, warn};

const INDEX_HTML: &str = include_str!("../../../viewer/index.html");
const APP_JS: &[u8] = include_bytes!("../../../viewer/app.js");
const MAPLIBRE_JS: &[u8] = include_bytes!("../../../viewer/vendor/maplibre-gl.js");
const MAPLIBRE_CSS: &[u8] = include_bytes!("../../../viewer/vendor/maplibre-gl.css");
const PMTILES_JS: &[u8] = include_bytes!("../../../viewer/vendor/pmtiles.js");
const ARCHIVE_FILE: &str = "zonen.pmtiles";
const ATTRIBUTION: &str = "&copy; OpenStreetMap-Mitwirkende (ODbL)";

#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Archiv als NAME=PFAD (Datei oder Verzeichnis mit zonen.pmtiles);
    /// mehrfach moeglich, das erste ist der Standard fuer /zonen/...
    #[arg(long = "dataset", value_name = "NAME=PFAD", required = true)]
    pub datasets: Vec<String>,
    /// Adresse:Port
    #[arg(long, default_value = "127.0.0.1:8787")]
    pub bind: SocketAddr,
    /// Verzeichnis der Seite (config.js, vendor/basemaps/ mit Glyphen und
    /// Sprites); alles darin wird statisch ausgeliefert, mit Range-Anfragen
    #[arg(long, default_value = "viewer")]
    pub viewer_dir: PathBuf,
    /// Protomaps-Grundkarte (PMTiles), ausgeliefert als /basemap.pmtiles;
    /// ohne sie zeigt die Seite OSM-Rasterkacheln als Notnagel
    #[arg(long)]
    pub basemap: Option<PathBuf>,
}

/// Identitaet der Archivdatei auf der Platte; aendert sich mit jedem
/// `rename` eines neuen Laufs und bildet den ETag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Ident {
    ino: u64,
    mtime_ns: i128,
    len: u64,
}

impl Ident {
    fn of(path: &Path) -> std::io::Result<Self> {
        let m = std::fs::metadata(path)?;
        Ok(Self {
            ino: m.ino(),
            mtime_ns: i128::from(m.mtime()) * 1_000_000_000 + i128::from(m.mtime_nsec()),
            len: m.len(),
        })
    }

    fn etag(&self) -> HeaderValue {
        HeaderValue::from_str(&format!(
            "\"{:x}-{:x}-{:x}\"",
            self.ino, self.mtime_ns, self.len
        ))
        .expect("hex ist ASCII")
    }

    fn modified_secs(&self) -> i128 {
        self.mtime_ns.div_euclid(1_000_000_000)
    }
}

struct Opened {
    reader: AsyncPmTilesReader<MmapBackend>,
    ident: Ident,
    etag: HeaderValue,
    metadata: Value,
}

pub struct Dataset {
    name: String,
    path: PathBuf,
    opened: RwLock<Option<Arc<Opened>>>,
}

impl Dataset {
    /// `path` ist die Archivdatei oder ein Verzeichnis mit `zonen.pmtiles`.
    /// Die Datei muss beim Start nicht existieren (ein Lauf kann sie noch
    /// schreiben); bis dahin antwortet der Server mit 503.
    pub fn new(name: &str, path: &Path) -> Result<Self> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            bail!("Archivname {name:?}: nur [A-Za-z0-9_-]");
        }
        let path = if path.is_dir() {
            path.join(ARCHIVE_FILE)
        } else {
            path.to_path_buf()
        };
        Ok(Self {
            name: name.to_string(),
            path,
            opened: RwLock::new(None),
        })
    }

    async fn open(&self) -> Result<Arc<Opened>> {
        let ident = Ident::of(&self.path).with_context(|| self.path.display().to_string())?;
        if let Some(o) = self.opened.read().await.as_ref()
            && o.ident == ident
        {
            return Ok(o.clone());
        }
        let reader = AsyncPmTilesReader::new_with_path(&self.path)
            .await
            .with_context(|| format!("{} oeffnen", self.path.display()))?;
        let metadata = match reader.get_metadata().await {
            Ok(s) => serde_json::from_str(&s).unwrap_or(Value::Null),
            Err(e) => {
                warn!("serve: {}: Metadaten unlesbar: {e}", self.name);
                Value::Null
            }
        };
        let h = reader.get_header();
        info!(
            "serve: {} = {} ({} Bytes, z{}-{}, {:?}, Kacheln {:?})",
            self.name,
            self.path.display(),
            ident.len,
            h.min_zoom,
            h.max_zoom,
            h.tile_type,
            h.tile_compression
        );
        let o = Arc::new(Opened {
            reader,
            ident,
            etag: ident.etag(),
            metadata,
        });
        *self.opened.write().await = Some(o.clone());
        Ok(o)
    }

    fn summary_path(&self) -> PathBuf {
        let stem = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("zonen");
        self.path
            .with_file_name(format!("{stem}_tiles_summary.json"))
    }
}

type Shared = Arc<Vec<Dataset>>;

fn find<'a>(ds: &'a [Dataset], name: &str) -> Option<&'a Dataset> {
    ds.iter().find(|d| d.name == name)
}

/// Basis-URL fuer absolute Kachel-URLs in TileJSON (MapLibre verlangt
/// absolute), aus Host bzw. Proxy-Headern.
fn base_url(headers: &HeaderMap) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1");
    format!("{scheme}://{host}")
}

fn unavailable(d: &Dataset, e: &anyhow::Error) -> Response {
    warn!("serve: {} nicht verfuegbar: {e:#}", d.name);
    (
        StatusCode::SERVICE_UNAVAILABLE,
        format!("{}: {e:#}\n", d.name),
    )
        .into_response()
}

async fn tile_for(d: Option<&Dataset>, z: u8, x: u32, y: &str, headers: &HeaderMap) -> Response {
    let Some(d) = d else {
        return (StatusCode::NOT_FOUND, "unbekanntes Archiv\n").into_response();
    };
    let y = y
        .strip_suffix(".mvt")
        .or_else(|| y.strip_suffix(".pbf"))
        .unwrap_or(y);
    let Ok(y) = y.parse::<u32>() else {
        return (StatusCode::BAD_REQUEST, "y\n").into_response();
    };
    let Ok(coord) = TileCoord::new(z, x, y) else {
        return (StatusCode::BAD_REQUEST, "Kachelkoordinate\n").into_response();
    };
    let o = match d.open().await {
        Ok(o) => o,
        Err(e) => return unavailable(d, &e),
    };
    if headers
        .get(header::IF_NONE_MATCH)
        .is_some_and(|v| v == o.etag)
    {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, o.etag.clone())
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::empty())
            .expect("Antwort");
    }
    match o.reader.get_tile(coord).await {
        Ok(Some(bytes)) => {
            let h = o.reader.get_header();
            let mut b = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, h.tile_type.content_type())
                .header(header::ETAG, o.etag.clone())
                .header(header::CACHE_CONTROL, "no-cache");
            if let Some(enc) = h.tile_compression.content_encoding() {
                b = b.header(header::CONTENT_ENCODING, enc);
            }
            b.body(Body::from(bytes)).expect("Antwort")
        }
        Ok(None) => Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header(header::ETAG, o.etag.clone())
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::empty())
            .expect("Antwort"),
        Err(e) => {
            warn!("serve: {} {z}/{x}/{y}: {e}", d.name);
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}\n")).into_response()
        }
    }
}

async fn tile(
    State(ds): State<Shared>,
    UrlPath((name, z, x, y)): UrlPath<(String, u8, u32, String)>,
    headers: HeaderMap,
) -> Response {
    tile_for(find(&ds, &name), z, x, &y, &headers).await
}

async fn tile_default(
    State(ds): State<Shared>,
    UrlPath((z, x, y)): UrlPath<(u8, u32, String)>,
    headers: HeaderMap,
) -> Response {
    tile_for(ds.first(), z, x, &y, &headers).await
}

fn describe(d: &Dataset, o: &Opened) -> Value {
    let h = o.reader.get_header();
    json!({
        "minzoom": h.min_zoom,
        "maxzoom": h.max_zoom,
        "bounds": [h.min_longitude, h.min_latitude, h.max_longitude, h.max_latitude],
        "center": [h.center_longitude, h.center_latitude, h.center_zoom],
        "bytes": o.ident.len,
        "modified": o.ident.modified_secs(),
        "etag": o.etag.to_str().unwrap_or(""),
        "path": d.path.display().to_string(),
    })
}

async fn datasets_json(State(ds): State<Shared>, headers: HeaderMap) -> Json<Value> {
    let base = base_url(&headers);
    let mut out = Vec::with_capacity(ds.len());
    for d in ds.iter() {
        let mut v = json!({
            "name": d.name,
            "tiles": format!("{base}/{}/zonen/{{z}}/{{x}}/{{y}}.mvt", d.name),
            "tilejson": format!("{base}/{}/tiles.json", d.name),
        });
        match d.open().await {
            Ok(o) => {
                if let (Value::Object(m), Value::Object(extra)) = (&mut v, describe(d, &o)) {
                    m.extend(extra);
                }
            }
            Err(e) => v["error"] = Value::String(format!("{e:#}")),
        }
        out.push(v);
    }
    Json(Value::Array(out))
}

async fn tilejson(
    State(ds): State<Shared>,
    UrlPath(name): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    let Some(d) = find(&ds, &name) else {
        return (StatusCode::NOT_FOUND, "unbekanntes Archiv\n").into_response();
    };
    let o = match d.open().await {
        Ok(o) => o,
        Err(e) => return unavailable(d, &e),
    };
    let h = o.reader.get_header();
    let mut v = json!({
        "tilejson": "3.0.0",
        "name": d.name,
        "tiles": [format!("{}/{}/zonen/{{z}}/{{x}}/{{y}}.mvt", base_url(&headers), d.name)],
        "minzoom": h.min_zoom,
        "maxzoom": h.max_zoom,
        "bounds": [h.min_longitude, h.min_latitude, h.max_longitude, h.max_latitude],
        "center": [h.center_longitude, h.center_latitude, h.center_zoom],
        "attribution": ATTRIBUTION,
    });
    if let Some(layers) = o.metadata.get("vector_layers") {
        v["vector_layers"] = layers.clone();
    }
    ([(header::CACHE_CONTROL, "no-cache")], Json(v)).into_response()
}

async fn summary(State(ds): State<Shared>, UrlPath(name): UrlPath<String>) -> Response {
    let Some(d) = find(&ds, &name) else {
        return (StatusCode::NOT_FOUND, "unbekanntes Archiv\n").into_response();
    };
    match tokio::fs::read(d.summary_path()).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "keine Summary\n").into_response(),
    }
}

fn asset(content_type: &'static str, bytes: &'static [u8]) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response()
}

async fn cors(mut r: Response) -> Response {
    r.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    r
}

pub fn router(datasets: Vec<Dataset>, viewer_dir: Option<&Path>, basemap: Option<&Path>) -> Router {
    let mut r = Router::new();
    if let Some(p) = basemap {
        r = r.route_service("/basemap.pmtiles", ServeFile::new(p));
    }
    if let Some(d) = viewer_dir {
        // config.js, vendor/basemaps/... und alles andere von der Platte; die
        // eingebetteten Dateien unten haben Vorrang (Routen vor dem Fallback).
        r = r.fallback_service(ServeDir::new(d));
    }
    r.route(
        "/",
        get(|| async { ([(header::CACHE_CONTROL, "no-cache")], Html(INDEX_HTML)) }),
    )
    .route(
        "/app.js",
        get(|| async { asset("text/javascript", APP_JS) }),
    )
    .route("/healthz", get(|| async { "ok\n" }))
    .route("/datasets", get(datasets_json))
    .route(
        "/vendor/maplibre-gl.js",
        get(|| async { asset("text/javascript", MAPLIBRE_JS) }),
    )
    .route(
        "/vendor/maplibre-gl.css",
        get(|| async { asset("text/css", MAPLIBRE_CSS) }),
    )
    .route(
        "/vendor/pmtiles.js",
        get(|| async { asset("text/javascript", PMTILES_JS) }),
    )
    .route("/zonen/{z}/{x}/{y}", get(tile_default))
    .route("/{name}/tiles.json", get(tilejson))
    .route("/{name}/summary.json", get(summary))
    .route("/{name}/zonen/{z}/{x}/{y}", get(tile))
    .layer(axum::middleware::map_response(cors))
    .with_state(Arc::new(datasets))
}

pub fn parse_datasets(specs: &[String]) -> Result<Vec<Dataset>> {
    let mut out = Vec::with_capacity(specs.len());
    for s in specs {
        let (name, path) = s
            .split_once('=')
            .with_context(|| format!("--dataset {s:?}: Form NAME=PFAD"))?;
        if out.iter().any(|d: &Dataset| d.name == name) {
            bail!("--dataset {name}: doppelt");
        }
        out.push(Dataset::new(name, Path::new(path))?);
    }
    if out.is_empty() {
        bail!("mindestens ein --dataset");
    }
    Ok(out)
}

pub fn run(args: ServeArgs) -> Result<()> {
    let datasets = parse_datasets(&args.datasets)?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        for d in &datasets {
            if let Err(e) = d.open().await {
                warn!(
                    "serve: {} noch nicht lesbar ({e:#}); wird bei Bedarf erneut versucht",
                    d.name
                );
            }
        }
        let listener = tokio::net::TcpListener::bind(args.bind)
            .await
            .with_context(|| format!("{} binden", args.bind))?;
        let names: Vec<&str> = datasets.iter().map(|d| d.name.as_str()).collect();
        info!(
            "serve: http://{}/  Archive: {}",
            args.bind,
            names.join(", ")
        );
        let viewer_dir = args.viewer_dir.is_dir().then_some(args.viewer_dir.as_path());
        if viewer_dir.is_none() {
            warn!(
                "serve: --viewer-dir {} fehlt; config.js und Grundkartenstil werden nicht ausgeliefert",
                args.viewer_dir.display()
            );
        }
        axum::serve(listener, router(datasets, viewer_dir, args.basemap.as_deref()))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
                info!("serve: Ende");
            })
            .await
            .context("HTTP-Server")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use pmtiles::{Compression, PmTilesWriter, TileType};
    use tower::ServiceExt;

    const Z: u8 = 12;
    const X: u32 = 2145;
    const Y: u32 = 1378;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bubatz-serve-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    fn write_archive(path: &Path, gzip: bool, payload: &[u8]) {
        let f = std::fs::File::create(path).unwrap();
        let mut w = PmTilesWriter::new(TileType::Mvt)
            .min_zoom(8)
            .max_zoom(14)
            .bounds(5.0, 47.0, 15.0, 55.0)
            .metadata(r#"{"vector_layers":[{"id":"zonen","fields":{}}]}"#);
        if !gzip {
            w = w.tile_compression(Compression::None);
        }
        let mut s = w.create(f).unwrap();
        let c = TileCoord::new(Z, X, Y).unwrap();
        if gzip {
            s.add_tile(c, payload).unwrap();
        } else {
            s.add_raw_tile(c, payload).unwrap();
        }
        s.finalize().unwrap();
    }

    async fn get(
        app: &Router,
        uri: &str,
        etag: Option<&HeaderValue>,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut req = Request::builder().uri(uri).header(header::HOST, "h:1");
        if let Some(t) = etag {
            req = req.header(header::IF_NONE_MATCH, t);
        }
        let r = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = r.status();
        let headers = r.headers().clone();
        let body = r.into_body().collect().await.unwrap().to_bytes().to_vec();
        (status, headers, body)
    }

    #[tokio::test]
    async fn tiles_headers_and_routes() {
        let p = tmp("gz.pmtiles");
        write_archive(&p, true, b"kachel");
        let app = router(vec![Dataset::new("t", &p).unwrap()], None, None);

        let (st, h, body) = get(&app, &format!("/t/zonen/{Z}/{X}/{Y}.mvt"), None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(
            h[header::CONTENT_TYPE],
            "application/vnd.mapbox-vector-tile"
        );
        assert_eq!(h[header::CONTENT_ENCODING], "gzip");
        assert_eq!(h[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert_eq!(h[header::CACHE_CONTROL], "no-cache");
        assert_eq!(
            &body[..2],
            &[0x1f, 0x8b],
            "gzip-Magic, unveraendert durchgereicht"
        );
        let etag = h[header::ETAG].clone();

        let (st, _, _) = get(&app, &format!("/t/zonen/{Z}/{X}/{Y}.mvt"), Some(&etag)).await;
        assert_eq!(st, StatusCode::NOT_MODIFIED);
        let (st, _, _) = get(&app, &format!("/zonen/{Z}/{X}/{Y}.mvt"), None).await;
        assert_eq!(st, StatusCode::OK, "erstes Archiv ist der Standard");
        let (st, _, _) = get(&app, &format!("/t/zonen/{Z}/0/0.mvt"), None).await;
        assert_eq!(st, StatusCode::NO_CONTENT, "fehlende Kachel = leer");
        let (st, _, _) = get(&app, &format!("/t/zonen/{Z}/{X}/abc.mvt"), None).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        let (st, _, _) = get(&app, "/t/zonen/40/1/1.mvt", None).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "z > 30");
        let (st, _, _) = get(&app, &format!("/nope/zonen/{Z}/{X}/{Y}.mvt"), None).await;
        assert_eq!(st, StatusCode::NOT_FOUND);

        let (st, h, body) = get(&app, "/t/tiles.json", None).await;
        assert_eq!(st, StatusCode::OK);
        assert!(
            h[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json")
        );
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["tiles"][0], "http://h:1/t/zonen/{z}/{x}/{y}.mvt");
        assert_eq!(v["minzoom"], 8);
        assert_eq!(v["maxzoom"], 14);
        assert_eq!(v["vector_layers"][0]["id"], "zonen");
        assert_eq!(v["bounds"][2], 15.0);

        let (st, _, body) = get(&app, "/datasets", None).await;
        assert_eq!(st, StatusCode::OK);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["name"], "t");
        assert!(v[0]["bytes"].as_u64().unwrap() > 0);
        assert!(v[0].get("error").is_none());

        let (st, _, _) = get(&app, "/t/summary.json", None).await;
        assert_eq!(st, StatusCode::NOT_FOUND);
        std::fs::write(tmp("gz_tiles_summary.json"), b"{\"n\":1}").unwrap();
        let (st, _, body) = get(&app, "/t/summary.json", None).await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body, b"{\"n\":1}");

        let (st, _, body) = get(&app, "/healthz", None).await;
        assert_eq!((st, body.as_slice()), (StatusCode::OK, &b"ok\n"[..]));
        let (st, h, body) = get(&app, "/", None).await;
        assert_eq!(st, StatusCode::OK);
        assert!(
            h[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        assert!(
            std::str::from_utf8(&body)
                .unwrap()
                .contains("vendor/maplibre-gl.js")
        );
        for (uri, ct) in [
            ("/vendor/maplibre-gl.js", "text/javascript"),
            ("/vendor/maplibre-gl.css", "text/css"),
            ("/vendor/pmtiles.js", "text/javascript"),
        ] {
            let (st, h, body) = get(&app, uri, None).await;
            assert_eq!(st, StatusCode::OK, "{uri}");
            assert_eq!(h[header::CONTENT_TYPE], ct);
            assert!(body.len() > 1000);
        }
    }

    #[tokio::test]
    async fn reopens_after_atomic_replace() {
        let p = tmp("swap.pmtiles");
        write_archive(&p, false, b"alt");
        let app = router(vec![Dataset::new("s", &p).unwrap()], None, None);
        let uri = format!("/s/zonen/{Z}/{X}/{Y}.mvt");
        let (st, h, body) = get(&app, &uri, None).await;
        assert_eq!((st, body.as_slice()), (StatusCode::OK, &b"alt"[..]));
        assert!(
            h.get(header::CONTENT_ENCODING).is_none(),
            "unkomprimiertes Archiv"
        );
        let etag_alt = h[header::ETAG].clone();

        // Wie tiles.rs: neue Datei daneben schreiben, per rename ersetzen.
        let tmp_new = tmp("swap.pmtiles.tmp");
        write_archive(&tmp_new, false, b"neu");
        std::fs::rename(&tmp_new, &p).unwrap();

        let (st, h, body) = get(&app, &uri, None).await;
        assert_eq!((st, body.as_slice()), (StatusCode::OK, &b"neu"[..]));
        assert_ne!(h[header::ETAG], etag_alt);
        let (st, _, _) = get(&app, &uri, Some(&etag_alt)).await;
        assert_eq!(
            st,
            StatusCode::OK,
            "alter ETag passt nicht mehr -> voller Inhalt"
        );
    }

    #[tokio::test]
    async fn missing_archive_is_503_until_it_appears() {
        let p = tmp("later.pmtiles");
        let _ = std::fs::remove_file(&p);
        let app = router(vec![Dataset::new("l", &p).unwrap()], None, None);
        let uri = format!("/l/zonen/{Z}/{X}/{Y}.mvt");
        let (st, _, _) = get(&app, &uri, None).await;
        assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
        let (st, _, body) = get(&app, "/datasets", None).await;
        assert_eq!(st, StatusCode::OK);
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert!(v[0]["error"].as_str().unwrap().contains("later.pmtiles"));
        write_archive(&p, false, b"da");
        let (st, _, body) = get(&app, &uri, None).await;
        assert_eq!((st, body.as_slice()), (StatusCode::OK, &b"da"[..]));
    }

    #[test]
    fn dataset_specs() {
        assert!(parse_datasets(&["ohne-gleich".into()]).is_err());
        assert!(
            parse_datasets(&["a=/x".into(), "a=/y".into()]).is_err(),
            "doppelt"
        );
        assert!(
            parse_datasets(&["a b=/x".into()]).is_err(),
            "Leerzeichen im Namen"
        );
        let ds = parse_datasets(&["a=/x".into(), "b=/y/zonen.pmtiles".into()]).unwrap();
        assert_eq!(ds[0].path, Path::new("/x"));
        assert_eq!(
            ds[1].summary_path(),
            Path::new("/y/zonen_tiles_summary.json")
        );
        let dir = tmp("");
        let d = Dataset::new("d", &dir).unwrap();
        assert_eq!(
            d.path,
            dir.join(ARCHIVE_FILE),
            "Verzeichnis -> zonen.pmtiles darin"
        );
    }
}
