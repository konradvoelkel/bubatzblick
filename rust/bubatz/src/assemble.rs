//! Flaechen-Assemblierung -- Port des libosmium-`Assembler`s
//! (`area/detail/basic_assembler.hpp`, `segment_list.hpp`,
//! `node_ref_segment.hpp`, `proto_ring.hpp`), so weit die Pipeline ihn
//! braucht: geschlossene Ways und `type=multipolygon`/`boundary`-Relationen zu
//! Ringen, mit denselben Regeln fuer Reihenfolge, Startpunkt und Orientierung,
//! damit die Geometrie byte-gleich zu pyosmiums `create_multipolygon` wird:
//!
//! - Segmente zwischen aufeinanderfolgenden Knoten (gleiche Lage -> kein
//!   Segment), kleinere Lage (x, dann y; Festkomma 1e-7) zuerst.
//! - Sortierung: nach erster Lage; bei gleicher erster Lage steilere Steigung
//!   zuerst, bei gleicher Steigung das kuerzere Segment.
//! - Doppelte Segmente paarweise entfernen; Kreuzungen (exakte
//!   Ganzzahlarithmetik) -> ungueltig; offene Ringe -> ungueltig.
//! - Ringe von der kleinsten Lage aus laufen; Aussenring CCW, Loch CW
//!   (`fix_direction`); Loch/Aussen per Strahltest gegen fertige Ringe
//!   (`find_enclosing_ring`).
//! - Ausgabe: Aussenringe in Entstehungsreihenfolge, je gefolgt von ihren
//!   Loechern; Ring = Startknoten + Endknoten jedes Segments.
//!
//! Der "komplexe Fall" (Lagen, an denen sich mehr als zwei Segmente treffen,
//! d. h. sich beruehrende Ringe) folgt `create_rings_complex_case`: Teilringe
//! ab den Beruehrpunkten, `try_to_merge` an Lagen mit genau zwei offenen
//! Enden, sonst `join_connected_rings` (Kandidatensuche, kleinste Flaeche
//! fuer Aussenringe, groesste fuer Loecher), dann `find_inner_outer_complex`.
//! Mehr als 100 Beruehrpunkte -> ungueltig (wie libosmium).

pub type Loc = [i32; 2];

#[derive(Debug, Clone)]
struct Segment {
    first: Loc,
    second: Loc,
    reverse: bool,
    ring: Option<usize>,
    /// `is_direction_done`: Richtung endgueltig (fuer `find_enclosing_ring`)
    direction_done: bool,
    role_inner: bool,
    way: usize,
}

impl Segment {
    fn new(a: Loc, b: Loc, role_inner: bool, way: usize) -> Self {
        let (first, second) = if loc_lt(a, b) { (a, b) } else { (b, a) };
        Segment {
            first,
            second,
            reverse: false,
            ring: None,
            direction_done: false,
            role_inner,
            way,
        }
    }
    fn start(&self) -> Loc {
        if self.reverse {
            self.second
        } else {
            self.first
        }
    }
    fn stop(&self) -> Loc {
        if self.reverse {
            self.first
        } else {
            self.second
        }
    }
    fn det(&self) -> i64 {
        let a = self.start();
        let b = self.stop();
        cross(a, b)
    }
}

#[inline]
fn loc_lt(a: Loc, b: Loc) -> bool {
    if a[0] == b[0] {
        a[1] < b[1]
    } else {
        a[0] < b[0]
    }
}

#[inline]
fn cross(a: Loc, b: Loc) -> i64 {
    a[0] as i64 * b[1] as i64 - a[1] as i64 * b[0] as i64
}

/// `operator<(NodeRefSegment, NodeRefSegment)`.
fn seg_less(l: &Segment, r: &Segment) -> bool {
    if l.first == r.first {
        let p = [
            l.second[0] as i64 - l.first[0] as i64,
            l.second[1] as i64 - l.first[1] as i64,
        ];
        let q = [
            r.second[0] as i64 - r.first[0] as i64,
            r.second[1] as i64 - r.first[1] as i64,
        ];
        if p[0] == 0 && q[0] == 0 {
            return p[1] < q[1];
        }
        let a = p[1] * q[0];
        let b = q[1] * p[0];
        if a == b {
            return p[0] < q[0];
        }
        return a > b;
    }
    loc_lt(l.first, r.first)
}

fn seg_eq(l: &Segment, r: &Segment) -> bool {
    l.first == r.first && l.second == r.second
}

