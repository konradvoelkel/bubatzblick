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

Quelle: [gesetze-im-internet.de/kcang/__5.html](https://www.gesetze-im-internet.de/kcang/__5.html).

## Wie die Zonen berechnet werden

Das Gesetz spricht von **Sichtweite**, nicht von Abstand. Die 100 m sind
eine Obergrenze: Weiter als 100 m vom Eingangsbereich entfernt gilt
nichts mehr als Sichtweite — näher dran kommt es darauf an, ob man die
Einrichtung tatsächlich sehen kann. Die Bubatzkarte 2.0 rechnet genau das:

1. Aus OpenStreetMap werden alle Einrichtungen der genannten Arten und alle
   **Gebäudegrundrisse** Deutschlands geladen (rund 500 000 Einrichtungen,
   39 Millionen Gebäude).
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

Das Verfahren folgt der Bachelorarbeit von **Oliver Kleisch**, *2D-Sichtbarkeit
anhand von Gebäudekonturen aus Openstreetmaps* (Heinrich-Heine-Universität
Düsseldorf, Juni 2025), [als PDF, 3 MB](kleisch-2025-2d-sichtbarkeit-osm.pdf).

## Für Informatikerinnen und Informatiker: die Algorithmen

Wer es genauer wissen will — die Berechnung ist freie Software (siehe
Lizenzen), hier die Bausteine:

- **Sichtbarkeitspolygon.** Für jeden Beobachterpunkt wird das
  [Sichtbarkeitspolygon](https://de.wikipedia.org/wiki/Sichtbarkeitspolygon)
  gegenüber den Gebäudekanten in Reichweite mit einem **Rotationssweep**
  berechnet (Winkelsortierung der Kanten-Endpunkte, Sweep über den
  Vollkreis mit einer nach Entfernung geordneten Menge aktiver Kanten,
  O(n log n) je Punkt). Klassische Referenzen: D. T. Lee, *Visibility of a
  simple polygon*, Computer Vision, Graphics, and Image Processing 22
  (1983); T. Asano, *An efficient algorithm for finding the visibility
  polygon for a polygonal region with holes*, IEICE Transactions E68
  (1985); P. J. Heffernan, J. S. B. Mitchell, *An optimal algorithm for
  computing visibility in the plane*, SIAM Journal on Computing 24 (1995).
  Die Bachelorarbeit verwendet die *triangular expansion* (F. Bungiu,
  M. Hemmer, J. Hershberger, K. Huang, A. Kröller, *Efficient computation
  of visibility polygons*, [arXiv:1403.3905](https://arxiv.org/abs/1403.3905),
  2014, in CGAL implementiert); der Sweep war für viele Punkte in
  derselben Szene die einfachere, exakt nachrechenbare Wahl.
- **Hindernissuche.** Die 39 Millionen Gebäude liegen in einem
  [R-Baum](https://de.wikipedia.org/wiki/R-Baum) (A. Guttman, *R-trees: a
  dynamic index structure for spatial searching*, SIGMOD 1984); je
  Einrichtung werden alle Grundrisse im Umkreis von 102 m geholt und
  verschmolzen.
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

| | |
|---|---|
| Einrichtungen (Verbotsobjekte) | 492 934 |
| Gebäudegrundrisse als Hindernisse | 39,3 Millionen |
| Beobachterpunkte insgesamt | 192 Millionen (Median 235 je Einrichtung, Kontur 95 800 km) |
| Rechenzellen | 52 804 à 2 km × 2 km |
| Fläche aller Zonen | ~20 300 km² (Summe je Einrichtung) |
| Vollständige Neuberechnung | 4 h 30 min auf dem Server (4 vCPU, 8 GB), ~2 h auf 16 Threads |
| Tägliche Aktualisierung | ~20 min auf dem Server |
| Arbeitsspeicher, Spitze | ~5 GB |
| Kachelarchiv | 354 MB, Zoom 8–14 |
| Gemessen | vollständiger Lauf auf dem Server; der Datenstand steht unter „Aktualität“ |

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
  [MapComplete](https://mapcomplete.org). Die Bubatzkarte übernimmt die
  Änderung automatisch.
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

Der Server schreibt ein Zugriffsprotokoll. Es enthält keine Cookies und keine
vollständigen IP-Adressen: die Adresse wird schon beim Schreiben gekürzt (IPv4
auf die ersten drei Blöcke, IPv6 auf die ersten 48 Bit), lässt sich also keinem
Anschluss mehr zuordnen.

Einmal am Tag wird dieses Protokoll zu Tageszahlen verdichtet und dabei
geleert — eine Protokollzeile liegt damit höchstens bis zum nächsten Lauf auf
dem Server, in der Regel weniger als 48 Stunden.

Die Tageszahlen enthalten **keine** Adressen, keine einzelnen Seitenpfade und
keine Browserkennungen. Je Tag bleibt stehen: Zahl der Anfragen, Aufrufe der
Karte und der Über-Seite, Zahl der unterschiedlichen gekürzten Adressen als
Näherung für „Besucher", Zahl der erkannten Bots, übertragene Bytes, die
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
übertragen, wenn Sie die Adresse selbst weitergeben.

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

**Ist das eine amtliche Karte?** Nein. Es ist ein privates,
nicht-kommerzielles Projekt einer Einzelperson auf Basis offener Daten;
keine Behörde und keine Hochschule stehen dahinter.

**Warum ist meine Straße rot, obwohl ich von dort keine Schule sehe?**
Weil die Karte nur Gebäude als Sichthindernis kennt. Bäume, Mauern oder
ein Bahndamm dazwischen fehlen im Modell — die Karte irrt hier zur
sicheren Seite. Oder es liegt eine Einrichtung in der Nähe, die man
nicht als solche erkennt (eine Kita im Wohnhaus, ein Bolzplatz).

**Warum ist ein Ort weiß, an dem eine Kita ist?** Dann fehlt die Kita in
OpenStreetMap oder ist dort anders getaggt. Eintragen — nach ein bis zwei
Tagen ist sie auf der Karte.

**Auf einer weißen Fläche darf ich also konsumieren?** Nicht, wenn
Minderjährige in unmittelbarer Nähe sind (§ 5 Abs. 1), nicht in einer
Fußgängerzone zwischen 7 und 20 Uhr, nicht in militärischen Bereichen,
nicht in einer lila Zone — und nicht, wenn ein Hausrecht oder eine
örtliche Regel es verbietet, die die Karte nicht kennt.

**Warum gibt es keinen Abstandsregler?** Weil das Gesetz keinen Abstand
vorgibt, sondern die Sichtweite mit 100 m Obergrenze — und genau die wird
berechnet.

**Kann ich die Zonen für eigene Zwecke nutzen?** Ja, unter der ODbL (siehe
Lizenzen); das Archiv gibt es auf Anfrage.

## Kontakt und Impressum

BubatzBlick (Bubatzkarte 2.0, bubatzblick.de) ist ein nicht-kommerzielles Angebot von

Konrad Völkel
Universitätsstr. 1
40225 Düsseldorf

E-Mail: kontakt [at] bubatzblick [punkt] de

Dieses Angebot ist privat und steht in keiner Verbindung zur
Heinrich-Heine-Universität Düsseldorf; die Anschrift dient allein der
Erreichbarkeit.
