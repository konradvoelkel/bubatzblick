**Was ändert sich und warum?**

**Berührt die Änderung die Zonengeometrie?**
Wenn ja: `PARAM_VERSION` mitgezogen? (`rust/bubatz/src/config.rs` — ohne das
übernimmt der nächste Lauf Bänder aus einem anderen Verfahren.) Entsprechend
`SIMPLIFY_VERSION` für den Kachelinhalt, `CELL_FP_VERSION` für die Spalten
einer Zonen-Zelle.

**Geprüft**
- [ ] `make lint test` lokal grün (oder der `pre-push`-Hook ist gelaufen)
- [ ] `make check-publishable` grün
- [ ] bei Änderungen an `viewer/about.md`: `make about` gelaufen
- [ ] die Zone wird nicht kleiner (siehe `contract_tests.rs`)