/// `calculate_intersection`: Some(..) wenn sich zwei Segmente schneiden oder
/// ueberlappen (nicht: Beruehrung an einem gemeinsamen Endpunkt).
fn intersects(s1: &Segment, s2: &Segment) -> bool {
    let p0 = s1.first;
    let p1 = s1.second;
    let q0 = s2.first;
    let q1 = s2.second;
    if (p0 == q0 && p1 == q1) || (p0 == q1 && p1 == q0) {
        return false;
    }
    let sub = |a: Loc, b: Loc| [a[0] as i64 - b[0] as i64, a[1] as i64 - b[1] as i64];
    let pd = sub(p1, p0);
    let qd = sub(q1, q0);
    let d = pd[0] * qd[1] - pd[1] * qd[0];
    if d != 0 {
        if p0 == q0 || p0 == q1 || p1 == q0 || p1 == q1 {
            return false;
        }
        let pq = sub(p0, q0);
        let na = qd[0] * pq[1] - qd[1] * pq[0];
        let nb = pd[0] * pq[1] - pd[1] * pq[0];
        return (d > 0 && na >= 0 && na <= d && nb >= 0 && nb <= d)
            || (d < 0 && na <= 0 && na >= d && nb <= 0 && nb >= d);
    }
    // kollinear: auf derselben Geraden?
    let qp = sub(q0, p0);
    if pd[0] * qp[1] - pd[1] * qp[0] == 0 {
        let mut sl = [(0u8, p0), (0u8, p1), (1u8, q0), (1u8, q1)];
        sl.sort_by(|a, b| {
            if loc_lt(a.1, b.1) {
                std::cmp::Ordering::Less
            } else if a.1 == b.1 {
                std::cmp::Ordering::Equal
            } else {
                std::cmp::Ordering::Greater
            }
        });
        if sl[1].1 == sl[2].1 {
            return false;
        }
        if sl[0].0 != sl[1].0 {
            return true;
        }
    }
    false
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub duplicate_nodes: u64,
    pub duplicate_segments: u64,
    pub intersections: u64,
    pub open_rings: u64,
    pub complex: u64,
    /// libosmium `area_really_complex_case`: offene Teilringe mussten verbunden werden
    pub really_complex: u64,
}

#[derive(Debug, Clone)]
pub struct Ring {
    pub locs: Vec<Loc>,
    pub inner: Vec<Vec<Loc>>,
}

struct ProtoRing {
    segments: Vec<usize>,
    outer: Option<usize>,
    inner: Vec<usize>,
    sum: i64,
    /// kleinstes Segment (nach `seg_less`), wie `ProtoRing::min_segment`
    min_segment: usize,
    /// false, wenn der Ring in einen anderen verschmolzen wurde (`m_rings.erase`)
    alive: bool,
}

/// Eintrag der `location_to_ring_map`: Start- oder Endlage eines offenen Rings.
#[derive(Clone, Copy, Debug)]
struct RingEnd {
    loc: Loc,
    ring: usize,
    start: bool,
}

/// `candidate` der Kandidatensuche in `join_connected_rings`.
#[derive(Clone, Debug)]
struct Candidate {
    sum: i64,
    rings: Vec<(RingEnd, bool)>,
    start: Loc,
    stop: Loc,
}

const MAX_SPLIT_LOCATIONS: usize = 100;
const MAX_DEPTH: u32 = 20;

pub struct Assembler {
    segs: Vec<Segment>,
    /// (Segmentindex, reverse) sortiert nach Lage.
    locations: Vec<(usize, bool)>,
    rings: Vec<ProtoRing>,
    split: Vec<Loc>,
    pub stats: Stats,
}

impl Assembler {
    pub fn new() -> Self {
        Assembler {
            segs: Vec::new(),
            locations: Vec::new(),
            rings: Vec::new(),
            split: Vec::new(),
            stats: Stats::default(),
        }
    }

    /// `extract_segments_from_way`: Knotenlagen eines Ways (None = ungueltige Lage).
    pub fn add_way(&mut self, locs: &[Option<Loc>], role_inner: bool, way: usize) -> u32 {
        let mut invalid = 0u32;
        let mut prev: Option<Loc> = None;
        for l in locs {
            let Some(l) = l else {
                invalid += 1;
                continue;
            };
            if let Some(p) = prev {
                if p != *l {
                    self.segs.push(Segment::new(p, *l, role_inner, way));
                } else {
                    self.stats.duplicate_nodes += 1;
                }
            }
            prev = Some(*l);
        }
        invalid
    }

