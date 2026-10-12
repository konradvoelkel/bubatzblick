// Von deploy/bin/build_about.py erzeugt -- nicht von Hand aendern.
(async () => {
  const cfg = Object.assign({ stand: "stand.json" }, window.BUBATZ_CONFIG || {});
  const el = document.getElementById("stand");
  try { const r = await fetch(cfg.stand); const j = await r.json();
    el.textContent = (j.pbf_timestamp || "").slice(0, 10)
      + " (veröffentlicht " + (j.published || "").slice(0, 10) + ")"; }
  catch (e) { el.textContent = "unbekannt"; }
  // Der Ausschnitt reist als ?karte=<hash> mit und geht so zurueck: vorher
  // stand man nach "Zur Karte" wieder im Anfangsausschnitt.
  const karte = new URLSearchParams(location.search).get("karte");
  if (karte) {
    for (const a of document.querySelectorAll("a.zur-karte")) a.href = "./#" + karte;
  }
  const m = document.getElementById("mail"), u = "kontakt",
        d = ["bubatzblick", "de"].join(".");
  const a = document.createElement("a"); a.href = "mailto:" + u + "@" + d;
  a.textContent = u + "@" + d; m.replaceWith(a);
})();
