# Sicherheitshinweise melden

## Unterstützte Versionen

Es gibt genau einen unterstützten Stand: den, der auf
[bubatzblick.de](https://bubatzblick.de) läuft, und damit den aktuellen
`master`-Branch. Ältere Veröffentlichungen werden nicht zurückgepflegt.


Bitte **keine** öffentlichen Issues für Sicherheitslücken. Stattdessen eine
E-Mail an die Kontaktadresse der Über-Seite der Karte
(https://bubatzblick.de/about.html, Abschnitt „Kontakt und
Impressum“). Eine Antwort folgt in der Regel innerhalb weniger Tage.

Was dazuzählt: alles, was die ausgelieferte Seite oder die Kacheldaten
manipulieren könnte (XSS im Viewer, Schwächen in `deploy/`, im
Veröffentlichungspfad `scripts/publish_germany.sh` oder in der
Verarbeitung der OSM-Daten in `rust/`).

Was nicht dazuzählt: fehlende oder falsch getaggte Einrichtungen in
OpenStreetMap — das ist eine Datenfrage und wird direkt in OSM korrigiert
(siehe Über-Seite, „Fehler melden“).