    fn loc_of(&self, e: (usize, bool)) -> Loc {
        let s = &self.segs[e.0];
        if e.1 { s.second } else { s.first }
    }

    /// Ringe bauen; Ok(rings) oder Err(()) bei ungueltiger Flaeche.
    pub fn create_rings(mut self) -> Result<Vec<Ring>, Stats> {
        // sortieren
        self.segs.sort_by(|a, b| {
            if seg_less(a, b) {
                std::cmp::Ordering::Less
            } else if seg_eq(a, b) {
                std::cmp::Ordering::Equal
            } else {
                std::cmp::Ordering::Greater
            }
        });
        // doppelte paarweise entfernen
        let mut i = 0;
        while i + 1 < self.segs.len() {
            if seg_eq(&self.segs[i], &self.segs[i + 1]) {
                let same_way = self.segs[i].way == self.segs[i + 1].way;
                if same_way || !self.segs[i].role_inner || !self.segs[i + 1].role_inner {
                    self.stats.duplicate_segments += 1;
                }
                self.segs.drain(i..i + 2);
                i = i.saturating_sub(1);
            } else {
                i += 1;
            }
        }
        if self.segs.is_empty() {
            return Err(self.stats);
        }
        // Kreuzungen
        for i in 0..self.segs.len() - 1 {
            for j in i + 1..self.segs.len() {
                let (s1, s2) = (&self.segs[i], &self.segs[j]);
                if s1.first[0] > s2.second[0] {
                    break;
                }
                let (m1a, m1b) = (s1.first[1].min(s1.second[1]), s1.first[1].max(s1.second[1]));
                let (m2a, m2b) = (s2.first[1].min(s2.second[1]), s2.first[1].max(s2.second[1]));
                if !(m1a > m2b || m2a > m1b) && intersects(s1, s2) {
                    self.stats.intersections += 1;
                }
            }
        }
        if self.stats.intersections > 0 {
            return Err(self.stats);
        }
        // Lagenliste
        self.locations = (0..self.segs.len())
            .flat_map(|n| [(n, false), (n, true)])
            .collect();
        let segs = &self.segs;
        self.locations.sort_by(|a, b| {
            let la = if a.1 {
                segs[a.0].second
            } else {
                segs[a.0].first
            };
            let lb = if b.1 {
                segs[b.0].second
            } else {
                segs[b.0].first
            };
            if loc_lt(la, lb) {
                std::cmp::Ordering::Less
            } else if la == lb {
                std::cmp::Ordering::Equal
            } else {
                std::cmp::Ordering::Greater
            }
        });
        // offene Ringe / Verzweigungen
        let mut i = 0;
        let mut prev: Option<Loc> = None;
        while i < self.locations.len() {
            let loc = self.loc_of(self.locations[i]);
            if i + 1 >= self.locations.len() || self.loc_of(self.locations[i + 1]) != loc {
                self.stats.open_rings += 1;
                i += 1;
            } else {
                if Some(loc) == prev && self.split.last() != Some(&loc) {
                    self.split.push(loc);
                }
                i += 2;
            }
            prev = Some(loc);
        }
        if self.stats.open_rings > 0 {
            return Err(self.stats);
        }
        if !self.split.is_empty() {
            self.stats.complex += 1;
        }
        // Ringe laufen: einfacher Fall (keine Beruehrpunkte) wie
        // `create_rings_simple_case`, sonst `create_rings_complex_case`.
        if self.split.is_empty() {
            let mut remaining = self.segs.len();
            for li in 0..self.locations.len() {
                let (si, rev) = self.locations[li];
                if self.segs[si].ring.is_some() {
                    continue;
                }
                remaining -= self.add_new_ring(si, rev);
                if remaining == 0 {
                    break;
                }
            }
        } else if self.split.len() > MAX_SPLIT_LOCATIONS || !self.create_rings_complex_case() {
            // mehr als 100 Beruehrpunkte -> ungueltig (libosmium `max_split_locations`)
            return Err(self.stats);
        }
        // Ausgabe: Aussenringe in Entstehungsreihenfolge, je mit Loechern
        let mut out = Vec::new();
        for r in &self.rings {
            if !r.alive || r.outer.is_some() {
                continue;
            }
            out.push(Ring {
                locs: self.ring_locs(r),
                inner: r
                    .inner
                    .iter()
                    .map(|&j| self.ring_locs(&self.rings[j]))
                    .collect(),
            });
        }
        Ok(out)
    }

