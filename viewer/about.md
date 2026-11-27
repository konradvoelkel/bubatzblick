# Über BubatzBlick

Die Karte zeigt, wo in Deutschland der öffentliche Konsum von Cannabis nach
§ 5 Abs. 2 des Konsumcannabisgesetzes (KCanG) verboten ist, weil eine
Schule, ein Kinderspielplatz, eine Kinder- oder Jugendeinrichtung, eine
öffentlich zugängliche Sportstätte oder eine Anbauvereinigung **in
Sichtweite** liegt. Rot ist die Fläche, von der aus eine solche Einrichtung
zu sehen sein kann — höchstens 100 m weit, und nur dort, wo kein Gebäude
die Sicht verdeckt.

**Die roten Zonen sind nicht die einzigen Orte, an denen Konsum verboten
ist.** Unabhängig vom Ort gilt § 5 Abs. 1 KCanG: Der Konsum in
unmittelbarer Gegenwart von Minderjährigen ist verboten — auf einer
weißen Fläche genauso wie auf einer roten. Dazu kommen Fußgängerzonen
zwischen 7 und 20 Uhr und militärische Bereiche (siehe unten).

Die Karte ist eine Orientierungshilfe, keine Rechtsauskunft. Sie beruht
auf freiwillig gesammelten Daten (OpenStreetMap) und einem Rechenmodell;
beides kann im Einzelfall falsch sein. Verantwortlich für das eigene
Verhalten bleibt jede und jeder selbst.

