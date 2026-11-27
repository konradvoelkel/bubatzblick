// Karte (Bubatzkarte 2.0 Viewer). Eigene Datei statt Inline-Skript, damit die
// CSP `script-src 'self'` ohne 'unsafe-inline' auskommt. Regel: nichts aus
// der URL, aus stand.json oder aus Kacheleigenschaften als HTML einsetzen,
// immer textContent / createElement.

// Trusted Types: eine enge Standard-Policy fuer die Stellen, an denen MapLibre
// Zeichenketten an gefaehrliche DOM-Schnittstellen gibt. Wirksam erst, wenn der
// Server `require-trusted-types-for 'script'` sendet; Browser ohne Trusted Types
// lassen den Block aus. Alles andere lehnt die Policy ab (null), der Browser
// meldet es dann als Verstoss.
//  - DOMParser.parseFromString: die Quellenangaben der Kartenquellen. MapLibre
//    parst sie in ein inertes Dokument und laesst nur erlaubte Elemente stehen.
//  - innerHTML "&#215;": das Schliessen-Zeichen jedes Popups.
//  - Worker: nur die eigene Worker-Datei (MapLibre 6 startet sie als Modul-Worker).
// Die Policy entsteht hier, bevor app.js MapLibre per import() laedt: ein ES-Modul
// im script-Tag liefe vor jedem Code dieser Datei. Eine benannte Policy ueber
// setTrustedTypesPolicy liesse die CSP (`trusted-types default`) nicht zu.
const WORKER_URL = new URL("vendor/maplibre-gl-worker.mjs", location.href).href;
if (window.trustedTypes && trustedTypes.createPolicy) {
  trustedTypes.createPolicy("default", {
    createHTML: (s, _typ, senke) => (senke === "DOMParser parseFromString" || s === "&#215;" ? s : null),
    createScriptURL: s => (new URL(s, location.href).href === WORKER_URL ? s : null),
  });
}

// Scheitert der Start, sagt die Seite es. Vorher blieb sie stumm bei
// "Karte laedt ..." stehen, wenn der Browser kein WebGL hergab: der Fehler stand
// nur in der Konsole, und alles nach der Kartenerzeugung lief nie an.
function startFehler(text) {
  const el = document.getElementById("laden");
  el.hidden = false;
  el.className = "fehler";
  // role=alert steht schon im HTML: eine erst jetzt gesetzte Rolle sagten
  // Screenreader nicht zuverlaessig an, der neue Text in einer bestehenden schon.
  el.textContent = text;
  // "lade ..." im Bedienfeld stimmt jetzt nicht mehr.
  document.getElementById("status").replaceChildren();
}

// Datenfarben aus den CSS-Tokens in index.html (:root), damit Karte und Farblegende
// eine Quelle haben. Der Ersatzwert greift nur, wenn das Stylesheet fehlt.
const farbe = (token, ersatz) =>
  getComputedStyle(document.documentElement).getPropertyValue(token).trim() || ersatz;

// Nach einem Klick auf eine Stadt ohne eigene Seite (/stadt/<name> leitet mit ?interesse=<name>
// hierher): ein Hinweis ueber der Karte. Der Abruf von /stadt/<name> ist die Stimme, die Karte
// zaehlt nichts selbst. Laeuft unabhaengig vom Kartenstart, damit der Hinweis auch dann kommt,
// wenn WebGL fehlt; der Parameter verschwindet danach aus der Adresse (kein zweiter Hinweis
// beim Neuladen), der Ausschnitt im Fragment bleibt.
(async () => {
  const anfrage = new URLSearchParams(location.search);
  const slug = anfrage.get("interesse");
  if (!slug || !/^[a-z0-9-]{1,60}$/.test(slug)) return;
  anfrage.delete("interesse");
  history.replaceState(null, "", location.pathname + (anfrage.size ? "?" + anfrage : "") + location.hash);
  const namen = await fetch("stadt/vorschlaege.json").then(r => (r.ok ? r.json() : {}), () => ({}));
  const name = namen[slug];
  const dlg = document.getElementById("interesse");
  if (!name || !dlg || !dlg.showModal) return;
  document.getElementById("interesse-titel").textContent = name;
  document.getElementById("interesse-text").textContent =
    `Danke! Wir zählen, für welche Städte sich Besucher interessieren; ${name} hat jetzt eine ` +
    `Stimme mehr. Vielleicht gibt es hier bald eine ausführlichere Beschreibung. Bis dahin zeigt ` +
    `die Karte schon, wo in ${name} das Konsumverbot gilt.`;
  dlg.showModal();
})().catch(err => console.error(err));