    fn ring_locs(&self, r: &ProtoRing) -> Vec<Loc> {
        let mut v = Vec::with_capacity(r.segments.len() + 1);
        v.push(self.segs[r.segments[0]].start());
        for &si in &r.segments {
            v.push(self.segs[si].stop());
        }
        v
    }

    fn next_segment(&self, loc: Loc) -> usize {
        // lower_bound auf die Lage, dann das erste noch freie Segment dort
        let mut it = self.lower_bound(loc);
        while it < self.locations.len() && self.loc_of(self.locations[it]) == loc {
            if self.segs[self.locations[it].0].ring.is_none() {
                return self.locations[it].0;
            }
            it += 1;
        }
        usize::MAX
    }

    fn lower_bound(&self, loc: Loc) -> usize {
        let mut lo = 0usize;
        let mut hi = self.locations.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if loc_lt(self.loc_of(self.locations[mid]), loc) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    // ---- ProtoRing-Operationen (proto_ring.hpp) ----

    fn new_ring(&mut self, si: usize) -> usize {
        let r = self.rings.len();
        self.rings.push(ProtoRing {
            segments: Vec::new(),
            outer: None,
            inner: Vec::new(),
            sum: 0,
            min_segment: si,
            alive: true,
        });
        self.ring_add_back(r, si);
        r
    }

    fn ring_add_back(&mut self, r: usize, si: usize) {
        if seg_less(&self.segs[si], &self.segs[self.rings[r].min_segment]) {
            self.rings[r].min_segment = si;
        }
        self.rings[r].segments.push(si);
        self.segs[si].ring = Some(r);
        self.rings[r].sum += self.segs[si].det();
    }

    fn ring_start(&self, r: usize) -> Loc {
        self.segs[self.rings[r].segments[0]].start()
    }

    fn ring_stop(&self, r: usize) -> Loc {
        self.segs[*self.rings[r].segments.last().unwrap()].stop()
    }

    fn ring_closed(&self, r: usize) -> bool {
        self.ring_start(r) == self.ring_stop(r)
    }

    fn ring_reverse(&mut self, r: usize) {
        for k in 0..self.rings[r].segments.len() {
            let s = self.rings[r].segments[k];
            self.segs[s].reverse = !self.segs[s].reverse;
        }
        self.rings[r].segments.reverse();
        self.rings[r].sum = -self.rings[r].sum;
    }

    /// `fix_direction`: is_cw (sum <= 0) == is_outer -> umdrehen
    fn ring_fix_direction(&mut self, r: usize) {
        let is_cw = self.rings[r].sum <= 0;
        let is_outer = self.rings[r].outer.is_none();
        if is_cw == is_outer {
            self.ring_reverse(r);
        }
    }

    fn ring_mark_direction(&mut self, r: usize, done: bool) {
        for k in 0..self.rings[r].segments.len() {
            let s = self.rings[r].segments[k];
            self.segs[s].direction_done = done;
        }
    }

    fn ring_join_forward(&mut self, r1: usize, r2: usize) {
        let segs = self.rings[r2].segments.clone();
        for s in segs {
            self.ring_add_back(r1, s);
        }
    }

    fn ring_join_backward(&mut self, r1: usize, r2: usize) {
        let segs = self.rings[r2].segments.clone();
        for s in segs.into_iter().rev() {
            self.segs[s].reverse = !self.segs[s].reverse;
            self.ring_add_back(r1, s);
        }
    }

    // ---- einfacher Fall ----

    fn add_new_ring(&mut self, si: usize, rev: bool) -> usize {
        if rev {
            self.segs[si].reverse = !self.segs[si].reverse;
        }
        let outer = if si != 0 {
            self.find_enclosing_ring(si)
        } else {
            None
        };
        self.segs[si].direction_done = true;
        let r = self.new_ring(si);
        if let Some(o) = outer {
            self.rings[o].inner.push(r);
            self.rings[r].outer = Some(o);
        }
        let first = self.segs[si].start();
        let mut last = self.segs[si].stop();
        let mut nodes = 1usize;
        while first != last {
            nodes += 1;
            let ni = self.next_segment(last);
            if ni == usize::MAX {
                break; // sollte nach der Offen-Pruefung nicht vorkommen
            }
            self.segs[ni].direction_done = true;
            if self.segs[ni].start() != last {
                self.segs[ni].reverse = !self.segs[ni].reverse;
            }
            self.ring_add_back(r, ni);
            last = self.segs[ni].stop();
        }
        self.ring_fix_direction(r);
        nodes
    }

    // ---- komplexer Fall (sich beruehrende Ringe) ----

    /// `add_new_ring_complex`: Teilring ab `si`, endet am Ringschluss oder an
    /// einem Beruehrpunkt; Richtung und Loch/Aussen werden spaeter bestimmt.
    fn add_new_ring_complex(&mut self, si: usize, rev: bool) -> usize {
        if rev {
            self.segs[si].reverse = !self.segs[si].reverse;
        }
        let r = self.new_ring(si);
        let first = self.segs[si].start();
        let mut last = self.segs[si].stop();
        let mut nodes = 1usize;
        while first != last && !self.split.contains(&last) {
            nodes += 1;
            let ni = self.next_segment(last);
            if ni == usize::MAX {
                break;
            }
            if self.segs[ni].start() != last {
                self.segs[ni].reverse = !self.segs[ni].reverse;
            }
            self.ring_add_back(r, ni);
            last = self.segs[ni].stop();
        }
        nodes
    }

    fn create_rings_complex_case(&mut self) -> bool {
        // Teilringe zuerst an den Beruehrpunkten beginnen ...
        let mut remaining = self.segs.len();
        let split = self.split.clone();
        'outer: for loc in split {
            let mut it = self.lower_bound(loc);
            while it < self.locations.len() && self.loc_of(self.locations[it]) == loc {
                let (si, rev) = self.locations[it];
                if self.segs[si].ring.is_none() {
                    remaining -= self.add_new_ring_complex(si, rev);
                    if remaining == 0 {
                        break 'outer;
                    }
                }
                it += 1;
            }
        }
        // ... dann der Rest
        if remaining > 0 {
            for li in 0..self.locations.len() {
                let (si, rev) = self.locations[li];
                if self.segs[si].ring.is_none() {
                    remaining -= self.add_new_ring_complex(si, rev);
                    if remaining == 0 {
                        break;
                    }
                }
            }
        }
        // offene Teilringe verbinden
        let mut open: Vec<usize> = (0..self.rings.len())
            .filter(|&r| self.rings[r].alive && !self.ring_closed(r))
            .collect();
        if !open.is_empty() {
            self.stats.really_complex += 1;
            while !open.is_empty() {
                while self.try_to_merge(&mut open) {}
                if !open.is_empty() && !self.join_connected_rings(&mut open) {
                    return false;
                }
            }
        }
        self.find_inner_outer_complex();
        true
    }

