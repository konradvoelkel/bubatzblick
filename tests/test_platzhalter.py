"""Jeder Platzhalter `__NAME__` in den Seiten unter viewer/ wird beim Ausliefern ersetzt.

Beide Deploy-Wege (der Server-Hook deploy-site im Infra-Repo und der Rueckfall
deploy/bin/deploy_vps.sh) halten an, wenn nach dem Ersetzen noch ein Platzhalter
in einer Seite steht. Ein neuer Platzhalter in viewer/ ohne Ersetzung legte also
den Deploy still. Dieser Test prueft die Haelfte in diesem Repo; den Server-Hook
muss die Server-Sitzung nachziehen (so bei __TILES_ORIGIN__ fuer preconnect).
"""

from __future__ import annotations

import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[1]
DEPLOY = ROOT / "deploy/bin/deploy_vps.sh"


def _platzhalter_in_seiten() -> set[str]:
    gefunden: set[str] = set()
    for seite in (ROOT / "viewer").rglob("*.html"):
        if "vendor" in seite.parts:
            continue
        gefunden |= set(re.findall(r"__[A-Z0-9_]+__", seite.read_text(encoding="utf-8")))
    return gefunden


def test_seiten_nutzen_platzhalter() -> None:
    assert {"__SITE_URL__", "__TILES_ORIGIN__"} <= _platzhalter_in_seiten()


def test_rueckfall_deploy_ersetzt_jeden_platzhalter() -> None:
    ersetzt = set(re.findall(r"s\|(__[A-Z0-9_]+__)\|", DEPLOY.read_text(encoding="utf-8")))
    fehlt = sorted(_platzhalter_in_seiten() - ersetzt)
    assert not fehlt, f"deploy_vps.sh ersetzt nicht: {fehlt}"


def test_preconnect_zu_den_kacheln() -> None:
    html = (ROOT / "viewer/index.html").read_text(encoding="utf-8")
    assert re.search(r'<link rel="preconnect" href="__TILES_ORIGIN__" crossorigin>', html)
