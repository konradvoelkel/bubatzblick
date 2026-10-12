#!/usr/bin/env python3
"""Statistikseite des Tageszyklus: eine statische, nirgends verlinkte
Seite unter einem geheimen Pfad (Caddy liefert STATS_DIR, Infra-Repo Rolle bubatzweb).

Liest den letzten Zyklus aus dem Tageslog, die *_summary.json der Stufen und
stand.json, haengt einen Datensatz an history.jsonl an und schreibt index.html
und stats.json. Zugriffszahlen kommen aus STATS_DIR/access/<datum>.json
(aggregate_access.py, taeglich als root). Nur Standardbibliothek: laeuft in der
abgeschotteten Einheit als bubatz.

Umgebung wie bubatz-daily.sh (LOG_DIR, INTERIM_DIR, OUT_DIR, PUBLISH_DIR), dazu
STATS_DIR (Vorgabe PUBLISH_DIR/stats) und optional BUBATZ_START/BUBATZ_END/
BUBATZ_RESULT vom Aufrufer (robuster als das eigene, noch nicht gespuelte Log).
"""

from __future__ import annotations

import contextlib
import datetime as dt
import html
import json
import os
import re
import sys
from pathlib import Path

ANSI = re.compile(r"\x1b\[[0-9;]*m")
HISTORY_ROWS = 30
ACCESS_DAYS = 30
STATUS_NAME = "_status.json"  # von aggregate_access.py
# Die Ueber-Seite sagt zu, dass eine Protokollzeile "in der Regel weniger als
# 48 Stunden" liegen bleibt. Darueber ist es ein Befund, kein Hinweis.
LOG_MAX_AGE_H = 48
TAGESDATEI = re.compile(r"^\d{4}-\d{2}-\d{2}$")


def env(name: str, default: str | None = None) -> str:
    v = os.environ.get(name, default)
    if v is None:
        sys.exit(f"build_stats: {name} fehlt")
    return v


