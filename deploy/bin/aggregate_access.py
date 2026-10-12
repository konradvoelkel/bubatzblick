#!/usr/bin/env python3
"""Zugriffsprotokoll von Caddy (JSON-Zeilen, IP-Adressen schon von Caddy auf /24 bzw.
/48 gekuerzt) einmal taeglich zu einer kleinen Kennzahlendatei je Tag verdichten
fuer die Statistikseite. Danach bleiben im Protokoll nur die Zeilen von heute.

    aggregate_access.py <caddy-log> <ausgabeverzeichnis>

Je abgeschlossenem Tag <ausgabe>/<YYYY-MM-DD>.json: requests, page_views (/ und
/index.html), about_views, visitors (verschiedene gekuerzte Adressen), bots (User-
Agent nennt sich bot/crawler/spider o. ae.; page_views/visitors ohne Bots), bytes,
status_2xx..5xx, referer_hosts (Top 10 fremde Hosts). Nichts Personenbezogenes:
keine Adressen, keine Pfade je Anfrage, keine User-Agents. Tagesdateien aelter
als KEEP_DAYS werden geloescht.
"""

from __future__ import annotations

import datetime as dt
import json
import os
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path
from urllib.parse import urlsplit

# Aufbewahrung der Tageszahlen. Sie sind nicht personenbezogen, aber ohne
# Grenze waechst der Bestand unbegrenzt -- und auf der Ueber-Seite steht eine
# Frist, die stimmen muss.
KEEP_DAYS = 400

# Rueckmeldung an build_stats.py. Die Statistikseite laeuft als `bubatz` und
# kommt an das Caddy-Protokoll (root) nicht heran; sie liest hier ab, wie alt
# die aelteste offene Zeile ist -- und wie alt diese Datei selbst, denn ein
# ausgefallener Timer faellt sonst niemandem auf.
STATUS_NAME = "_status.json"

BOT = re.compile(
    r"bot|crawl|spider|slurp|curl|wget|python-requests|go-http-client|httpx|scan", re.I
)
PAGE = {"/", "/index.html"}


def header(req: dict, name: str) -> str:
    v = (req.get("headers") or {}).get(name) or []
    return v[0] if isinstance(v, list) and v else (v if isinstance(v, str) else "")


def schreibe_status(out: Path, aeltest: dt.datetime | None, zeilen: int) -> None:
    rec = {
        "written": dt.datetime.now().isoformat(timespec="seconds"),
        "oldest_kept": aeltest.isoformat(timespec="seconds") if aeltest else None,
        "kept_lines": zeilen,
    }
    p = out / STATUS_NAME
    tmp = p.with_suffix(f".json.tmp{os.getpid()}")
    tmp.write_text(json.dumps(rec, ensure_ascii=False, indent=1))
    tmp.chmod(0o644)
    tmp.rename(p)


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    log, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    if not log.exists():
        schreibe_status(out, None, 0)
        print("aggregate_access: kein Protokoll, nichts zu tun")
        return
    today = dt.date.today()
    days: dict[dt.date, dict] = defaultdict(
        lambda: {
            "requests": 0,
            "page_views": 0,
            "about_views": 0,
            "bots": 0,
            "bytes": 0,
            "ips": set(),
            "status": Counter(),
            "referers": Counter(),
        }
    )
    keep: list[str] = []
    aeltest_behalten: dt.datetime | None = None
    with log.open("r", errors="replace") as f:
        for line in f:
            try:
                e = json.loads(line)
                ts = dt.datetime.fromtimestamp(float(e["ts"]))
            except (ValueError, KeyError, TypeError):
                continue
            day = ts.date()
            if day >= today:
                keep.append(line)
                if aeltest_behalten is None or ts < aeltest_behalten:
                    aeltest_behalten = ts
                continue
            req = e.get("request") or {}
            d = days[day]
            d["requests"] += 1
            d["bytes"] += int(e.get("size") or 0)
            d["status"][f"{int(e.get('status') or 0) // 100}xx"] += 1
            if BOT.search(header(req, "User-Agent")):
                d["bots"] += 1
                continue
            uri = (req.get("uri") or "").split("?", 1)[0]
            if uri in PAGE:
                d["page_views"] += 1
            elif uri == "/about.html":
                d["about_views"] += 1
            ip = req.get("remote_ip") or req.get("client_ip") or ""
            if ip:
                d["ips"].add(ip)
            ref = header(req, "Referer")
            if ref:
                host = urlsplit(ref).netloc.lower()
                if host and host != (req.get("host") or "").lower():
                    d["referers"][host] += 1
    for day, d in sorted(days.items()):
        rec = {
            "date": day.isoformat(),
            "requests": d["requests"],
            "page_views": d["page_views"],
            "about_views": d["about_views"],
            "visitors": len(d["ips"]),
            "bots": d["bots"],
            "bytes": d["bytes"],
            "status_2xx": d["status"]["2xx"],
            "status_3xx": d["status"]["3xx"],
            "status_4xx": d["status"]["4xx"],
            "status_5xx": d["status"]["5xx"],
            "referer_hosts": d["referers"].most_common(10),
        }
        p = out / f"{day.isoformat()}.json"
        tmp = p.with_suffix(f".json.tmp{os.getpid()}")
        tmp.write_text(json.dumps(rec, ensure_ascii=False, indent=1))
        tmp.chmod(0o644)
        tmp.rename(p)
        print(
            f"aggregate_access: {day} {rec['requests']} Anfragen, "
            f"{rec['page_views']} Seitenaufrufe, {rec['visitors']} Besucher"
        )
    # Tageszahlen jenseits der Frist entfernen.
    grenze = today - dt.timedelta(days=KEEP_DAYS)
    alt = 0
    for p in out.glob("*.json"):
        try:
            tag = dt.date.fromisoformat(p.stem)
        except ValueError:
            continue
        if tag < grenze:
            p.unlink()
            alt += 1
    if alt:
        print(f"aggregate_access: {alt} Tagesdatei(en) aelter als {KEEP_DAYS} Tage entfernt")

    # Protokoll auf die heutigen Zeilen kuerzen (Caddy schreibt mit O_APPEND weiter;
    # nicht per rename ersetzen, sonst schriebe Caddy in die alte Inode).
    with log.open("r+") as f:
        f.truncate(0)
        f.writelines(keep)
    schreibe_status(out, aeltest_behalten, len(keep))
    print(f"aggregate_access: {len(days)} Tag(e) verdichtet, {len(keep)} Zeilen von heute behalten")


if __name__ == "__main__":
    main()