    /// `create_location_to_ring_map`: Start- und Endlage jedes offenen Rings,
    /// stabil nach Lage sortiert.
    fn location_map(&self, open: &[usize]) -> Vec<RingEnd> {
        let mut x = Vec::with_capacity(open.len() * 2);
        for &r in open {
            x.push(RingEnd {
                loc: self.ring_start(r),
                ring: r,
                start: true,
            });
            x.push(RingEnd {
                loc: self.ring_stop(r),
                ring: r,
                start: false,
            });
        }
        x.sort_by(|a, b| {
            if loc_lt(a.loc, b.loc) {
                std::cmp::Ordering::Less
            } else if a.loc == b.loc {
                std::cmp::Ordering::Equal
            } else {
                std::cmp::Ordering::Greater
            }
        });
        x
    }

    fn merge_two_rings(&mut self, open: &mut Vec<usize>, m1: RingEnd, m2: RingEnd) {
        let (r1, r2) = (m1.ring, m2.ring);
        if self.ring_stop(r1) == self.ring_start(r2) {
            self.ring_join_forward(r1, r2);
        } else if self.ring_stop(r1) == self.ring_stop(r2) {
            self.ring_join_backward(r1, r2);
        } else if self.ring_start(r1) == self.ring_start(r2) {
            self.ring_reverse(r1);
            self.ring_join_forward(r1, r2);
        } else if self.ring_start(r1) == self.ring_stop(r2) {
            self.ring_reverse(r1);
            self.ring_join_backward(r1, r2);
        } else {
            unreachable!("merge_two_rings: Ringe beruehren sich nicht");
        }
        open.retain(|&r| r != r2);
        self.rings[r2].alive = false;
        if self.ring_closed(r1) {
            open.retain(|&r| r != r1);
        }
    }