def read_json(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def parse_ts(s: str) -> dt.datetime | None:
    try:
        return dt.datetime.strptime(s, "%Y-%m-%d %H:%M:%S")
    except ValueError:
        return None


def last_cycle_lines(log_dir: Path) -> list[str]:
    logs = sorted(log_dir.glob("daily-*.log"))
    if not logs:
        return []
    lines = [
        ANSI.sub("", ln.rstrip("\n")) for ln in logs[-1].read_text(errors="replace").splitlines()
    ]
    starts = [i for i, ln in enumerate(lines) if ln.startswith("== bubatz-daily Start ")]
    return lines[starts[-1] :] if starts else []


def parse_cycle(lines: list[str]) -> dict:
    c: dict = {
        "start": None,
        "end": None,
        "result": None,
        "seq_from": None,
        "seq_to": None,
        "germany_s": None,
        "clip": None,
        "checks": [],
        "published": False,
        "r2": False,
    }
    for ln in lines:
        if m := re.match(r"== bubatz-daily Start (\S+ \S+)", ln):
            c["start"] = m.group(1)
        elif m := re.match(r"update-pbf Durchgang \d+: Folge (\S+) -> (\S+)", ln):
            if c["seq_from"] is None:
                c["seq_from"] = m.group(1)
            c["seq_to"] = m.group(2)
        elif m := re.match(r"== keine neue Folge \((\S+)\).*Ende (\d\d:\d\d:\d\d)", ln):
            c["result"] = "keine neue Folge"
            c["end"] = m.group(2)
        elif m := re.match(r"== germany fertig in (\d+) s", ln):
            c["germany_s"] = int(m.group(1))
        elif m := re.match(r"clip: (\d+) von (\d+) Verbotsobjekten", ln):
            c["clip"] = {"inside": int(m.group(1)), "total": int(m.group(2))}
        elif m := re.match(r"publish: (OK|FEHLER) +(\w+): (.*)", ln):
            c["checks"].append({"name": m.group(2), "ok": m.group(1) == "OK", "text": m.group(3)})
        elif m := re.match(r"publish: (plausibel|tiles): (.*)", ln):
            c["checks"].append({"name": m.group(1), "ok": None, "text": m.group(2)})
        elif ln.startswith("publish: veroeffentlicht:"):
            c["published"] = True
        elif re.match(r"publish: R2: .* aktualisiert", ln):
            c["r2"] = True
        elif m := re.match(r"== FEHLER: (.*) \((\d\d:\d\d:\d\d)\)", ln):
            c["result"] = "Fehler: " + m.group(1)
            c["end"] = m.group(2)
        elif m := re.match(r"== bubatz-daily fertig (\S+ \S+), PBF-Folge (\S+)", ln):
            c["result"] = "ok"
            c["end"] = m.group(1)
            c["seq_to"] = m.group(2)
    # Aufrufer weiss es genauer (das Log ist beim Aufruf noch nicht vollstaendig gespuelt)
    c["start"] = os.environ.get("BUBATZ_START") or c["start"]
    c["end"] = os.environ.get("BUBATZ_END") or c["end"]
    c["result"] = os.environ.get("BUBATZ_RESULT") or c["result"] or "laeuft/abgebrochen"
    if c["start"] and c["end"] and len(c["end"]) == 8:
        c["end"] = c["start"][:10] + " " + c["end"]
        if c["end"] < c["start"]:
            d = parse_ts(c["start"])
            if d:
                c["end"] = (d + dt.timedelta(days=1)).strftime("%Y-%m-%d") + " " + c["end"][11:]
    s, e = parse_ts(c["start"] or ""), parse_ts(c["end"] or "")
    c["duration_s"] = int((e - s).total_seconds()) if s and e else None
    return c


def fresh(path: Path, start: dt.datetime | None) -> dict | None:
    """Summary einer Stufe, nur wenn sie in DIESEM Zyklus geschrieben wurde."""
    try:
        mtime = dt.datetime.fromtimestamp(path.stat().st_mtime)
    except OSError:
        return None
    if start and mtime < start:
        return None
    return read_json(path)


def pick(d: dict | None, keys: list[str]) -> dict | None:
    if d is None:
        return None
    return {k: d.get(k) for k in keys}


def build_record() -> dict:
    log_dir = Path(env("LOG_DIR"))
    interim = Path(env("INTERIM_DIR")) / "germany"
    out = Path(env("OUT_DIR")) / "germany"
    publish = Path(env("PUBLISH_DIR"))
    c = parse_cycle(last_cycle_lines(log_dir))
    start = parse_ts(c["start"] or "")
    c["extract"] = pick(
        fresh(interim / "raw" / "extract_summary.json", start),
        ["n_protected", "n_buildings", "n_geometry_errors", "n_incomplete_relations", "elapsed_s"],
    )
    c["zones"] = pick(
        fresh(interim / "zones" / "zones_summary.json", start),
        [
            "computed_cells",
            "linked_cells",
            "computed_objects",
            "reused_objects",
            "total_zones",
            "fp_phase_s",
            "elapsed_s",
        ],
    )
    c["tiles"] = pick(
        fresh(out / "zonen_tiles_summary.json", start),
        [
            "n_cells",
            "n_cells_reused",
            "blocks_built",
            "blocks_reused",
            "n_features",
            "cache_phase_s",
            "tippecanoe_s",
            "join_s",
            "elapsed_s",
        ],
    )
    stand = read_json(publish / "stand.json")
    c["stand"] = pick(
        stand,
        [
            "published",
            "bands",
            "archive_bytes",
            "anchor_m2",
            "pbf_sequence",
            "pbf_timestamp",
            "objects",
            "warnings",
            "categories",
        ],
    )
    c["generated"] = dt.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    return c


def append_history(path: Path, rec: dict) -> list[dict]:
    hist: list[dict] = []
    if path.exists():
        for ln in path.read_text().splitlines():
            with contextlib.suppress(ValueError):
                hist.append(json.loads(ln))
    hist = [h for h in hist if h.get("start") != rec.get("start")]
    hist.append(rec)
    hist.sort(key=lambda h: h.get("start") or "")
    path.write_text("".join(json.dumps(h, ensure_ascii=False) + "\n" for h in hist))
    return hist


def load_access(d: Path) -> list[dict]:
    tage = sorted(p for p in d.glob("*.json") if TAGESDATEI.match(p.stem))
    return [r for p in tage[-ACCESS_DAYS:] if (r := read_json(p))]


def alter(iso: str | None) -> dt.timedelta | None:
    if not iso:
        return None
    try:
        return dt.datetime.now() - dt.datetime.fromisoformat(iso)
    except ValueError:
        return None


def dauer(td: dt.timedelta) -> str:
    """Als Ergaenzung zu "vor ..." lesbar, also im Dativ."""
    m = int(td.total_seconds() // 60)
    if m < 1:
        return "<1 min"
    if m < 90:
        return f"{m} min"
    if m < LOG_MAX_AGE_H * 60:
        return f"{m // 60} h"
    return f"{m // 1440} Tagen"


# --- HTML ---------------------------------------------------------------------


def h(x) -> str:
    return html.escape("" if x is None else str(x))


def n(x) -> str:
    if x is None:
        return "–"
    if isinstance(x, float):
        return f"{x:,.0f}".replace(",", " ")
    if isinstance(x, int):
        return f"{x:,}".replace(",", " ")
    return h(x)


def dur(s) -> str:
    if s is None:
        return "–"
    s = int(s)
    if s < 90:
        return f"{s} s"
    if s < 5400:
        return f"{s / 60:.0f} min"
    return f"{s // 3600} h {s % 3600 // 60:02d} min"


def mb(b) -> str:
    return "–" if b is None else f"{b / 1e6:,.0f} MB".replace(",", " ")


def kv_table(rows: list[tuple[str, str]]) -> str:
    return (
        "<table>" + "".join(f"<tr><th>{h(k)}</th><td>{v}</td></tr>" for k, v in rows) + "</table>"
    )


def access_status_html(status: dict | None) -> str:
    """Eine Zeile darueber, ob die Verdichtung laeuft. Zwei Zusagen der
    Ueber-Seite haengen daran: gekuerzte Adressen nuetzen nichts, wenn das
    Protokoll nie geleert wird, und die genannte Frist stimmt dann auch nicht.
    Stillstand faellt sonst niemandem auf -- deshalb steht es hier."""
    if not status:
        return (
            f'<p class="err">Die Verdichtung meldet sich nicht ('
            f"<code>access/{STATUS_NAME}</code> fehlt). Solange das so ist, wächst das "
            "Zugriffsprotokoll unbegrenzt, und die Aufbewahrungsfrist auf der "
            "Über-Seite stimmt nicht.</p>"
        )
    grenze = dt.timedelta(hours=LOG_MAX_AGE_H)
    a_lauf, a_zeile = alter(status.get("written")), alter(status.get("oldest_kept"))
    teile = []
    if a_lauf is None:
        teile.append('<span class="err">Zeitpunkt der Verdichtung unlesbar</span>')
    elif a_lauf > grenze:
        teile.append(f'<span class="err">Verdichtung: zuletzt vor {h(dauer(a_lauf))}</span>')
    else:
        teile.append(f'<span class="ok">Verdichtung: vor {h(dauer(a_lauf))}</span>')
    if a_zeile is None:
        teile.append("keine Protokollzeile offen")
    elif a_zeile > grenze:
        teile.append(f'<span class="err">älteste Zeile: von vor {h(dauer(a_zeile))}</span>')
    else:
        teile.append(f"älteste Zeile: von vor {h(dauer(a_zeile))}")
    teile.append(f"{h(status.get('kept_lines'))} Zeilen offen")
    return f'<p class="small">{" · ".join(teile)} — Zusage: unter {LOG_MAX_AGE_H} h</p>'


def render(rec: dict, hist: list[dict], access: list[dict], status: dict | None) -> str:
    result = rec["result"] or ""
    cls = "ok" if result == "ok" else ("warn" if result.startswith("keine") else "err")
    rows = [
        ("Start", h(rec["start"])),
        ("Ende", h(rec["end"])),
        ("Dauer", dur(rec["duration_s"])),
        ("Ergebnis", f'<span class="{cls}">{h(result)}</span>'),
        ("PBF-Folge", f"{h(rec['seq_from'])} → {h(rec['seq_to'])}"),
    ]
    if rec.get("germany_s") is not None:
        rows.append(("make germany", dur(rec["germany_s"])))
    ex, zo, ti, cl = rec.get("extract"), rec.get("zones"), rec.get("tiles"), rec.get("clip")
    if ex:
        rows.append(
            (
                "extract",
                f"{n(ex['n_protected'])} Verbotsobjekte, {n(ex['n_buildings'])} Gebäude, {n(ex['n_geometry_errors'])} Geometriefehler, {dur(ex['elapsed_s'])}",
            )
        )
    if cl:
        rows.append(("clip", f"{n(cl['inside'])} von {n(cl['total'])} innerhalb der Grenze"))
    if zo:
        total_cells = (zo["computed_cells"] or 0) + (zo["linked_cells"] or 0)
        rows.append(
            (
                "zones",
                f"{n(zo['linked_cells'])} von {n(total_cells)} Zellen per Fingerabdruck übernommen, {n(zo['computed_cells'])} gerechnet; davon {n(zo['reused_objects'])} von {n(zo['computed_objects'])} Objekten per cache_key; {n(zo['total_zones'])} Bänder gesamt; {dur(zo['elapsed_s'])}",
            )
        )
    if ti:
        rows.append(
            (
                "tiles",
                f"{n(ti['n_cells_reused'])} von {n(ti['n_cells'])} Zellen aus dem Cache; Blöcke {n(ti['blocks_reused'])} übernommen / {n(ti['blocks_built'])} gebaut; tippecanoe {dur(ti['tippecanoe_s'])}, gesamt {dur(ti['elapsed_s'])}",
            )
        )
    if rec["checks"]:
        items = []
        for ch in rec["checks"]:
            mark = "✓" if ch["ok"] else ("✗" if ch["ok"] is False else "·")
            items.append(
                f'<li><span class="{"ok" if ch["ok"] else ("err" if ch["ok"] is False else "")}">{mark} {h(ch["name"])}</span>: {h(ch["text"])}</li>'
            )
        rows.append(("publish", "<ul>" + "".join(items) + "</ul>"))
        rows.append(
            (
                "Veröffentlicht",
                ("ja" if rec["published"] else "nein") + (", R2 aktualisiert" if rec["r2"] else ""),
            )
        )
    st = rec.get("stand")
    stand_html = ""
    if st:
        cats = st.get("categories") or {}
        stand_html = kv_table(
            [
                ("Veröffentlicht", h(st["published"])),
                ("PBF-Folge / Stand", f"{h(st['pbf_sequence'])} / {h(st['pbf_timestamp'])}"),
                ("Bänder", n(st["bands"])),
                ("Archiv", mb(st["archive_bytes"])),
                ("Anker", f"{n(st['anchor_m2'])} m²"),
                ("Objekte", f"{n(st['objects'])} ({n(st['warnings'])} Plausibilitätswarnungen)"),
                ("Kategorien", ", ".join(f"{h(k)} {n(v)}" for k, v in sorted(cats.items()))),
            ]
        )
    hist_rows = []
    for r in reversed(hist[-HISTORY_ROWS:]):
        res = r.get("result") or ""
        c2 = "ok" if res == "ok" else ("warn" if res.startswith("keine") else "err")
        zo2, s2 = r.get("zones") or {}, r.get("stand") or {}
        hist_rows.append(
            f"<tr><td>{h(r.get('start'))}</td><td>{dur(r.get('duration_s'))}</td>"
            f"<td>{h(r.get('seq_from'))} → {h(r.get('seq_to'))}</td>"
            f'<td class="{c2}">{h(res)}</td>'
            f"<td>{n(zo2.get('computed_cells'))}</td><td>{n(zo2.get('computed_objects'))}</td>"
            f"<td>{n(s2.get('bands')) if r.get('published') else '–'}</td>"
            f"<td>{mb(s2.get('archive_bytes')) if r.get('published') else '–'}</td></tr>"
        )
    acc_rows = []
    for a in reversed(access):
        refs = ", ".join(f"{h(k)} ({n(v)})" for k, v in (a.get("referer_hosts") or [])[:5])
        acc_rows.append(
            f"<tr><td>{h(a.get('date'))}</td><td>{n(a.get('requests'))}</td><td>{n(a.get('page_views'))}</td>"
            f"<td>{n(a.get('about_views'))}</td><td>{n(a.get('visitors'))}</td><td>{n(a.get('bots'))}</td>"
            f"<td>{mb(a.get('bytes'))}</td><td>{n(a.get('status_4xx'))} / {n(a.get('status_5xx'))}</td><td>{refs}</td></tr>"
        )
    return f"""<!doctype html>
<html lang="de"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex,nofollow"><title>BubatzBlick – Betrieb</title>
<style>
body{{font:15px/1.45 system-ui,sans-serif;margin:2rem auto;max-width:72rem;padding:0 1rem;color:#222;background:#fafafa}}
h1{{font-size:1.4rem}} h2{{font-size:1.1rem;margin-top:2rem}}
table{{border-collapse:collapse;margin:.5rem 0;font-size:.92rem}} th,td{{text-align:left;vertical-align:top;padding:.25rem .6rem;border-bottom:1px solid #ddd}}
th{{font-weight:600;white-space:nowrap}} ul{{margin:0;padding-left:1.1rem}}
.ok{{color:#1a7f37}} .warn{{color:#9a6700}} .err{{color:#b3261e;font-weight:600}}
.small{{color:#666;font-size:.85rem}} .scroll{{overflow-x:auto}}
</style></head><body>
<h1>BubatzBlick – Betrieb</h1>
<p class="small">Erzeugt {h(rec["generated"])} vom Tageszyklus auf dem Server. Diese Seite ist nicht verlinkt und nicht indexiert.</p>
<h2>Letzter Zyklus</h2>
{kv_table(rows)}
<h2>Veröffentlichter Stand</h2>
{stand_html or "<p>kein stand.json</p>"}
<h2>Verlauf (letzte {HISTORY_ROWS} Zyklen)</h2>
<div class="scroll"><table><tr><th>Start</th><th>Dauer</th><th>PBF-Folge</th><th>Ergebnis</th><th>Zellen gerechnet</th><th>Objekte gerechnet</th><th>Bänder</th><th>Archiv</th></tr>
{"".join(hist_rows)}</table></div>
<h2>Zugriffe auf die Seite (je Tag)</h2>
<p class="small">Aus dem Caddy-Zugriffsprotokoll, IP-Adressen schon beim Schreiben auf /24 bzw. /48 gekürzt; Besucher ≈ verschiedene gekürzte Adressen. Kachelabrufe laufen über R2 und fehlen hier.</p>
{access_status_html(status)}
<div class="scroll"><table><tr><th>Datum</th><th>Anfragen</th><th>Karte</th><th>Über</th><th>Besucher ≈</th><th>Bots</th><th>Volumen</th><th>4xx / 5xx</th><th>Herkunft (Referer)</th></tr>
{"".join(acc_rows) or '<tr><td colspan="9">noch keine Auswertung</td></tr>'}</table></div>
</body></html>
"""


def main() -> None:
    stats_dir = Path(os.environ.get("STATS_DIR") or (Path(env("PUBLISH_DIR")) / "stats"))
    stats_dir.mkdir(parents=True, exist_ok=True)
    rec = build_record()
    hist = append_history(stats_dir / "history.jsonl", rec)
    access = load_access(stats_dir / "access")
    status = read_json(stats_dir / "access" / STATUS_NAME)
    tmp = stats_dir / f"index.html.tmp{os.getpid()}"
    tmp.write_text(render(rec, hist, access, status))
    tmp.rename(stats_dir / "index.html")
    (stats_dir / "stats.json").write_text(
        json.dumps(
            {
                "last": rec,
                "history": hist[-HISTORY_ROWS:],
                "access": access,
                "access_status": status,
            },
            ensure_ascii=False,
            indent=1,
        )
    )
    print(
        f"stats: {stats_dir / 'index.html'} ({rec['result']}, {len(hist)} Zyklen, {len(access)} Tage Zugriffe)"
    )


if __name__ == "__main__":
    main()
