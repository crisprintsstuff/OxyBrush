//! Hierarchical sparse VDB tree structures and spatial transforms.
//!
//! Implements the sparse volumetric data structure described by Ken Museth (2013),
//! "VDB: High-Resolution Sparse Volumes with Dynamic Topology", ACM Trans. Graph. 32(3):
//! - Standard leaf size of 8^3 = 512 voxels (log2dim = 3).
//! - 512-bit active value mask per leaf.
//! - Index-to-world linear transform with uniform or anisotropic voxel scale.

use glam::{IVec3, Vec3};
use oxy_mesh::Aabb;
use rustc_hash::FxHashMap;

pub const LEAF_LOG2DIM: usize = 3;
pub const LEAF_DIM: i32 = 1 << LEAF_LOG2DIM; // 8
pub const LEAF_VOXELS: usize = LEAF_DIM as usize * LEAF_DIM as usize * LEAF_DIM as usize; // 512
pub const LEAF_MASK_WORDS: usize = LEAF_VOXELS / 64; // 8 words

/// A 512-bit bitmask tracking active states for all voxels in an 8x8x8 leaf node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitMask512 {
    pub words: [u64; LEAF_MASK_WORDS],
}

impl Default for BitMask512 {
    fn default() -> Self {
        Self { words: [0; LEAF_MASK_WORDS] }
    }
}

impl BitMask512 {
    #[inline]
    pub fn is_set(&self, offset: usize) -> bool {
        let word = offset >> 6;
        let bit = offset & 63;
        (self.words[word] & (1u64 << bit)) != 0
    }

    #[inline]
    pub fn set_on(&mut self, offset: usize) {
        let word = offset >> 6;
        let bit = offset & 63;
        self.words[word] |= 1u64 << bit;
    }

    #[inline]
    pub fn set_off(&mut self, offset: usize) {
        let word = offset >> 6;
        let bit = offset & 63;
        self.words[word] &= !(1u64 << bit);
    }

    #[inline]
    pub fn count_ones(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }
}

/// An 8x8x8 leaf node containing 512 contiguous voxel values and an active bitmask.
#[derive(Clone)]
pub struct LeafNode {
    pub origin: IVec3,
    pub mask: BitMask512,
    pub values: Box<[f32; LEAF_VOXELS]>,
}

impl LeafNode {
    pub fn new(origin: IVec3, background: f32) -> Self {
        debug_assert_eq!(origin.x & 7, 0);
        debug_assert_eq!(origin.y & 7, 0);
        debug_assert_eq!(origin.z & 7, 0);
        Self { origin, mask: BitMask512::default(), values: Box::new([background; LEAF_VOXELS]) }
    }

    #[inline]
    pub fn offset_of(coord: IVec3) -> usize {
        let x = (coord.x & 7) as usize;
        let y = (coord.y & 7) as usize;
        let z = (coord.z & 7) as usize;
        x | (y << 3) | (z << 6)
    }

    #[inline]
    pub fn coord_of(&self, offset: usize) -> IVec3 {
        let x = (offset & 7) as i32;
        let y = ((offset >> 3) & 7) as i32;
        let z = ((offset >> 6) & 7) as i32;
        self.origin + IVec3::new(x, y, z)
    }

    #[inline]
    pub fn get(&self, offset: usize) -> f32 {
        self.values[offset]
    }

    #[inline]
    pub fn set(&mut self, offset: usize, val: f32) {
        self.values[offset] = val;
        self.mask.set_on(offset);
    }
}

/// Linear spatial transform between discrete index space and world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub voxel_size: f32,
    pub origin: Vec3,
}

impl Transform {
    pub fn new(voxel_size: f32, origin: Vec3) -> Self {
        Self { voxel_size: voxel_size.max(1e-6), origin }
    }

    #[inline]
    pub fn index_to_world(&self, coord: IVec3) -> Vec3 {
        self.origin + coord.as_vec3() * self.voxel_size
    }

    #[inline]
    pub fn index_to_world_f(&self, coord: Vec3) -> Vec3 {
        self.origin + coord * self.voxel_size
    }

    #[inline]
    pub fn world_to_index(&self, world: Vec3) -> IVec3 {
        ((world - self.origin) / self.voxel_size).round().as_ivec3()
    }

    #[inline]
    pub fn world_to_continuous_index(&self, world: Vec3) -> Vec3 {
        (world - self.origin) / self.voxel_size
    }
}