    /// `try_to_merge`: an einer Lage, an der genau zwei offene Ringenden
    /// liegen, diese beiden verbinden.
    fn try_to_merge(&mut self, open: &mut Vec<usize>) -> bool {
        if open.is_empty() {
            return false;
        }
        let x = self.location_map(open);
        let mut i = 0usize;
        while i < x.len() {
            // adjacent_find
            while i + 1 < x.len() && x[i].loc != x[i + 1].loc {
                i += 1;
            }
            if i + 1 >= x.len() {
                return false;
            }
            let after = i + 2;
            if after >= x.len() || x[after].loc != x[i].loc {
                self.merge_two_rings(open, x[i], x[i + 1]);
                return true;
            }
            while i < x.len() && x[i].loc == x[after].loc {
                i += 1;
            }
        }
        false
    }

    fn find_candidates(
        &self,
        candidates: &mut Vec<Candidate>,
        loc_done: &mut Vec<Loc>,
        x: &[RingEnd],
        cand: &Candidate,
        depth: u32,
    ) -> Result<(), ()> {
        if depth > MAX_DEPTH {
            return Err(());
        }
        let leading = cand.rings.last().unwrap().0.ring;
        for m in x.iter().filter(|m| m.loc == cand.stop) {
            if m.ring == leading {
                continue;
            }
            let mut c = cand.clone();
            if m.start {
                c.rings.push((*m, false));
                c.stop = self.ring_stop(m.ring);
                c.sum += self.rings[m.ring].sum;
            } else {
                c.rings.push((*m, true));
                c.stop = self.ring_start(m.ring);
                c.sum -= self.rings[m.ring].sum;
            }
            if c.start == c.stop {
                if candidates.is_empty() {
                    candidates.push(c);
                } else if candidates.len() == 1 {
                    if c.sum.abs() < candidates[0].sum.abs() {
                        candidates.insert(0, c);
                    } else {
                        candidates.push(c);
                    }
                } else if c.sum.abs() < candidates[0].sum.abs() {
                    candidates[0] = c;
                } else if c.sum.abs() > candidates.last().unwrap().sum.abs() {
                    *candidates.last_mut().unwrap() = c;
                }
            } else if !loc_done.contains(&c.stop) {
                loc_done.push(c.stop);
                self.find_candidates(candidates, loc_done, x, &c, depth + 1)?;
                loc_done.pop();
            }
        }
        Ok(())
    }

    /// `join_connected_rings`: den offenen Ring mit dem kleinsten Segment auf
    /// allen Wegen schliessen, den Kandidaten mit kleinster (Aussenring) bzw.
    /// groesster (Loch) Flaeche nehmen.
    fn join_connected_rings(&mut self, open: &mut Vec<usize>) -> bool {
        let x = self.location_map(open);
        let mut ring_min = x[0];
        for m in &x[1..] {
            if seg_less(
                &self.segs[self.rings[m.ring].min_segment],
                &self.segs[self.rings[ring_min.ring].min_segment],
            ) {
                ring_min = *m;
            }
        }
        self.find_inner_outer_complex();
        let ring_min_is_outer = self
            .find_enclosing_ring(self.rings[ring_min.ring].min_segment)
            .is_none();
        for r in 0..self.rings.len() {
            if self.rings[r].alive {
                self.rings[r].inner.clear();
                self.rings[r].outer = None;
                self.ring_mark_direction(r, false);
            }
        }
        let cand = Candidate {
            sum: self.rings[ring_min.ring].sum,
            rings: vec![(ring_min, false)],
            start: self.ring_start(ring_min.ring),
            stop: self.ring_stop(ring_min.ring),
        };
        let mut loc_done = vec![cand.stop];
        let mut candidates = Vec::new();
        if self
            .find_candidates(&mut candidates, &mut loc_done, &x, &cand, 0)
            .is_err()
        {
            return false;
        }
        if candidates.is_empty() {
            self.stats.open_rings += 1;
            return false;
        }
        let chosen = if ring_min_is_outer {
            candidates.first().unwrap().clone()
        } else {
            candidates.last().unwrap().clone()
        };
        let first = chosen.rings[0].0;
        for (m, _) in &chosen.rings[1..] {
            self.merge_two_rings(open, first, *m);
        }
        true
    }

