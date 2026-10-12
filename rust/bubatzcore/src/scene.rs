//! Hindernisszene EINES Verbotsobjekts: Kanten und Ecken einmal
//! aufbereitet, dann beliebig viele Beobachterpunkte auswerten.
//!
//! Kanten kommen ring-weise (Aussenring, dann Loecher, je Hindernis), Ringe
//! offen; die Kante `i` laeuft von Ecke `i` zu Ecke `(i+1) mod n`. Die Ecken
//! der Szene sind die Startpunkte aller Kanten -- dieselbe Menge und
//! Reihenfolge wie `obstacle_segments` in Python (Ring-Koordinaten mit
//! wiederholtem Schlusspunkt, Paare aufeinanderfolgender Punkte).

use crate::geom::Ring;

#[derive(Debug, Clone)]
pub struct Scene {
    pub(crate) p0x: Vec<f64>,
    pub(crate) p0y: Vec<f64>,
    pub(crate) p1x: Vec<f64>,
    pub(crate) p1y: Vec<f64>,
    pub(crate) seg_len: Vec<f64>,
    /// Index der Folgekante im selben Ring: `p1[k] == p0[next[k]]`
    pub(crate) next: Vec<usize>,
    pub reach: f64,
    /// Strahllaenge des Sweeps (Trefferlimit UND Ringkappung), Default `reach`.
    /// `zones` setzt `reach + Durchmesser der Quelle`: ein Beobachter auf der
    /// fernen Seite der Quelle muss ueber sie hinweg bis `reach` jenseits des
    /// naechsten Randes sehen koennen (v7-Befund am frueheren Anker: 2 %
    /// zu wenig Flaeche bei Kappung auf `reach`).
    pub max_len: f64,
}

impl Scene {
    /// Szene aus Hindernisringen (jeder Ring ein geschlossener Kantenzug,
    /// Schlusspunkt nicht wiederholt). Ringe mit weniger als zwei Punkten
    /// tragen keine Kante bei.
    pub fn from_rings<R: AsRef<[[f64; 2]]>>(rings: &[R], reach: f64) -> Self {
        let n: usize = rings.iter().map(|r| r.as_ref().len()).sum();
        let mut s = Scene {
            p0x: Vec::with_capacity(n),
            p0y: Vec::with_capacity(n),
            p1x: Vec::with_capacity(n),
            p1y: Vec::with_capacity(n),
            seg_len: Vec::with_capacity(n),
            next: Vec::with_capacity(n),
            reach,
            max_len: reach,
        };
        for ring in rings {
            let ring = ring.as_ref();
            if ring.len() < 2 {
                continue;
            }
            let base = s.p0x.len();
            for i in 0..ring.len() {
                s.next.push(base + (i + 1) % ring.len());
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                s.p0x.push(a[0]);
                s.p0y.push(a[1]);
                s.p1x.push(b[0]);
                s.p1y.push(b[1]);
                s.seg_len.push((b[0] - a[0]).hypot(b[1] - a[1]));
            }
        }
        s
    }

    pub fn from_ring_vecs(rings: &[Ring], reach: f64) -> Self {
        Self::from_rings(rings, reach)
    }

    pub fn n_segments(&self) -> usize {
        self.p0x.len()
    }

    /// Ecken = Startpunkte aller Kanten (wie `VisibilityScene._corners`).
    pub fn corners(&self) -> impl Iterator<Item = [f64; 2]> + '_ {
        self.p0x.iter().zip(&self.p0y).map(|(&x, &y)| [x, y])
    }

    /// Strahllaenge des Sweeps: Trefferlimit und Ringkappung sind dieselbe
    /// Zahl (ein Treffer jenseits davon ist wie kein Treffer). Default
    /// `reach`; siehe Feld `max_len`.
    pub fn max_len(&self) -> f64 {
        self.max_len
    }

    pub fn with_max_len(mut self, max_len: f64) -> Self {
        self.max_len = max_len.max(self.reach);
        self
    }
}
