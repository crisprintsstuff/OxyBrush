use super::{CLICK_PT, Editor, Vec2, Vec3};
use crate::editor::JobResult;
use oxy_mesh::retopo::Cage;

pub struct Anchor {
    pub points: Vec<Vec3>,
    pub attached: bool,
    generation: u64,
    revision: u64,
}

impl Editor {
    pub(super) fn retopo_cut_contour(&mut self, start: Vec2, end: Vec2, background: bool) {
        if start.distance(end) < CLICK_PT * self.pixels_per_point.max(0.5) {
            self.status = "Drag a line across the source to cut a contour".into();
            return;
        }
        let size = self.canvas_size();
        let midpoint = (start + end) * 0.5;
        let Some((anchor, _)) = self.retopo_surface_at(midpoint) else {
            self.status = "The middle of the contour stroke missed the surface".into();
            return;
        };
        let ray = |p| self.camera.ray(self.canvas_local(p), size);
        let (a, da) = ray(start);
        let (b, db) = ray(end);
        let (_, through) = ray(midpoint);
        // Intersect both end rays with the screen-parallel plane at the hit.
        // Works for perspective and orthographic views.
        let point = |origin: Vec3, dir: Vec3| origin + dir * ((anchor - origin).dot(through) / dir.dot(through));
        let normal = (point(b, db) - point(a, da)).cross(through);
        let Some(retopo) = self.retopo.as_ref() else { return };
        let Some(mesh) = self.snapping_surface() else { return };
        let (surface, generation, revision, count) =
            (retopo.surface, retopo.generation, self.history.revision(), retopo.contour_points);
        if background && mesh.triangle_count() > 250_000 {
            let mesh = mesh.clone();
            self.start_job("Cut retopology contour".into(), Vec::new(), move || {
                let ring = oxy_mesh::contour::ring(&mesh, anchor, normal, count);
                Ok(JobResult::RetopoContour { surface, generation, revision, ring })
            });
        } else {
            let ring = oxy_mesh::contour::ring(mesh, anchor, normal, count);
            self.status = self.retopo_finish_contour(surface, generation, revision, ring);
        }
    }

    pub(crate) fn retopo_finish_contour(
        &mut self,
        surface: u64,
        generation: u64,
        revision: u64,
        ring: Option<Vec<Vec3>>,
    ) -> String {
        let Some(retopo) = self.retopo.as_ref() else { return "Retopology was closed".into() };
        if !retopo.contour_mode
            || retopo.surface != surface
            || retopo.generation != generation
            || revision != self.history.revision()
        {
            return "Contour cancelled because the retopology changed".into();
        }
        let Some(ring) = ring else {
            return "The stroke did not cut a closed contour on the visible mesh".into();
        };
        if ring.len() != retopo.contour_points {
            return "Contour cancelled because the point count changed".into();
        }
        let joined = if let Some(anchor) = retopo.contour_anchor.as_ref().filter(|anchor| {
            anchor.points.len() == ring.len() && anchor.generation == generation && anchor.revision == revision
        }) {
            let prior = anchor.points.clone();
            let attached = anchor.attached;
            self.status = "Could not bridge these contours. Cut a new strip on a single limb.".into();
            if !self.retopo_edit("Retopo: bridge contours", |_, cage| bridge(cage, &prior, &ring, attached)) {
                return self.status.clone();
            }
            true
        } else {
            false
        };
        let revision = self.history.revision();
        if let Some(r) = self.retopo.as_mut() {
            r.contour_anchor = Some(Anchor { points: ring, attached: joined, generation: r.generation, revision });
        }
        if joined {
            "Contour bridged with quads. Draw another cut to continue the strip.".into()
        } else {
            "Contour placed. Draw the next cut on the same limb to bridge it.".into()
        }
    }
}

fn bridge(cage: &mut Cage, prior: &[Vec3], ring: &[Vec3], attached: bool) -> bool {
    let n = ring.len();
    let Some(base) = (if attached {
        prior.iter().map(|p| cage.positions.iter().position(|q| p == q).map(|i| i as u32)).collect()
    } else {
        Some(append(cage, prior.iter().copied()))
    }) else {
        return false;
    };
    let ordered: Vec<_> = align(prior, ring);
    if prior.iter().zip(&ordered).all(|(a, b)| a.abs_diff_eq(*b, 1e-5)) {
        return false;
    }
    let next = append(cage, ordered.iter().copied());
    let center = (prior.iter().sum::<Vec3>() + ordered.iter().sum::<Vec3>()) / (2 * n) as f32;
    for i in 0..n {
        let j = (i + 1) % n;
        let outward = (prior[i] + prior[j] + ordered[j] + ordered[i]) * 0.25 - center;
        if cage.add_face(&[base[i], base[j], next[j], next[i]], outward).is_none() {
            return false;
        }
    }
    true
}

fn append(cage: &mut Cage, points: impl Iterator<Item = Vec3>) -> Vec<u32> {
    let start = cage.positions.len() as u32;
    cage.positions.extend(points);
    (start..cage.positions.len() as u32).collect()
}

fn align(prior: &[Vec3], ring: &[Vec3]) -> Vec<Vec3> {
    let n = ring.len();
    let index = |i, start, reverse| if reverse { (start + n - i) % n } else { (start + i) % n };
    let cost = |start, reverse| {
        prior.iter().enumerate().map(|(i, &p)| p.distance_squared(ring[index(i, start, reverse)])).sum::<f32>()
    };
    let (start, reverse) = [false, true]
        .into_iter()
        .flat_map(|reverse| (0..n).map(move |start| (start, reverse)))
        .min_by(|&(a, ar), &(b, br)| cost(a, ar).total_cmp(&cost(b, br)))
        .unwrap_or((0, false));
    (0..n).map(|i| ring[index(i, start, reverse)]).collect()
}
