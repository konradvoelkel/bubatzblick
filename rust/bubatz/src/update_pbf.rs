//! `bubatz update-pbf <pbf>` -- Ersatz fuer `pyosmium-up-to-date`: liest den
//! osmosis-Replikationskopf der PBF (`osmosis_replication_base_url`,
//! `_sequence_number`, `_timestamp`), holt `state.txt` des Servers, laedt die
//! `.osc.gz`-Diffs ab Folgenummer bis zum aktuellen Stand (Groessengrenze wie
//! `--size`) und wendet sie mit **`osmium apply-changes`** (osmium-tool, C++)
//! an. Ohne osmium-tool bricht der Schritt mit Hinweis ab; `--dry-run` zeigt
//! nur Kopf, Serverstand und den Plan.
//!
//! **Quellenwechsel (`--replication-url`):** zeigt
//! der Kopf auf eine andere Quelle (z. B. osm.fr-Minutendiffs, 1.440 Dateien
//! je Tag), wird per Zeitstempel die letzte Folge der neuen Quelle gesucht,
//! die nicht juenger als der Datenstand ist (Rueckwaertssuche ueber die
//! `<folge>.state.txt`), und ab dort angewendet. Das ist sicher, weil ein
//! Diff-Fenster, das vor dem Datenstand beginnt, fuer jedes Objekt die
//! letzte Version im Fenster traegt -- nie aelter als die in der PBF.
//! Gibt es noch nichts anzuwenden, wird nur der Kopf umgeschrieben
//! (`osmium cat`), damit der naechste Lauf an der neuen Quelle anknuepft.
//! Der Download bleibt seriell, um keine Ratenbegrenzung zu riskieren; mit
//! Tagesdiffs ist er ohnehin eine Datei.
//!
//! `--diff-dir` behaelt die angewandten Diffs (`<folge>.osc.gz`) fuer
//! spaetere Stufen (der Grenzcache prueft, ob die Diffs die
//! Grenzrelation beruehren); `--keep-diffs` begrenzt den Bestand.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Args;
use tracing::{info, warn};

#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Zu aktualisierende .osm.pbf
    pub pbf_path: PathBuf,
    /// Ausgabe (Default: <pbf>.new.osm.pbf); der Aufrufer verschiebt sie
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// Groessengrenze der Diffs in MB (wie pyosmium-up-to-date --size)
    #[arg(long, default_value_t = 2000)]
    pub size: u64,
    /// Nur Kopf und Plan anzeigen, nichts laden oder anwenden
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
    /// Verzeichnis fuer die Diffs; ohne Angabe neben der Ausgabe und danach geloescht
    #[arg(long)]
    pub diff_dir: Option<PathBuf>,
    /// Mit --diff-dir: nur die juengsten N Diffs behalten (nach Folgenummer)
    #[arg(long, default_value_t = 30)]
    pub keep_diffs: usize,
    /// Replikationsquelle (state.txt darunter); weicht sie vom Kopf ab, wird
    /// per Zeitstempel umgestiegen (z. B. https://download.geofabrik.de/europe/germany-updates)
    #[arg(long)]
    pub replication_url: Option<String>,
    /// Beim Umstieg: Startfolge der neuen Quelle statt der Zeitstempelsuche
    #[arg(long)]
    pub replication_sequence: Option<i64>,
}

pub struct ReplicationHeader {
    pub base_url: String,
    pub sequence: i64,
    pub timestamp: Option<i64>,
}

pub fn read_header(pbf: &Path) -> Result<ReplicationHeader> {
    let mut reader =
        osmpbf::BlobReader::from_path(pbf).with_context(|| format!("{} oeffnen", pbf.display()))?;
    let Some(blob) = reader.next() else {
        bail!("leere PBF {}", pbf.display());
    };
    if let osmpbf::BlobDecode::OsmHeader(h) = blob?.decode()? {
        let base_url = h
            .osmosis_replication_base_url()
            .context("PBF ohne osmosis_replication_base_url im Kopf")?
            .trim_end_matches('/')
            .to_string();
        let sequence = h
            .osmosis_replication_sequence_number()
            .context("PBF ohne osmosis_replication_sequence_number")?;
        return Ok(ReplicationHeader {
            base_url,
            sequence,
            timestamp: h.osmosis_replication_timestamp(),
        });
    }
    bail!("kein Kopfblock in {}", pbf.display())
}