BubatzBlick, die „Bubatzkarte 2.0“, ist ein privates, nicht-kommerzielles
Projekt und unabhängig von der [Bubatzkarte](https://bubatzkarte.de) von
2023; was die beiden unterscheidet, steht weiter unten.

## Was rot ist — und was nicht

**Rot:** Konsumverbot in Sichtweite (§ 5 Abs. 2 Satz 1 Nr. 1–4 und 6 KCanG)
von den folgenden Einrichtungen. Die Liste wird beim Bauen dieser Seite aus
genau der Tag-Datei erzeugt, mit der die Karte gerechnet wird
([TAGS-DATEI EINBLENDEN]); sie kann daher nicht von der Karte abweichen.

[KATEGORIEN AUS tags.json EINBLENDEN]

**Nicht auf der Karte**, obwohl ebenfalls verboten:

- Konsum **in unmittelbarer Gegenwart von Minderjährigen** (§ 5 Abs. 1) —
  das hängt von Menschen ab, nicht von Orten, und gilt überall.
- Konsum in **Fußgängerzonen zwischen 7 und 20 Uhr** (§ 5 Abs. 2 Nr. 5) —
  keine Sichtweite, sondern eine Fläche mit Uhrzeit. Dafür gibt es eine
  eigene, zuschaltbare Ebene in Blau; sie zeigt `highway=pedestrian` aus
  OpenStreetMap unverändert, Flächen als Fläche und Wege als Linie, ohne
  Puffer. Das erfasst mehr, als das Gesetz meint — auch Parkwege und
  Bahnhofsvorplätze tragen dieses Merkmal —, denn das Verkehrszeichen
  „Fußgängerzone" ist nur bei 3,8 % der Objekte eingetragen. Fußgängerzonen,
  die in OpenStreetMap aus mehreren Teilen zusammengesetzt sind
  (Multipolygon-Relationen), sind ebenfalls erfasst: nach Stückzahl rund 5 %,
  nach Fläche fast die Hälfte dessen, was die einfachen Flächen ausmachen —
  es sind die großen Plätze.
- **Örtliche Verbote außerhalb des KCanG.** Städte können auf eigener
  Rechtsgrundlage weitere Flächen belegen — in Bayern per Verordnung nach
  Art. 30 LStVG (Konsum und meist auch das Mitführen zum Konsum, befristet
  auf höchstens vier Jahre), anderswo per Allgemeinverfügung. Dafür gibt es
  eine eigene Ebene in **Lila**: eine von Hand gepflegte Liste der Rechtsakte
  mit Fläche und Laufzeit, derzeit die Cannabisverbotszone rund um den
  Nürnberger Hauptbahnhof und der Alte Botanische Garten in München. Die
  Grenzen sind aus den im Rechtsakt genannten Straßen und Plätzen
  näherungsweise gezeichnet; maßgeblich ist die amtliche Karte, die im Popup
  verlinkt ist. Gezeigt wird nur, was am heutigen Tag gilt. Verfügungen für
  ein Volksfest oder Stadtfest über wenige Tage nimmt die Karte nicht auf.
  Fehlt ein Rechtsakt, bitte mit Quelle melden (siehe „Fehler melden“).
- **Militärische Bereiche der Bundeswehr** (§ 5 Abs. 3).
- Alles, was in OpenStreetMap fehlt oder falsch eingetragen ist (siehe
  „Fehler melden“).

## Der Gesetzestext

> **§ 5 KCanG – Konsumverbot**
>
> (1) Der Konsum von Cannabis in unmittelbarer Gegenwart von Personen, die
> das 18. Lebensjahr noch nicht vollendet haben, ist verboten.
>
> (2) Der öffentliche Konsum von Cannabis ist verboten:
>
> 1. in Schulen und in deren Sichtweite,
> 2. auf Kinderspielplätzen und in deren Sichtweite,
> 3. in Kinder- und Jugendeinrichtungen und in deren Sichtweite,
> 4. in öffentlich zugänglichen Sportstätten und in deren Sichtweite,
> 5. in Fußgängerzonen zwischen 7 und 20 Uhr und
> 6. innerhalb des befriedeten Besitztums von Anbauvereinigungen und in
>    deren Sichtweite.
>
> Im Sinne von Satz 1 ist eine Sichtweite bei einem Abstand von mehr als
> 100 Metern von dem Eingangsbereich der in Satz 1 Nummer 1 bis 4 und 6
> genannten Einrichtungen nicht mehr gegeben.
>
> (3) In militärischen Bereichen der Bundeswehr ist der Konsum von Cannabis
> verboten.

Quelle: <cite>Konsumcannabisgesetz (KCanG)</cite>, § 5,
[gesetze-im-internet.de/kcang/__5.html](https://www.gesetze-im-internet.de/kcang/__5.html).

## Wie die Zonen berechnet werden

Das Gesetz spricht von **Sichtweite**, nicht von Abstand. Die 100 m sind
eine Obergrenze: Weiter als 100 m vom Eingangsbereich entfernt gilt
nichts mehr als Sichtweite — näher dran kommt es darauf an, ob man die
Einrichtung tatsächlich sehen kann. Die Bubatzkarte 2.0 rechnet genau das:

1. Aus OpenStreetMap werden alle Einrichtungen der genannten Arten und alle
   **Gebäudegrundrisse** Deutschlands geladen (rund 500 000 Einrichtungen,
   knapp 40 Millionen Gebäude).
2. Entlang der gesamten Umrisslinie jeder Einrichtung werden alle 0,5 m
   Beobachterpunkte gesetzt. Weil OpenStreetMap den „Eingangsbereich“
   meist nicht kennt, gilt die **ganze Kontur** als Eingangsbereich — das
   macht die Zone eher größer als kleiner.
3. Für jeden Punkt wird exakt berechnet, welche Fläche im Umkreis von 100 m
   von dort aus sichtbar ist, wenn Gebäude die Sicht verdecken. Bäume,
   Mauern, Zäune, Hecken und Geländeformen werden **nicht** als
   Sichthindernis gewertet — auch das vergrößert die Zone.
4. Die Sichtfelder aller Punkte werden vereinigt, um eine kleine
   Sicherheitsmarge (0,5 m) nach außen erweitert und mit 2 m Zuschlag für
   die Lagegenauigkeit der OSM-Daten versehen.

Das Ergebnis ist bewusst **konservativ**: Wo die Karte weiß ist, ist man
nach diesem Modell sicher außerhalb der Sichtweite. Wo sie rot ist, *kann*
ein Verbot gelten — die Karte kann kein Gebüsch und keinen Bauzaun kennen,
der die Sicht in Wirklichkeit versperrt.

Ab Zoomstufe 14 sind die Gebäude selbst aus dem Rot herausgeschnitten:
Rot endet an der Hauswand. In kleineren Maßstäben sind die Flächen für die
Darstellung vergröbert (immer nach außen, nie nach innen).

Das Verfahren folgt der Bachelorarbeit von **Oliver Kleisch**, <cite>2D-Sichtbarkeit
anhand von Gebäudekonturen aus Openstreetmaps</cite> (Heinrich-Heine-Universität
Düsseldorf, Juni 2025), [als PDF, 3 MB](kleisch-2025-2d-sichtbarkeit-osm.pdf).

## Für Informatikerinnen und Informatiker: die Algorithmen

Wer es genauer wissen will — die Berechnung ist freie Software (siehe
Lizenzen), hier die Bausteine:

- **Sichtbarkeitspolygon.** Für jeden Beobachterpunkt wird das
  [Sichtbarkeitspolygon](https://de.wikipedia.org/wiki/Sichtbarkeitspolygon)
  gegenüber den Gebäudekanten in Reichweite mit einem **Rotationssweep**
  berechnet (Winkelsortierung der Kanten-Endpunkte, Sweep über den
  Vollkreis mit einer nach Entfernung geordneten Menge aktiver Kanten,
  O(n log n) je Punkt). Klassische Referenzen: D. T. Lee, <cite lang="en">Visibility of a
  simple polygon</cite>, Computer Vision, Graphics, and Image Processing 22
  (1983); T. Asano, <cite lang="en">An efficient algorithm for finding the visibility
  polygon for a polygonal region with holes</cite>, IEICE Transactions E68
  (1985); P. J. Heffernan, J. S. B. Mitchell, <cite lang="en">An optimal algorithm for
  computing visibility in the plane</cite>, SIAM Journal on Computing 24 (1995).
  Die Bachelorarbeit verwendet die <i lang="en">triangular expansion</i> (F. Bungiu,
  M. Hemmer, J. Hershberger, K. Huang, A. Kröller, <cite lang="en">Efficient computation
  of visibility polygons</cite>, [arXiv:1403.3905](https://arxiv.org/abs/1403.3905),
  2014, in CGAL implementiert); der Sweep war für viele Punkte in
  derselben Szene die einfachere, exakt nachrechenbare Wahl.
- **Hindernissuche.** Die knapp 40 Millionen Gebäude liegen nach
  1-km-Kacheln sortiert auf der Platte; je Rechenzelle wird nur gelesen, was
  in ihrem Suchkasten liegt. Je Einrichtung werden alle Grundrisse im Umkreis
  von 102 m geholt und verschmolzen. So kommt die Rechnung für ganz
  Deutschland mit 8 GB Arbeitsspeicher aus.
- **Vereinigung und Marge.** Die Punktpolygone werden mit exakter
  Ganzzahlarithmetik vereinigt
  ([Boolesche Operationen auf Polygonen](https://en.wikipedia.org/wiki/Boolean_operations_on_polygons),
  Rust-Bibliothek [iOverlay](https://github.com/iShape-Rust/iOverlay)); die
  Sicherheitsmarge ist eine
  [Minkowski-Summe](https://de.wikipedia.org/wiki/Minkowski-Summe) mit
  einem 200-Eck als Kreis. Fließkomma-Rundung, die Löcher „zuschmiert“
  oder Flächen verliert, gibt es dadurch nicht.
- **Konservativität als Test.** Die Zusicherungen — Zone enthält die
  Quelle und den gebäudefreien Kreis, deckt eine vierfach dichtere
  Abtastung ab, wird durch zusätzliche Hindernisse nie größer, ist
  deterministisch bis aufs Byte und invariant unter Drehung/Verschiebung —
  sind automatisierte Tests an eingefrorenen realen Objekten. Ein
  Ankerobjekt in Düsseldorf muss bei jeder Veröffentlichung dieselbe
  Fläche ergeben.
- **Inkrementell.** Jede Zone trägt einen Inhaltsschlüssel
  ([BLAKE2b](https://de.wikipedia.org/wiki/BLAKE_(Hashfunktion)) über
  Kontur, Hindernisse in Reichweite und Parameterversion). Beim täglichen
  Lauf wird nur neu gerechnet, was sich in OpenStreetMap geändert hat —
  typisch 0,1 % der Objekte; ganze 2-km-Zellen werden per Fingerabdruck
  übernommen.
- **Kacheln.** Die Zonen werden je Zoomstufe *einseitig* vereinfacht
  ([Douglas-Peucker](https://de.wikipedia.org/wiki/Douglas-Peucker-Algorithmus)
  plus Puffer, sodass jede Vereinfachung eine Obermenge bleibt), mit
  [tippecanoe](https://github.com/felt/tippecanoe) zu
  [Vektorkacheln](https://github.com/mapbox/vector-tile-spec) gerendert
  und als eine einzige [PMTiles](https://github.com/protomaps/PMTiles)-Datei
  ausgeliefert, aus der der Browser per HTTP-Range-Anfragen nur die
  sichtbaren Kacheln liest. Anzeige mit
  [MapLibre GL JS](https://maplibre.org); die Hintergrundkarte ist ein
  [Protomaps](https://docs.protomaps.com)-Ausschnitt für Deutschland
  (5,7 GB, Zoom 0–15), ebenfalls eine PMTiles-Datei. Gerechnet wird in ETRS89 / UTM
  32N ([EPSG:25832](https://epsg.io/25832)), metrisch und bundesweit
  einheitlich.

## Zahlen

| Kennzahl | Wert |
|---|---|
| Einrichtungen (Verbotsobjekte) | 496 217 |
| Gebäudegrundrisse als Hindernisse | 39,6 Millionen |
| Beobachterpunkte insgesamt | rund 190 Millionen (Median 235 je Einrichtung, Kontur rund 96 000 km; gemessen im September 2026) |
| Rechenzellen | 52 955 à 2 km × 2 km |
| Fläche aller Zonen | rund 11 400 km², 3,2 % der Fläche Deutschlands (jede Stelle einmal gezählt; je Einrichtung einzeln gezählt, Überlappungen mehrfach, rund 20 000 km²) |
| Vollständige Neuberechnung | 4 h 45 min auf dem Server (4 vCPU, 8 GB; zuletzt im Oktober 2026), ~2 h auf 16 Threads |
| Tägliche Aktualisierung | 22 min auf dem Server, davon 12 min Rechnen |
| Arbeitsspeicher, Spitze | ~5 GB |
| Kachelarchiv | 372 MB, Zoom 8–14 |
| Stand | täglicher Lauf auf dem Server am 17. November 2026; der heutige Datenstand steht unter „Aktualität“ |

### Zonenfläche je Bundesland

Wie viel Fläche je Land in Sichtweite einer Einrichtung liegt, jede Stelle
einmal gezählt, auch wo sich mehrere Zonen überlappen. Fußgängerzonen und
örtliche Verbote sind nicht enthalten. Die Landesgrenzen stammen aus
OpenStreetMap, der Anteil bezieht sich auf die amtliche Fläche des Landes
([Statistisches Bundesamt, Tabelle 33111-0008](https://www-genesis.destatis.de/datenbank/online/statistic/33111/table/33111-0008)).
Die Stadtstaaten liegen vorn: viele Einrichtungen auf wenig Fläche. Stand:
Lauf vom 24. November 2026.

| Land | Zonen | Anteil an der Landesfläche |
|---|--:|--:|
| Berlin | 205 km² | 23,0 % |
| Hamburg | 125 km² | 16,5 % |
| Bremen | 55 km² | 13,2 % |
| Nordrhein-Westfalen | 2 046 km² | 6,0 % |
| Saarland | 127 km² | 4,9 % |
| Baden-Württemberg | 1 563 km² | 4,4 % |
| Hessen | 804 km² | 3,8 % |
| Rheinland-Pfalz | 645 km² | 3,3 % |
| Schleswig-Holstein | 501 km² | 3,2 % |
| Sachsen | 552 km² | 3,0 % |
| Bayern | 2 039 km² | 2,9 % |
| Niedersachsen | 1 377 km² | 2,9 % |
| Thüringen | 343 km² | 2,1 % |
| Sachsen-Anhalt | 321 km² | 1,6 % |
| Brandenburg | 431 km² | 1,5 % |
| Mecklenburg-Vorpommern | 285 km² | 1,2 % |
| **Deutschland** | **11 425 km²** | **3,2 %** |

Für einzelne Städte gibt es eigene Seiten mit Karte, Parks, Plätzen und den Regeln vor Ort:
[Kiffen in deutschen Städten](stadt/).

## Unterschied zur Bubatzkarte (2023)

Die [Bubatzkarte](https://bubatzkarte.de) (seit Juni 2023) war die
Inspiration für dieses Projekt; darüber hinaus gibt es keine Verbindung.
Sie zeichnet um jede Einrichtung einen Kreis mit einstellbarem Abstand
(Voreinstellung 100 m). Die Bubatzkarte 2.0 rechnet stattdessen die
tatsächliche Sichtweite durch die Bebauung. In Innenstädten sind die
Zonen dadurch deutlich kleiner — oft nur die Straßenzüge, von denen aus
man die Einrichtung sieht — und es gibt keinen Abstandsregler: Es gilt
allein die gesetzliche Sichtweite.

## Daten, Aktualität, Genauigkeit

- **Datenquelle:** [OpenStreetMap](https://www.openstreetmap.org), der
  Deutschland-Auszug von [Geofabrik](https://download.geofabrik.de). Die
  Karte wird **täglich** mit den Änderungen des Vortags neu berechnet;
  eine Korrektur in OpenStreetMap ist in der Regel nach ein bis zwei Tagen
  zu sehen. Stand der Daten: [STAND AUS stand.json EINBLENDEN].
- **Einrichtungen** werden über ihre OSM-Tags erkannt; welche, steht oben
  unter „Was rot ist“ — erzeugt aus [TAGS-DATEI EINBLENDEN] im Quellcode.
- **Grenznähe:** Einrichtungen im Ausland werden nicht berücksichtigt;
  Gebäude jenseits der Grenze verdecken in der Rechnung keine Sicht.
- **Fehlerquellen:** fehlende oder falsch getaggte Einrichtungen, fehlende
  Gebäude, Einrichtungen, die geschlossen sind, aber noch in OSM stehen,
  private Sportanlagen, die in OSM nicht als privat markiert sind.

## Fehler melden

- **Etwas fehlt oder ist falsch eingezeichnet?** Ein Klick auf eine rote
  Fläche nennt die Einrichtung, die sie verursacht, und führt mit „In
  OpenStreetMap ansehen“ direkt zu diesem Objekt. Das ist fast immer eine
  Frage der OpenStreetMap-Daten — und die kann jede und jeder selbst
  korrigieren: auf [openstreetmap.org](https://www.openstreetmap.org) mit
  einem Hinweis („Notiz hinzufügen“, kein Konto nötig) oder direkt mit den
  Apps [StreetComplete](https://streetcomplete.app) und
  [MapComplete](https://mapcomplete.org). BubatzBlick übernimmt die
  Änderung beim nächsten täglichen Lauf automatisch.
- **Die Karte selbst funktioniert nicht richtig?** E-Mail an die Adresse
  unter „Kontakt“.
- **Fehler in der Hintergrundkarte** (Straßen, Namen): ebenfalls
  OpenStreetMap.

## Datenschutz

Die Seite setzt keine Cookies, verwendet kein Tracking und keine
Werbung. Die Seite selbst kommt von einem eigenen Server bei Hetzner
(Deutschland), Kartendaten und Hintergrundkarte kommen aus einem Speicher
bei Cloudflare (R2, Rechenzentrum EU); beide verarbeiten dabei als
Auftragsverarbeiter Ihre IP-Adresse, wie jeder Hoster
([Hetzner](https://www.hetzner.com/de/legal/privacy-policy/),
[Cloudflare](https://www.cloudflare.com/privacypolicy/)). Für die Kartendaten bei Cloudflare wird kein Protokoll
ausgewertet.

### Was der eigene Server protokolliert

Der Server schreibt ein Zugriffsprotokoll. Eine Zeile darin enthält den
Zeitpunkt, Methode, Host und Protokoll der Anfrage, die angefragte Adresse samt
allem hinter einem „?“, TLS-Version und Verschlüsselung, Antwortcode, Größe
und Dauer der Antwort sowie die Kopfzeilen der Anfrage, darunter die
Browserkennung (User-Agent) und die Seite, von der Sie gekommen sind
(Referer). Cookies, Anmeldedaten und die Sprachwahl des Browsers werden vor
dem Schreiben entfernt. Vollständige IP-Adressen enthält das Protokoll
nicht: die Adresse wird schon beim Schreiben gekürzt (IPv4 auf die ersten drei
Blöcke, IPv6 auf die ersten 48 Bit), lässt sich also keinem Anschluss mehr
zuordnen.

Einmal am Tag wird dieses Protokoll zu Tageszahlen verdichtet und dabei
geleert — eine Protokollzeile liegt damit höchstens bis zum nächsten Lauf auf
dem Server, in der Regel weniger als 48 Stunden.

Die Tageszahlen enthalten **keine** Adressen und keine Browserkennungen, und
von den Seitenpfaden nur die der Städteseiten. Je Tag bleibt stehen: Zahl der
Anfragen, Aufrufe der Karte, der Über-Seite und je Stadt der Städteseiten, für
die Städte ohne eigene Seite die Zahl der Stimmen (je Stadt die
unterschiedlichen gekürzten Adressen, die über „Welche Stadt sollen wir noch
ausführlicher beschreiben?“ kamen), Zahl der unterschiedlichen gekürzten
Adressen als Näherung für „Besucher", Zahl der erkannten Bots, übertragene Bytes, die
Anzahl der Antworten je Statusklasse und die zehn häufigsten fremden Websites,
von denen Besucher kamen. Diese Tageszahlen werden nach **400 Tagen** gelöscht,
damit ein Jahresvergleich möglich bleibt und nicht mehr.

Daraus entsteht eine kleine Statistikseite für den Betrieb — Laufzeiten der
Berechnung, Größe des Kartenarchivs, die Tageszahlen von oben. Sie ist nicht
verlinkt und enthält nichts, was über diese Zahlen hinausgeht.

Es gibt keine Analysedienste, keine Zählpixel und keine Weitergabe dieser
Zahlen an Dritte. Beim Laden und Bewegen der Karte wird **nichts** von anderen
Servern nachgeladen — auch die Hintergrundkarte kommt aus einer eigenen Kopie
(Protomaps), nicht von openstreetmap.org. Die einzige Ausnahme ist die
Ortssuche: Wenn Sie einen Suchbegriff eingeben und absenden, geht er zusammen
mit Ihrer IP-Adresse an den Geocoder Nominatim der OpenStreetMap Foundation
(nominatim.openstreetmap.org, Vereinigtes Königreich;
[Datenschutzerklärung der OSMF](https://wiki.osmfoundation.org/wiki/Privacy_Policy)).
Mehr als den Suchbegriff bekommt der Dienst nicht, beim Tippen wird nichts
gesendet, und wer die Suche nicht benutzt, löst diesen Aufruf nie aus. Der
Link „In OpenStreetMap ansehen“ im Popup einer Zone ist ein gewöhnlicher Link:
Er lädt nichts nach, sondern führt erst, wenn Sie ihn anklicken, zu
openstreetmap.org. Der
aktuelle Kartenausschnitt steht in der Adresszeile (nach dem `#`) und wird nur
übertragen, wenn Sie die Adresse selbst weitergeben. „Mein Standort“ fragt die
Position nach Ihrer Freigabe über den Browser ab; die Seite sendet sie nirgendwohin.
Die Karte springt dorthin und lädt die Kacheln für diesen Ausschnitt wie für
jeden anderen.

## Lizenzen und Weiterverwendung

- Kartendaten © [OpenStreetMap-Mitwirkende](https://www.openstreetmap.org/copyright),
  [ODbL 1.0](https://opendatacommons.org/licenses/odbl/). Die berechneten
  Verbotszonen sind eine daraus abgeleitete Datenbank und stehen ebenfalls
  unter der ODbL; das vollständige Kachelarchiv gibt es auf Anfrage
  (Kontakt).
- Die Software (Berechnung und diese Seite) ist freie Software unter der
  [AGPL-3.0-or-later](https://www.gnu.org/licenses/agpl-3.0.de.html); der
  Quellcode liegt auf [GitHub](https://github.com/konradvoelkel/bubatzblick).
- Screenshots dürfen mit Quellenangabe „BubatzBlick / © OpenStreetMap-
  Mitwirkende“ frei verwendet werden.
- Hintergrundkarte: [Protomaps](https://protomaps.com) Basemap aus
  OpenStreetMap-Daten (ODbL), Stil `@protomaps/basemaps` (BSD-3-Clause),
  Schrift Noto Sans (SIL Open Font License); als eigene Kopie gehostet.

## Häufige Fragen

### Wo ist Kiffen in Deutschland verboten?

Nach § 5 KCanG überall in unmittelbarer Gegenwart von Minderjährigen und in
militärischen Bereichen der Bundeswehr. Öffentlich verboten ist der Konsum
außerdem in Schulen, auf Kinderspielplätzen, in Kinder- und
Jugendeinrichtungen, in öffentlich zugänglichen Sportstätten und in
Anbauvereinigungen, jeweils auch in deren Sichtweite (höchstens 100 m vom
Eingangsbereich), sowie in Fußgängerzonen zwischen 7 und 20 Uhr. Einzelne
Städte verbieten weitere Flächen, etwa rund um den Nürnberger Hauptbahnhof.
BubatzBlick zeigt die Sichtweiten-Zonen und diese örtlichen Verbote; es ist
eine Orientierungshilfe, keine Rechtsauskunft.

### Ist das eine amtliche Karte?

Nein. Es ist ein privates,
nicht-kommerzielles Projekt einer Einzelperson auf Basis offener Daten;
keine Behörde und keine Hochschule stehen dahinter.

### Warum ist meine Straße rot, obwohl ich von dort keine Schule sehe?

Weil die Karte nur Gebäude als Sichthindernis kennt. Bäume, Mauern oder
ein Bahndamm dazwischen fehlen im Modell — die Karte irrt hier zur
sicheren Seite. Oder es liegt eine Einrichtung in der Nähe, die man
nicht als solche erkennt (eine Kita im Wohnhaus, ein Bolzplatz).

### Warum ist ein Ort weiß, an dem eine Kita ist?

Dann fehlt die Kita in
OpenStreetMap oder ist dort anders getaggt. Eintragen — nach ein bis zwei
Tagen ist sie auf der Karte.

### Auf einer weißen Fläche darf ich also konsumieren?

Nicht, wenn
Minderjährige in unmittelbarer Nähe sind (§ 5 Abs. 1), nicht in einer
Fußgängerzone zwischen 7 und 20 Uhr, nicht in militärischen Bereichen,
nicht in einer lila Zone — und nicht, wenn ein Hausrecht oder eine
örtliche Regel es verbietet, die die Karte nicht kennt.

### Warum gibt es keinen Abstandsregler?

Weil das Gesetz keinen Abstand
vorgibt, sondern die Sichtweite mit 100 m Obergrenze — und genau die wird
berechnet.

### Kann ich die Zonen für eigene Zwecke nutzen?

Ja, unter der ODbL (siehe
Lizenzen); das Archiv gibt es auf Anfrage.

## Kontakt und Impressum

BubatzBlick (Bubatzkarte 2.0, bubatzblick.de) ist ein nicht-kommerzielles Angebot von

Konrad Völkel<br>
Universitätsstr. 1<br>
40225 Düsseldorf

E-Mail: kontakt [at] bubatzblick [punkt] de

Dieses Angebot ist privat und steht in keiner Verbindung zur
Heinrich-Heine-Universität Düsseldorf; die Anschrift dient allein der
Erreichbarkeit.
