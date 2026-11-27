"""Staedteseiten in den Tageszahlen: Aufrufe je /stadt/<name> und Stimmen fuer Staedte ohne
eigene Seite (deploy/bin/aggregate_access.py), dazu die Diagramme der Statistikseite
(deploy/bin/build_stats.py).

Eine Stimme ist der Abruf der Karte mit ?interesse=<name>, auf den der Platzhalter einer Stadt
ohne eigene Seite weiterleitet; gezaehlt wird je Tag jede gekuerzte Adresse einmal. Bots,
Fehlerantworten und unsaubere Namen zaehlen nicht.
"""

from __future__ import annotations

import importlib.util
import json
import pathlib
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
BIN = ROOT / "deploy/bin"


def _zeile(uri: str, ip: str, ua: str = "Mozilla/5.0", status: int = 200) -> str:
    gestern = time.time() - 86400
    return json.dumps(
        {
            "ts": gestern,
            "status": status,
            "size": 100,
            "request": {
                "uri": uri,
                "remote_ip": ip,
                "host": "bubatzblick.de",
                "headers": {"User-Agent": [ua]},
            },
        }
    )


def _tageszahlen(tmp_path: pathlib.Path) -> tuple[dict, pathlib.Path]:
    zeilen = [
        _zeile("/stadt/bochum", "1.2.3.0"),
        _zeile("/?interesse=bochum", "1.2.3.0"),
        _zeile("/?interesse=bochum", "1.2.3.0"),  # dieselbe Adresse am selben Tag: eine Stimme
        _zeile("/stadt/bochum", "5.6.7.0"),
        _zeile("/?interesse=bochum", "5.6.7.0"),
        _zeile("/stadt/berlin", "1.2.3.0"),
        _zeile("/stadt/berlin.html", "2.2.2.0"),
        _zeile("/stadt/", "1.2.3.0"),
        _zeile("/stadt/index.html", "9.9.9.0"),
        _zeile("/stadt/gibtsnicht", "1.2.3.0", status=404),
        _zeile("/stadt/bochum", "8.8.8.0", ua="Googlebot/2.1"),
        _zeile("/?interesse=Bochum%3Cx%3E", "1.2.3.0"),
    ]
    log = tmp_path / "access.log"
    log.write_text("\n".join(zeilen) + "\n")
    out = tmp_path / "access"
    subprocess.run(
        [sys.executable, str(BIN / "aggregate_access.py"), str(log), str(out)],
        check=True,
        capture_output=True,
    )
    (tag,) = list(out.glob("2*.json"))
    return json.loads(tag.read_text()), out


def test_aufrufe_und_stimmen_je_stadt(tmp_path: pathlib.Path) -> None:
    d, _ = _tageszahlen(tmp_path)
    assert d["city_views"] == {"berlin": 2, "bochum": 2}
    assert d["city_votes"] == {"bochum": 2}
    assert d["city_index_views"] == 2


def test_statistikseite_trennt_platzhalter_von_seiten(tmp_path: pathlib.Path) -> None:
    _, out = _tageszahlen(tmp_path)
    spec = importlib.util.spec_from_file_location("build_stats", BIN / "build_stats.py")
    assert spec and spec.loader
    bs = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bs)
    html = bs.staedte_html(bs.staedte_summen(out))
    ohne, mit = html.split("<h3>Ohne eigene Seite")[1].split("<h3>Mit eigener Seite")
    assert "bochum" in ohne and "berlin" not in ohne
    assert "berlin" in mit and "bochum" not in mit