fn seq_path(seq: i64) -> String {
    format!(
        "{:03}/{:03}/{:03}",
        seq / 1_000_000,
        (seq / 1000) % 1000,
        seq % 1000
    )
}

/// GET mit Zeitlimit und drei Versuchen (Geofabrik antwortet gelegentlich
/// erst nach einem Verbindungsabbruch).
fn fetch(url: &str) -> Result<Vec<u8>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(180))
        .build();
    let mut last = None;
    for attempt in 1..=3 {
        match agent.get(url).call() {
            Ok(resp) => {
                let mut buf = Vec::new();
                resp.into_reader().read_to_end(&mut buf)?;
                return Ok(buf);
            }
            Err(e) => {
                warn!("update-pbf: GET {url} (Versuch {attempt}/3): {e}");
                last = Some(e);
                std::thread::sleep(Duration::from_secs(5 * attempt as u64));
            }
        }
    }
    Err(anyhow::Error::from(last.unwrap())).with_context(|| format!("GET {url}"))
}

struct State {
    sequence: i64,
    timestamp: String,
}

fn parse_state(text: &str) -> Result<State> {
    let mut seq = None;
    let mut ts = String::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("sequenceNumber=") {
            seq = v.trim().parse::<i64>().ok();
        }
        if let Some(v) = line.strip_prefix("timestamp=") {
            ts = v.trim().replace("\\:", ":");
        }
    }
    Ok(State {
        sequence: seq.context("state.txt ohne sequenceNumber")?,
        timestamp: ts,
    })
}

pub fn server_state(base_url: &str) -> Result<(i64, String)> {
    let s = parse_state(&String::from_utf8(fetch(&format!(
        "{base_url}/state.txt"
    ))?)?)?;
    Ok((s.sequence, s.timestamp))
}

/// `state.txt` einer bestimmten Folge (`.../000/004/901.state.txt`).
fn sequence_state(base_url: &str, seq: i64) -> Result<State> {
    let url = format!("{base_url}/{}.state.txt", seq_path(seq));
    parse_state(&String::from_utf8(fetch(&url)?)?).with_context(|| url)
}

fn diff_state_timestamp(base_url: &str, seq: i64) -> Result<String> {
    let s = sequence_state(base_url, seq)?;
    if s.timestamp.is_empty() {
        bail!("state.txt der Folge {seq} ohne timestamp");
    }
    Ok(s.timestamp)
}

