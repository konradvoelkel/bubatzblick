// Karte (Bubatzkarte 2.0 Viewer). Eigene Datei statt Inline-Skript, damit die
// CSP `script-src 'self'` ohne 'unsafe-inline' auskommt. Regel: nichts aus
// der URL, aus stand.json oder aus Kacheleigenschaften als HTML einsetzen,
// immer textContent / createElement.
(async () => {
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
  const schmal = matchMedia("(max-width: 640px)");
  const panel = document.getElementById("panel");
  const panelToggle = document.getElementById("panel-toggle");
  let panelBeruehrt = false;
  const panelOffen = offen => {
    panel.classList.toggle("zu", !offen);
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
    panelOffen(panel.classList.contains("zu"));
  });
  // KEIN `toggle`-Horcher auf dem Block: `open = …` oben loest ihn selbst aus,
  // und weil `toggle` verzoegert zugestellt wird, laesst er sich nicht sauber
  // von einer echten Eingabe unterscheiden. Er setzte `panelBeruehrt` und
  // schaltete damit die Breitenlogik ab -- auf dem Telefon blieb das Feld
  // offen und verdeckte die Karte.
  schmal.addEventListener("change", e => { if (!panelBeruehrt) breiteAnwenden(e.matches); });

  const cfg = Object.assign({ zones: "zonen.pmtiles", basemap: "basemap.pmtiles",
    assets: "vendor/basemaps", stand: "stand.json" }, window.BUBATZ_CONFIG || {});
  const abs = u => new URL(u, location.href).href;
  // Fussgaengerzonen liegen neben den Zonen (gleicher Bucket bzw. gleiche Seite).
  // Abgeleitet statt als eigener config.js-Schluessel: die Datei wird an zwei
  // Stellen erzeugt (deploy/bin/deploy_vps.sh und der post-receive-Hook im
  // Infra-Repo), ein neuer Schluessel muesste an beiden nachgezogen werden.
  const fussgaengerUrl = cfg.fussgaenger
    || cfg.zones.replace(/[^/]+$/, "fussgaengerzonen.pmtiles");
  const exists = async u => { try { const r = await fetch(u, { method: "HEAD" }); return r.ok; } catch (e) { return false; } };

  // Quelle der Verbotszonen, in dieser Reihenfolge:
  // (1) ?pmtiles=<URL einer .pmtiles-Datei> (Range-Anfragen),
  // (2) ?tiles=<URL-Vorlage mit {z}/{x}/{y}> (ein fremder Kachelserver),
  // (3) ?ds=<name> bzw. das erste Archiv von `bubatz serve` am eigenen Origin (TileJSON
  //     unter /<name>/tiles.json, Liste unter /datasets), sonst
  // (4) cfg.zones -- neben der Seite (statisch) oder die absolute R2-URL aus config.js.
  const params = new URLSearchParams(location.search);
  let archive = params.get("pmtiles");
  const protocol = new pmtiles.Protocol();
  maplibregl.addProtocol("pmtiles", protocol.tile);

  let datasets = [];
  if (!archive && !params.get("tiles")) {
    // Erst die Datei neben der Seite, dann die Archivliste von `bubatz serve`.
    // Andersherum fragte der statische Stand bei JEDEM Aufruf `/datasets` und
    // bekam 404 -- in jeder Browserkonsole sichtbar.
    if (await exists(cfg.zones)) archive = abs(cfg.zones);
    if (!archive) {
      try { const r = await fetch("datasets"); if (r.ok) datasets = await r.json(); } catch (e) { /* statisch gehostet */ }
    }
  }
  const dsName = params.get("ds") || (datasets[0] && datasets[0].name);
  let source, label;
  if (archive) {
    source = { type: "vector", url: `pmtiles://${archive}` }; label = archive;
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
    sel.onchange = () => { params.set("ds", sel.value); location.search = params.toString(); };
    document.getElementById("datasets").style.display = "block";
  }

  // Grundkarte: Protomaps (Vektor, PMTiles, Stil @protomaps/basemaps "light",
  // Glyphen und Sprites aus vendor/basemaps -- nichts von fremden Servern).
  // Fehlt das Archiv (lokale Vorschau ohne --basemap), OSM-Rasterkacheln als
  // Notnagel; der ist NICHT fuer den Betrieb gedacht (OSMF-Kachelrichtlinie).
  const attribution = '<a href="https://protomaps.com">Protomaps</a> &copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap-Mitwirkende</a>';
  const haveBasemap = await exists(cfg.basemap);
  const sources = { zonen: source };
  let layers;
  if (haveBasemap) {
    sources.basemap = { type: "vector", url: `pmtiles://${abs(cfg.basemap)}`, attribution };
    layers = basemaps.layers("basemap", basemaps.namedFlavor("light"), { lang: "de" });
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
      paint: { "fill-extrusion-color": "#c0392b", "fill-extrusion-opacity": 0.4,
               "fill-extrusion-height": 1, "fill-extrusion-base": 0 } },
    { id: "zonen-line", type: "line", source: "zonen", "source-layer": "zonen", minzoom: 12,
      filter: bandFilter,
      paint: { "line-color": "#7b241c", "line-width": 0.5, "line-opacity": 0.35 } },
  );

  // Fussgaengerzonen (§ 5 Abs. 2 Nr. 5): eigene, zuschaltbare Ebene, OHNE
  // Sichtweite und ohne Puffer -- Flaechen als Flaeche, Wege als Linie (bubatz
  // pedestrian). Blau statt Rot, weil es eine andere Rechtsfolge ist; bei 60 %
  // Deckkraft vom Zonenrot messbar unterscheidbar (Delta E
  // 16,1 normal, 10,6 bei Farbfehlsichtigkeit). Fehlt das Archiv, fehlt die
  // Ebene -- kein Fehler, nur kein Angebot.
  const haveFuss = await exists(fussgaengerUrl);

  const map = new maplibregl.Map({
    container: "map",
    center: [6.7735, 51.2277], zoom: 13, hash: true,
    // MapLibre beschriftet seine Bedienelemente sonst englisch -- auf einer
    // durchgehend deutschen Seite hoert ein Screenreader-Publikum sonst
    // "Reset bearing to north".
    locale: {
      "Map.Title": "Karte",
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
  map.addControl(new maplibregl.NavigationControl());

  // Legende: eine Zeile je Kategorie, jede blendet ihre Zonen aus und ein.
  // Ohne Auswahl waere die Karte leer -- die letzte Kategorie laesst sich nicht
  // abwaehlen, das erspart den leeren Zustand ohne Erklaerung.
  const legende = document.getElementById("legende");
  const kaesten = KATEGORIEN.map(([key, titel]) => {
    const zeile = document.createElement("label");
    zeile.className = "legende-zeile";
    const kasten = document.createElement("input");
    kasten.type = "checkbox"; kasten.checked = true; kasten.value = key;
    zeile.appendChild(kasten);
    zeile.appendChild(document.createTextNode(" " + titel));
    legende.appendChild(zeile);
    return kasten;
  });
  const anwenden = () => {
    const aktiv = kaesten.filter(k => k.checked).map(k => k.value);
    kaesten.forEach(k => { k.disabled = k.checked && aktiv.length === 1; });
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
        paint: { "fill-color": "#2a78d6", "fill-opacity": jetzt ? 0.6 : 0.25 } },
      { id: "fuss-line", type: "line", source: "fussgaenger", "source-layer": "fussgaengerzonen",
        filter: ["==", ["geometry-type"], "LineString"],
        paint: { "line-color": "#2a78d6", "line-opacity": jetzt ? 0.85 : 0.35, "line-width": 4 } },
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
      // Blau in der Farblegende nur zeigen, solange auch Blau auf der Karte ist.
      document.getElementById("farblegende").classList.toggle("mit-fuss", an);
    };
    const fussZeichnen = () => {
      fussBeschriften();
      try {
        fussAnwenden();
        fussWartet = false;
      } catch (err) {
        if (!fussWartet) { fussWartet = true; }
        setTimeout(fussZeichnen, 1000);
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
        box.appendChild(document.createTextNode(
          (p.osm_type === "relation" ? "relation " : "way ") + String(p.osm_id)));
        box.appendChild(document.createElement("br"));
        box.appendChild(document.createTextNode(
          "§ 5 Abs. 2 Nr. 5 KCanG, 7–20 Uhr — " + (giltJetzt() ? "gilt jetzt" : "gilt jetzt nicht")));
        new maplibregl.Popup().setLngLat(e.lngLat).setDOMContent(box).addTo(map);
      });
    }
  }
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
  const sucheMeldung = text => {
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
  sucheFeld.addEventListener("keydown", e => {
    if (e.key === "Enter") { e.preventDefault(); sucheForm.requestSubmit(); }
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
      for (const t of treffer) {
        // display_name ist OSM-Freitext: nur textContent, nie innerHTML.
        const b = document.createElement("button");
        b.type = "button";
        b.textContent = t.display_name;
        b.onclick = () => {
          sucheTreffer.replaceChildren();
          if (t.boundingbox) {
            const [s1, n1, w1, e1] = t.boundingbox.map(Number);
            map.fitBounds([[w1, s1], [e1, n1]], { maxZoom: 17, padding: 40 });
          } else {
            map.flyTo({ center: [Number(t.lon), Number(t.lat)], zoom: 16 });
          }
        };
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
    if (warn) small.style.color = "#b00";
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
  // 30 s, und ohne Schuldzuweisung: beim ersten Aufruf werden ein 365-MB-
  // Kachelarchiv und die Grundkarte per Range-Anfragen angelesen, ein zaeher
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
  geolocate.on("geolocate", () => setMeldung(""));

  // Eigener Standort als beschrifteter Knopf im Bedienfeld: das Kartensymbol
  // ist unbeschriftet und auf dem Telefon unten rechts -- in Tests die erste
  // gesuchte und nicht gefundene Funktion.
  document.getElementById("standort-knopf").addEventListener("click", () => {
    setMeldung("");
    geolocate.trigger();
  });

  // Ausschnitt teilen. `hash: true` schreibt ihn ohnehin in die Adresse; nur
  // wusste das niemand, und auf dem Tablet ist die Adresszeile muehsam
  // weil der Teilen-Weg sonst nicht gefunden wird.
  const teilenKnopf = document.getElementById("teilen-knopf");
  teilenKnopf.addEventListener("click", async () => {
    const url = location.href;
    try {
      await navigator.clipboard.writeText(url);
      teilenKnopf.textContent = "Adresse kopiert";
    } catch (e) {
      // Ohne Zwischenablage-Freigabe: Adresse sichtbar und markierbar zeigen.
      setMeldung(url);
    }
    setTimeout(() => { teilenKnopf.textContent = "Ausschnitt teilen"; }, 3000);
  });

  // Der Weg zur Ueber-Seite nimmt den Ausschnitt mit und zurueck: vorher stand
  // man nach der Rueckkehr wieder im Anfangsausschnitt.
  const ueberLink = document.getElementById("ueber-link");
  const ueberZiel = () => "about.html" + (location.hash ? "?karte=" + encodeURIComponent(location.hash.slice(1)) : "");
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
    new maplibregl.Popup().setLngLat(e.lngLat).setDOMContent(box).addTo(map);
  });
})();
