//! Fixed integer-grid constraints for cut UV charts (RFC 0044).

use glam::{DVec2, IVec2, Vec2};

use crate::uv_cut::UvCutTopology;

/// Map the first side of a cut to its second side, rotating counterclockwise
/// around the UV origin before applying the translation.
#[derive(Clone, Copy)]
pub struct UvSeamTransition {
    pub quarter_turns: u8,
    pub translation: IVec2,
}

#[derive(Clone, Copy, PartialEq)]
struct Transform {
    turns: u8,
    shift: DVec2,
}

impl Transform {
    const IDENTITY: Self = Self { turns: 0, shift: DVec2::ZERO };

    fn rotate(uv: DVec2, turns: u8) -> DVec2 {
        match turns % 4 {
            0 => uv,
            1 => DVec2::new(-uv.y, uv.x),
            2 => -uv,
            _ => DVec2::new(uv.y, -uv.x),
        }
    }

    fn apply(self, uv: DVec2) -> DVec2 {
        Self::rotate(uv, self.turns) + self.shift
    }

    fn after(self, before: Self) -> Self {
        Self { turns: (self.turns + before.turns) % 4, shift: self.apply(before.shift) }
    }

    fn inverse(self) -> Self {
        let turns = (4 - self.turns) % 4;
        Self { turns, shift: Self::rotate(-self.shift, turns) }
    }
}

impl UvCutTopology {
    /// Find the closest cut coordinates to the initial fit in squared UV
    /// distance, subject to one fixed integer-grid transition per seam.
    /// Return `None` if the transitions conflict or cannot be represented
    /// at the precision required by the isoline extractor.
    pub fn fit_seam_uvs(&self, initial: &[Vec2], transitions: &[UvSeamTransition]) -> Option<Vec<Vec2>> {
        const EPS: f64 = 1e-4;
        if self.seams.len() != transitions.len()
            || self.source_vertices.len() != initial.len()
            || initial.iter().any(|uv| !uv.is_finite())
        {
            return None;
        }
        let mut edges = vec![Vec::new(); initial.len()];
        for (&[first, second], transition) in self.seams.iter().zip(transitions) {
            let forward = Transform { turns: transition.quarter_turns % 4, shift: transition.translation.as_dvec2() };
            for (a, b) in first.into_iter().zip(second) {
                edges.get_mut(a)?.push((b, forward));
                edges.get_mut(b)?.push((a, forward.inverse()));
            }
        }

        let mut maps = vec![None; initial.len()];
        let mut output = initial.to_vec();
        let mut component = Vec::new();
        for root in 0..initial.len() {
            if maps[root].is_some() || edges[root].is_empty() {
                continue;
            }
            maps[root] = Some(Transform::IDENTITY);
            component.clear();
            component.push(root);
            let mut fixed: Option<DVec2> = None;
            let mut sum = DVec2::ZERO;
            let mut cursor = 0;
            while cursor < component.len() {
                let i = component[cursor];
                cursor += 1;
                let map = maps[i].expect("queued vertex has a transform");
                sum += map.inverse().apply(initial[i].as_dvec2());
                for &(j, edge) in &edges[i] {
                    let path = edge.after(map);
                    if let Some(existing) = maps[j] {
                        let cycle = existing.inverse().after(path);
                        if cycle.turns == 0 {
                            if cycle != Transform::IDENTITY {
                                return None;
                            }
                            continue;
                        }
                        // Solve (I - R) x = t for the root UV. For a nonzero
                        // quarter turn this matrix is always invertible.
                        let x = DVec2::X - Transform::rotate(DVec2::X, cycle.turns);
                        let y = DVec2::Y - Transform::rotate(DVec2::Y, cycle.turns);
                        let det = x.perp_dot(y);
                        let point = DVec2::new(cycle.shift.perp_dot(y), x.perp_dot(cycle.shift)) / det;
                        if fixed.is_some_and(|old| (old - point).abs().max_element() > EPS) {
                            return None;
                        }
                        fixed = Some(point);
                    } else {
                        maps[j] = Some(path);
                        component.push(j);
                    }
                }
            }
            let uv = fixed.unwrap_or(sum / component.len() as f64);
            for &i in &component {
                output[i] = maps[i]?.apply(uv).as_vec2();
            }
        }
        for (i, row) in edges.iter().enumerate() {
            for &(j, map) in row {
                let error = (map.apply(output[i].as_dvec2()) - output[j].as_dvec2()).abs().max_element();
                if !error.is_finite() || error > EPS {
                    return None;
                }
            }
        }
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seam_fit_rotates_and_translates_both_endpoints() {
        let cut = UvCutTopology::from_seams(&[[0, 1, 2], [1, 0, 3]], &[[0, 1]]).unwrap();
        let initial = [Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::new(3.0, 5.0), Vec2::new(3.0, 4.0), Vec2::ONE];
        let aligned = cut
            .fit_seam_uvs(&initial, &[UvSeamTransition { quarter_turns: 1, translation: IVec2::new(2, 4) }])
            .unwrap();
        let [a, b] = cut.seams[0];
        assert_eq!(aligned[a[0]], Vec2::new(0.0, -0.5));
        for (a, b) in a.into_iter().zip(b) {
            assert_eq!(aligned[b], Vec2::new(-aligned[a].y + 2.0, aligned[a].x + 4.0));
        }
        assert_eq!(aligned[2], initial[2]);
        assert_eq!(aligned[5], initial[5]);
    }

    #[test]
    fn rotational_cycle_pins_a_seam_tip_and_translation_cycle_is_rejected() {
        let cut =
            UvCutTopology { triangles: Vec::new(), source_vertices: vec![0, 1, 1], seams: vec![[[0, 1], [0, 2]]] };
        let initial = [Vec2::splat(8.0); 3];
        let output = cut
            .fit_seam_uvs(&initial, &[UvSeamTransition { quarter_turns: 1, translation: IVec2::new(3, 1) }])
            .unwrap();
        assert_eq!(output[0], Vec2::new(1.0, 2.0));
        assert!(cut.fit_seam_uvs(&initial, &[UvSeamTransition { quarter_turns: 0, translation: IVec2::X }]).is_none());
    }
}