(async () => {
// MapLibre 6 gibt es nur als ES-Modul. Das Laden beginnt sofort (modulepreload im
// Kopf hat die Datei meist schon) und laeuft neben den Pruefungen unten; erwartet
// wird es erst vor dem Kartenaufbau. Das leere catch verhindert nur die Meldung
// "unbehandelte Ablehnung", bevor das await unten die Ablehnung uebernimmt.
const maplibreLaden = import("./vendor/maplibre-gl.mjs");
maplibreLaden.catch(() => {});

// Kategorien in der Reihenfolge des § 5 Abs. 2 KCanG. Schluessel = Eigenschaft
// `category` der Kacheln, Titel wortgleich aus config/tags.json --
// tests/test_viewer_categories.py prueft beides gegeneinander.
//
// Warum EINE Flaechenfarbe statt Farben je Kategorie: auf der Karte
// kann jede Kategorie an jede grenzen, und die Flaechen liegen mit 40 % Deckkraft
// ueber der Grundkarte. Gemessen (CIE Delta E 2000, alle Paare, heller Grund):
// fuenf Gruppen kommen auf ein schlechtestes Paar von Delta E 3,6 bei normalem
// Farbsehen -- ununterscheidbar; erst drei Farben bestehen. Die Kategorie steht
// deshalb in Legende und Popup, und die Legende filtert.
const KATEGORIEN = [
  ["school", "Schulen"],
  ["playground", "Kinderspielpl\u00e4tze"],
  ["kindergarten", "Kinderg\u00e4rten und Kitas"],
  ["childcare", "Kindertagespflege"],
  ["youth", "Kinder- und Jugendeinrichtungen"],
  ["sports", "\u00d6ffentlich zug\u00e4ngliche Sportst\u00e4tten"],
  ["cannabis_club", "Anbauvereinigungen"],
];
const TITEL = new Map(KATEGORIEN);

  // Bedienfeld ein-/ausklappen. Auf schmalen Bildschirmen (Telefon) verdeckte das
  // ausgeklappte Feld die ganze Karte -- dort beginnt es eingeklappt,
  // Titel und Suche bleiben. Die Wahl wird nicht gespeichert: die Seite legt nichts
  // im Browser ab. Vor dem ersten `await`, damit der Knopf sofort geht.
  // Die Vorgabe folgt der Breite auch spaeter (Telefon gedreht, Fenster erst nach
  // dem Laden auf Groesse gebracht), solange niemand den Knopf benutzt hat.
  // In em wie im CSS: mit groesserer Schrift im Browser kippt das Layout frueher.
  const schmal = matchMedia("(max-width: 40em)");
  const panelToggle = document.getElementById("panel-toggle");
  let panelBeruehrt = false;
  // Der Zustand steht nur in aria-expanded; das CSS blendet per :has() danach ein und aus.
  const panelOffen = offen => {
    panelToggle.setAttribute("aria-expanded", String(offen));
    // "Mehr" versprach Hilfe und lieferte Filter; der Knopf nennt stattdessen,
    // was hinter ihm liegt.
    panelToggle.textContent = offen ? "Ausblenden" : "Kategorien & Infos";
  };
  // Auf breiten Schirmen sind die Kategorien schon aufgeklappt: eingeklappt
  // laegen sie drei Ebenen tief (Bedienfeld -> Knopf -> details).
  const kategorienBlock = document.getElementById("kategorien");
  const breiteAnwenden = schmalJetzt => {
    panelOffen(!schmalJetzt);
    if (kategorienBlock) kategorienBlock.open = !schmalJetzt;
  };
  breiteAnwenden(schmal.matches);
  panelToggle.addEventListener("click", () => {
    panelBeruehrt = true;
    panelOffen(panelToggle.getAttribute("aria-expanded") === "false");
  });
  // KEIN `toggle`-Horcher auf dem Block: `open = …` oben loest ihn selbst aus,
  // und weil `toggle` verzoegert zugestellt wird, laesst er sich nicht sauber
  // von einer echten Eingabe unterscheiden. Er setzte `panelBeruehrt` und
  // schaltete damit die Breitenlogik ab -- auf dem Telefon blieb das Feld
  // offen und verdeckte die Karte.
  schmal.addEventListener("change", e => { if (!panelBeruehrt) breiteAnwenden(e.matches); });
  // Bis die Ortssuche unten ihren Handler hat (nach dem Kartenaufbau), schickte
  // Enter das Formular nativ ab: die Seite lud neu und verlor den Ausschnitt.
  document.getElementById("suche").addEventListener("submit", e => e.preventDefault());

  const cfg = Object.assign({ zones: "zonen.pmtiles", basemap: "basemap.pmtiles",
    assets: "vendor/basemaps", stand: "stand.json" }, window.BUBATZ_CONFIG || {});
  const abs = u => new URL(u, location.href).href;
  // Fussgaengerzonen liegen neben den Zonen (gleicher Bucket bzw. gleiche Seite).
  // Abgeleitet statt als eigener config.js-Schluessel: die Datei wird an zwei
  // Stellen erzeugt (deploy/bin/deploy_vps.sh und der post-receive-Hook im
  // Infra-Repo), ein neuer Schluessel muesste an beiden nachgezogen werden.
  const fussgaengerUrl = cfg.fussgaenger
    || cfg.zones.replace(/[^/]+$/, "fussgaengerzonen.pmtiles");
  // HEAD mit Frist: ohne sie blieb die Seite bei "Karte laedt ..." stehen, wenn eine
  // Antwort ausblieb. Dreiwertig, weil "fehlt" (404, z. B. keine Fussgaengerzonen)
  // etwas anderes ist als "nicht erreichbar" (Netz, Frist, 5xx).
  const pruefe = async u => {
    try {
      const r = await fetch(u, { method: "HEAD", signal: AbortSignal.timeout(10_000) });
      return r.ok ? "da" : (r.status === 404 ? "fehlt" : "unerreichbar");
    } catch (e) { return "unerreichbar"; }
  };
  // Absolute Adressen stehen nur in der config.js des Betriebs (R2). Dort ist ein
  // fehlendes Archiv ein Fehler, den die Seite sagt, und kein Anlass fuer einen
  // Rueckfall (Archivliste, OSM-Raster), der nur lokal Sinn hat.
  const fern = u => /^https?:\/\//i.test(u);

  // Quelle der Verbotszonen, in dieser Reihenfolge:
  // (1) ?pmtiles=<URL einer .pmtiles-Datei> (Range-Anfragen),
  // (2) ?tiles=<URL-Vorlage mit {z}/{x}/{y}> (ein fremder Kachelserver),
  // (3) ?ds=<name> bzw. das erste Archiv von `bubatz serve` am eigenen Origin (TileJSON
  //     unter /<name>/tiles.json, Liste unter /datasets), sonst
  // (4) cfg.zones -- neben der Seite (statisch) oder die absolute R2-URL aus config.js.
  const params = new URLSearchParams(location.search);
  let archive = params.get("pmtiles");
  const maplibregl = await maplibreLaden;
  // Der Worker kommt aus der eigenen Datei (CSP `worker-src 'self'`, ohne blob:).
  // Ohne diesen Aufruf naehme MapLibre dieselbe Datei mit "?v=<Fassung>" -- die
  // Trusted-Types-Policy oben laesst aber nur genau WORKER_URL durch.
  maplibregl.setWorkerUrl(WORKER_URL);
  const protocol = new pmtiles.Protocol();
  maplibregl.addProtocol("pmtiles", protocol.tile);

  // Die drei HEAD-Pruefungen laufen gleichzeitig statt nacheinander: zwei
  // Umlaeufe zu R2 weniger vor dem ersten Kartenbild.
  const eigeneZonen = !archive && !params.get("tiles");
  const zonenPruefung = eigeneZonen ? pruefe(cfg.zones) : null;
  const grundPruefung = pruefe(cfg.basemap);
  const fussPruefung = pruefe(fussgaengerUrl);

  let datasets = [];
  if (eigeneZonen) {
    // Erst die Datei neben der Seite, dann die Archivliste von `bubatz serve`.
    // Andersherum fragte der statische Stand bei JEDEM Aufruf `/datasets` und
    // bekam 404 -- in jeder Browserkonsole sichtbar.
    if ((await zonenPruefung) === "da") {
      archive = abs(cfg.zones);
    } else if (fern(cfg.zones)) {
      startFehler("Die Kartendaten sind gerade nicht erreichbar. Bitte die Seite in ein paar Minuten neu laden.");
      return;
    }
    if (!archive) {
      try {
        const r = await fetch("datasets", { signal: AbortSignal.timeout(10_000) });
        if (r.ok) datasets = await r.json();
      } catch (e) { /* statisch gehostet */ }
    }
  }
  const dsName = params.get("ds") || (datasets[0] && datasets[0].name);
  let source, label;
  if (archive) {
    // Das Archiv traegt keine Quellenangabe in seinen Metadaten; ohne diese Zeile
    // nannte die Karte im Betrieb nur MapLibre und die Grundkarte.
    source = { type: "vector", url: `pmtiles://${archive}`,
      attribution: '© <a href="https://www.openstreetmap.org/copyright">OpenStreetMap-Mitwirkende</a> (ODbL)' };
    label = archive;
  } else if (params.get("tiles")) {
    source = { type: "vector", tiles: [params.get("tiles")], minzoom: 8, maxzoom: 14 }; label = params.get("tiles");
  } else if (dsName) {
    label = `${location.origin}/${dsName}/tiles.json`; source = { type: "vector", url: label };
  } else {
    label = `${location.origin}/zonen/{z}/{x}/{y}.mvt`; source = { type: "vector", tiles: [label], minzoom: 8, maxzoom: 14 };
  }

  // Archivauswahl, wenn `bubatz serve` mehr als eines ausliefert; Wechsel per ?ds=,
  // der Kartenausschnitt bleibt ueber den URL-Hash (hash: true) erhalten.
  if (datasets.length > 1) {
    const sel = document.getElementById("ds");
    for (const d of datasets) {
      const o = document.createElement("option");
      o.value = d.name; o.textContent = d.error ? `${d.name} (nicht verfügbar)` : d.name;
      o.selected = d.name === dsName; sel.appendChild(o);
    }
    // Erst auf den Knopf laden, nicht schon bei `change`: mit den Pfeiltasten loest
    // eine geschlossene Auswahl je Schritt `change` aus und luede jedes Archiv.
    document.getElementById("ds-wechseln").addEventListener("click", () => {
      params.set("ds", sel.value); location.search = params.toString();
    });
    document.getElementById("datasets").style.display = "block";
  }

  // Grundkarte: Protomaps (Vektor, PMTiles, Stil @protomaps/basemaps "light",
  // Glyphen und Sprites aus vendor/basemaps -- nichts von fremden Servern).
  // Fehlt das Archiv (lokale Vorschau ohne --basemap), OSM-Rasterkacheln als
  // Notnagel; der ist NICHT fuer den Betrieb gedacht (OSMF-Kachelrichtlinie). Im
  // Betrieb (absolute Adresse) sagt die Seite stattdessen, dass die Grundkarte
  // fehlt -- vorher lief jeder Fehler hierher, und weil die CSP
  // tile.openstreetmap.org sperrt, blieb die Grundkarte still leer.
  const attribution = '<a href="https://protomaps.com">Protomaps</a> &copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap-Mitwirkende</a>';
  const haveBasemap = (await grundPruefung) === "da";
  if (!haveBasemap && fern(cfg.basemap)) {
    startFehler("Die Grundkarte ist gerade nicht erreichbar. Bitte die Seite in ein paar Minuten neu laden.");
    return;
  }
  const sources = { zonen: source };
  let layers;
  if (haveBasemap) {
    sources.basemap = { type: "vector", url: `pmtiles://${abs(cfg.basemap)}`, attribution };
    layers = basemaps.layers("basemap", basemaps.namedFlavor("light"), { lang: "de" });
    // Der Stil zeigt Rathaeuser (kind "townhall") mit einem gleichnamigen Symbol,
    // das basemaps-assets nie ausgeliefert hat (Sprite v4, auch upstream): MapLibre
    // warnte bei jedem Laden und zeichnete keins. Rathaeuser bekommen "building"
    // aus derselben Farbgruppe. tests/test_viewer_grundkarte.py haelt das gegen
    // Stil und Sprite.
    const pois = layers.find(l => l.id === "pois");
    const bild = pois && pois.layout && pois.layout["icon-image"];
    if (Array.isArray(bild) && bild[0] === "match" && !bild.includes("townhall")) {
      pois.layout["icon-image"] = [...bild.slice(0, -1), "townhall", "building", bild[bild.length - 1]];
    }
  } else {
    console.warn("basemap.pmtiles fehlt -- OSM-Raster als Notnagel");
    sources.osm = { type: "raster", tiles: ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"], tileSize: 256,
                    attribution: "&copy; OpenStreetMap-Mitwirkende" };
    layers = [{ id: "osm", type: "raster", source: "osm", paint: { "raster-saturation": -0.6, "raster-opacity": 0.9 } }];
  }
  // Bandfilter (<= 100 m) und Kategorieauswahl der Legende; `null` = alle.
  const bandFilter = ["<=", ["get", "band_max_m"], 100];
  const filterFor = aktiv => (aktiv
    ? ["all", bandFilter, ["in", ["get", "category"], ["literal", aktiv]]]
    : bandFilter);

  layers.push(
    // fill-extrusion statt fill: bei einem fill-Layer addiert sich die
    // Transparenz ueberlappender Merkmale (die Baender eines Objekts
    // ueberlappen nach der einseitigen Vereinfachung um wenige Meter,
    // Nachbarobjekte ohnehin) zu dunklen Ringen; ein flacher Extrusions-
    // Layer wird mit Tiefentest gezeichnet und wirkt wie EINE Flaeche.
    { id: "zonen-fill", type: "fill-extrusion", source: "zonen", "source-layer": "zonen",
      filter: bandFilter,
      paint: { "fill-extrusion-color": farbe("--zone", "#c0392b"), "fill-extrusion-opacity": 0.4,
               "fill-extrusion-height": 1, "fill-extrusion-base": 0 } },
    { id: "zonen-line", type: "line", source: "zonen", "source-layer": "zonen", minzoom: 12,
      filter: bandFilter,
      paint: { "line-color": farbe("--zone-rand", "#7b241c"), "line-width": 0.5, "line-opacity": 0.35 } },
  );

  // Fussgaengerzonen (§ 5 Abs. 2 Nr. 5): eigene, zuschaltbare Ebene, OHNE
  // Sichtweite und ohne Puffer -- Flaechen als Flaeche, Wege als Linie (bubatz
  // pedestrian). Blau statt Rot, weil es eine andere Rechtsfolge ist; bei 60 %
  // Deckkraft vom Zonenrot messbar unterscheidbar (Delta E
  // 16,1 normal, 10,6 bei Farbfehlsichtigkeit). Fehlt das Archiv, fehlt die
  // Ebene -- kein Fehler, nur kein Angebot.
  const haveFuss = (await fussPruefung) === "da";

  // Ohne WebGL zeichnet MapLibre nichts, der Konstruktor wirft (abgeschaltete
  // Hardwarebeschleunigung, gesperrter Grafiktreiber, manche Fernzugriffe und
  // gehaertete Browser). Kein Vorab-Test: der Konstruktor ist die Stelle, die
  // es wirklich braucht, und merkt auch "API da, Kontext nicht erzeugbar".
  let map;
  try {
    map = new maplibregl.Map({
    container: "map",
    center: [6.7735, 51.2277], zoom: 13, hash: true,
    // MapLibre beschriftet seine Bedienelemente sonst englisch -- auf einer
    // durchgehend deutschen Seite hoert ein Screenreader-Publikum sonst
    // "Reset bearing to north".
    locale: {
      // Der Name sagt auch, was die Tastatur auf der Karte kann (siehe zeigeMitte).
      "Map.Title": "Karte – Pfeiltasten verschieben, Enter nennt, was in der Kartenmitte liegt",
      "NavigationControl.ZoomIn": "Hineinzoomen",
      "NavigationControl.ZoomOut": "Herauszoomen",
      "NavigationControl.ResetBearing": "Nach Norden ausrichten",
      "GeolocateControl.FindMyLocation": "Meinen Standort finden",
      "GeolocateControl.LocationNotAvailable": "Standort nicht verfügbar",
      "AttributionControl.ToggleAttribution": "Quellenangabe ein- und ausblenden",
      "AttributionControl.MapFeedback": "Rückmeldung zur Karte",
      "Popup.Close": "Schließen",
      "ScaleControl.Meters": "m",
      "ScaleControl.Kilometers": "km",
    },
    style: {
      version: 8,
      glyphs: `${abs(cfg.assets)}/fonts/{fontstack}/{range}.pbf`,
      sprite: `${abs(cfg.assets)}/sprites/v4/light`,
      sources, layers,
    },
    });
  } catch (err) {
    // MapLibre 6 wirft GPUInitializationError, wenn es kein WebGL 2 bekommt.
    if (!(err instanceof maplibregl.GPUInitializationError) && !/webgl/i.test(String(err && err.message))) throw err;
    console.error(err);
    startFehler("Die Karte braucht WebGL 2, und dieser Browser stellt es gerade nicht bereit. "
      + "Meist hilft es, in den Browser-Einstellungen die Hardwarebeschleunigung einzuschalten "
      + "oder einen anderen Browser zu nehmen.");
    return;
  }
  map.addControl(new maplibregl.NavigationControl());

  // Popups: Esc schliesst sie, und der Fokus kehrt auf die Karte zurueck.
  // MapLibre setzt ihn beim Oeffnen auf den Schliessen-Knopf; nach dem Schliessen
  // lag er sonst auf dem Seitenanfang.
  let offenesPopup = null;
  // `fokus: false` fuer Popups, die nicht auf eine Eingabe auf der Karte folgen
  // (nach Suche und Standort): dort bleibt der Fokus, wo er war.
  const zeigePopup = (lngLat, box, fokus = true) => {
    if (offenesPopup) offenesPopup.remove();
    const popup = new maplibregl.Popup({ focusAfterOpen: fokus }).setLngLat(lngLat).setDOMContent(box);
    popup.on("close", () => {
      if (offenesPopup === popup) offenesPopup = null;
      const f = document.activeElement;
      if (!f || f === document.body) map.getCanvas().focus({ preventScroll: true });
    });
    offenesPopup = popup.addTo(map);
  };
  map.getContainer().addEventListener("keydown", e => {
    if (e.key === "Escape" && offenesPopup) offenesPopup.remove();
  });

  // Legende: eine Zeile je Kategorie, jede blendet ihre Zonen aus und ein.
  // Ohne Auswahl waere die Karte leer -- die letzte Kategorie laesst sich nicht
  // abwaehlen, das erspart den leeren Zustand ohne Erklaerung.
  const legende = document.getElementById("legende");
  const kaesten = KATEGORIEN.map(([key, titel]) => {
    const zeile = document.createElement("label");
    zeile.className = "legende-zeile";
    const kasten = document.createElement("input");
    kasten.type = "checkbox"; kasten.checked = true; kasten.value = key;
    kasten.id = "kat-" + key; zeile.htmlFor = kasten.id;
    const text = document.createElement("span");
    text.textContent = titel;
    zeile.append(kasten, text);
    legende.appendChild(zeile);
    // Gesperrt per aria-disabled statt disabled: ein disabled-Kasten fiel aus der
    // Tabulatorreihenfolge, und niemand erfuhr, warum er nicht ging.
    kasten.addEventListener("click", e => {
      if (kasten.getAttribute("aria-disabled") === "true") e.preventDefault();
    });
    return kasten;
  });
  const anwenden = () => {
    const aktiv = kaesten.filter(k => k.checked).map(k => k.value);
    kaesten.forEach(k => {
      const gesperrt = k.checked && aktiv.length === 1;
      k.setAttribute("aria-disabled", String(gesperrt));
      if (gesperrt) k.setAttribute("aria-describedby", "kat-hinweis");
      else k.removeAttribute("aria-describedby");
    });
    const f = filterFor(aktiv.length === KATEGORIEN.length ? null : aktiv);
    for (const id of ["zonen-fill", "zonen-line"]) map.setFilter(id, f);
  };
  kaesten.forEach(k => k.addEventListener("change", anwenden));

  // Fussgaengerzonen: Schalter und Uhrzeit. Das Verbot gilt 7-20 Uhr, und zwar
  // nach deutscher Zeit -- nicht nach der Uhr des Geraets, die im Ausland oder
  // falsch gestellt eine falsche Antwort gaebe. Ausserhalb der Zeit bleibt die
  // Ebene sichtbar (man will wissen, wo sie ist), aber blass und beschriftet.
  // formatToParts statt format(): `de-DE` mit `hour: "numeric"` liefert
  // "11 Uhr", und Number("11 Uhr") ist NaN -- die Pruefung sagte dann immer
  // "gilt jetzt nicht". `hourCycle:
  // "h23"` verhindert ausserdem die 12 statt 0 um Mitternacht.
  const stunde = () => Number(new Intl.DateTimeFormat("de-DE", {
    timeZone: "Europe/Berlin", hour: "2-digit", hourCycle: "h23",
  }).formatToParts(new Date()).find(p => p.type === "hour").value);
  const giltJetzt = () => { const h = stunde(); return h >= 7 && h < 20; };
  const fussSchalter = document.getElementById("fuss-an");
  const fussZeit = document.getElementById("fuss-zeit");
  if (!haveFuss) {
    const zeile = document.getElementById("fuss-zeile");
    if (zeile) zeile.remove();
  } else {
    // Quelle und Ebenen kommen erst beim Einschalten dazu. Gemessen am
    // Steht die pmtiles-Quelle schon im ANFANGSSTIL, wird dieser
    // spuerbar langsamer fertig. Und alles, was Ebenen anfasst -- addSource,
    // addLayer, getLayer, setLayoutProperty --, wirft "Style is not done
    // loading", solange er es nicht ist. Ohne Grundkarte (Rasternotnagel)
    // kann das Minuten dauern, und `styledata`/`load` feuern in der Zeit
    // nicht. Deshalb wird es schlicht wiederholt, bis es geht: ein
    // Fehlversuch kostet nichts, eine Frist hiesse, die Ebene bliebe stumm aus.
    const fussEbenen = jetzt => [
      { id: "fuss-fill", type: "fill", source: "fussgaenger", "source-layer": "fussgaengerzonen",
        filter: ["==", ["geometry-type"], "Polygon"],
        paint: { "fill-color": farbe("--fuss", "#2a78d6"), "fill-opacity": jetzt ? 0.6 : 0.25 } },
      { id: "fuss-line", type: "line", source: "fussgaenger", "source-layer": "fussgaengerzonen",
        filter: ["==", ["geometry-type"], "LineString"],
        paint: { "line-color": farbe("--fuss", "#2a78d6"), "line-opacity": jetzt ? 0.85 : 0.35, "line-width": 4 } },
    ];
    let fussQuelleDa = false;
    let fussStand = null;    // null = keine Ebenen, sonst der gezeigte Zeitzustand
    let fussWartet = false;  // laeuft schon ein Wiederholer?
    const fussAnwenden = () => {
      const an = fussSchalter.checked;
      const jetzt = giltJetzt();
      const soll = an ? jetzt : null;
      if (soll === fussStand) return true;
      if (fussStand !== null) {
        for (const id of ["fuss-fill", "fuss-line"]) map.removeLayer(id);
      }
      if (an) {
        if (!fussQuelleDa) {
          map.addSource("fussgaenger", { type: "vector", url: `pmtiles://${abs(fussgaengerUrl)}` });
          fussQuelleDa = true;
        }
        for (const e of fussEbenen(jetzt)) map.addLayer(e);
      }
      fussStand = soll;
      return true;
    };
    // Was zur Rechtslage dasteht, darf nicht an einem Renderzustand haengen:
    // eigene Funktion, damit Schalter, Beschriftung und Legende schon vor dem
    // ersten Kartenbild zusammenpassen.
    const fussBeschriften = () => {
      const an = fussSchalter.checked;
      fussZeit.textContent = an
        ? (giltJetzt() ? "gilt jetzt (7–20 Uhr)" : "gilt jetzt nicht (7–20 Uhr)")
        : "";
      // Blau in der Farblegende steht nur, solange auch Blau auf der Karte ist:
      // das CSS folgt dem Haken (#panel:has(#fuss-an:checked)).
    };
    const fussZeichnen = () => {
      fussBeschriften();
      try {
        fussAnwenden();
      } catch (err) {
        // Hoechstens ein Wiederholer: jeder Fehlversuch (Schalter, Minutentakt)
        // startete vorher eine eigene Kette, solange der Stil lud.
        if (fussWartet) return;
        fussWartet = true;
        setTimeout(() => { fussWartet = false; fussZeichnen(); }, 1000);
      }
    };
    // Voreinstellung: die Ebene ist an, wenn das Verbot gerade gilt (7-20 Uhr
    // nach Europe/Berlin, nicht nach der Uhr des Geraets). NUR beim Laden --
    // danach gehoert der Schalter der Person davor, und der Minutentakt unten
    // zeichnet zwar Deckkraft und Beschriftung nach, ruehrt den Schalter aber
    // nicht an. Wer die Seite um 19:58 oeffnet, behaelt seine Wahl um 20:01.
    fussSchalter.checked = giltJetzt();
    fussBeschriften();
    fussSchalter.addEventListener("change", fussZeichnen);
    map.once("load", fussZeichnen);
    // Der Zustand wechselt zweimal am Tag; minuetlich nachsehen reicht.
    setInterval(fussZeichnen, 60_000);
    for (const id of ["fuss-fill", "fuss-line"]) {
      map.on("click", id, e => {
        const box = document.createElement("div");
        const b = document.createElement("b");
        b.textContent = "Fußgängerzone";
        box.appendChild(b);
        box.appendChild(document.createElement("br"));
        const p = e.features[0].properties;
        // Amtlich belegte Ergaenzung (config/fussgaengerzonen.toml): Quelle statt OSM-Nummer.
        // Aus Vektorkacheln kommt der Wahrheitswert je nach Version als true oder "true".
        if (p.amtlich === true || p.amtlich === "true") {
          box.appendChild(document.createTextNode(String(p.ort) + ": " + String(p.name)));
          box.appendChild(document.createElement("br"));
          box.appendChild(document.createTextNode(
            "nach " + String(p.beleg) + "; in OpenStreetMap noch nicht als Fußgängerzone eingetragen"));
          if (String(p.quelle).startsWith("https://")) {
            box.appendChild(document.createElement("br"));
            const a = document.createElement("a");
            a.href = String(p.quelle); a.target = "_blank"; a.rel = "noopener noreferrer";
            a.textContent = "Quelle";
            box.appendChild(a);
          }
        } else {
          box.appendChild(document.createTextNode(
            (p.osm_type === "relation" ? "relation " : "way ") + String(p.osm_id)));
        }
        box.appendChild(document.createElement("br"));
        box.appendChild(document.createTextNode(
          "§ 5 Abs. 2 Nr. 5 KCanG, 7–20 Uhr — " + (giltJetzt() ? "gilt jetzt" : "gilt jetzt nicht")));
        zeigePopup(e.lngLat, box);
      });
    }
  }
  // Oertliche Verbote ausserhalb von Paragraf 5 KCanG: Verordnungen und
  // Allgemeinverfuegungen einer Stadt mit fester Flaeche und Laufzeit
  // (config/sonderzonen.toml -> scripts/sonderzonen.py -> sonderzonen.json,
  // liegt neben der Seite, nicht im Bucket). Lila, weil es eine dritte
  // Rechtsfolge ist: Konsum verboten, teils auch das Mitfuehren. Gezeigt wird
  // nur, was heute (Europe/Berlin) gilt; die Datei traegt gueltig_ab/bis.
  const sonderUrl = cfg.sonderzonen || "sonderzonen.json";
  const sonderSchalter = document.getElementById("sonder-an");
  const sonderZahl = document.getElementById("sonder-zahl");
  const sonderZeile = document.getElementById("sonder-zeile");
  const heuteBerlin = () => new Intl.DateTimeFormat("sv-SE", { timeZone: "Europe/Berlin" }).format(new Date());
  const datumDe = iso => iso ? new Date(iso + "T12:00:00Z").toLocaleDateString("de-DE", { timeZone: "UTC" }) : "";
  // Laeuft neben dem Rest her: vorher wartete alles darunter (Suche, Standort,
  // Teilen) bis zu 8 s auf diese Datei.
  (async () => {
  let sonderDaten = null;
  try {
    const r = await fetch(sonderUrl, { cache: "no-cache", signal: AbortSignal.timeout(8000) });
    if (r.ok) {
      const fc = await r.json();
      const heute = heuteBerlin();
      sonderDaten = {
        type: "FeatureCollection",
        features: (fc.features || []).filter(f => {
          const p = f.properties || {};
          return p.gueltig_ab && p.gueltig_ab <= heute && (!p.gueltig_bis || heute <= p.gueltig_bis);
        }),
      };
    }
  } catch (e) { /* keine Datei, oder zu langsam: dann kein Angebot */ }
  if (!sonderDaten || !sonderDaten.features.length) {
    if (sonderZeile) sonderZeile.remove();
  } else {
    const orte = [...new Set(sonderDaten.features.map(f => f.properties.ort))];
    sonderZahl.textContent = "(" + String(sonderDaten.features.length) + ": " + orte.join(", ") + ")";
    let sonderStand = false;
    let sonderQuelleDa = false;
    const sonderAnwenden = () => {
      const an = sonderSchalter.checked;
      if (an === sonderStand) return;
      if (an) {
        if (!sonderQuelleDa) {
          map.addSource("sonder", { type: "geojson", data: sonderDaten });
          sonderQuelleDa = true;
        }
        map.addLayer({ id: "sonder-fill", type: "fill", source: "sonder",
          paint: { "fill-color": farbe("--sonder", "#7d3c98"), "fill-opacity": 0.45 } });
        map.addLayer({ id: "sonder-line", type: "line", source: "sonder",
          paint: { "line-color": farbe("--sonder", "#7d3c98"), "line-width": 2, "line-opacity": 0.9 } });
      } else {
        for (const id of ["sonder-fill", "sonder-line"]) map.removeLayer(id);
      }
      sonderStand = an;
    };
    // Wie bei den Fussgaengerzonen: wiederholen, bis der Stil fertig ist, mit
    // hoechstens einem Wiederholer. Direkt aufgerufen statt auf `load`, denn
    // die Datei kann nach `load` ankommen.
    let sonderWartet = false;
    const sonderZeichnen = () => {
      try { sonderAnwenden(); } catch (err) {
        if (sonderWartet) return;
        sonderWartet = true;
        setTimeout(() => { sonderWartet = false; sonderZeichnen(); }, 1000);
      }
    };
    sonderSchalter.addEventListener("change", sonderZeichnen);
    sonderZeichnen();
    map.on("click", "sonder-fill", e => {
      const p = e.features[0].properties;
      // MapLibre 6 gibt Listen aus GeoJSON-Eigenschaften als echte Arrays heraus, 5 gab
      // sie als JSON-Text. Beides annehmen: als Text zerfiele ["konsum","mitfuehren"]
      // sonst zu einem Eintrag, und das Popup verschwiege das Mitfuehrverbot.
      const verbot = Array.isArray(p.verbot) ? p.verbot.map(String)
        : (() => { try { return JSON.parse(p.verbot); } catch (x) { return [String(p.verbot)]; } })();
      const box = document.createElement("div");
      const b = document.createElement("b");
      b.textContent = String(p.name);
      box.appendChild(b);
      const zeile = t => { box.appendChild(document.createElement("br")); box.appendChild(document.createTextNode(t)); };
      zeile(String(p.ort) + " · " + String(p.rechtsgrundlage));
      zeile((verbot.includes("mitfuehren") ? "Konsum und Mitführen zum Konsum verboten" : "Konsum verboten")
        + (p.zeit && p.zeit !== "ganztags" ? ", " + String(p.zeit) : ", ganztags"));
      zeile("gilt seit " + datumDe(p.gueltig_ab) + (p.gueltig_bis && p.gueltig_bis !== "null" ? " bis " + datumDe(p.gueltig_bis) : ""));
      if (p.hinweis) zeile(String(p.hinweis));
      box.appendChild(document.createElement("br"));
      const link = (href, text) => {
        const a = document.createElement("a");
        a.href = String(href); a.target = "_blank"; a.rel = "noopener noreferrer"; a.textContent = text;
        box.appendChild(a);
      };
      link(p.quelle, "Rechtsakt");
      if (p.karte && p.karte !== "null") { box.appendChild(document.createTextNode(" · ")); link(p.karte, "Amtliche Karte"); }
      zeigePopup(e.lngLat, box);
    });
    map.on("mouseenter", "sonder-fill", () => { map.getCanvas().style.cursor = "pointer"; });
    map.on("mouseleave", "sonder-fill", () => { map.getCanvas().style.cursor = ""; });
  }
  })().catch(err => console.error(err));
  // Eigener Standort: Knopf unter der Navigation, folgt der Position
  // (trackUserLocation) mit Genauigkeitskreis. Braucht einen sicheren Kontext
  // (HTTPS oder localhost) und die Freigabe im Browser; die Seite selbst
  // speichert nichts, die Koordinaten bleiben in MapLibre.
  const geolocate = new maplibregl.GeolocateControl({
    positionOptions: { enableHighAccuracy: true },
    fitBoundsOptions: { maxZoom: 16 },
    trackUserLocation: true,
    showAccuracyCircle: true,
  });
  map.addControl(geolocate);

  // Ortssuche über Nominatim (OpenStreetMap). Die EINZIGE Stelle, an der die
  // Seite einen fremden Dienst anspricht -- deshalb steht der Hinweis darauf
  // fest im Panel und nicht erst in der Datenschutzerklärung.
  //
  // Erst beim Absenden, nie während des Tippens: Nominatims Nutzungsregeln
  // verbieten client-seitige Vervollständigung ausdrücklich ("you must not
  // implement such a service on the client side using the API"), und es
  // verrät nebenbei deutlich weniger. Ein Lauf je Absenden, höchstens fünf
  // Treffer, auf Deutschland beschränkt -- die Karte endet ohnehin dort.
  //
  // Die CSP der Seite (Infra-Repo) muss den Geocoder in `connect-src` führen,
  // Fehlt der Eintrag in der CSP, scheitert der Aufruf.
  // Deshalb eine verständliche Meldung statt einer stummen leeren Liste.
  const sucheForm = document.getElementById("suche");
  const sucheFeld = document.getElementById("suche-feld");
  const sucheTreffer = document.getElementById("suche-treffer");
  const sucheKnopf = sucheForm.querySelector("button");
  // Angesagt wird ein kurzer Satz in einer eigenen Live-Region, nicht die
  // Trefferliste selbst (fuenf Knoepfe waeren zu geschwaetzig). Gleicher Text
  // zweimal: ein Leerzeichen anhaengen, sonst schweigen manche Screenreader.
  const ansage = document.getElementById("ansage");
  const sagAn = text => { ansage.textContent = ansage.textContent === text ? text + "\u00a0" : text; };
  const sucheMeldung = text => {
    sagAn(text);
    sucheTreffer.replaceChildren();
    const s = document.createElement("span");
    s.className = "leer";
    s.textContent = text;
    sucheTreffer.appendChild(s);
  };
  // Enter muss suchen. Das implizite Absenden eines Formulars mit Absendeknopf
  // kam in der Vorschau nicht an (gemessen: Feld fokussiert, Enter
  // gedrueckt, kein Aufruf) -- also ausdruecklich. Eine Suche, die auf Enter
  // nichts tut, ist kaputt.
  // Waehrend einer IME-Eingabe (Japanisch, Chinesisch ...) bestaetigt Enter das
  // Zeichen; das darf nicht schon halbfertig an Nominatim gehen.
  sucheFeld.addEventListener("keydown", e => {
    if (e.key !== "Enter") return;
    e.preventDefault();
    if (e.isComposing || e.keyCode === 229) return;
    sucheForm.requestSubmit();
  });
  sucheForm.addEventListener("submit", async e => {
    e.preventDefault();
    const q = sucheFeld.value.trim();
    if (!q) return;
    sucheKnopf.disabled = true;
    sucheMeldung("sucht …");
    try {
      const u = new URL("https://nominatim.openstreetmap.org/search");
      u.search = new URLSearchParams({
        q, format: "jsonv2", limit: "5", countrycodes: "de", "accept-language": "de",
      });
      const r = await fetch(u, { signal: AbortSignal.timeout(10000) });
      if (!r.ok) throw new Error("HTTP " + r.status);
      const treffer = await r.json();
      if (!treffer.length) { sucheMeldung("nichts gefunden"); return; }
      sucheTreffer.replaceChildren();
      sagAn(treffer.length + " Treffer");
      for (const t of treffer) {
        // display_name ist OSM-Freitext: nur textContent, nie innerHTML.
        const b = document.createElement("button");
        b.type = "button";
        b.textContent = t.display_name;
        b.addEventListener("click", () => {
          sucheTreffer.replaceChildren();
          // Der fokussierte Knopf ist jetzt weg; der Fokus kehrt ins Suchfeld zurueck
          // statt an den Seitenanfang.
          sucheFeld.focus();
          sagAn("Karte zeigt " + t.display_name);
          nachBewegung(() => sagAn(zeigeMitte(false)));
          if (t.boundingbox) {
            const [s1, n1, w1, e1] = t.boundingbox.map(Number);
            map.fitBounds([[w1, s1], [e1, n1]], { maxZoom: 17, padding: 40 });
          } else {
            map.flyTo({ center: [Number(t.lon), Number(t.lat)], zoom: 16 });
          }
        });
        sucheTreffer.appendChild(b);
      }
    } catch (err) {
      sucheMeldung("Suche nicht erreichbar");
    } finally {
      sucheKnopf.disabled = false;
    }
  });

  // Statuszeile beim ersten Leerlauf (Datenstand aus stand.json, wenn vorhanden),
  // Groesse nachziehen, falls der Container beim Start noch keine Abmessungen
  // hatte (eingebettete Browser).
  requestAnimationFrame(() => map.resize());
  // Alles per textContent: `label` stammt aus der URL (?tiles=, ?pmtiles=),
  // MapLibre-Fehlertexte enthalten URLs, stand.json liegt im Bucket.
  const status = document.getElementById("status");
  const setStatus = (parts, warn) => {
    status.replaceChildren();
    const small = document.createElement("small");
    small.classList.toggle("warn", Boolean(warn));
    small.textContent = parts.join(" \u00b7 ");
    status.appendChild(small);
  };
  // Fluechtige Meldungen stehen NEBEN dem Datenstand, nicht an seiner Stelle:
  // vorher loeschte eine verweigerte Ortungsfreigabe das Datum der Daten
  // sonst bleibt die Meldung stehen.
  const meldung = document.getElementById("meldung");
  const setMeldung = text => { meldung.textContent = text || ""; };

  // Der Datenstand ist eine Eigenschaft der DATEN, nicht des Renderers --
  // deshalb sofort holen, nicht erst im ersten `idle` der Karte. Vorher blieb
  // "lade ..." stehen, solange die Karte nicht fertig wurde: gemessen ueber
  // 30 s, ohne Grundkarte unbegrenzt. Eigene Frist, damit
  // der Platzhalter nicht wieder haengen bleibt.
  (async () => {
    const parts = [];
    try {
      const r = await fetch(cfg.stand, { signal: AbortSignal.timeout(5000) });
      if (r.ok) {
        const j = await r.json();
        if (j.pbf_timestamp) parts.push("OSM-Stand " + String(j.pbf_timestamp).slice(0, 10));
        // Veraltet-Hinweis: der Tageszyklus laeuft nicht mehr (Timer/Server aus,
        // Pruefkette schlaegt fehl) -- fuer Nutzer sichtbar, nicht nur im Log.
        const age = (Date.now() - Date.parse(j.published || "")) / 86400e3;
        if (age > 3) parts.push("ACHTUNG: seit " + Math.floor(age) + " Tagen nicht aktualisiert");
      }
    } catch (e) { /* kein stand.json, oder zu langsam */ }
    if (!parts.length) parts.push("Datenstand unbekannt");
    setStatus(parts, parts.some(t => t.startsWith("ACHTUNG")));
    // Die rohe Kachel-URL ist Technik und stand vorher im Bedienfeld; sie
    // bleibt erreichbar, aber nur noch als Titel der Zeile.
    status.title = "Kacheln: " + label;
  })();

  // Ladeschleier: die Grundkarte malt bis zu ihrer Ankunft ihre eigene
  // Void-Farbe (#cccccc), Zonen auf Grau lesen sich als Fehler (V5).
  const ladeschleier = document.getElementById("laden");
  // `load` (Stil steht, erstes Bild gezeichnet) ODER `idle` (nichts mehr
  // unterwegs), je nachdem was zuerst kommt: `idle` allein bleibt aus,
  // solange ueberhaupt noch Kacheln nachlaufen.
  const fertig = () => { ladeschleier.hidden = true; };
  map.once("load", fertig);
  map.once("idle", fertig);
  // 30 s, und ohne Schuldzuweisung: beim ersten Aufruf werden ein rund
  // 370 MB grosses Kachelarchiv und die Grundkarte per Range-Anfragen angelesen, ein zaeher
  // Aufbau ist der Normalfall und kein Verbindungsfehler. Die erste Fassung
  // (15 s, "Verbindung pruefen") schlug auf der Live-Seite bei einem voellig
  // gewoehnlichen Kaltstart an.
  setTimeout(() => {
    if (!ladeschleier.hidden) ladeschleier.textContent = "Die Karte lädt noch \u2013 das Kachelarchiv ist groß.";
  }, 30000);

  map.on("error", e => { if (e.error) setMeldung(String(e.error.message)); });
  // GeolocationPositionError: 1 verweigert, 2 keine Position, 3 Zeitueberschreitung.
  geolocate.on("error", e => {
    const why = { 1: "Freigabe im Browser verweigert", 2: "Position nicht ermittelbar", 3: "Zeitüberschreitung" };
    setMeldung("Standort: " + (why[e && e.code] || (isSecureContext ? "Fehler" : "nur über HTTPS")));
  });
  let standortFrage = false;  // nur nach einem Druck auf "Mein Standort", nicht bei jeder Nachfuehrung
  geolocate.on("geolocate", () => {
    setMeldung("");
    if (!standortFrage) return;
    standortFrage = false;
    nachBewegung(() => sagAn(zeigeMitte(false)));
  });

  // Eigener Standort als beschrifteter Knopf im Bedienfeld: das Kartensymbol
  // ist unbeschriftet und auf dem Telefon unten rechts -- in Tests die erste
  // gesuchte und nicht gefundene Funktion.
  document.getElementById("standort-knopf").addEventListener("click", () => {
    setMeldung("");
    standortFrage = true;
    geolocate.trigger();
  });

  // Ausschnitt teilen. `hash: true` schreibt ihn ohnehin in die Adresse; nur
  // wusste das niemand, und auf dem Tablet ist die Adresszeile muehsam
  // weil der Teilen-Weg sonst nicht gefunden wird.
  const teilenKnopf = document.getElementById("teilen-knopf");
  // Der neue Knopftext allein wird nicht verlaesslich angesagt, daher zusaetzlich
  // die Live-Region. Ein Zeitgeber, nicht einer je Klick.
  let teilenUhr = 0;
  teilenKnopf.addEventListener("click", async () => {
    const url = location.href;
    try {
      await navigator.clipboard.writeText(url);
      teilenKnopf.textContent = "Adresse kopiert";
      sagAn("Adresse kopiert");
    } catch (e) {
      // Ohne Zwischenablage-Freigabe: Adresse sichtbar und markierbar zeigen.
      setMeldung(url);
    }
    clearTimeout(teilenUhr);
    teilenUhr = setTimeout(() => { teilenKnopf.textContent = "Ausschnitt teilen"; }, 3000);
  });

  // Der Weg zur Ueber-Seite nimmt den Ausschnitt mit und zurueck: vorher stand
  // man nach der Rueckkehr wieder im Anfangsausschnitt. Er reist im Fragment
  // (#karte=...), das der Browser nicht an den Server schickt; als Query stand er
  // im Zugriffsprotokoll, nach "Mein Standort" also die eigene Position.
  const ueberLink = document.getElementById("ueber-link");
  const ueberZiel = () => "about.html" + (location.hash ? "#" + new URLSearchParams({ karte: location.hash.slice(1) }) : "");
  ueberLink.addEventListener("click", () => { ueberLink.href = ueberZiel(); });
  ueberLink.href = ueberZiel();
  map.on("moveend", () => { ueberLink.href = ueberZiel(); });
  map.on("click", "zonen-fill", e => {
    // Eigenschaften kommen aus den Kacheln: category (Name aus config/tags.*.json),
    // osm_id, band_max_m. Sollten je OSM-Freitexte (name, operator) dazukommen:
    // weiterhin nur textContent, nie setHTML.
    const p = e.features[0].properties;
    const box = document.createElement("div");
    const b = document.createElement("b");
    b.textContent = TITEL.get(p.category) || String(p.category);
    box.appendChild(b);
    box.appendChild(document.createElement("br"));
    box.appendChild(document.createTextNode("Diese Einrichtung verursacht die Zone."));
    box.appendChild(document.createElement("br"));
    box.appendChild(document.createTextNode("Sichtweite bis " + String(p.band_max_m) + " m"));
    box.appendChild(document.createElement("br"));
    // `osm_type` steht im Kachelvertrag. Fehlt es doch (ein Archiv von vor
    // dem Vertrag), bleibt es bei der nackten Kennung statt bei einem
    // geratenen Link -- ein Weg und eine Relation koennen dieselbe Zahl tragen.
    if (p.osm_type) {
      const a = document.createElement("a");
      a.href = "https://www.openstreetmap.org/" + String(p.osm_type) + "/" + String(p.osm_id);
      a.target = "_blank";
      a.rel = "noopener noreferrer";
      a.textContent = "In OpenStreetMap ansehen (" + String(p.osm_type) + " " + String(p.osm_id) + ")";
      box.appendChild(a);
    } else {
      box.appendChild(document.createTextNode("osm_id " + String(p.osm_id)));
    }
    zeigePopup(e.lngLat, box);
  });

  // Was liegt in der Kartenmitte? Die Kernfrage der Karte ohne Zeiger: Enter auf
  // der fokussierten Karte, und nach Suche und Standort, wenn die Karte von selbst
  // dorthin faehrt. Gefragt wird, was gezeichnet ist: abgeschaltete Kategorien
  // und Ebenen zaehlen nicht, wie auf dem Bild. Die Woerter folgen der Legende.
  const MITTE_EBENEN = ["zonen-fill", "fuss-fill", "fuss-line", "sonder-fill"];
  const zeigeMitte = fokus => {
    const p = map.project(map.getCenter());
    const treffer = map.queryRenderedFeatures([[p.x - 3, p.y - 3], [p.x + 3, p.y + 3]],
      { layers: MITTE_EBENEN.filter(id => map.getLayer(id)) });
    const zeilen = [];
    const zonen = treffer.filter(f => f.layer.id === "zonen-fill");
    if (zonen.length) {
      // In den groben Zoombaendern sind Baender verschmolzen und tragen keine Kategorie.
      const kat = [...new Set(zonen.map(f => TITEL.get(f.properties.category)).filter(Boolean))];
      zeilen.push("rot: in Sichtweite – Konsumverbot nach § 5 Abs. 2 KCanG");
      if (kat.length) zeilen.push("Einrichtungen in Sichtweite: " + kat.join(", "));
    }
    if (treffer.some(f => f.layer.id.startsWith("fuss-"))) {
      zeilen.push("blau: Fußgängerzone – Verbot 7–20 Uhr, " + (giltJetzt() ? "gilt jetzt" : "gilt jetzt nicht"));
    }
    const sonder = [...new Set(treffer.filter(f => f.layer.id === "sonder-fill").map(f => String(f.properties.name)))];
    if (sonder.length) zeilen.push("lila: örtliches Verbot – " + sonder.join(", "));
    if (!zeilen.length) zeilen.push("weiß: keine Zone in diesen Daten – andere Verbote gelten trotzdem");
    // Erst das z14-Band rechnet mit Gebaeudegrundrissen; darunter ist die Grenze grob.
    if (map.getZoom() < 14) zeilen.push("Bei dieser Zoomstufe vereinfacht – für die genaue Grenze näher heranzoomen");
    const box = document.createElement("div");
    const b = document.createElement("b");
    b.textContent = "Kartenmitte";
    box.appendChild(b);
    for (const z of zeilen) {
      box.appendChild(document.createElement("br"));
      box.appendChild(document.createTextNode(z));
    }
    zeigePopup(map.getCenter(), box, fokus);
    return "Kartenmitte: " + zeilen.join(". ") + ".";
  };
  // Nach einer Kamerafahrt erst fragen, wenn die Kacheln am Ziel gezeichnet sind.
  // Vor der Fahrt anmelden: ohne Bewegung kommt kein `moveend`, dann keine Antwort.
  const nachBewegung = f => map.once("moveend", () => {
    if (map.areTilesLoaded()) f(); else map.once("idle", f);
  });
  map.getCanvas().addEventListener("keydown", e => {
    if (e.key !== "Enter" || e.repeat) return;
    e.preventDefault();
    sagAn(zeigeMitte(true));
  });
})().catch(err => {
  // Alles andere, was den Start abbricht (ein Skript unter vendor/ nicht geladen,
  // ein Fehler vor der ersten Karte): ebenfalls sagen statt schweigen.
  console.error(err);
  startFehler("Die Karte konnte nicht gestartet werden. Bitte die Seite neu laden.");
});