/// `YYYY-MM-DDTHH:MM:SSZ` -> Unix-Sekunden (ohne chrono; Tage seit 1970 nach
/// Howard Hinnants `days_from_civil`).
pub fn parse_iso_utc(s: &str) -> Option<i64> {
    let s = s.trim().trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.split(':').map(|p| p.parse::<i64>());
    let (hh, mm, ss) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// Umstieg auf eine andere Quelle: die groesste Folge, deren Zeitstempel
/// nicht nach dem Datenstand `data_ts` liegt -- Rueckwaertssuche ab
/// `latest`. `ts_of(seq)` liefert den Unix-Zeitstempel einer Folge.
/// Ergebnis `None`: selbst die aelteste geprüfte Folge ist juenger als der
/// Datenstand (Quelle reicht nicht weit genug zurueck).
pub fn switch_start_sequence(
    latest: i64,
    data_ts: i64,
    max_back: i64,
    mut ts_of: impl FnMut(i64) -> Result<i64>,
) -> Result<Option<i64>> {
    let floor = (latest - max_back).max(0);
    let mut seq = latest;
    while seq > floor {
        if ts_of(seq)? <= data_ts {
            return Ok(Some(seq));
        }
        seq -= 1;
    }
    Ok(None)
}

pub fn run(args: UpdateArgs) -> Result<()> {
    let t0 = Instant::now();
    let h = read_header(&args.pbf_path)?;
    let ts = h
        .timestamp
        .map(|t| format!("{t} (Unix)"))
        .unwrap_or_else(|| "?".into());
    info!(
        "update-pbf: {} -- Replikation {} ab Folge {} (Stand {ts})",
        args.pbf_path.display(),
        h.base_url,
        h.sequence
    );
    // Quelle und Startfolge: aus dem Kopf, oder Umstieg auf --replication-url.
    let target_url = args
        .replication_url
        .as_deref()
        .map(|u| u.trim_end_matches('/').to_string())
        .unwrap_or_else(|| h.base_url.clone());
    let switching = target_url != h.base_url;
    let (latest, latest_ts) = server_state(&target_url)?;
    info!("update-pbf: Server-Stand {target_url} Folge {latest} ({latest_ts})");
    let start = if !switching {
        h.sequence
    } else if let Some(s) = args.replication_sequence {
        info!("update-pbf: Umstieg auf {target_url} ab Folge {s} (--replication-sequence)");
        s
    } else {
        let data_ts = h.timestamp.context(
            "Umstieg braucht osmosis_replication_timestamp im Kopf (oder --replication-sequence)",
        )?;
        let found = switch_start_sequence(latest, data_ts, 400, |seq| {
            let st = sequence_state(&target_url, seq)?;
            parse_iso_utc(&st.timestamp)
                .with_context(|| format!("Folge {seq}: Zeitstempel {:?} unlesbar", st.timestamp))
        })?;
        let Some(s) = found else {
            bail!(
                "Umstieg auf {target_url}: keine Folge mit Zeitstempel <= Datenstand gefunden (400 Folgen zurueck)"
            );
        };
        info!(
            "update-pbf: Umstieg {} -> {target_url}: Datenstand {} entspricht Folge {s} (per Zeitstempel)",
            h.base_url, data_ts
        );
        s
    };
    if latest <= start {
        if switching {
            println!(
                "update-pbf: {} ist aktuell; nur Kopf auf {target_url} Folge {start} umschreiben",
                args.pbf_path.display()
            );
            if args.dry_run {
                return Ok(());
            }
            let output = output_path(&args);
            let ts = if start == latest {
                latest_ts
            } else {
                diff_state_timestamp(&target_url, start)?
            };
            rewrite_header(&args.pbf_path, &output, &target_url, start, &ts)?;
            println!(
                "update-pbf: {} -> {} (Kopf umgeschrieben, {:.0} s)",
                args.pbf_path.display(),
                output.display(),
                t0.elapsed().as_secs_f64()
            );
        } else {
            println!(
                "update-pbf: {} ist aktuell (Folge {}).",
                args.pbf_path.display(),
                h.sequence
            );
        }
        return Ok(());
    }
    let n = latest - start;
    println!(
        "update-pbf: {n} Diff(s) {}..{latest} von {target_url}",
        start + 1
    );
    if args.dry_run {
        println!(
            "(dry-run) osmium apply-changes --output-header=osmosis_replication_{{base_url,sequence_number,timestamp}}=... --output <neu> {} <diffs>",
            args.pbf_path.display()
        );
        return Ok(());
    }
    if which("osmium").is_none() {
        bail!(
            "osmium-tool nicht gefunden (z. B. `apt install osmium-tool`); alternativ ENGINE=python -> pyosmium-up-to-date"
        );
    }
    let output = output_path(&args);
    let diff_dir = args
        .diff_dir
        .clone()
        .unwrap_or_else(|| output.with_extension("diffs"));
    std::fs::create_dir_all(&diff_dir)?;
    let mut total = 0u64;
    let mut files = Vec::new();
    let mut applied = start;
    for seq in start + 1..=latest {
        let url = format!("{target_url}/{}.osc.gz", seq_path(seq));
        let path = diff_dir.join(format!("{seq}.osc.gz"));
        if !path.exists() {
            let data = fetch(&url)?;
            let tmp = path.with_extension("gz.part");
            std::fs::write(&tmp, &data)?;
            std::fs::rename(&tmp, &path)?;
        }
        total += std::fs::metadata(&path)?.len();
        files.push(path);
        applied = seq;
        if total > args.size * 1024 * 1024 {
            info!(
                "update-pbf: Groessengrenze {} MB erreicht bei Folge {seq}; Rest im naechsten Lauf",
                args.size
            );
            break;
        }
    }
    // Replikationskopf fortschreiben (wie pyosmium-up-to-date), sonst kann
    // der naechste Lauf nicht anknuepfen: Zeitstempel aus der state.txt der
    // letzten angewendeten Folge.
    let applied_ts = if applied == latest {
        latest_ts.clone()
    } else {
        diff_state_timestamp(&target_url, applied)?
    };
    let mut cmd = std::process::Command::new("osmium");
    cmd.arg("apply-changes").arg("--overwrite");
    for a in header_args(&target_url, applied, &applied_ts) {
        cmd.arg(a);
    }
    cmd.arg("--output").arg(&output).arg(&args.pbf_path);
    for f in &files {
        cmd.arg(f);
    }
    info!(
        "update-pbf: osmium apply-changes mit {} Diffs ({:.0} MB)",
        files.len(),
        total as f64 / 1e6
    );
    let status = cmd.status().context("osmium starten")?;
    if !status.success() {
        bail!("osmium apply-changes fehlgeschlagen ({status})");
    }
    // Ergebnis pruefen: Kopf muss anknuepfbar sein
    let nh = read_header(&output).context("Kopf der neuen PBF")?;
    if nh.sequence != applied || nh.base_url != target_url {
        bail!(
            "neue PBF traegt {} Folge {}, erwartet {target_url} Folge {applied}",
            nh.base_url,
            nh.sequence
        );
    }
    if args.diff_dir.is_none() {
        let _ = std::fs::remove_dir_all(&diff_dir);
    } else {
        prune_diffs(&diff_dir, args.keep_diffs)?;
    }
    println!(
        "update-pbf: {} -> {} ({} Diffs, {:.0} s)",
        args.pbf_path.display(),
        output.display(),
        files.len(),
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}

fn output_path(args: &UpdateArgs) -> PathBuf {
    args.output.clone().unwrap_or_else(|| {
        let name = args
            .pbf_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace(".osm.pbf", ".new.osm.pbf");
        args.pbf_path.with_file_name(name)
    })
}

fn header_args(base_url: &str, seq: i64, ts: &str) -> Vec<String> {
    vec![
        format!("--output-header=osmosis_replication_base_url={base_url}"),
        format!("--output-header=osmosis_replication_sequence_number={seq}"),
        format!("--output-header=osmosis_replication_timestamp={ts}"),
    ]
}

/// Nur den Replikationskopf aendern (Quellenwechsel ohne anzuwendende
/// Diffs): `osmium cat` schreibt die Datei einmal neu.
fn rewrite_header(input: &Path, output: &Path, base_url: &str, seq: i64, ts: &str) -> Result<()> {
    if which("osmium").is_none() {
        bail!("osmium-tool nicht gefunden (z. B. `apt install osmium-tool`)");
    }
    let mut cmd = std::process::Command::new("osmium");
    cmd.arg("cat").arg("--overwrite");
    for a in header_args(base_url, seq, ts) {
        cmd.arg(a);
    }
    cmd.arg("--output").arg(output).arg(input);
    info!("update-pbf: osmium cat (Kopf -> {base_url} Folge {seq})");
    let status = cmd.status().context("osmium starten")?;
    if !status.success() {
        bail!("osmium cat fehlgeschlagen ({status})");
    }
    let nh = read_header(output).context("Kopf der neuen PBF")?;
    if nh.sequence != seq || nh.base_url != base_url {
        bail!(
            "neue PBF traegt {} Folge {}, erwartet {base_url} Folge {seq}",
            nh.base_url,
            nh.sequence
        );
    }
    Ok(())
}

/// Im Diff-Verzeichnis nur die `keep` juengsten `<folge>.osc.gz` behalten.
fn prune_diffs(dir: &Path, keep: usize) -> Result<()> {
    let mut seqs: Vec<(i64, PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_string();
            let seq = name.strip_suffix(".osc.gz")?.parse::<i64>().ok()?;
            Some((seq, p))
        })
        .collect();
    seqs.sort();
    let n_del = seqs.len().saturating_sub(keep);
    for (_, p) in seqs.into_iter().take(n_del) {
        std::fs::remove_file(&p).with_context(|| p.display().to_string())?;
    }
    if n_del > 0 {
        info!(
            "update-pbf: {n_del} alte Diffs aus {} entfernt",
            dir.display()
        );
    }
    Ok(())
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(bin))
            .find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_paths_follow_the_replication_layout() {
        assert_eq!(seq_path(4321), "000/004/321");
        assert_eq!(seq_path(1_234_567), "001/234/567");
    }

    #[test]
    fn iso_timestamps_to_unix() {
        assert_eq!(parse_iso_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso_utc("2026-09-13T19:34:33Z"), Some(1_789_328_073));
        assert_eq!(parse_iso_utc("2026-09-12T20:21:58Z"), Some(1_789_244_518));
        assert_eq!(
            parse_iso_utc("2000-03-01T00:00:00Z"),
            Some(951_868_800),
            "Schaltjahr"
        );
        assert_eq!(parse_iso_utc("kaputt"), None);
    }

    #[test]
    fn state_txt_with_escaped_colons() {
        let s = parse_state("# original OSM minutely replication sequence number 7284053\ntimestamp=2026-09-12T20\\:21\\:58Z\nsequenceNumber=4906\n").unwrap();
        assert_eq!(
            (s.sequence, s.timestamp.as_str()),
            (4906, "2026-09-12T20:21:58Z")
        );
        assert!(parse_state("timestamp=x").is_err());
    }

    #[test]
    fn switch_finds_last_sequence_not_newer_than_the_data() {
        // Tagesfolgen 4900..4906, Zeitstempel je Tag 20:22 UTC.
        let ts_of = |seq: i64| -> Result<i64> { Ok(1_789_244_518 - (4906 - seq) * 86_400) };
        // Datenstand aus Minutendiffs -> juengste nicht neuere Tagesfolge 4906
        assert_eq!(
            switch_start_sequence(4906, 1_789_328_073, 400, ts_of).unwrap(),
            Some(4906)
        );
        // Datenstand genau auf einer Tagesfolge -> diese Folge
        assert_eq!(
            switch_start_sequence(4906, 1_789_244_518 - 3 * 86_400, 400, ts_of).unwrap(),
            Some(4903)
        );
        // Datenstand eine Sekunde davor -> die Folge davor
        assert_eq!(
            switch_start_sequence(4906, 1_789_244_518 - 3 * 86_400 - 1, 400, ts_of).unwrap(),
            Some(4902)
        );
        // Datenstand aelter als die Suche reicht -> None
        assert_eq!(switch_start_sequence(4906, 0, 5, ts_of).unwrap(), None);
        // Fehler der Quelle werden durchgereicht
        assert!(switch_start_sequence(4906, 0, 5, |_| bail!("netz")).is_err());
    }

    #[test]
    fn prune_keeps_newest_by_sequence() {
        let d = std::env::temp_dir().join(format!("bubatz-prune-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for s in [4901, 4903, 4902, 7285422, 4905] {
            std::fs::write(d.join(format!("{s}.osc.gz")), b"x").unwrap();
        }
        std::fs::write(d.join("notes.txt"), b"bleibt").unwrap();
        prune_diffs(&d, 2).unwrap();
        let mut left: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        left.sort();
        assert_eq!(left, vec!["4905.osc.gz", "7285422.osc.gz", "notes.txt"]);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