    /// `find_inner_outer_complex`: geschlossene Ringe nach kleinstem Segment,
    /// der erste ist aussen, die weiteren per Strahltest.
    fn find_inner_outer_complex(&mut self) {
        let mut rings: Vec<usize> = (0..self.rings.len())
            .filter(|&r| self.rings[r].alive && self.ring_closed(r))
            .collect();
        if rings.is_empty() {
            return;
        }
        rings.sort_by(|&a, &b| {
            let (sa, sb) = (
                &self.segs[self.rings[a].min_segment],
                &self.segs[self.rings[b].min_segment],
            );
            if seg_less(sa, sb) {
                std::cmp::Ordering::Less
            } else if seg_less(sb, sa) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        self.ring_fix_direction(rings[0]);
        self.ring_mark_direction(rings[0], true);
        for &r in &rings[1..] {
            if let Some(o) = self.find_enclosing_ring(self.rings[r].min_segment) {
                self.rings[o].inner.push(r);
                self.rings[r].outer = Some(o);
            }
            self.ring_fix_direction(r);
            self.ring_mark_direction(r, true);
        }
    }

    /// `find_enclosing_ring`: Strahl nach links von der ersten Lage des
    /// Segments; Nesting ueber fertige Segmente (Ringzugehoerigkeit gesetzt).
    fn find_enclosing_ring(&self, si: usize) -> Option<usize> {
        let location = self.segs[si].first;
        let end_location = self.segs[si].second;
        let mut idx = si;
        while self.segs[idx].first == location {
            if idx + 1 >= self.segs.len() {
                break;
            }
            idx += 1;
        }
        let mut nesting: i64 = 0;
        let mut outer_rings: Vec<(f64, usize)> = Vec::new();
        let mut k = idx as isize;
        while k >= 0 {
            let s = &self.segs[k as usize];
            if !s.direction_done {
                k -= 1;
                continue;
            }
            let a = s.first;
            let b = s.second;
            if a == location {
                let z = (b[0] as i64 - a[0] as i64) * (end_location[1] as i64 - a[1] as i64)
                    - (b[1] as i64 - a[1] as i64) * (end_location[0] as i64 - a[0] as i64);
                if z > 0 {
                    nesting += if s.reverse { -1 } else { 1 };
                    let r = s.ring.unwrap();
                    if self.rings[r].outer.is_none() {
                        outer_rings.push((a[1] as f64, r));
                    }
                }
            } else if a[0] <= location[0] && location[0] < b[0] {
                let z = (b[0] as i64 - a[0] as i64) * (location[1] as i64 - a[1] as i64)
                    - (b[1] as i64 - a[1] as i64) * (location[0] as i64 - a[0] as i64);
                if z >= 0 {
                    nesting += if s.reverse { -1 } else { 1 };
                    let r = s.ring.unwrap();
                    if self.rings[r].outer.is_none() {
                        let y = a[1] as f64
                            + ((b[1] as i64 - a[1] as i64) * (location[0] as i64 - a[0] as i64))
                                as f64
                                / (b[0] as i64 - a[0] as i64) as f64;
                        outer_rings.push((y, r));
                    }
                }
            }
            k -= 1;
        }
        if nesting % 2 == 0 {
            return None;
        }
        // stable_sort(rbegin, rend) = absteigend nach y (stabil, von hinten)
        outer_rings.reverse();
        outer_rings.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap_or(std::cmp::Ordering::Equal));
        // remove_duplicates: gleiche Ringe paarweise entfernen
        let mut i = 0;
        while i + 1 < outer_rings.len() {
            if outer_rings[i].1 == outer_rings[i + 1].1 {
                outer_rings.drain(i..i + 2);
            } else {
                i += 1;
            }
        }
        outer_rings.first().map(|x| x.1)
    }
}

impl Default for Assembler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(x: i32, y: i32) -> Option<Loc> {
        Some([x, y])
    }

