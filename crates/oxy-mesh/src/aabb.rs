use glam::Vec3;

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb { min: Vec3::splat(f32::INFINITY), max: Vec3::splat(f32::NEG_INFINITY) };

    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Self {
        let mut a = Self::EMPTY;
        for p in points {
            a.grow(p);
        }
        a
    }

    pub fn grow(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb { min: self.min.min(o.min), max: self.max.max(o.max) }
    }

    pub fn is_empty(&self) -> bool {
        self.min.cmpgt(self.max).any()
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn extent(&self) -> Vec3 {
        if self.is_empty() { Vec3::ZERO } else { self.max - self.min }
    }

    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    /// Squared distance from a point to the box (0 inside).
    pub fn distance_sq(&self, p: Vec3) -> f32 {
        if self.is_empty() {
            return f32::INFINITY;
        }
        let d = (self.min - p).max(Vec3::ZERO).max(p - self.max);
        d.length_squared()
    }

    /// Ray entry distance (clamped to 0) if the ray hits the box.
    pub fn ray_entry(&self, origin: Vec3, inv_dir: Vec3) -> Option<f32> {
        if self.is_empty() {
            return None;
        }
        let t1 = (self.min - origin) * inv_dir;
        let t2 = (self.max - origin) * inv_dir;
        let tmin = t1.min(t2).max_element().max(0.0);
        let tmax = t1.max(t2).min_element();
        (tmax >= tmin).then_some(tmin)
    }
}