/// A sparse hierarchical VDB grid storing scalar `f32` level-set signed distance values.
#[derive(Clone)]
pub struct VdbFloatGrid {
    pub name: String,
    pub transform: Transform,
    pub background: f32,
    /// Leaves by origin. A fast non-cryptographic hash: every voxel read
    /// looks a leaf up here.
    pub leaves: FxHashMap<IVec3, LeafNode>,
}

impl VdbFloatGrid {
    pub fn new(name: impl Into<String>, voxel_size: f32, background: f32) -> Self {
        Self {
            name: name.into(),
            transform: Transform::new(voxel_size, Vec3::ZERO),
            background,
            leaves: FxHashMap::default(),
        }
    }

    #[inline]
    pub fn leaf_origin(coord: IVec3) -> IVec3 {
        IVec3::new(coord.x & !7, coord.y & !7, coord.z & !7)
    }

    #[inline]
    pub fn get_value(&self, coord: IVec3) -> f32 {
        let l_orig = Self::leaf_origin(coord);
        if let Some(leaf) = self.leaves.get(&l_orig) {
            let offset = LeafNode::offset_of(coord);
            if leaf.mask.is_set(offset) {
                return leaf.values[offset];
            }
        }
        self.background
    }

    #[inline]
    pub fn set_value(&mut self, coord: IVec3, val: f32) {
        let l_orig = Self::leaf_origin(coord);
        let bg = self.background;
        let leaf = self.leaves.entry(l_orig).or_insert_with(|| LeafNode::new(l_orig, bg));
        let offset = LeafNode::offset_of(coord);
        leaf.set(offset, val);
    }

    #[inline]
    pub fn is_active(&self, coord: IVec3) -> bool {
        let l_orig = Self::leaf_origin(coord);
        if let Some(leaf) = self.leaves.get(&l_orig) { leaf.mask.is_set(LeafNode::offset_of(coord)) } else { false }
    }

    /// Trilinear interpolation of the level set value at any continuous world position.
    pub fn eval_trilinear(&self, world: Vec3) -> f32 {
        let idx = self.transform.world_to_continuous_index(world);
        let base = idx.floor().as_ivec3();
        let f = idx - base.as_vec3();

        let v000 = self.get_value(base + IVec3::new(0, 0, 0));
        let v100 = self.get_value(base + IVec3::new(1, 0, 0));
        let v010 = self.get_value(base + IVec3::new(0, 1, 0));
        let v110 = self.get_value(base + IVec3::new(1, 1, 0));
        let v001 = self.get_value(base + IVec3::new(0, 0, 1));
        let v101 = self.get_value(base + IVec3::new(1, 0, 1));
        let v011 = self.get_value(base + IVec3::new(0, 1, 1));
        let v111 = self.get_value(base + IVec3::new(1, 1, 1));

        let v00 = v000 * (1.0 - f.x) + v100 * f.x;
        let v10 = v010 * (1.0 - f.x) + v110 * f.x;
        let v01 = v001 * (1.0 - f.x) + v101 * f.x;
        let v11 = v011 * (1.0 - f.x) + v111 * f.x;

        let v0 = v00 * (1.0 - f.y) + v10 * f.y;
        let v1 = v01 * (1.0 - f.y) + v11 * f.y;

        v0 * (1.0 - f.z) + v1 * f.z
    }

    pub fn active_voxel_count(&self) -> usize {
        self.leaves.values().map(|l| l.mask.count_ones()).sum()
    }

    pub fn leaf_count(&self) -> usize {
        self.leaves.len()
    }

    /// Computes the bounding box of active voxels in index coordinates.
    pub fn index_bounds(&self) -> Option<(IVec3, IVec3)> {
        let mut min = IVec3::splat(i32::MAX);
        let mut max = IVec3::splat(i32::MIN);
        let mut any = false;

        for leaf in self.leaves.values() {
            for offset in 0..LEAF_VOXELS {
                if leaf.mask.is_set(offset) {
                    let c = leaf.coord_of(offset);
                    min = min.min(c);
                    max = max.max(c);
                    any = true;
                }
            }
        }

        if any { Some((min, max)) } else { None }
    }

    /// Computes the world-space bounding box of active voxels.
    pub fn world_bounds(&self) -> Aabb {
        if let Some((min, max)) = self.index_bounds() {
            let p_min = self.transform.index_to_world(min);
            let p_max = self.transform.index_to_world(max);
            Aabb { min: p_min, max: p_max }
        } else {
            Aabb::EMPTY
        }
    }
}