    #[test]
    fn square_way_starts_at_min_location_and_is_ccw() {
        // Way im Uhrzeigersinn ab (10,10)
        let mut a = Assembler::new();
        a.add_way(
            &[l(10, 10), l(10, 0), l(0, 0), l(0, 10), l(10, 10)],
            false,
            0,
        );
        let rings = a.create_rings().unwrap();
        assert_eq!(rings.len(), 1);
        assert_eq!(
            rings[0].locs,
            vec![[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]
        );
        assert!(rings[0].inner.is_empty());
    }

    #[test]
    fn duplicate_node_is_dropped_and_self_intersection_rejected() {
        let mut a = Assembler::new();
        a.add_way(
            &[l(0, 0), l(5, 0), l(5, 0), l(5, 5), l(0, 5), l(0, 0)],
            false,
            0,
        );
        let rings = a.create_rings().unwrap();
        assert_eq!(rings[0].locs.len(), 5);
        let mut b = Assembler::new();
        b.add_way(&[l(0, 0), l(10, 10), l(10, 0), l(0, 10), l(0, 0)], false, 0); // Bow-Tie
        let err = b.create_rings().unwrap_err();
        assert!(err.intersections > 0);
    }

    #[test]
    fn relation_with_hole_puts_inner_ring_clockwise_after_outer() {
        let mut a = Assembler::new();
        a.add_way(
            &[l(0, 0), l(100, 0), l(100, 100), l(0, 100), l(0, 0)],
            false,
            0,
        );
        a.add_way(
            &[l(20, 20), l(20, 40), l(40, 40), l(40, 20), l(20, 20)],
            true,
            1,
        );
        let rings = a.create_rings().unwrap();
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].locs[0], [0, 0]);
        assert_eq!(rings[0].locs[1], [100, 0], "Aussenring CCW");
        assert_eq!(rings[0].inner.len(), 1);
        let h = &rings[0].inner[0];
        assert_eq!(h[0], [20, 20]);
        assert_eq!(h[1], [20, 40], "Loch CW (von (20,20) nach oben)");
    }

    #[test]
    fn touching_rings_follow_osmium_complex_case() {
        // Relation 21068779 (Kindergarten Bonn): zwei geschlossene Wege mit
        // gemeinsamen Lagen, aber verschiedenen Knoten. pyosmium beginnt den
        // Aussenring am ersten Beruehrpunkt (70885802, 507324548), nicht an
        // der kleinsten Lage; die Dreiecks-Schleife wird zum Loch.
        let w1 = [
            (70891312, 507325184),
            (70888763, 507322857),
            (70888196, 507323199),
            (70888449, 507323374),
            (70886867, 507324290),
            (70886593, 507324100),
            (70885802, 507324548),
            (70886451, 507325005),
            (70888042, 507326125),
            (70886390, 507327065),
            (70886780, 507327246),
            (70887981, 507326757),
            (70889165, 507326198),
            (70891312, 507325184),
        ];
        let w2 = [
            (70888042, 507326125),
            (70885802, 507324548),
            (70883653, 507323036),
            (70882002, 507323975),
            (70883249, 507324853),
            (70886390, 507327065),
            (70888042, 507326125),
        ];
        let mut a = Assembler::new();
        a.add_way(&w1.map(|(x, y)| l(x, y)), false, 0);
        a.add_way(&w2.map(|(x, y)| l(x, y)), false, 1);
        let rings = a.create_rings().unwrap();
        assert_eq!(rings.len(), 1);
        let expected_outer: Vec<Loc> = [
            (70885802, 507324548),
            (70886593, 507324100),
            (70886867, 507324290),
            (70888449, 507323374),
            (70888196, 507323199),
            (70888763, 507322857),
            (70891312, 507325184),
            (70889165, 507326198),
            (70887981, 507326757),
            (70886780, 507327246),
            (70886390, 507327065),
            (70883249, 507324853),
            (70882002, 507323975),
            (70883653, 507323036),
            (70885802, 507324548),
        ]
        .iter()
        .map(|&(x, y)| [x, y])
        .collect();
        assert_eq!(rings[0].locs, expected_outer);
        assert_eq!(rings[0].inner.len(), 1);
        assert_eq!(
            rings[0].inner[0],
            vec![
                [70885802, 507324548],
                [70886451, 507325005],
                [70888042, 507326125],
                [70885802, 507324548]
            ],
            "Loch wie pyosmium: A -> x -> B -> A"
        );
    }

    #[test]
    fn open_ring_is_invalid_and_two_outers_are_two_rings() {
        let mut a = Assembler::new();
        a.add_way(&[l(0, 0), l(10, 0), l(10, 10)], false, 0);
        assert!(a.create_rings().is_err());
        let mut b = Assembler::new();
        b.add_way(&[l(0, 0), l(10, 0), l(10, 10), l(0, 10), l(0, 0)], false, 0);
        b.add_way(
            &[l(50, 0), l(60, 0), l(60, 10), l(50, 10), l(50, 0)],
            false,
            1,
        );
        let rings = b.create_rings().unwrap();
        assert_eq!(rings.len(), 2);
        assert_eq!(rings[0].locs[0], [0, 0]);
        assert_eq!(rings[1].locs[0], [50, 0]);
    }
}
