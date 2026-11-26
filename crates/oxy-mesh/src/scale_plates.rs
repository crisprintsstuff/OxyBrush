//! Built-in scale plate meshes for Scale cover (RFC 0046).
//!
//! Original, procedural plates only — never copied from other products.
//! Each plate's base sits at **min Y** so [`crate::instances::source_frame`]
//! plants the base on the host surface and +Y points along the normal.

use crate::primitives;
use crate::{Mesh, Vec3};

/// Built-in Scale cover plate kinds (Phase 1 library).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScalePlate {
    #[default]
    Teardrop,
    Diamond,
    Round,
    Plate,
    Ridge,
    Spike,
}

impl ScalePlate {
    pub const ALL: [ScalePlate; 6] =
        [Self::Teardrop, Self::Diamond, Self::Round, Self::Plate, Self::Ridge, Self::Spike];

    pub fn id(self) -> &'static str {
        match self {
            Self::Teardrop => "teardrop",
            Self::Diamond => "diamond",
            Self::Round => "round",
            Self::Plate => "plate",
            Self::Ridge => "ridge",
            Self::Spike => "spike",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Teardrop => "Teardrop",
            Self::Diamond => "Diamond",
            Self::Round => "Round",
            Self::Plate => "Plate",
            Self::Ridge => "Ridge",
            Self::Spike => "Spike",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Self::Teardrop => "Classic overlapping reptile scale.",
            Self::Diamond => "Pointed, fish-like plate.",
            Self::Round => "Soft pebble / armadillo scale.",
            Self::Plate => "Flat armor tile.",
            Self::Ridge => "Long raised strip along the surface.",
            Self::Spike => "Outward thorn (taller than wide).",
        }
    }

    /// Piece name used when the plate is added to the scene.
    pub fn piece_name(self) -> &'static str {
        match self {
            Self::Teardrop => "Teardrop scale",
            Self::Diamond => "Diamond scale",
            Self::Round => "Round scale",
            Self::Plate => "Armor plate",
            Self::Ridge => "Scale ridge",
            Self::Spike => "Scale spike",
        }
    }

    /// Build a unit-sized plate mesh (largest extent ≈ 1).
    pub fn mesh(self) -> Mesh {
        match self {
            Self::Teardrop => teardrop(),
            Self::Diamond => diamond(),
            Self::Round => round(),
            Self::Plate => plate(),
            Self::Ridge => ridge(),
            Self::Spike => spike(),
        }
    }
}

fn normalize_extent(mesh: Mesh) -> Mesh {
    let e = mesh.bounds().extent().max_element().max(1e-6);
    let mut positions: Vec<Vec3> = mesh.positions.iter().map(|p| *p / e).collect();
    let min_y = positions.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    for p in &mut positions {
        p.y -= min_y;
    }
    Mesh::new(positions, mesh.triangles.clone())
}

fn teardrop() -> Mesh {
    // Cone with a slightly pulled tip in +Z for a teardrop plan shape.
    let mut m = primitives::cone(20, 0.45, 0.35);
    let b = m.bounds();
    let min_y = b.min.y;
    let height = b.extent().y.max(1e-6);
    for p in &mut m.positions {
        let t = ((p.y - min_y) / height).clamp(0.0, 1.0);
        p.z *= 1.0 - 0.35 * t;
        p.x *= 1.0 - 0.15 * t;
        p.z += 0.08 * t;
    }
    normalize_extent(m)
}

fn diamond() -> Mesh {
    // Flat diamond (rhombus) with a slight dome.
    let pts = [
        Vec3::new(0.0, 0.0, -0.55),
        Vec3::new(0.4, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 0.55),
        Vec3::new(-0.4, 0.0, 0.0),
        Vec3::new(0.0, 0.12, 0.0), // dome tip
    ];
    let tris = [[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4], [0, 3, 2], [0, 2, 1]];
    normalize_extent(Mesh::new(pts.to_vec(), tris.to_vec()))
}

fn round() -> Mesh {
    // Flattened icosphere dome: squash Y and clip under the equator.
    let mut m = primitives::icosphere(1, 0.5);
    for p in &mut m.positions {
        p.y = p.y.max(0.0) * 0.45;
    }
    normalize_extent(m)
}

fn plate() -> Mesh {
    // Thin beveled box — flat armor tile, base on −Y.
    let hx = 0.45;
    let hz = 0.35;
    let hy = 0.06;
    let bevel = 0.04;
    let pts = [
        // bottom
        Vec3::new(-hx, 0.0, -hz),
        Vec3::new(hx, 0.0, -hz),
        Vec3::new(hx, 0.0, hz),
        Vec3::new(-hx, 0.0, hz),
        // top (inset)
        Vec3::new(-hx + bevel, hy, -hz + bevel),
        Vec3::new(hx - bevel, hy, -hz + bevel),
        Vec3::new(hx - bevel, hy, hz - bevel),
        Vec3::new(-hx + bevel, hy, hz - bevel),
    ];
    let tris = [
        [0, 2, 1],
        [0, 3, 2], // bottom (facing −Y)
        [4, 5, 6],
        [4, 6, 7], // top
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    normalize_extent(Mesh::new(pts.to_vec(), tris.to_vec()))
}

fn ridge() -> Mesh {
    // Long wedge along Z.
    let pts = [
        Vec3::new(-0.15, 0.0, -0.6),
        Vec3::new(0.15, 0.0, -0.6),
        Vec3::new(0.15, 0.0, 0.6),
        Vec3::new(-0.15, 0.0, 0.6),
        Vec3::new(0.0, 0.18, -0.55),
        Vec3::new(0.0, 0.18, 0.55),
    ];
    let tris =
        [[0, 2, 1], [0, 3, 2], [0, 1, 4], [1, 2, 5], [1, 5, 4], [2, 3, 5], [3, 0, 4], [3, 4, 5], [0, 4, 5], [0, 5, 3]];
    normalize_extent(Mesh::new(pts.to_vec(), tris.to_vec()))
}

fn spike() -> Mesh {
    normalize_extent(primitives::cone(16, 0.22, 0.9))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::source_frame;

    #[test]
    fn every_plate_has_base_at_min_y_and_unit_extent() {
        for kind in ScalePlate::ALL {
            let m = kind.mesh();
            assert!(m.triangle_count() > 0, "{}", kind.id());
            let e = m.bounds().extent().max_element();
            assert!((e - 1.0).abs() < 1e-3, "{} extent {e}", kind.id());
            let (base, _) = source_frame(&m);
            assert!((base.y - m.bounds().min.y).abs() < 1e-5, "{}", kind.id());
        }
    }

    #[test]
    fn ids_are_stable() {
        assert_eq!(ScalePlate::Teardrop.id(), "teardrop");
        assert_eq!(ScalePlate::Spike.label(), "Spike");
    }
}
