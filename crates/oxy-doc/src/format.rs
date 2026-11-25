//! The `.oxb` project file format (version 1).
//!
//! ```text
//! file    := magic:"OXYBRUSH" version:u32 flags:u32 count:u32 section*count
//! section := tag:[u8;4] raw_len:u64 packed_len:u64 crc32(raw):u32 lz4_block(raw)
//! ```
//!
//! All integers are little-endian. Sections:
//! - `META`: UTF-8 `key=value` lines (`app`, `active`).
//! - `PIEC`: one piece: `name_len:u32 name:utf8 visible:u8 vcount:u32
//!   tcount:u32 positions:f32*3v triangles:u32*3t mask:f32*v groups:u32*t
//!   hidden:u8*t`.
//! - `MRES` (optional, since 0.2): subdivision levels of the **preceding**
//!   `PIEC`: `levels:u32 active:u32 base_tcount:u32 base_triangles:u32*3t`
//!   then per level `vcount:u32 positions:f32*3v`. Finer levels' topology
//!   is derived by subdividing the base. When present, the preceding
//!   `PIEC` stores the active level in level order. Older readers skip it
//!   and simply load the active level as a plain mesh.
//! - `MRCC` (optional, since 4.0.0-beta.9): quad (Catmull–Clark)
//!   subdivision levels of the **preceding** `PIEC`, written instead of
//!   `MRES` for such levels. It starts with exactly `MRES`'s layout (base
//!   triangles, then every level's positions), followed by the base
//!   polygons: `faces:u32` then per face `n:u32 corner:u32*n m:u32
//!   triangle:u32*m` (its corners, and which base triangles make it up),
//!   then `creases:u32` and per crease `a:u32 b:u32 weight:f32`. Finer
//!   levels' topology is derived by Catmull–Clark subdivision of the base
//!   polygons. Older readers skip it and load the active level as a plain
//!   mesh.
//! - `MRLN` (optional, since 4.0.0-beta.9): which subdivision levels in the
//!   **preceding** `MRES` or `MRCC` were made without smoothing:
//!   `count:u32 smooth:u8*count` (one per level, level 0 first; 1 smoothed,
//!   0 split in place). Written only when some level isn't smoothed. Older
//!   readers skip it: the levels' points are the same, only edits on a
//!   lower level then carry the finer levels along as if they were smooth.
//! - `MATL` (optional, since 4.0.0-beta.10): the matcap of the
//!   **preceding** `PIEC` when it isn't the scene's, as UTF-8 text:
//!   `preset:KEY` or `custom:NAME`. Older readers skip it and show the piece
//!   with the scene's matcap.
//! - `INST` (optional, since 4.0.0-beta.12): surface instances on the
//!   **preceding** `PIEC` (RFC 0022): `count:u32` then per instancer
//!   `source:u32` (the source piece's place among the file's pieces, from
//!   0; not this piece) `place:u8` (0 faces, 1 points, 2 one face group)
//!   `group:u32` (for 2) `orient:u8` (0 a world direction, 1 longest edge,
//!   2 first edge, 3 random) `direction:f32*3` `size_mode:u8` (0 scene
//!   units, 1 times the face size) `size:f32 offset:f32 spin:f32
//!   vary_size:f32 vary_spin:f32 seed:u32`. Older readers skip it: the host
//!   and the (hidden) source open as ordinary pieces, without the copies.
//! - `INS2` (optional): the full surface-instance list when it uses mask or
//!   scatter placement. Layout as `INST`, with place 3 (mask threshold) or
//!   4 (scatter spacing), reusing `group` as f32 bits. Replaces `INST` on
//!   read; earlier readers still get legacy modes through `INST`.
//! - `PARN` (optional, since 4.0.0-beta.20): parent piece index of the
//!   **preceding** `PIEC`: `parent:u32`, the parent piece's place among the
//!   file's pieces (from 0; not this piece). Transforming the parent transforms
//!   this piece with it. Older readers skip it and keep pieces independent.
//! - `LINK` (optional, since 4.0.0-beta.23): the **preceding** `PIEC` is
//!   a linked copy of another piece (#327): `source:u32 transform:f32*16`,
//!   the original's place among the file's pieces (from 0; never this
//!   piece, nor a piece that is itself a copy) and the transform from the
//!   original to the copy, column by column. The `PIEC` holds the copy's
//!   mesh as it was, so older readers skip this and open an ordinary piece.
//! - `LITE` (optional, since 4.0.0-beta.23.2): the **preceding** `PIEC`
//!   was saved shown lighter (Lighter inactive pieces): its `MRES` is at a
//!   lower level than the one it was being worked at, `level:u32` (from 0).
//!   The app shows that level again. Older readers skip it and open the
//!   piece at the saved level, with every level still there.
//! - `ARMS` (optional, since 4.0.0-beta.23.3): OxySpheres (RFC 0034).
//! - `ARQD` (optional): u8 subdivision level for quad OxySpheres output.
//! - `ARML` (optional): u64 index of the mesh last made by the preceding `ARMS` recipe.
//!   `version:u32 resolution:u32 blend:f32 triangles:u32 fingerprint:f32*3
//!   mirrored:u8 mirror_x:f32 count:u32`, then per sphere `center:f32*3
//!   radius:f32 parent:u32 twin:u32` (`u32::MAX`: none); sphere 0 is the
//!   root. Version 2 (since 4.0.0-beta.24): the **preceding** `PIEC` is an
//!   OxySpheres piece, the spheres themselves (its mesh only draws them;
//!   triangles and fingerprint are those of the mesh they made last, 0
//!   before any, so Make mesh can update it while it is unchanged). Version 1 (4.0.0-beta.23.3 only):
//!   the spheres the preceding piece's mesh was made from, valid while the
//!   mesh is unchanged; readers add them back as an OxySpheres piece at the
//!   end of the list. Older readers skip the section, and 4.0.0-beta.23.3
//!   skips version 2: both open the piece as an ordinary mesh. A damaged
//!   or unknown-version section is ignored.
//! - `POLY` (optional, since 4.0.0-beta.22): the polygons of the
//!   **preceding** `PIEC` when it was imported from (or built from) quads
//!   and n-gons, as the edges inside them: `dcount:u32 diagonal:u32*2d
//!   ncount:u32 ngon_edge:u32*2n`, point pairs (low first) in the `PIEC`'s
//!   point order. A diagonal splits a quad into two triangles; an n-gon
//!   edge lies inside a polygon of five or more corners. Not written for a
//!   piece with subdivision levels (the `MRES` keeps its polygons) or whose
//!   quads are guessed from its shape. Older readers skip it and guess the
//!   quads, so quads that aren't flat open as two triangles.
//! - `REFI` (optional): reference image planes with embedded RGBA8 and placement.
//!   Placement flag bits 3–4 hold clockwise quarter-turns and bits 5–6
//!   hold horizontal and vertical flips. Earlier readers ignore these bits.
//! - `RROT` (optional): f32 fine rotation in radians for the immediately preceding REFI.
//! - `ALPH` (optional, since 4.0.0-beta.1): a brush alpha kept with the project,
//!   one section each, after the pieces: `name_len:u32 name:utf8 size:u32
//!   value:u16*size²` (row-major, row 0 at the top, 65535 = full strength).
//!   Not tied to a piece. Older readers skip it and lose only the alpha.
//! - `VCOL` (optional, since 0.4): vertex color of the **preceding** `PIEC`:
//!   `vcount:u32 rgb:f32*3v` (sRGB, 0..=1), indexed like its positions.
//!   Only written for painted pieces; older readers skip it and show the
//!   piece unpainted.
//! - `UVCO` (optional, since 4.0.0-beta.11): texture coordinates of the
//!   **preceding** `PIEC`, per triangle corner (RFC 0020): `tcount:u32
//!   uv:f32*6t` (u, v for each of a triangle's three corners, in the
//!   `PIEC`'s triangle order; v points up, UDIM tiles count up from 0..1).
//!   Older readers skip it and open the piece without UVs.
//! - `MRUV` (optional): base-level UVs of the preceding `MRCC` or `MRES`,
//!   using the `UVCO` layout in base triangle order. Higher levels are
//!   derived from the base. Older readers still show the active UVs in `UVCO`.
//! - `MRPT` (optional, since 4.0.0-beta.23): vertex color each level of
//!   the preceding `MRCC` or `MRES` has beyond what carrying the level
//!   below up gives, so paint finer than a lower level comes back when the
//!   project is reopened at that level and stepped up: `count:u32` (the
//!   number of levels), then per level `n:u32 rgb:f32*3n`, `n` 0 or the
//!   level's point count, in level point order. Written only when a level
//!   has some. Older readers skip it and lose that finer paint above the
//!   saved level; the saved level's own colors are in `VCOL`.
//! - `VMAP` (optional, since 2.5.8): vertex origin of the **preceding**
//!   `PIEC`: `vcount:u32 source:u32*vcount`. `source[i]` is the index of
//!   vertex `i` in the vertex array of the `PIEC` the piece was originally
//!   loaded from (OxyBrush reorders vertices when it loads a mesh), so a
//!   program that wrote the original file can match vertices exactly.
//!   Written only while the piece's topology is unchanged since it was
//!   loaded and it has no `MRES`; a file saved again keeps pointing at the
//!   first file. Older readers skip it.
//! - `SYMC` (optional, since 2.5.8): symmetry centre of the **preceding**
//!   `PIEC`: `x:f32 y:f32 z:f32`, the point mirror and radial symmetry act
//!   about. Without it (and in older readers) symmetry uses the world origin.
//! - `CAGE` (optional, since 3.0.5): retopology cage of the **preceding**
//!   `PIEC` (RFC 0012): `vcount:u32 positions:f32*3v fcount:u32
//!   faces:u32*4f`. Faces are quads, or triangles with `0xFFFFFFFF` as the
//!   fourth corner. Its first vertices are the `PIEC`'s vertices in the same
//!   order; any after them are loose points not yet used by a face. Written
//!   only while the piece's mesh is still the one built from the cage.
//!   Older readers skip it and load the piece as an ordinary mesh.
//! - `PINS` (optional, since 4.0.0-beta.2): pinned points of the cage in
//!   the **preceding** `CAGE`: `count:u32 vertex:u32*count`, cage vertex
//!   indices. Pinned points stay put when the cage is relaxed. Written only
//!   when something is pinned. Older readers skip it and nothing is pinned.
//! - `TUBE` (optional, since 3.0.6): how the tubes in the **preceding**
//!   `PIEC` were made (RFC 0009), so they can be edited again:
//!   `count:u32` then per tube `n:u32 points:f32*3n radii:f32*n sides:u32
//!   caps:u8 mirror:u8 mirror_x:f32 group:u32 triangles:u32
//!   fingerprint:f32*3`. Older readers skip it; the tubes stay ordinary
//!   triangles.
//! - `TUBX` (optional, since 4.0.0-beta.11): the shape of the tubes in the
//!   **preceding** `TUBE`, in the same order: `count:u32` then per tube
//!   `style:u8` (0 tube, 1 braid) `twist:f32 coil_size:f32 coil_turns:f32
//!   spread:f32 braid_turns:f32 triangles:u32`. Written only when some tube
//!   is twisted, coiled or braided. Such a tube's `TUBE` entry has
//!   `triangles` 0 and its real count is here, so an older reader, which
//!   skips `TUBX`, never matches it and keeps it as ordinary triangles
//!   instead of reopening it as a plain tube.
//! - `TUBC` (optional, since 4.0.0-beta.14): which points of the tubes in
//!   the **preceding** `TUBE` are sharp corners (#258), in the same order:
//!   `count:u32` then per tube `n:u32 sharp:u8*n triangles:u32`, `n` the
//!   tube's point count (0 for a tube without corners). Written only when
//!   some tube has a corner. Such a tube's entries in `TUBE` and `TUBX`
//!   have `triangles` 0 and its real count is here, so an older reader
//!   never reopens it with its corners rounded off.
//! - `TUBP` (optional, since 4.0.0-beta.14): the cross-section shapes of
//!   the tubes in the **preceding** `TUBE`, in the same order: `count:u32`
//!   then per tube `n:u32` (0 for a round tube) `smooth:u8
//!   points:f32*2n triangles:u32`, points in units of the tube's radius.
//!   Written only when some tube has a profile. Such a tube's entries in
//!   `TUBE`, `TUBX` and `TUBC` have `triangles` 0 and its real count is
//!   here, so an older reader never reopens it round.
//! - `TPRC` (optional, since 4.0.0-beta.15): which points of the tubes'
//!   profiles in the **preceding** `TUBP` are sharp corners of a smooth
//!   outline, in the same order and laid out as `TUBC`: `count:u32` then
//!   per tube `n:u32 sharp:u8*n triangles:u32`, `n` the profile's point
//!   count (0 for a tube without such corners). Written only when some
//!   profile has one. Such a tube's entries in the older tube sections have
//!   `triangles` 0, so an older reader never reopens it with the corners
//!   rounded off.
//! - `CURV` (optional, since 3.0.6): a Curve Move curve (RFC 0005) that was
//!   open on the **preceding** `PIEC` when it was saved; the piece itself is
//!   saved without the curve's bend. `n:u32` then `points:f32*6n` (position
//!   and normal), `rest:f32*6n`, `weights:f32*2n` (reach, strength), then
//!   `closed:u8 radius_pt:f32 world_per_pt:f32 strength:f32 falloff:u8
//!   pressure:f32 vertices:u32 triangles:u32`. Older readers skip it, and
//!   the open curve is lost, as it always was.
//! - `STRK` (optional, since 4.0.0-beta.9): editable strokes (Living
//!   strokes, RFC 0017) of the **preceding** `PIEC`, each stored as what it
//!   changed, so it can still be faded or removed after reopening. See
//!   `encode_strokes` for the layout. The piece's surface already includes
//!   every stroke, so older readers skip it and open the project as it
//!   looks, with the strokes fixed in place.
//! - `STRI` (optional, since 4.0.0-beta.16): what was drawn for the strokes
//!   of the **preceding** `STRK` (their recipes, RFC 0025), in the same
//!   order: `count:u32` then per stroke `len:u32 bytes:u8*len`, `len` 0 for
//!   a stroke without one. The bytes are an `oxy_sculpt::StrokeRecipe`,
//!   opaque to this crate. Each recipe starts with `OXSR` and its own
//!   version byte (1: an alpha is named and a sample of its raw sample
//!   bits; 2: an alpha is a hash of every pixel quantized to the 16-bit
//!   values `ALPH` stores, and the name is not an identity). A blob that
//!   starts with `OXNR` and one tag byte is not a recipe: it says why the
//!   stroke can't be edited (1 dynamic detail, 2 a mirrored copy or a split
//!   side, 3 a whole-piece tool). Older readers of `STRI` treat that blob
//!   as a recipe they can't read and keep the stroke without one. The
//!   section layout is unchanged, so a reader that skips unknown sections
//!   still skips `STRI`, and a reader of version 1 recipes still reads them.
//!   A newer recipe version drops that stroke's recipe and keeps the stroke.
//!   Written when some stroke has a recipe or a reason.
//!
//! Readers skip unknown sections, so new data can be added without breaking
//! older versions. A project may contain no `PIEC` sections at all: an empty
//! scene. Older readers reject that file as damaged ("no pieces"). The
//! layout of every existing section is unchanged, so a project that has
//! pieces still opens in older versions. Files are written to a temporary
//! file and renamed into place, so a crash never leaves a half-written project.

use crate::{PieceData, SceneData};
use glam::Vec3;
use oxy_mesh::MeshData;
use oxy_mesh::multires::Multires;
use std::fmt;
use std::io::{self, Write};
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"OXYBRUSH";
pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "oxb";

/// Refuse sections that would decompress to more than this.
const MAX_SECTION_BYTES: u64 = 4 << 30;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    NotOxb,
    UnsupportedVersion(u32),
    Corrupt(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::NotOxb => write!(f, "not an OxyBrush project file"),
            Error::UnsupportedVersion(v) => {
                write!(f, "project format version {v} is newer than this OxyBrush supports ({VERSION})")
            }
            Error::Corrupt(m) => write!(f, "project file is damaged: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn encode(scene: &SceneData) -> Vec<u8> {
    let mut sections: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    let meta = format!("app=oxybrush {}\nactive={}\n", env!("CARGO_PKG_VERSION"), scene.active);
    sections.push((*b"META", meta.into_bytes()));
    for p in &scene.pieces {
        sections.push((*b"PIEC", encode_piece(p)));
        if let Some(mr) = &p.multires {
            match mr.base_polygons() {
                Some(base) => {
                    let mut b = encode_multires(mr);
                    encode_base_polygons(&mut b, &base);
                    sections.push((*b"MRCC", b));
                }
                None => sections.push((*b"MRES", encode_multires(mr))),
            }
            if !mr.base_uvs().is_empty() {
                sections.push((*b"MRUV", encode_uvs(mr.base_uvs())));
            }
            let paint = mr.level_paint();
            if paint.iter().any(|p| !p.is_empty()) {
                let mut b = (paint.len() as u32).to_le_bytes().to_vec();
                for level in paint {
                    b.extend_from_slice(&(level.len() as u32).to_le_bytes());
                    for c in level {
                        for x in c.to_array() {
                            b.extend_from_slice(&x.to_le_bytes());
                        }
                    }
                }
                sections.push((*b"MRPT", b));
            }
            let smooth = mr.level_smooth();
            if smooth.iter().any(|s| !s) {
                let mut b = (smooth.len() as u32).to_le_bytes().to_vec();
                b.extend(smooth.iter().map(|&s| u8::from(s)));
                sections.push((*b"MRLN", b));
            }
        }
        if let Some(c) = p.symmetry_center {
            let mut b = Vec::with_capacity(12);
            for x in c.to_array() {
                b.extend_from_slice(&x.to_le_bytes());
            }
            sections.push((*b"SYMC", b));
        }
        if let Some(map) = &p.vertex_map {
            let mut b = Vec::with_capacity(4 + map.len() * 4);
            b.extend_from_slice(&(map.len() as u32).to_le_bytes());
            for i in map {
                b.extend_from_slice(&i.to_le_bytes());
            }
            sections.push((*b"VMAP", b));
        }
        if let Some(cage) = &p.cage {
            sections.push((*b"CAGE", encode_cage(cage)));
            // Polygons with more than four corners, and face groups, which
            // `CAGE` can't hold (it keeps n-gons as triangles for older
            // versions). Only written when there are any.
            if cage.ngon_count() > 0 || cage.faces.iter().any(|f| f.group != 0) {
                sections.push((*b"PLYC", encode_poly_faces(cage)));
            }
            let pinned: Vec<u32> = (0..cage.positions.len() as u32).filter(|&v| cage.is_pinned(v)).collect();
            if !pinned.is_empty() {
                let mut b = Vec::with_capacity(4 + pinned.len() * 4);
                b.extend_from_slice(&(pinned.len() as u32).to_le_bytes());
                for v in pinned {
                    b.extend_from_slice(&v.to_le_bytes());
                }
                sections.push((*b"PINS", b));
            }
            // Crease weights for smooth subdivision (Poly Edit), if any.
            let creases = cage.live_creases();
            if !creases.is_empty() {
                sections.push((*b"CRSE", encode_creases(&creases)));
            }
        }
        if !p.tubes.is_empty() {
            sections.push((*b"TUBE", encode_tubes(&p.tubes)));
            if p.tubes.iter().any(|t| !t.shape.is_plain()) {
                sections.push((*b"TUBX", encode_tube_shapes(&p.tubes)));
            }
            if p.tubes.iter().any(crate::TubeRecipe::has_corners) {
                sections.push((*b"TUBC", encode_tube_corners(&p.tubes)));
            }
            if p.tubes.iter().any(|t| t.profile.is_some()) {
                sections.push((*b"TUBP", encode_tube_profiles(&p.tubes)));
            }
            if p.tubes.iter().any(|t| t.profile.as_ref().is_some_and(|p| p.has_corners())) {
                sections.push((*b"TPRC", encode_profile_corners(&p.tubes)));
            }
        }
        if let Some(c) = &p.curve {
            sections.push((*b"CURV", encode_curve(c)));
        }
        if let Some(m) = &p.material {
            sections.push((*b"MATL", m.as_bytes().to_vec()));
        }
        if p.instancers.iter().any(|ins| ins.settings.place.extended()) {
            let legacy: Vec<_> = p.instancers.iter().filter(|ins| !ins.settings.place.extended()).cloned().collect();
            if !legacy.is_empty() {
                sections.push((*b"INST", encode_instancers(&legacy)));
            }
            sections.push((*b"INS2", encode_instancers(&p.instancers)));
        } else if !p.instancers.is_empty() {
            sections.push((*b"INST", encode_instancers(&p.instancers)));
        }
        if let Some(parent) = p.parent {
            sections.push((*b"PARN", parent.to_le_bytes().to_vec()));
        }
        if let Some(level) = p.own_level {
            sections.push((*b"LITE", level.to_le_bytes().to_vec()));
        }
        if let Some(a) = &p.spheres {
            sections.push((*b"ARMS", encode_armature(a, 2)));
            if let Some(level) = a.quad_subdivision {
                sections.push((*b"ARQD", vec![level]));
            }
            if let Some(index) = a.generated_mesh {
                sections.push((*b"ARML", index.to_le_bytes().to_vec()));
            }
        }
        if let Some((source, transform)) = p.link {
            let mut b = source.to_le_bytes().to_vec();
            for x in transform.to_cols_array() {
                b.extend(x.to_le_bytes());
            }
            sections.push((*b"LINK", b));
        }
        if let Some(st) = p.strokes.as_ref().filter(|st| !st.strokes.is_empty() || !st.parts.is_empty()) {
            if !st.strokes.is_empty() {
                sections.push((*b"STRK", encode_strokes(st)));
                if st.strokes.iter().any(|s| !s.recipe.is_empty()) {
                    sections.push((*b"STRI", encode_stroke_recipes(st)));
                }
            }
            if !st.parts.is_empty() {
                sections.push((*b"PART", encode_placed_parts(&st.parts)));
            }
        }
        if !p.mesh.uv.is_empty() && p.mesh.uv.len() == p.mesh.triangles.len() {
            sections.push((*b"UVCO", encode_uvs(&p.mesh.uv)));
        }
        if let Some(polys) = p.polygons.as_ref().filter(|_| p.multires.is_none()) {
            sections.push((*b"POLY", encode_polygons(polys)));
        }
        if !p.mesh.color.is_empty() {
            let mut b = Vec::with_capacity(4 + p.mesh.color.len() * 12);
            b.extend_from_slice(&(p.mesh.color.len() as u32).to_le_bytes());
            for c in &p.mesh.color {
                for x in c.to_array() {
                    b.extend_from_slice(&x.to_le_bytes());
                }
            }
            sections.push((*b"VCOL", b));
        }
    }

    for a in &scene.alphas {
        let mut b = Vec::with_capacity(12 + a.name.len() + a.data.len() * 2);
        b.extend_from_slice(&(a.name.len() as u32).to_le_bytes());
        b.extend_from_slice(a.name.as_bytes());
        b.extend_from_slice(&a.size.to_le_bytes());
        for v in &a.data {
            b.extend_from_slice(&v.to_le_bytes());
        }
        sections.push((*b"ALPH", b));
    }

    for reference in &scene.references {
        let p = &reference.placement;
        let name: String = p.name.chars().take(256).collect();
        let mut raw = Vec::new();
        raw.push(1);
        raw.push(crate::ReferenceAxis::ALL.iter().position(|&a| a == p.axis).unwrap_or(0) as u8);
        raw.push(
            u8::from(p.visible)
                | u8::from(p.all_views) << 1
                | u8::from(p.overlay) << 2
                | (p.quarter_turns & 3) << 3
                | u8::from(p.flip_horizontal) << 5
                | u8::from(p.flip_vertical) << 6,
        );
        raw.extend_from_slice(&(name.len() as u32).to_le_bytes());
        raw.extend_from_slice(name.as_bytes());
        for dim in reference.size {
            raw.extend_from_slice(&dim.to_le_bytes());
        }
        for value in [p.center.x, p.center.y, p.center.z, p.width, p.opacity] {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        raw.extend_from_slice(&reference.pixels);
        sections.push((*b"REFI", raw));
        if p.fine_rotation.is_finite() && p.fine_rotation != 0.0 {
            sections.push((*b"RROT", p.fine_rotation.to_le_bytes().to_vec()));
        }
    }

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    for (tag, raw) in sections {
        let packed = lz4_flex::block::compress(&raw);
        out.extend_from_slice(&tag);
        out.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        out.extend_from_slice(&(packed.len() as u64).to_le_bytes());
        out.extend_from_slice(&crc32fast::hash(&raw).to_le_bytes());
        out.extend_from_slice(&packed);
    }
    out
}

fn encode_piece(p: &PieceData) -> Vec<u8> {
    let m = &p.mesh;
    let (v, t) = (m.positions.len(), m.triangles.len());
    let mut b = Vec::with_capacity(16 + p.name.len() + v * 16 + t * 17);
    b.extend_from_slice(&(p.name.len() as u32).to_le_bytes());
    b.extend_from_slice(p.name.as_bytes());
    b.push(u8::from(p.visible));
    b.extend_from_slice(&(v as u32).to_le_bytes());
    b.extend_from_slice(&(t as u32).to_le_bytes());
    for q in &m.positions {
        for c in q.to_array() {
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    for tri in &m.triangles {
        for i in tri {
            b.extend_from_slice(&i.to_le_bytes());
        }
    }
    for i in 0..v {
        b.extend_from_slice(&m.mask.get(i).copied().unwrap_or(0.0).to_le_bytes());
    }
    for i in 0..t {
        b.extend_from_slice(&m.groups.get(i).copied().unwrap_or(0).to_le_bytes());
    }
    for i in 0..t {
        b.push(u8::from(m.hidden.get(i).copied().unwrap_or(false)));
    }
    b
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len()).ok_or_else(truncated)?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Ensure `count` items of `size` bytes can exist before allocating.
    fn check(&self, count: usize, size: usize) -> Result<()> {
        if count.checked_mul(size).is_none_or(|n| n > self.remaining()) { Err(truncated()) } else { Ok(()) }
    }
}

/// The `ARMS` section (see the module docs).
fn encode_armature(a: &crate::ArmatureRecipe, version: u32) -> Vec<u8> {
    let spheres = &a.armature.spheres;
    let mut b = Vec::with_capacity(41 + spheres.len() * 24);
    let u = |b: &mut Vec<u8>, x: u32| b.extend_from_slice(&x.to_le_bytes());
    let f = |b: &mut Vec<u8>, x: f32| b.extend_from_slice(&x.to_le_bytes());
    u(&mut b, version);
    u(&mut b, a.resolution);
    f(&mut b, a.blend);
    u(&mut b, a.triangles);
    for x in a.fingerprint.to_array() {
        f(&mut b, x);
    }
    b.push(a.armature.mirror.is_some() as u8);
    f(&mut b, a.armature.mirror.unwrap_or(0.0));
    u(&mut b, spheres.len() as u32);
    for s in spheres {
        for x in s.center.to_array() {
            f(&mut b, x);
        }
        f(&mut b, s.radius);
        u(&mut b, s.parent.unwrap_or(u32::MAX));
        u(&mut b, s.twin.unwrap_or(u32::MAX));
    }
    b
}

/// RETURNS: the version (1: the spheres a piece's mesh was made from; 2:
/// the piece is the spheres) and the recipe; None for a version this
/// reader doesn't know.
/// A malformed tree (see [`oxy_mesh::armature::Armature::is_valid`]) is an
/// error.
fn decode_armature(raw: &[u8]) -> Result<Option<(u32, crate::ArmatureRecipe)>> {
    use oxy_mesh::armature::{Armature, Sphere};
    let mut r = Reader { data: raw, pos: 0 };
    let version = r.u32()?;
    if !(1..=2).contains(&version) {
        return Ok(None);
    }
    let resolution = r.u32()?;
    let blend = r.f32()?;
    let triangles = r.u32()?;
    let fingerprint = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
    let mirrored = r.u8()? != 0;
    let mirror_x = r.f32()?;
    let count = r.u32()? as usize;
    r.check(count, 24)?;
    let opt = |x: u32| (x != u32::MAX).then_some(x);
    let mut spheres = Vec::with_capacity(count);
    for _ in 0..count {
        let center = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
        let radius = r.f32()?;
        let parent = opt(r.u32()?);
        let twin = opt(r.u32()?);
        spheres.push(Sphere { center, radius, parent, twin });
    }
    let armature = Armature { spheres, mirror: (mirrored && mirror_x.is_finite()).then_some(mirror_x) };
    if !armature.is_valid() || !blend.is_finite() {
        return Err(Error::Corrupt("malformed OxySpheres armature".into()));
    }
    Ok(Some((
        version,
        crate::ArmatureRecipe {
            armature,
            resolution,
            blend,
            triangles,
            fingerprint,
            generated_mesh: None,
            quad_subdivision: None,
        },
    )))
}

fn truncated() -> Error {
    Error::Corrupt("unexpected end of data".into())
}

pub fn decode(data: &[u8]) -> Result<SceneData> {
    let mut r = Reader { data, pos: 0 };
    if data.len() < 8 || r.take(8)? != MAGIC {
        return Err(Error::NotOxb);
    }
    let version = r.u32()?;
    if version > VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let _flags = r.u32()?;
    let count = r.u32()?;
    let mut scene = SceneData::default();
    let mut reference_bytes = 0;
    let mut rotation_target = None;
    for _ in 0..count {
        let tag: [u8; 4] = r.take(4)?.try_into().expect("4 bytes");
        let target = rotation_target.take();
        let raw_len = r.u64()?;
        let packed_len = r.u64()?;
        let crc = r.u32()?;
        if raw_len > MAX_SECTION_BYTES {
            return Err(Error::Corrupt("section too large".into()));
        }
        let packed = r.take(usize::try_from(packed_len).map_err(|_| truncated())?)?;
        if &tag == b"REFI"
            && (scene.references.len() >= crate::MAX_REFERENCE_IMAGES
                || raw_len > (crate::MAX_REFERENCE_BYTES - reference_bytes + 2048) as u64)
        {
            log::warn!("skipping reference image beyond the project image budget");
            continue;
        }
        let raw = lz4_flex::block::decompress(packed, raw_len as usize)
            .map_err(|e| Error::Corrupt(format!("decompression failed: {e}")))?;
        if crc32fast::hash(&raw) != crc {
            return Err(Error::Corrupt(format!("checksum mismatch in {} section", String::from_utf8_lossy(&tag))));
        }
        match &tag {
            b"META" => {
                for line in String::from_utf8_lossy(&raw).lines() {
                    if let Some(v) = line.strip_prefix("active=") {
                        scene.active = v.trim().parse().unwrap_or(0);
                    }
                }
            }
            b"PIEC" => scene.pieces.push(decode_piece(&raw)?),
            b"MRCC" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("MRCC before any piece".into()))?;
                match decode_multires_cc(&raw) {
                    Ok(mr)
                        if mr.active_triangles() == piece.mesh.triangles.as_slice()
                            && mr.parts().1[mr.active()].len() == piece.mesh.positions.len() =>
                    {
                        piece.multires = Some(mr)
                    }
                    Ok(_) => log::warn!("ignoring subdivision levels that don't match piece {:?}", piece.name),
                    Err(e) => log::warn!("ignoring damaged subdivision levels on piece {:?}: {e}", piece.name),
                }
            }
            b"MRLN" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("MRLN before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                let n = r.u32()? as usize;
                let flags: Vec<bool> = r.take(n)?.iter().map(|&b| b != 0).collect();
                if !piece.multires.as_mut().is_some_and(|mr| mr.set_level_smooth(&flags)) {
                    log::warn!("ignoring level smoothing that doesn't match piece {:?}", piece.name);
                }
            }
            b"MRES" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("MRES before any piece".into()))?;
                let mr = decode_multires(&raw)?;
                // The stack must describe exactly the saved active mesh; if
                // not, keep the mesh and drop the levels rather than fail.
                let tris_match = mr.active_triangles() == piece.mesh.triangles.as_slice();
                let verts_match = mr.parts().1[mr.active()].len() == piece.mesh.positions.len();
                if tris_match && verts_match {
                    piece.multires = Some(mr);
                } else {
                    log::warn!("ignoring subdivision levels that don't match piece {:?}", piece.name);
                }
            }
            b"UVCO" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("UVCO before any piece".into()))?;
                if let Some(uv) = decode_uvs(&raw, piece.mesh.triangles.len())? {
                    piece.mesh.uv = uv;
                } else {
                    log::warn!("ignoring UVs that don't match piece {:?}", piece.name);
                }
            }
            b"POLY" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("POLY before any piece".into()))?;
                match decode_polygons(&raw, piece.mesh.positions.len()) {
                    Ok(p) => piece.polygons = Some(p),
                    Err(e) => log::warn!("ignoring polygons that don't match piece {:?}: {e}", piece.name),
                }
            }
            b"MRPT" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("MRPT before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                let mut read = || -> Result<Vec<Vec<Vec3>>> {
                    let count = r.u32()? as usize;
                    r.check(count, 4)?;
                    (0..count)
                        .map(|_| {
                            let n = r.u32()? as usize;
                            r.check(n, 12)?;
                            (0..n).map(|_| Ok(Vec3::new(r.f32()?, r.f32()?, r.f32()?))).collect()
                        })
                        .collect()
                };
                let restored = match read() {
                    Ok(paint) => piece.multires.as_mut().is_some_and(|mr| mr.restore_level_paint(paint)),
                    Err(_) => false,
                };
                if !restored {
                    log::warn!("ignoring level paint that doesn't match subdivision levels on piece {:?}", piece.name);
                }
            }
            b"MRUV" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("MRUV before any piece".into()))?;
                let restored = piece.multires.as_mut().is_some_and(|mr| {
                    decode_uvs(&raw, mr.triangle_count(0)).ok().flatten().is_some_and(|uv| mr.restore_base_uvs(uv))
                });
                if !restored {
                    log::warn!("ignoring base UVs that don't match subdivision levels on piece {:?}", piece.name);
                }
            }
            b"VCOL" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("VCOL before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                let n = r.u32()? as usize;
                if n == piece.mesh.positions.len() {
                    piece.mesh.color =
                        (0..n).map(|_| Ok(Vec3::new(r.f32()?, r.f32()?, r.f32()?))).collect::<Result<_>>()?;
                } else {
                    log::warn!("ignoring vertex color that doesn't match piece {:?}", piece.name);
                }
            }
            b"SYMC" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("SYMC before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                let c = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
                piece.symmetry_center = c.is_finite().then_some(c);
            }
            b"VMAP" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("VMAP before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                let n = r.u32()? as usize;
                r.check(n, 4)?;
                if n == piece.mesh.positions.len() && piece.multires.is_none() {
                    piece.vertex_map = Some((0..n).map(|_| r.u32()).collect::<Result<_>>()?);
                } else {
                    log::warn!("ignoring vertex map that doesn't match piece {:?}", piece.name);
                }
            }
            b"TUBE" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("TUBE before any piece".into()))?;
                match decode_tubes(&raw) {
                    Ok(t) => piece.tubes = t,
                    Err(e) => log::warn!("ignoring damaged tube recipes on piece {:?}: {e}", piece.name),
                }
            }
            b"TUBX" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("TUBX before any piece".into()))?;
                match decode_tube_shapes(&raw) {
                    Ok(shapes) if shapes.len() == piece.tubes.len() => {
                        for (t, (shape, triangles)) in piece.tubes.iter_mut().zip(shapes) {
                            if !shape.is_plain() {
                                t.shape = shape;
                                t.triangles = triangles;
                            }
                        }
                    }
                    Ok(_) => log::warn!("ignoring tube shapes that don't match the tubes of piece {:?}", piece.name),
                    Err(e) => log::warn!("ignoring damaged tube shapes on piece {:?}: {e}", piece.name),
                }
            }
            b"TUBC" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("TUBC before any piece".into()))?;
                match decode_tube_corners(&raw) {
                    Ok(corners)
                        if corners.len() == piece.tubes.len()
                            && corners
                                .iter()
                                .zip(&piece.tubes)
                                .all(|((s, _), t)| s.is_empty() || s.len() == t.points.len()) =>
                    {
                        for (t, (sharp, triangles)) in piece.tubes.iter_mut().zip(corners) {
                            if sharp.iter().any(|&s| s) {
                                t.sharp = sharp;
                                t.triangles = triangles;
                            }
                        }
                    }
                    Ok(_) => log::warn!("ignoring tube corners that don't match the tubes of piece {:?}", piece.name),
                    Err(e) => log::warn!("ignoring damaged tube corners on piece {:?}: {e}", piece.name),
                }
            }
            b"TUBP" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("TUBP before any piece".into()))?;
                match decode_tube_profiles(&raw) {
                    Ok(list) if list.len() == piece.tubes.len() => {
                        for (t, (profile, triangles)) in piece.tubes.iter_mut().zip(list) {
                            if profile.is_some() {
                                t.profile = profile;
                                t.triangles = triangles;
                            }
                        }
                    }
                    Ok(_) => log::warn!("ignoring tube profiles that don't match the tubes of piece {:?}", piece.name),
                    Err(e) => log::warn!("ignoring damaged tube profiles on piece {:?}: {e}", piece.name),
                }
            }
            b"TPRC" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("TPRC before any piece".into()))?;
                // Same layout as TUBC, with the profile's points.
                match decode_tube_corners(&raw) {
                    Ok(list)
                        if list.len() == piece.tubes.len()
                            && list.iter().zip(&piece.tubes).all(|((s, _), t)| {
                                s.is_empty() || t.profile.as_ref().is_some_and(|p| p.points.len() == s.len())
                            }) =>
                    {
                        for (t, (sharp, triangles)) in piece.tubes.iter_mut().zip(list) {
                            if let Some(p) = &mut t.profile
                                && p.smooth
                                && sharp.iter().any(|&s| s)
                            {
                                p.sharp = sharp;
                                t.triangles = triangles;
                            }
                        }
                    }
                    Ok(_) => {
                        log::warn!("ignoring profile corners that don't match the tubes of piece {:?}", piece.name)
                    }
                    Err(e) => log::warn!("ignoring damaged profile corners on piece {:?}: {e}", piece.name),
                }
            }
            b"CURV" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("CURV before any piece".into()))?;
                match decode_curve(&raw) {
                    Ok(c) => piece.curve = Some(Box::new(c)),
                    Err(e) => log::warn!("ignoring a damaged Curve Move curve on piece {:?}: {e}", piece.name),
                }
            }
            b"CAGE" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("CAGE before any piece".into()))?;
                match decode_cage(&raw) {
                    Ok(c) if piece.multires.is_none() => piece.cage = Some(c),
                    Ok(_) => log::warn!("ignoring a retopology cage on a piece with subdivision levels"),
                    Err(e) => log::warn!("ignoring a damaged retopology cage on piece {:?}: {e}", piece.name),
                }
            }
            b"PLYC" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("PLYC before any piece".into()))?;
                match piece.cage.as_mut() {
                    Some(cage) => match decode_poly_faces(&raw, cage.positions.len()) {
                        Ok(faces) => cage.faces = faces,
                        Err(e) => log::warn!("ignoring damaged cage polygons on piece {:?}: {e}", piece.name),
                    },
                    None => log::warn!("ignoring cage polygons without a cage"),
                }
            }
            b"PINS" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("PINS before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                let pins = r.u32().and_then(|n| {
                    r.check(n as usize, 4)?;
                    (0..n).map(|_| r.u32()).collect::<Result<Vec<u32>>>()
                });
                match (pins, piece.cage.as_mut()) {
                    (Ok(pins), Some(cage)) => pins.into_iter().for_each(|v| cage.set_pinned(v, true)),
                    (Ok(_), None) => log::warn!("ignoring pinned points without a retopology cage"),
                    (Err(e), _) => log::warn!("ignoring damaged pinned points on piece {:?}: {e}", piece.name),
                }
            }
            b"CRSE" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("CRSE before any piece".into()))?;
                match piece.cage.as_mut() {
                    Some(cage) => match decode_creases(&raw, cage.positions.len()) {
                        Ok(list) => list.into_iter().for_each(|([a, b], w)| cage.set_crease(a, b, w)),
                        Err(e) => log::warn!("ignoring damaged creases on piece {:?}: {e}", piece.name),
                    },
                    None => log::warn!("ignoring creases without a cage"),
                }
            }
            b"MATL" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("MATL before any piece".into()))?;
                match String::from_utf8(raw.clone()) {
                    Ok(m) if !m.is_empty() && m.len() <= 512 => piece.material = Some(m),
                    _ => log::warn!("ignoring a damaged material on piece {:?}", piece.name),
                }
            }
            b"INST" | b"INS2" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("INST before any piece".into()))?;
                match decode_instancers(&raw, &tag == b"INS2") {
                    Ok(list) => piece.instancers = list,
                    Err(e) => log::warn!("ignoring damaged surface instances on piece {:?}: {e}", piece.name),
                }
            }
            b"PARN" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("PARN before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                piece.parent = Some(r.u32()?);
            }
            b"ARMS" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("ARMS before any piece".into()))?;
                match decode_armature(&raw) {
                    Ok(Some((2, a))) => piece.spheres = Some(Box::new(a)),
                    Ok(Some((_, a))) => piece.skin_of = Some(Box::new(a)),
                    Ok(None) => log::warn!("ignoring an OxySpheres armature of a newer version on {:?}", piece.name),
                    Err(e) => log::warn!("ignoring a damaged OxySpheres armature on piece {:?}: {e}", piece.name),
                }
            }
            b"ARQD" => {
                if let [level] = raw.as_slice()
                    && let Some(recipe) = scene.pieces.last_mut().and_then(|p| p.spheres.as_deref_mut())
                {
                    recipe.quad_subdivision = Some(*level);
                }
            }
            b"ARML" => {
                if let Some(recipe) = scene.pieces.last_mut().and_then(|p| p.spheres.as_deref_mut()) {
                    let mut r = Reader { data: &raw, pos: 0 };
                    recipe.generated_mesh = r.u64().ok();
                }
            }
            b"LITE" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("LITE before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                piece.own_level = Some(r.u32()?);
            }
            b"LINK" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("LINK before any piece".into()))?;
                let mut r = Reader { data: &raw, pos: 0 };
                let source = r.u32()?;
                let mut cols = [0.0f32; 16];
                for c in &mut cols {
                    *c = r.f32()?;
                }
                let transform = glam::Mat4::from_cols_array(&cols);
                if transform.is_finite() && transform.determinant().abs() > 1e-12 {
                    piece.link = Some((source, transform));
                } else {
                    log::warn!("ignoring a damaged link on piece {:?}", piece.name);
                }
            }
            b"STRK" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("STRK before any piece".into()))?;
                match decode_strokes(&raw, piece.mesh.positions.len()) {
                    Ok(st) => piece.strokes = Some(st),
                    Err(e) => log::warn!("ignoring damaged strokes on piece {:?}: {e}", piece.name),
                }
            }
            b"STRI" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("STRI before any piece".into()))?;
                match (decode_stroke_recipes(&raw), &mut piece.strokes) {
                    (Ok(list), Some(st)) if list.len() == st.strokes.len() => {
                        for (s, recipe) in st.strokes.iter_mut().zip(list) {
                            s.recipe = recipe;
                        }
                    }
                    (Ok(_), _) => {
                        log::warn!("ignoring stroke recipes that don't match the strokes of piece {:?}", piece.name)
                    }
                    (Err(e), _) => log::warn!("ignoring damaged stroke recipes on piece {:?}: {e}", piece.name),
                }
            }
            b"PART" => {
                let piece = scene.pieces.last_mut().ok_or_else(|| Error::Corrupt("PART before any piece".into()))?;
                match (decode_placed_parts(&raw), &mut piece.strokes) {
                    (Ok(parts), Some(st)) => {
                        st.parts = parts;
                    }
                    (Ok(parts), None) => {
                        piece.strokes = Some(crate::SavedStrokes { parts, ..Default::default() });
                    }
                    (Err(e), _) => log::warn!("ignoring damaged placed parts on piece {:?}: {e}", piece.name),
                }
            }
            b"REFI" => match decode_reference(&raw) {
                Ok(reference) if reference.pixels.len() <= crate::MAX_REFERENCE_BYTES - reference_bytes => {
                    reference_bytes += reference.pixels.len();
                    rotation_target = Some(scene.references.len());
                    scene.references.push(reference);
                }
                Ok(_) => log::warn!("skipping reference image beyond the project image budget"),
                Err(e) => log::warn!("skipping reference image: {e}"),
            },
            b"RROT" => {
                if let (Some(index), Ok(bytes)) = (target, <[u8; 4]>::try_from(raw.as_slice())) {
                    let angle = f32::from_le_bytes(bytes);
                    if angle.is_finite() && angle.abs() <= std::f32::consts::PI {
                        scene.references[index].placement.fine_rotation = angle;
                    }
                }
            }
            b"ALPH" => match decode_alpha(&raw) {
                Ok(a) => scene.alphas.push(a),
                Err(e) => log::warn!("ignoring a damaged brush alpha: {e}"),
            },
            _ => log::warn!("skipping unknown section {:?}", String::from_utf8_lossy(&tag)),
        }
    }
    // No `PIEC` sections is an empty scene, not a damaged file.
    Ok(scene)
}

/// `STRK`: a piece's editable strokes. Point numbers are the preceding
/// `PIEC`'s, sorted within each stroke.
///
/// ```text
/// vcount:u32                         the piece's point count
/// groups:u32  then per group:  id:u32  name_len:u32  name:utf8  strength:f32  visible:u8
/// strokes:u32 then per stroke (oldest first):
///   brush_len:u32  brush:utf8        settings key, e.g. "draw"
///   made:u64                         seconds since 1970, UTC
///   strength:f32
///   flags:u8                         1 = in a group, 2 = has color changes
///   group:u32                        group id (0 when not in a group)
///   n:u32  vertex:u32*n  delta:f32*3n  [color:f32*3n]
/// ```
fn encode_strokes(st: &crate::SavedStrokes) -> Vec<u8> {
    let mut b = Vec::new();
    let u = |b: &mut Vec<u8>, x: u32| b.extend_from_slice(&x.to_le_bytes());
    let f = |b: &mut Vec<u8>, x: f32| b.extend_from_slice(&x.to_le_bytes());
    u(&mut b, st.vertices);
    u(&mut b, st.groups.len() as u32);
    for g in &st.groups {
        u(&mut b, g.id);
        u(&mut b, g.name.len() as u32);
        b.extend_from_slice(g.name.as_bytes());
        f(&mut b, g.strength);
        b.push(u8::from(g.visible));
    }
    u(&mut b, st.strokes.len() as u32);
    for s in &st.strokes {
        u(&mut b, s.brush.len() as u32);
        b.extend_from_slice(s.brush.as_bytes());
        b.extend_from_slice(&s.made.to_le_bytes());
        f(&mut b, s.strength);
        let colored = !s.colors.is_empty();
        b.push(u8::from(s.group.is_some()) | (u8::from(colored) << 1));
        u(&mut b, s.group.unwrap_or(0));
        u(&mut b, s.vertices.len() as u32);
        for &v in &s.vertices {
            u(&mut b, v);
        }
        for d in &s.deltas {
            d.to_array().into_iter().for_each(|x| f(&mut b, x));
        }
        if colored {
            for c in &s.colors {
                c.to_array().into_iter().for_each(|x| f(&mut b, x));
            }
        }
    }
    b
}

fn encode_stroke_recipes(st: &crate::SavedStrokes) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&(st.strokes.len() as u32).to_le_bytes());
    for s in &st.strokes {
        b.extend_from_slice(&(s.recipe.len() as u32).to_le_bytes());
        b.extend_from_slice(&s.recipe);
    }
    b
}

/// RETURNS: per stroke, its recipe's bytes (empty: none).
fn decode_stroke_recipes(raw: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut r = Reader { data: raw, pos: 0 };
    let count = r.u32()? as usize;
    r.check(count, 4)?;
    (0..count)
        .map(|_| {
            let n = r.u32()? as usize;
            Ok(r.take(n)?.to_vec())
        })
        .collect()
}

/// `PART`: placed recipes from the Recipe Box (RFC 0026) on the preceding
/// `PIEC`, written after its `STRK`/`STRI`.
///
/// ```text
/// version:u8                       2 (1: stroke ids were session ids)
/// count:u32 then per placement:
///   id:u64  name_len:u32 name:utf8  group:u32
///   n:u32  stroke:u64*n            version 2: indices into this piece's STRK list
///   anchor:f32*3 normal:f32*3 tangent:f32*3  scale:f32  detail_size:f32
///   content:u8*32                  SHA-256 of the library file at the drop
///   flags:u8                       1 linked, 2 source side, 4 mirrored frame
///   mirror_axis:u8                 0 none, 1 X, 2 Y, 3 Z
///   partner:u64                    the other side's id, 0 for none
/// ```
/// A reader skips a version it doesn't know, keeping the strokes.
fn encode_placed_parts(parts: &[crate::SavedPlacedPart]) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(2u8);
    b.extend_from_slice(&(parts.len() as u32).to_le_bytes());
    for p in parts {
        b.extend_from_slice(&p.id.to_le_bytes());
        let name_bytes = p.name.as_bytes();
        b.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        b.extend_from_slice(name_bytes);
        b.extend_from_slice(&p.group.to_le_bytes());
        b.extend_from_slice(&(p.stroke_ids.len() as u32).to_le_bytes());
        for &sid in &p.stroke_ids {
            b.extend_from_slice(&sid.to_le_bytes());
        }
        for v in [p.anchor, p.normal, p.tangent] {
            for x in v.to_array() {
                b.extend_from_slice(&x.to_le_bytes());
            }
        }
        b.extend_from_slice(&p.scale.to_le_bytes());
        b.extend_from_slice(&p.detail_size.to_le_bytes());
        b.extend_from_slice(&p.content);
        b.push(p.flags);
        b.push(p.mirror_axis);
        b.extend_from_slice(&p.partner.to_le_bytes());
    }
    b
}

fn decode_placed_parts(raw: &[u8]) -> Result<Vec<crate::SavedPlacedPart>> {
    let mut r = Reader { data: raw, pos: 0 };
    let version = r.u8()?;
    if version != 1 && version != 2 {
        return Ok(Vec::new());
    }
    let count = r.u32()? as usize;
    let mut parts = Vec::with_capacity(count);
    for _ in 0..count {
        let id = r.u64()?;
        let name_len = r.u32()? as usize;
        let name = String::from_utf8(r.take(name_len)?.to_vec())
            .map_err(|_| Error::Corrupt("part name is not UTF-8".into()))?;
        let group = r.u32()?;
        let n = r.u32()? as usize;
        let mut stroke_ids = Vec::with_capacity(n);
        for _ in 0..n {
            stroke_ids.push(r.u64()?);
        }
        let anchor = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
        let normal = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
        let tangent = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
        let scale = r.f32()?;
        let detail_size = r.f32()?;
        let content: [u8; 32] = r.take(32)?.try_into().map_err(|_| Error::Corrupt("part content hash short".into()))?;
        let flags = r.u8()?;
        let mirror_axis = r.u8()?;
        let partner = r.u64()?;
        parts.push(crate::SavedPlacedPart {
            id,
            name,
            group,
            stroke_ids,
            anchor,
            normal,
            tangent,
            scale,
            detail_size,
            content,
            flags,
            mirror_axis,
            partner,
            legacy_ids: version == 1,
        });
    }
    Ok(parts)
}

fn decode_strokes(raw: &[u8], vertex_count: usize) -> Result<crate::SavedStrokes> {
    let mut r = Reader { data: raw, pos: 0 };
    let vertices = r.u32()?;
    if vertices as usize != vertex_count {
        return Err(Error::Corrupt("strokes saved for a different point count".into()));
    }
    let text = |r: &mut Reader| -> Result<String> {
        let n = r.u32()? as usize;
        String::from_utf8(r.take(n)?.to_vec()).map_err(|_| Error::Corrupt("text is not UTF-8".into()))
    };
    let ng = r.u32()? as usize;
    r.check(ng, 13)?;
    let mut groups = Vec::with_capacity(ng);
    for _ in 0..ng {
        let id = r.u32()?;
        let name = text(&mut r)?;
        let strength = r.f32()?;
        let visible = r.u8()? != 0;
        if !strength.is_finite() {
            return Err(Error::Corrupt("stroke group strength is not a number".into()));
        }
        groups.push(crate::SavedStrokeGroup { id, name, strength, visible });
    }
    let ns = r.u32()? as usize;
    r.check(ns, 25)?;
    let mut strokes = Vec::with_capacity(ns);
    let vec3 = |r: &mut Reader| -> Result<Vec3> {
        let v = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
        if v.is_finite() { Ok(v) } else { Err(Error::Corrupt("stroke change is not a number".into())) }
    };
    for _ in 0..ns {
        let brush = text(&mut r)?;
        let made = r.u64()?;
        let strength = r.f32()?;
        let flags = r.u8()?;
        let group = r.u32()?;
        let n = r.u32()? as usize;
        r.check(n, 16)?;
        let vs: Vec<u32> = (0..n).map(|_| r.u32()).collect::<Result<_>>()?;
        if vs.iter().any(|&v| v as usize >= vertex_count) || vs.windows(2).any(|w| w[0] >= w[1]) {
            return Err(Error::Corrupt("stroke refers to a missing point".into()));
        }
        let deltas: Vec<Vec3> = (0..n).map(|_| vec3(&mut r)).collect::<Result<_>>()?;
        let colors: Vec<Vec3> = if flags & 2 != 0 {
            r.check(n, 12)?;
            (0..n).map(|_| vec3(&mut r)).collect::<Result<_>>()?
        } else {
            Vec::new()
        };
        if !strength.is_finite() {
            return Err(Error::Corrupt("stroke strength is not a number".into()));
        }
        let group = (flags & 1 != 0).then_some(group).filter(|g| groups.iter().any(|x| x.id == *g));
        strokes.push(crate::SavedStroke {
            brush,
            made,
            strength,
            group,
            vertices: vs,
            deltas,
            colors,
            recipe: Vec::new(),
        });
    }
    Ok(crate::SavedStrokes { vertices, groups, strokes, parts: Vec::new() })
}

fn encode_curve(c: &crate::SavedCurve) -> Vec<u8> {
    let mut b = Vec::new();
    let f = |b: &mut Vec<u8>, x: f32| b.extend_from_slice(&x.to_le_bytes());
    let u = |b: &mut Vec<u8>, x: u32| b.extend_from_slice(&x.to_le_bytes());
    u(&mut b, c.points.len() as u32);
    for list in [&c.points, &c.rest] {
        for (p, n) in list {
            for x in p.to_array().into_iter().chain(n.to_array()) {
                f(&mut b, x);
            }
        }
    }
    for &(r, s) in &c.weights {
        f(&mut b, r);
        f(&mut b, s);
    }
    b.push(c.closed as u8);
    f(&mut b, c.radius_pt);
    f(&mut b, c.world_per_pt);
    f(&mut b, c.strength);
    b.push(c.falloff);
    f(&mut b, c.pressure);
    u(&mut b, c.vertices);
    u(&mut b, c.triangles);
    b
}

fn decode_reference(raw: &[u8]) -> Result<crate::ReferenceImage> {
    let invalid = || Error::Corrupt("invalid reference image".into());
    let mut r = Reader { data: raw, pos: 0 };
    if r.u8()? != 1 {
        return Err(invalid());
    }
    let axis = *crate::ReferenceAxis::ALL.get(r.u8()? as usize).ok_or_else(invalid)?;
    let flags = r.u8()?;
    let name_len = r.u32()? as usize;
    if name_len > 1024 {
        return Err(invalid());
    }
    let name = String::from_utf8_lossy(r.take(name_len)?).into_owned();
    let size = [r.u32()?, r.u32()?];
    let center = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
    let width = r.f32()?;
    let opacity = r.f32()?;
    let len = crate::ReferenceImage::pixel_bytes(size).ok_or_else(invalid)?;
    if r.remaining() != len
        || !center.is_finite()
        || !width.is_finite()
        || width <= 0.0
        || !opacity.is_finite()
        || !(0.0..=1.0).contains(&opacity)
    {
        return Err(invalid());
    }
    Ok(crate::ReferenceImage {
        size,
        pixels: r.take(len)?.into(),
        placement: crate::ReferencePlacement {
            name,
            axis,
            center,
            width,
            opacity,
            visible: flags & 1 != 0,
            all_views: flags & 2 != 0,
            overlay: flags & 4 != 0,
            quarter_turns: (flags >> 3) & 3,
            fine_rotation: 0.0,
            flip_horizontal: flags & 32 != 0,
            flip_vertical: flags & 64 != 0,
        },
    })
}

fn decode_alpha(raw: &[u8]) -> Result<crate::AlphaData> {
    let mut r = Reader { data: raw, pos: 0 };
    let name_len = r.u32()? as usize;
    let name = String::from_utf8_lossy(r.take(name_len)?).into_owned();
    let size = r.u32()?;
    let n = (size as usize).checked_mul(size as usize).ok_or_else(truncated)?;
    if size == 0 {
        return Err(Error::Corrupt("empty alpha".into()));
    }
    r.check(n, 2)?;
    let data =
        (0..n).map(|_| Ok(u16::from_le_bytes(r.take(2)?.try_into().expect("2 bytes")))).collect::<Result<_>>()?;
    Ok(crate::AlphaData { name, size, data })
}

fn decode_curve(raw: &[u8]) -> Result<crate::SavedCurve> {
    let mut r = Reader { data: raw, pos: 0 };
    let n = r.u32()? as usize;
    r.check(n, 56)?;
    let pairs = |r: &mut Reader| -> Result<Vec<(Vec3, Vec3)>> {
        (0..n)
            .map(|_| {
                let p = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
                let q = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
                Ok((p, q))
            })
            .collect()
    };
    let points = pairs(&mut r)?;
    let rest = pairs(&mut r)?;
    let weights: Vec<(f32, f32)> = (0..n).map(|_| Ok((r.f32()?, r.f32()?))).collect::<Result<_>>()?;
    let c = crate::SavedCurve {
        points,
        rest,
        weights,
        closed: r.u8()? != 0,
        radius_pt: r.f32()?,
        world_per_pt: r.f32()?,
        strength: r.f32()?,
        falloff: r.u8()?,
        pressure: r.f32()?,
        vertices: r.u32()?,
        triangles: r.u32()?,
    };
    let finite = |v: &[(Vec3, Vec3)]| v.iter().all(|(p, q)| p.is_finite() && q.is_finite());
    let ok = n >= 2
        && finite(&c.points)
        && finite(&c.rest)
        && c.weights.iter().all(|(a, b)| a.is_finite() && b.is_finite())
        && [c.radius_pt, c.world_per_pt, c.strength, c.pressure].iter().all(|x| x.is_finite() && *x >= 0.0)
        && c.radius_pt > 0.0
        && c.world_per_pt > 0.0;
    if !ok {
        return Err(Error::Corrupt("bad Curve Move curve".into()));
    }
    Ok(c)
}

fn encode_tubes(tubes: &[crate::TubeRecipe]) -> Vec<u8> {
    let mut b = Vec::new();
    let f = |b: &mut Vec<u8>, x: f32| b.extend_from_slice(&x.to_le_bytes());
    let u = |b: &mut Vec<u8>, x: u32| b.extend_from_slice(&x.to_le_bytes());
    u(&mut b, tubes.len() as u32);
    for t in tubes {
        u(&mut b, t.points.len() as u32);
        for p in &t.points {
            for x in p.to_array() {
                f(&mut b, x);
            }
        }
        for &r in &t.radii {
            f(&mut b, r);
        }
        u(&mut b, t.sides);
        b.push(t.caps as u8);
        b.push(t.mirror as u8);
        f(&mut b, t.mirror_x);
        u(&mut b, t.group);
        // Shaped tubes: the real count is in TUBX; tubes with corners: in
        // TUBC (see the header).
        u(&mut b, t.triangles_in(0));
        for x in t.fingerprint.to_array() {
            f(&mut b, x);
        }
    }
    b
}

fn decode_tubes(raw: &[u8]) -> Result<Vec<crate::TubeRecipe>> {
    let mut r = Reader { data: raw, pos: 0 };
    let count = r.u32()? as usize;
    r.check(count, 30)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let n = r.u32()? as usize;
        r.check(n, 16)?;
        let points: Vec<Vec3> = (0..n).map(|_| Ok(Vec3::new(r.f32()?, r.f32()?, r.f32()?))).collect::<Result<_>>()?;
        let radii: Vec<f32> = (0..n).map(|_| r.f32()).collect::<Result<_>>()?;
        let sides = r.u32()?;
        let caps = r.u8()? != 0;
        let mirror = r.u8()? != 0;
        let mirror_x = r.f32()?;
        let group = r.u32()?;
        let triangles = r.u32()?;
        let fingerprint = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
        let ok = n >= 2
            && (3..=256).contains(&sides)
            && points.iter().all(|p| p.is_finite())
            && radii.iter().all(|x| x.is_finite() && *x > 0.0)
            && mirror_x.is_finite();
        if !ok {
            return Err(Error::Corrupt("invalid tube recipe".into()));
        }
        out.push(crate::TubeRecipe {
            points,
            radii,
            sides,
            caps,
            mirror,
            mirror_x,
            shape: Default::default(),
            sharp: Vec::new(),
            profile: None,
            group,
            triangles,
            fingerprint,
        });
    }
    Ok(out)
}

fn encode_tube_shapes(tubes: &[crate::TubeRecipe]) -> Vec<u8> {
    let mut b = Vec::with_capacity(4 + tubes.len() * 25);
    b.extend_from_slice(&(tubes.len() as u32).to_le_bytes());
    for t in tubes {
        let s = &t.shape;
        b.push(s.braid as u8);
        for x in [s.twist, s.coil_size, s.coil_turns, s.spread, s.braid_turns] {
            b.extend_from_slice(&x.to_le_bytes());
        }
        // The real count is only in the newest section the tube needs.
        b.extend_from_slice(&t.triangles_in(1).to_le_bytes());
    }
    b
}

fn encode_tube_corners(tubes: &[crate::TubeRecipe]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&(tubes.len() as u32).to_le_bytes());
    for t in tubes {
        let sharp: &[bool] = if t.has_corners() { &t.sharp } else { &[] };
        b.extend_from_slice(&(sharp.len() as u32).to_le_bytes());
        b.extend(sharp.iter().map(|&s| s as u8));
        b.extend_from_slice(&t.triangles_in(2).to_le_bytes());
    }
    b
}

fn encode_tube_profiles(tubes: &[crate::TubeRecipe]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&(tubes.len() as u32).to_le_bytes());
    for t in tubes {
        match &t.profile {
            Some(p) => {
                b.extend_from_slice(&(p.points.len() as u32).to_le_bytes());
                b.push(p.smooth as u8);
                for q in &p.points {
                    b.extend_from_slice(&q.x.to_le_bytes());
                    b.extend_from_slice(&q.y.to_le_bytes());
                }
            }
            None => {
                b.extend_from_slice(&0u32.to_le_bytes());
                b.push(0);
            }
        }
        b.extend_from_slice(&t.triangles_in(3).to_le_bytes());
    }
    b
}

fn encode_profile_corners(tubes: &[crate::TubeRecipe]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&(tubes.len() as u32).to_le_bytes());
    for t in tubes {
        let sharp: Vec<bool> = match &t.profile {
            Some(p) if p.has_corners() => (0..p.points.len()).map(|i| p.is_sharp(i)).collect(),
            _ => Vec::new(),
        };
        b.extend_from_slice(&(sharp.len() as u32).to_le_bytes());
        b.extend(sharp.iter().map(|&s| s as u8));
        b.extend_from_slice(&t.triangles_in(4).to_le_bytes());
    }
    b
}

/// Most points a profile may have (the Tube window caps it lower).
const MAX_PROFILE_POINTS: usize = 256;

/// RETURNS: per tube, its profile (None: round) and real triangle count.
fn decode_tube_profiles(raw: &[u8]) -> Result<Vec<(Option<oxy_mesh::primitives::TubeProfile>, u32)>> {
    let mut r = Reader { data: raw, pos: 0 };
    let count = r.u32()? as usize;
    r.check(count, 9)?;
    (0..count)
        .map(|_| {
            let n = r.u32()? as usize;
            if n > MAX_PROFILE_POINTS || n == 1 || n == 2 {
                return Err(Error::Corrupt("invalid tube profile".into()));
            }
            let smooth = match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err(Error::Corrupt("invalid tube profile".into())),
            };
            r.check(n, 8)?;
            let points: Vec<glam::Vec2> =
                (0..n).map(|_| Ok(glam::Vec2::new(r.f32()?, r.f32()?))).collect::<Result<_>>()?;
            if points.iter().any(|p| !p.is_finite() || p.abs().max_element() > 100.0) {
                return Err(Error::Corrupt("invalid tube profile point".into()));
            }
            let triangles = r.u32()?;
            let profile = (n > 0).then_some(oxy_mesh::primitives::TubeProfile { points, smooth, sharp: Vec::new() });
            Ok((profile, triangles))
        })
        .collect()
}

/// RETURNS: per tube, its corner flags (empty: none) and real triangle count.
fn decode_tube_corners(raw: &[u8]) -> Result<Vec<(Vec<bool>, u32)>> {
    let mut r = Reader { data: raw, pos: 0 };
    let count = r.u32()? as usize;
    r.check(count, 8)?;
    (0..count)
        .map(|_| {
            let n = r.u32()? as usize;
            r.check(n, 1)?;
            let sharp = (0..n)
                .map(|_| match r.u8()? {
                    0 => Ok(false),
                    1 => Ok(true),
                    _ => Err(Error::Corrupt("invalid tube corner flag".into())),
                })
                .collect::<Result<Vec<bool>>>()?;
            Ok((sharp, r.u32()?))
        })
        .collect()
}

fn decode_tube_shapes(raw: &[u8]) -> Result<Vec<(oxy_mesh::primitives::TubeShape, u32)>> {
    let mut r = Reader { data: raw, pos: 0 };
    let count = r.u32()? as usize;
    r.check(count, 25)?;
    (0..count)
        .map(|_| {
            let style = r.u8()?;
            let [twist, coil_size, coil_turns, spread, braid_turns] =
                [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?];
            let triangles = r.u32()?;
            let shape = oxy_mesh::primitives::TubeShape {
                twist,
                coil_size,
                coil_turns,
                braid: style == 1,
                spread,
                braid_turns,
            };
            let ok = style <= 1
                && [twist, coil_size, coil_turns, spread, braid_turns].iter().all(|x| x.is_finite())
                && (0.0..=MAX_TUBE_SHAPE).contains(&coil_size)
                && (0.0..=1.0).contains(&spread)
                && [twist, coil_turns, braid_turns].iter().all(|x| x.abs() <= MAX_TUBE_SHAPE);
            if ok { Ok((shape, triangles)) } else { Err(Error::Corrupt("invalid tube shape".into())) }
        })
        .collect()
}

/// One instancer's bytes in `INST` (see the header).
const INSTANCER_BYTES: usize = 47;

fn encode_instancers(list: &[crate::SavedInstancer]) -> Vec<u8> {
    use oxy_mesh::instances::{Orient, Place, SizeMode};
    let mut b = Vec::with_capacity(4 + list.len() * INSTANCER_BYTES);
    b.extend_from_slice(&(list.len() as u32).to_le_bytes());
    for ins in list {
        let s = &ins.settings;
        b.extend_from_slice(&ins.source.to_le_bytes());
        let (place, group) = match s.place {
            Place::Faces => (0u8, 0u32),
            Place::Points => (1, 0),
            Place::Group(g) => (2, g),
            Place::Mask(threshold) => (3, threshold.to_bits()),
            Place::Scatter(spacing) => (4, spacing.to_bits()),
        };
        b.push(place);
        b.extend_from_slice(&group.to_le_bytes());
        b.push(match s.orient {
            Orient::World => 0,
            Orient::LongestEdge => 1,
            Orient::FirstEdge => 2,
            Orient::Random => 3,
        });
        for x in s.direction.to_array() {
            b.extend_from_slice(&x.to_le_bytes());
        }
        b.push(match s.size_mode {
            SizeMode::Absolute => 0,
            SizeMode::Face => 1,
        });
        for x in [s.size, s.offset, s.spin, s.vary_size, s.vary_spin] {
            b.extend_from_slice(&x.to_le_bytes());
        }
        b.extend_from_slice(&s.seed.to_le_bytes());
    }
    b
}

fn decode_instancers(raw: &[u8], extended: bool) -> Result<Vec<crate::SavedInstancer>> {
    use oxy_mesh::instances::{Orient, Place, Settings, SizeMode};
    let mut r = Reader { data: raw, pos: 0 };
    let count = r.u32()? as usize;
    r.check(count, INSTANCER_BYTES)?;
    (0..count)
        .map(|_| {
            let source = r.u32()?;
            let place = match (r.u8()?, r.u32()?) {
                (0, _) => Place::Faces,
                (1, _) => Place::Points,
                (2, g) => Place::Group(g),
                (3, bits) if extended && (0.01..=1.0).contains(&f32::from_bits(bits)) => {
                    Place::Mask(f32::from_bits(bits))
                }
                (4, bits) if extended && f32::from_bits(bits).is_finite() && f32::from_bits(bits) > 0.0 => {
                    Place::Scatter(f32::from_bits(bits))
                }
                _ => return Err(Error::Corrupt("unknown or invalid instance placement".into())),
            };
            let orient = match r.u8()? {
                0 => Orient::World,
                1 => Orient::LongestEdge,
                2 => Orient::FirstEdge,
                3 => Orient::Random,
                _ => return Err(Error::Corrupt("unknown instance orientation".into())),
            };
            let direction = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
            let size_mode = match r.u8()? {
                0 => SizeMode::Absolute,
                1 => SizeMode::Face,
                _ => return Err(Error::Corrupt("unknown instance size".into())),
            };
            let [size, offset, spin, vary_size, vary_spin] = [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?];
            let seed = r.u32()?;
            let finite =
                direction.is_finite() && [size, offset, spin, vary_size, vary_spin].iter().all(|x| x.is_finite());
            if !finite || size <= 0.0 {
                return Err(Error::Corrupt("invalid instance settings".into()));
            }
            let settings = Settings {
                place,
                orient,
                direction,
                size_mode,
                size,
                offset,
                spin,
                vary_size: vary_size.clamp(0.0, 1.0),
                vary_spin: vary_spin.clamp(0.0, 1.0),
                seed,
            };
            Ok(crate::SavedInstancer { source, settings })
        })
        .collect()
}

/// Bound on a tube's twist, coil and braid numbers in a file: far past
/// what the Tube window offers, but keeps a damaged file from asking for a
/// mesh too fine to build.
const MAX_TUBE_SHAPE: f32 = 1000.0;

fn encode_cage(c: &oxy_mesh::retopo::Cage) -> Vec<u8> {
    let mut b = Vec::with_capacity(8 + c.positions.len() * 12 + c.faces.len() * 16);
    b.extend_from_slice(&(c.positions.len() as u32).to_le_bytes());
    for p in &c.positions {
        for x in p.to_array() {
            b.extend_from_slice(&x.to_le_bytes());
        }
    }
    // Every face as four corners: triangles end in NONE, n-gons are split
    // into triangles (a reader that knows `PLYC` gets them back from it).
    let mut quads: Vec<[u32; 4]> = Vec::with_capacity(c.faces.len());
    for f in &c.faces {
        match f.as_quad() {
            Some(q) => quads.push(q),
            None => quads.extend(
                oxy_mesh::retopo::ear_clip(&c.positions, &f.corners)
                    .into_iter()
                    .map(|[a, b, c]| [a, b, c, oxy_mesh::retopo::NONE]),
            ),
        }
    }
    b.extend_from_slice(&(quads.len() as u32).to_le_bytes());
    for f in &quads {
        for i in f {
            b.extend_from_slice(&i.to_le_bytes());
        }
    }
    b
}

/// `PLYC`: the cage's faces with any number of corners, and their face
/// groups. When present it replaces the faces read from `CAGE` (the
/// positions still come from there).
///
/// ```text
/// count:u32  then per face: corners:u32  index:u32 * corners  group:u32
/// ```
fn encode_poly_faces(c: &oxy_mesh::retopo::Cage) -> Vec<u8> {
    let mut b = Vec::with_capacity(4 + c.faces.iter().map(|f| 8 + f.len() * 4).sum::<usize>());
    b.extend_from_slice(&(c.faces.len() as u32).to_le_bytes());
    for f in &c.faces {
        b.extend_from_slice(&(f.len() as u32).to_le_bytes());
        for i in &f.corners {
            b.extend_from_slice(&i.to_le_bytes());
        }
        b.extend_from_slice(&f.group.to_le_bytes());
    }
    b
}

/// `CRSE`: crease weights of cage edges, for smooth subdivision. Point
/// numbers are the cage's (as in `CAGE`); weights run from 0 (smooth) to 1
/// (sharp). Versions without it read the cage without creases.
///
/// ```text
/// count:u32  then per crease: a:u32  b:u32  weight:f32
/// ```
fn encode_creases(list: &[([u32; 2], f32)]) -> Vec<u8> {
    let mut b = Vec::with_capacity(4 + list.len() * 12);
    b.extend_from_slice(&(list.len() as u32).to_le_bytes());
    for ([x, y], w) in list {
        b.extend_from_slice(&x.to_le_bytes());
        b.extend_from_slice(&y.to_le_bytes());
        b.extend_from_slice(&w.to_le_bytes());
    }
    b
}

fn decode_creases(raw: &[u8], vertex_count: usize) -> Result<Vec<([u32; 2], f32)>> {
    let mut r = Reader { data: raw, pos: 0 };
    let n = r.u32()? as usize;
    r.check(n, 12)?;
    (0..n)
        .map(|_| {
            let (a, b, w) = (r.u32()?, r.u32()?, r.f32()?);
            if a as usize >= vertex_count || b as usize >= vertex_count || !w.is_finite() {
                return Err(Error::Corrupt("crease refers to a missing point".into()));
            }
            Ok(([a, b], w))
        })
        .collect()
}

fn decode_poly_faces(raw: &[u8], vertex_count: usize) -> Result<Vec<oxy_mesh::retopo::Face>> {
    let mut r = Reader { data: raw, pos: 0 };
    let n = r.u32()? as usize;
    r.check(n, 20)?;
    let mut faces = Vec::with_capacity(n);
    for _ in 0..n {
        let k = r.u32()? as usize;
        if k < 3 {
            return Err(Error::Corrupt("cage polygon with fewer than three corners".into()));
        }
        r.check(k, 4)?;
        let corners: Vec<u32> = (0..k).map(|_| r.u32()).collect::<Result<_>>()?;
        if corners.iter().any(|&i| i as usize >= vertex_count) {
            return Err(Error::Corrupt("cage polygon refers to a missing vertex".into()));
        }
        let group = r.u32()?;
        faces.push(oxy_mesh::retopo::Face::with_group(corners, group));
    }
    Ok(faces)
}

fn decode_cage(raw: &[u8]) -> Result<oxy_mesh::retopo::Cage> {
    use oxy_mesh::retopo::NONE;
    let mut r = Reader { data: raw, pos: 0 };
    let n = r.u32()? as usize;
    r.check(n, 12)?;
    let positions: Vec<Vec3> = (0..n).map(|_| Ok(Vec3::new(r.f32()?, r.f32()?, r.f32()?))).collect::<Result<_>>()?;
    let f = r.u32()? as usize;
    r.check(f, 16)?;
    let mut faces = Vec::with_capacity(f);
    for _ in 0..f {
        let face = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        let ok = face[..3].iter().all(|&i| (i as usize) < n) && (face[3] == NONE || (face[3] as usize) < n);
        if !ok {
            return Err(Error::Corrupt("cage face refers to a missing vertex".into()));
        }
        faces.push(oxy_mesh::retopo::Face::new(if face[3] == NONE { face[..3].to_vec() } else { face.to_vec() }));
    }
    if !positions.iter().all(|p| p.is_finite()) {
        return Err(Error::Corrupt("cage has non-finite positions".into()));
    }
    Ok(oxy_mesh::retopo::Cage { positions, faces, ..Default::default() })
}

fn decode_piece(raw: &[u8]) -> Result<PieceData> {
    let mut r = Reader { data: raw, pos: 0 };
    let name_len = r.u32()? as usize;
    let name = String::from_utf8_lossy(r.take(name_len)?).into_owned();
    let visible = r.u8()? != 0;
    let v = r.u32()? as usize;
    let t = r.u32()? as usize;
    r.check(v, 16)?;
    r.check(t, 17)?;
    let mut m = MeshData::default();
    m.positions.reserve(v);
    for _ in 0..v {
        m.positions.push(Vec3::new(r.f32()?, r.f32()?, r.f32()?));
    }
    m.triangles.reserve(t);
    for _ in 0..t {
        let tri = [r.u32()?, r.u32()?, r.u32()?];
        if tri.iter().any(|&i| i as usize >= v) {
            return Err(Error::Corrupt("triangle index out of range".into()));
        }
        m.triangles.push(tri);
    }
    m.mask = (0..v).map(|_| r.f32()).collect::<Result<_>>()?;
    m.groups = (0..t).map(|_| r.u32()).collect::<Result<_>>()?;
    m.hidden = (0..t).map(|_| r.u8().map(|b| b != 0)).collect::<Result<_>>()?;
    Ok(PieceData {
        name,
        visible,
        mesh: m,
        multires: None,
        vertex_map: None,
        symmetry_center: None,
        cage: None,
        tubes: Vec::new(),
        curve: None,
        strokes: None,
        material: None,
        instancers: Vec::new(),
        parent: None,
        link: None,
        own_level: None,
        spheres: None,
        skin_of: None,
        polygons: None,
    })
}

/// The `POLY` section (see the table at the top).
fn encode_polygons(p: &crate::SavedPolygons) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + (p.diagonals.len() + p.ngon_edges.len()) * 8);
    for list in [&p.diagonals, &p.ngon_edges] {
        bytes.extend_from_slice(&(list.len() as u32).to_le_bytes());
        for v in list.iter().flatten() {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    bytes
}

/// Read a `POLY` section for a piece of `points` points.
/// RETURNS: an error when it's cut short or names a missing point.
fn decode_polygons(raw: &[u8], points: usize) -> Result<crate::SavedPolygons> {
    let mut r = Reader { data: raw, pos: 0 };
    let mut list = || -> Result<Vec<[u32; 2]>> {
        let n = r.u32()? as usize;
        r.check(n, 8)?;
        (0..n)
            .map(|_| {
                let e = [r.u32()?, r.u32()?];
                if e.iter().any(|&v| v as usize >= points) {
                    return Err(Error::Corrupt("polygon edge point out of range".into()));
                }
                Ok(e)
            })
            .collect()
    };
    let diagonals = list()?;
    let ngon_edges = list()?;
    Ok(crate::SavedPolygons { diagonals, ngon_edges })
}

fn encode_uvs(uv: &[[oxy_mesh::Vec2; 3]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4 + uv.len() * 24);
    bytes.extend_from_slice(&(uv.len() as u32).to_le_bytes());
    for corner in uv.iter().flatten() {
        bytes.extend_from_slice(&corner.x.to_le_bytes());
        bytes.extend_from_slice(&corner.y.to_le_bytes());
    }
    bytes
}

fn decode_uvs(raw: &[u8], triangles: usize) -> Result<Option<Vec<[oxy_mesh::Vec2; 3]>>> {
    let mut r = Reader { data: raw, pos: 0 };
    let n = r.u32()? as usize;
    if n != triangles {
        return Ok(None);
    }
    (0..n)
        .map(|_| {
            let mut uv = || -> Result<oxy_mesh::Vec2> { Ok(oxy_mesh::Vec2::new(r.f32()?, r.f32()?)) };
            Ok([uv()?, uv()?, uv()?])
        })
        .collect::<Result<_>>()
        .map(Some)
}

fn encode_multires(mr: &Multires) -> Vec<u8> {
    let (tris, levels, active) = mr.parts();
    let mut b = Vec::new();
    b.extend_from_slice(&(levels.len() as u32).to_le_bytes());
    b.extend_from_slice(&(active as u32).to_le_bytes());
    b.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for t in tris {
        for i in t {
            b.extend_from_slice(&i.to_le_bytes());
        }
    }
    for pos in levels {
        b.extend_from_slice(&(pos.len() as u32).to_le_bytes());
        for p in pos {
            for c in p.to_array() {
                b.extend_from_slice(&c.to_le_bytes());
            }
        }
    }
    b
}

/// The polygons that follow the `MRES` layout in `MRCC`.
fn encode_base_polygons(b: &mut Vec<u8>, base: &oxy_mesh::multires::BasePolygons) {
    let u = |b: &mut Vec<u8>, x: u32| b.extend_from_slice(&x.to_le_bytes());
    let faces = base.offsets.len() - 1;
    u(b, faces as u32);
    for f in 0..faces {
        let k = &base.corners[base.offsets[f] as usize..base.offsets[f + 1] as usize];
        u(b, k.len() as u32);
        k.iter().for_each(|&c| u(b, c));
        let t = &base.tris[base.tri_offsets[f] as usize..base.tri_offsets[f + 1] as usize];
        u(b, t.len() as u32);
        t.iter().for_each(|&c| u(b, c));
    }
    u(b, base.creases.len() as u32);
    for ([x, y], w) in base.creases {
        u(b, *x);
        u(b, *y);
        b.extend_from_slice(&w.to_le_bytes());
    }
}

fn decode_multires_cc(raw: &[u8]) -> Result<Multires> {
    let mut r = Reader { data: raw, pos: 0 };
    let (tris, levels, active) = decode_multires_parts(&mut r)?;
    let faces = r.u32()? as usize;
    r.check(faces, 16)?;
    let (mut offsets, mut corners, mut tri_offsets, mut face_tris) = (vec![0u32], Vec::new(), vec![0u32], Vec::new());
    for _ in 0..faces {
        let n = r.u32()? as usize;
        r.check(n, 4)?;
        for _ in 0..n {
            corners.push(r.u32()?);
        }
        offsets.push(corners.len() as u32);
        let m = r.u32()? as usize;
        r.check(m, 4)?;
        for _ in 0..m {
            face_tris.push(r.u32()?);
        }
        tri_offsets.push(face_tris.len() as u32);
    }
    let nc = r.u32()? as usize;
    r.check(nc, 12)?;
    let creases = (0..nc).map(|_| Ok(([r.u32()?, r.u32()?], r.f32()?))).collect::<Result<Vec<_>>>()?;
    let base = oxy_mesh::multires::OwnedBasePolygons { offsets, corners, tri_offsets, tris: face_tris, creases };
    Multires::from_catmull_clark_parts(tris, base, levels, active)
        .ok_or_else(|| Error::Corrupt("quad subdivision levels don't match".into()))
}

/// Base triangles, every level's positions and the active level: the
/// `MRES` layout, which `MRCC` starts with.
type MultiresParts = (Vec<[u32; 3]>, Vec<Vec<Vec3>>, usize);

fn decode_multires(raw: &[u8]) -> Result<Multires> {
    let mut r = Reader { data: raw, pos: 0 };
    let (tris, levels, active) = decode_multires_parts(&mut r)?;
    Multires::from_parts(tris, levels, active).ok_or_else(|| Error::Corrupt("subdivision levels don't match".into()))
}

fn decode_multires_parts(r: &mut Reader) -> Result<MultiresParts> {
    let level_count = r.u32()? as usize;
    let active = r.u32()? as usize;
    if level_count == 0 || level_count > 16 || active >= level_count {
        return Err(Error::Corrupt("bad subdivision level header".into()));
    }
    let t = r.u32()? as usize;
    r.check(t, 12)?;
    let tris = (0..t).map(|_| Ok([r.u32()?, r.u32()?, r.u32()?])).collect::<Result<Vec<_>>>()?;
    let mut levels = Vec::with_capacity(level_count);
    for _ in 0..level_count {
        let v = r.u32()? as usize;
        r.check(v, 12)?;
        levels.push((0..v).map(|_| Ok(Vec3::new(r.f32()?, r.f32()?, r.f32()?))).collect::<Result<Vec<_>>>()?);
    }
    Ok((tris, levels, active))
}

/// Write atomically: temp file in the same directory, flush, then rename.
pub fn save(scene: &SceneData, path: &Path) -> Result<()> {
    write_atomic(&RealFs, path, &encode(scene)).map_err(Error::Io)
}

/// The file operations [`write_atomic`] uses, so tests can make them fail
/// the way network drives do.
trait SaveFs {
    fn create(&self, path: &Path) -> io::Result<std::fs::File> {
        std::fs::File::create(path)
    }
    fn sync(&self, f: &std::fs::File) -> io::Result<()> {
        f.sync_all()
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
}

struct RealFs;
impl SaveFs for RealFs {}

/// Write `bytes` to `path` without ever leaving it half-written: write a
/// temporary file next to it, then rename it over `path` (architecture
/// invariant 6).
///
/// Network drives (SMB in particular, issue #117) refuse some of the steps
/// a local disk allows, so each has a fallback that keeps the guarantee:
/// - a hidden (dot) temporary file may be refused: use a visible name;
/// - the full flush to disk (`F_FULLFSYNC` on macOS) may be unsupported:
///   log it and go on, since the data is written and closing the file
///   hands it to the server;
/// - renaming over an existing file may be refused: move the old file
///   aside first and put it back if the new one can't take its place.
///
/// Afterwards the file must exist with the right size, or it's an error.
fn write_atomic(fs: &impl SaveFs, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    let pid = std::process::id();
    let context = |what: String| move |e: io::Error| io::Error::new(e.kind(), format!("{what}: {e}"));
    let hidden = dir.join(format!(".{file_name}.{pid}.tmp"));
    let visible = dir.join(format!("{file_name}.{pid}.tmp"));
    let (tmp, mut f) = match fs.create(&hidden) {
        Ok(f) => (hidden, f),
        Err(first) => {
            log::warn!("could not create {} ({first}); trying a visible temporary file", hidden.display());
            let f = fs.create(&visible).map_err(context(format!("could not create a file in {}", dir.display())))?;
            (visible, f)
        }
    };
    let cleanup = |e: io::Error| {
        let _ = std::fs::remove_file(&tmp);
        e
    };
    f.write_all(bytes).map_err(context(format!("could not write {}", tmp.display()))).map_err(cleanup)?;
    if let Err(e) = fs.sync(&f) {
        // Not a reason to fail: the bytes are written; this drive just
        // can't promise they're on its disk yet.
        log::warn!("the drive holding {} doesn't support a full flush ({e}); saving without it", dir.display());
    }
    drop(f);
    // On Windows, `rename` replaces an existing file as well.
    if let Err(e) = fs.rename(&tmp, path) {
        if !path.exists() {
            let _ = std::fs::remove_file(&tmp);
            return Err(context(format!("could not move the saved file into place as {}", path.display()))(e));
        }
        log::warn!("could not replace {} directly ({e}); moving the old file aside first", path.display());
        let aside = dir.join(format!("{file_name}.{pid}.old"));
        fs.rename(path, &aside).map_err(context(format!("could not replace {}", path.display()))).map_err(cleanup)?;
        if let Err(e) = fs.rename(&tmp, path) {
            // Put the old file back; it was never modified.
            let _ = fs.rename(&aside, path);
            let _ = std::fs::remove_file(&tmp);
            return Err(context(format!("could not replace {}", path.display()))(e));
        }
        let _ = std::fs::remove_file(&aside);
    }
    match std::fs::metadata(path) {
        Ok(m) if m.len() == bytes.len() as u64 => Ok(()),
        Ok(m) => Err(io::Error::other(format!(
            "{} has {} bytes after saving, not {}; the drive didn't keep the file",
            path.display(),
            m.len(),
            bytes.len()
        ))),
        Err(e) => Err(context(format!("{} isn't there after saving", path.display()))(e)),
    }
}

pub fn load(path: &Path) -> Result<SceneData> {
    decode(&std::fs::read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_mesh::primitives;

    #[test]
    fn project_references_round_trip_without_meshes() {
        use crate::{ReferenceAxis, ReferenceImage, ReferencePlacement};
        let image = ReferenceImage {
            placement: ReferencePlacement {
                name: "Front drawing".into(),
                axis: ReferenceAxis::Front,
                center: Vec3::X,
                width: 2.5,
                opacity: 0.4,
                visible: true,
                all_views: true,
                overlay: true,
                quarter_turns: 3,
                fine_rotation: 0.35,
                flip_horizontal: true,
                flip_vertical: true,
            },
            size: [2, 1],
            pixels: [255, 0, 0, 0, 0, 255, 0, 200].into(),
        };
        let scene = SceneData { references: vec![image], ..Default::default() };
        let decoded = decode(&encode(&scene)).unwrap();
        assert_eq!(decoded, scene);
        assert_eq!(crate::Scene::from_data(decoded).unwrap().references, scene.references);
    }

    #[test]
    fn brush_alphas_round_trip_and_a_damaged_one_is_skipped() {
        let mut scene = sample();
        let n = 4u32;
        scene.alphas = vec![
            crate::AlphaData {
                name: "Capture 1".into(),
                size: n,
                data: (0..n * n).map(|k| (k * 4000) as u16).collect(),
            },
            crate::AlphaData { name: "rivet".into(), size: 2, data: vec![0, 65535, 32768, 1] },
        ];
        let bytes = encode(&scene);
        assert_eq!(bytes.windows(4).filter(|w| w == b"ALPH").count(), 2);
        assert_eq!(decode(&bytes).unwrap(), scene);
        // A section claiming more pixels than it holds is skipped; the rest loads.
        let mut broken = scene.clone();
        broken.alphas[1].size = 3;
        let back = decode(&encode(&broken)).unwrap();
        assert_eq!(back.alphas, scene.alphas[..1]);
        assert_eq!(back.pieces, scene.pieces);
    }

    fn sample() -> SceneData {
        let mut a = primitives::icosphere(3, 1.0).to_data();
        a.mask = (0..a.positions.len()).map(|i| (i % 7) as f32 / 7.0).collect();
        a.groups = (0..a.triangles.len() as u32).map(|i| i % 3).collect();
        a.hidden = (0..a.triangles.len()).map(|i| i % 5 == 0).collect();
        a.color = (0..a.positions.len()).map(|i| Vec3::new((i % 3) as f32 / 2.0, 0.25, 1.0)).collect();
        let b = primitives::cube(4, 1.0).to_data();
        SceneData {
            pieces: vec![
                PieceData {
                    name: "Head".into(),
                    visible: true,
                    mesh: a,
                    multires: None,
                    vertex_map: None,
                    symmetry_center: None,
                    cage: None,
                    tubes: Vec::new(),
                    curve: None,
                    strokes: None,
                    material: None,
                    instancers: Vec::new(),
                    parent: None,
                    link: None,
                    own_level: None,
                    spheres: None,
                    skin_of: None,
                    polygons: None,
                },
                PieceData {
                    name: "Böx ✓".into(),
                    visible: false,
                    mesh: b,
                    multires: None,
                    vertex_map: None,
                    symmetry_center: None,
                    cage: None,
                    tubes: Vec::new(),
                    curve: None,
                    strokes: None,
                    material: None,
                    instancers: Vec::new(),
                    parent: None,
                    link: None,
                    own_level: None,
                    spheres: None,
                    skin_of: None,
                    polygons: None,
                },
            ],
            active: 1,
            alphas: Vec::new(),
            references: Vec::new(),
        }
    }

    #[test]
    fn uvs_roundtrip_per_corner_and_are_optional() {
        let mut s = sample();
        assert_eq!(encode(&s).windows(4).filter(|w| w == b"UVCO").count(), 0, "no UVs, no section");
        let m = &mut s.pieces[0].mesh;
        m.uv = (0..m.triangles.len())
            .map(|t| std::array::from_fn(|k| oxy_mesh::Vec2::new(t as f32 + 0.25 * k as f32, 1.5 - k as f32)))
            .collect();
        let bytes = encode(&s);
        assert_eq!(bytes.windows(4).filter(|w| w == b"UVCO").count(), 1);
        let d = decode(&bytes).unwrap();
        assert_eq!(d.pieces[0].mesh.uv, s.pieces[0].mesh.uv);
        assert!(d.pieces[1].mesh.uv.is_empty());
        // UVs that don't cover every triangle aren't written.
        s.pieces[0].mesh.uv.pop();
        assert_eq!(encode(&s).windows(4).filter(|w| w == b"UVCO").count(), 0);
    }

    #[test]
    fn color_is_optional_and_checked() {
        // Unpainted pieces write no VCOL section.
        let s = sample();
        let bytes = encode(&s);
        assert_eq!(bytes.windows(4).filter(|w| w == b"VCOL").count(), 1);
        // A color section that doesn't match its piece is ignored, not fatal.
        let mut bad = s.clone();
        bad.pieces[0].mesh.color.pop();
        let back = decode(&encode(&bad)).unwrap();
        assert!(back.pieces[0].mesh.color.is_empty());
        assert_eq!(back.pieces[0].mesh.positions, s.pieces[0].mesh.positions);
    }

    #[test]
    fn retopology_cage_roundtrips_and_older_readers_skip_it() {
        use crate::Piece;
        use oxy_mesh::retopo::Cage;
        // A 2-quad strip and a loose point.
        let mut cage = Cage::default();
        for y in 0..2 {
            for x in 0..3 {
                cage.add_vertex(Vec3::new(x as f32, y as f32, 0.0));
            }
        }
        cage.add_face(&[0, 1, 4, 3], Vec3::Z).unwrap();
        cage.add_face(&[1, 2, 5, 4], Vec3::Z).unwrap();
        cage.add_vertex(Vec3::new(7.0, 7.0, 7.0));
        let mesh = cage.build_mesh().unwrap();
        let mut piece = Piece::new("Retopo", mesh);
        piece.cage = Some(cage);
        // A brush moved a vertex after the last cage edit: the saved cage has it.
        piece.mesh.positions[0].z = 0.25;
        let data = SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() };
        let bytes = encode(&data);
        assert_eq!(bytes.windows(4).filter(|w| w == b"CAGE").count(), 1);
        let loaded = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        let c = loaded.cage.as_ref().expect("cage loaded and linked");
        assert!(c.is_linked_to(&loaded.mesh));
        assert_eq!(loaded.mesh.quad_count(), 2);
        assert_eq!(c.quad_count(), 2);
        assert_eq!(c.dot_count(), 1);
        assert!(loaded.mesh.positions.iter().any(|p| p.z == 0.25));
        for i in 0..loaded.mesh.vertex_count() {
            assert_eq!(c.positions[i], loaded.mesh.positions[i]);
        }
        // A piece whose topology changed since (e.g. remeshed) saves no cage.
        let mut changed = loaded;
        changed.mesh = primitives::cube(1, 1.0);
        assert!(changed.to_data().cage.is_none());
        // An older reader skips CAGE like any unknown section.
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"CAGE").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        let plain = decode(&old).unwrap().pieces.remove(0).into_piece();
        assert!(plain.cage.is_none());
        assert_eq!(plain.mesh.triangle_count(), 4);
    }

    #[test]
    fn polygons_and_face_groups_roundtrip_and_older_readers_get_triangles() {
        use crate::Piece;
        use oxy_mesh::retopo::{Cage, Face};
        // An L-shaped hexagon (group 4) next to a quad (group 2).
        let mut cage = Cage::default();
        for (x, y) in [(0., 0.), (2., 0.), (2., 1.), (1., 1.), (1., 2.), (0., 2.), (3., 0.), (3., 1.)] {
            cage.add_vertex(Vec3::new(x, y, 0.0));
        }
        cage.faces.push(Face::with_group(vec![0, 1, 2, 3, 4, 5], 4));
        cage.faces.push(Face::with_group(vec![1, 6, 7, 2], 2));
        let mesh = cage.build_mesh().unwrap();
        assert_eq!(mesh.triangle_count(), 6);
        let mut piece = Piece::new("Block", mesh);
        piece.cage = Some(cage);
        let bytes =
            encode(&SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"PLYC").count(), 1);
        let loaded = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        let c = loaded.cage.as_ref().expect("cage loaded and linked");
        assert!(c.is_linked_to(&loaded.mesh));
        assert_eq!(c.ngon_count(), 1);
        let mut groups: Vec<u32> = c.faces.iter().map(|f| f.group).collect();
        groups.sort_unstable();
        assert_eq!(groups, vec![2, 4]);
        // A version without PLYC reads CAGE: the hexagon as triangles, still
        // a usable cage of the same mesh.
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"PLYC").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        let plain = decode(&old).unwrap().pieces.remove(0).into_piece();
        let c = plain.cage.as_ref().expect("still a cage");
        assert!(c.is_linked_to(&plain.mesh));
        assert_eq!((c.ngon_count(), c.triangle_count(), c.quad_count()), (0, 4, 1));
        // A plain quad cage writes no PLYC at all (files stay as they were).
        let mut q = Cage::default();
        for (x, y) in [(0., 0.), (1., 0.), (1., 1.), (0., 1.)] {
            q.add_vertex(Vec3::new(x, y, 0.0));
        }
        q.add_face(&[0, 1, 2, 3], Vec3::Z).unwrap();
        let mut p = Piece::new("Quad", q.build_mesh().unwrap());
        p.cage = Some(q);
        let bytes =
            encode(&SceneData { pieces: vec![p.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"PLYC").count(), 0);
    }

    #[test]
    fn strokes_roundtrip_renumbered_and_older_readers_skip_them() {
        use crate::{SavedStroke, SavedStrokeGroup, SavedStrokes};
        // A mesh as another app would write it: loading reorders its points.
        let data = primitives::icosphere(3, 1.0).to_data();
        let n = data.positions.len();
        let picks: Vec<u32> = vec![0, 5, 17, 300, (n - 1) as u32];
        let stroke = SavedStroke {
            brush: "clay".into(),
            made: 1_700_000_000,
            strength: 0.5,
            group: Some(7),
            vertices: picks.clone(),
            deltas: picks.iter().map(|&v| Vec3::splat(v as f32)).collect(),
            colors: Vec::new(),
            // Any bytes: this crate keeps a recipe as it is.
            recipe: vec![1, 2, 3, 250],
        };
        let painted = SavedStroke {
            brush: "paint".into(),
            group: None,
            colors: picks.iter().map(|&v| Vec3::new(v as f32, 0.0, 1.0)).collect(),
            recipe: Vec::new(),
            ..stroke.clone()
        };
        let group = SavedStrokeGroup { id: 7, name: "Wrinkles".into(), strength: 1.25, visible: false };
        let piece = PieceData {
            name: "S".into(),
            visible: true,
            mesh: data.clone(),
            multires: None,
            vertex_map: None,
            symmetry_center: None,
            cage: None,
            tubes: Vec::new(),
            curve: None,
            strokes: Some(SavedStrokes {
                vertices: n as u32,
                groups: vec![group.clone()],
                strokes: vec![stroke, painted],
                parts: Vec::new(),
            }),
            material: None,
            instancers: Vec::new(),
            parent: None,
            link: None,
            own_level: None,
            spheres: None,
            skin_of: None,
            polygons: None,
        };
        let bytes = encode(&SceneData { pieces: vec![piece], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"STRK").count(), 1);
        let decoded = decode(&bytes).unwrap().pieces.remove(0);
        assert_eq!(decoded.strokes.as_ref().unwrap().groups, vec![group]);
        let loaded = decoded.into_piece();
        let st = loaded.strokes.as_ref().expect("strokes loaded");
        assert_eq!(st.vertices as usize, loaded.mesh.vertex_count());
        assert_eq!(st.strokes.len(), 2);
        assert_eq!(st.strokes[0].group, Some(7));
        assert_eq!(st.strokes[1].group, None);
        // Recipes come back with their strokes (STRI), one of them empty.
        assert_eq!(bytes.windows(4).filter(|w| w == b"STRI").count(), 1);
        assert_eq!(st.strokes[0].recipe, vec![1, 2, 3, 250]);
        assert!(st.strokes[1].recipe.is_empty());
        // A reader without STRI (4.0.0-beta.15) keeps the strokes, without recipes.
        let mut older = bytes.clone();
        let i = older.windows(4).position(|w| w == b"STRI").unwrap();
        older[i..i + 4].copy_from_slice(b"ZZZZ");
        let kept = decode(&older).unwrap().pieces.remove(0).into_piece();
        let kept = kept.strokes.as_ref().expect("strokes still load");
        assert!(kept.strokes.len() == 2 && kept.strokes.iter().all(|s| s.recipe.is_empty()));
        for s in &st.strokes {
            assert!(s.vertices.windows(2).all(|w| w[0] < w[1]), "sorted");
            assert_eq!(s.vertices.len(), picks.len());
            // Each renumbered point is the same point as before, and keeps its change.
            for (j, &v) in s.vertices.iter().enumerate() {
                let old = s.deltas[j].x as usize;
                assert!(loaded.mesh.positions[v as usize].distance(data.positions[old]) < 1e-6);
                if !s.colors.is_empty() {
                    assert_eq!(s.colors[j].x, s.deltas[j].x);
                }
            }
        }
        // Older readers skip the section and keep the surface.
        let mut old = bytes.clone();
        let i = old.windows(4).position(|w| w == b"STRK").unwrap();
        old[i..i + 4].copy_from_slice(b"ZZZZ");
        assert!(decode(&old).unwrap().pieces.remove(0).into_piece().strokes.is_none());
        // Strokes made for another point count are dropped, not trusted.
        let mut bad = decode(&bytes).unwrap().pieces.remove(0);
        bad.strokes.as_mut().unwrap().vertices += 1;
        assert!(bad.into_piece().strokes.is_none());
    }

    #[test]
    fn creases_roundtrip_and_older_readers_skip_them() {
        use crate::Piece;
        use oxy_mesh::retopo::Cage;
        let mut q = Cage::default();
        for (x, y) in [(0., 0.), (1., 0.), (1., 1.), (0., 1.), (2., 0.), (2., 1.)] {
            q.add_vertex(Vec3::new(x, y, 0.0));
        }
        q.add_face(&[0, 1, 2, 3], Vec3::Z).unwrap();
        q.add_face(&[1, 4, 5, 2], Vec3::Z).unwrap();
        q.set_crease(1, 2, 0.75);
        // A crease on an edge that's gone isn't saved.
        q.creases.insert([0, 5], 1.0);
        let mut p = Piece::new("Two", q.build_mesh().unwrap());
        let at = |c: &Cage, x: f32, y: f32| {
            (0..c.positions.len() as u32)
                .find(|&v| c.positions[v as usize].distance(Vec3::new(x, y, 0.0)) < 1e-6)
                .unwrap()
        };
        p.cage = Some(q);
        let bytes =
            encode(&SceneData { pieces: vec![p.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"CRSE").count(), 1);
        let loaded = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        let c = loaded.cage.as_ref().unwrap();
        assert!(c.is_linked_to(&loaded.mesh));
        assert_eq!(c.live_creases().len(), 1);
        assert_eq!(c.crease(at(c, 1.0, 0.0), at(c, 1.0, 1.0)), 0.75);
        let mut old = bytes.clone();
        let i = old.windows(4).position(|w| w == b"CRSE").unwrap();
        old[i..i + 4].copy_from_slice(b"ZZZZ");
        let plain = decode(&old).unwrap().pieces.remove(0).into_piece();
        assert!(plain.cage.unwrap().creases.is_empty());
    }

    #[test]
    fn pinned_cage_points_roundtrip() {
        use crate::Piece;
        use oxy_mesh::retopo::Cage;
        let mut cage = Cage::default();
        for y in 0..2 {
            for x in 0..3 {
                cage.add_vertex(Vec3::new(x as f32, y as f32, 0.0));
            }
        }
        cage.add_face(&[0, 1, 4, 3], Vec3::Z).unwrap();
        cage.add_face(&[1, 2, 5, 4], Vec3::Z).unwrap();
        let loose = cage.add_vertex(Vec3::new(7.0, 7.0, 7.0));
        cage.set_pinned(2, true);
        cage.set_pinned(loose, true);
        let pinned_points = |c: &Cage| -> Vec<Vec3> {
            let mut p: Vec<Vec3> =
                (0..c.positions.len() as u32).filter(|&v| c.is_pinned(v)).map(|v| c.positions[v as usize]).collect();
            p.sort_by(|a, b| a.x.total_cmp(&b.x));
            p
        };
        let mesh = cage.build_mesh().unwrap();
        let expect = pinned_points(&cage);
        assert_eq!(expect, [Vec3::new(2.0, 0.0, 0.0), Vec3::new(7.0, 7.0, 7.0)]);
        let mut piece = Piece::new("Retopo", mesh);
        piece.cage = Some(cage);
        let bytes =
            encode(&SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"PINS").count(), 1);
        let loaded = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        assert_eq!(pinned_points(loaded.cage.as_ref().unwrap()), expect, "the same points, after relinking");
        // An older reader skips PINS: the cage loads with nothing pinned.
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"PINS").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        let plain = decode(&old).unwrap().pieces.remove(0).into_piece();
        assert_eq!(plain.cage.as_ref().unwrap().pinned_count(), 0);
        // Nothing pinned: no section.
        let mut none = loaded;
        none.cage.as_mut().unwrap().pinned.clear();
        let bytes =
            encode(&SceneData { pieces: vec![none.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"PINS").count(), 0);
    }

    /// A drive that refuses some operations, like the SMB share in issue #117.
    #[derive(Default)]
    struct Picky {
        no_hidden_files: bool,
        no_full_flush: bool,
        /// Renaming onto an existing file fails.
        no_replace: bool,
        /// Every rename fails.
        no_rename: bool,
    }

    impl SaveFs for Picky {
        fn create(&self, path: &Path) -> io::Result<std::fs::File> {
            let hidden = path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
            if self.no_hidden_files && hidden {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "vetoed"));
            }
            std::fs::File::create(path)
        }
        fn sync(&self, f: &std::fs::File) -> io::Result<()> {
            if self.no_full_flush {
                return Err(io::Error::new(io::ErrorKind::Unsupported, "F_FULLFSYNC not supported"));
            }
            f.sync_all()
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if self.no_rename || (self.no_replace && to.exists()) {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "refused"));
            }
            std::fs::rename(from, to)
        }
    }

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("oxy-save-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn files_in(d: &Path) -> Vec<String> {
        let mut v: Vec<String> =
            std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn saving_works_on_drives_that_refuse_some_steps() {
        let cases = [
            ("hidden", Picky { no_hidden_files: true, ..Default::default() }),
            ("flush", Picky { no_full_flush: true, ..Default::default() }),
            ("replace", Picky { no_replace: true, ..Default::default() }),
            ("all", Picky { no_hidden_files: true, no_full_flush: true, no_replace: true, no_rename: false }),
        ];
        for (name, fs) in cases {
            let d = scratch_dir(name);
            let path = d.join("head.oxb");
            // A new file, then replacing it.
            write_atomic(&fs, &path, b"first version").unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(std::fs::read(&path).unwrap(), b"first version");
            write_atomic(&fs, &path, b"second, longer version").unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(std::fs::read(&path).unwrap(), b"second, longer version");
            assert_eq!(files_in(&d), vec!["head.oxb".to_string()], "{name}: nothing left behind");
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn a_failed_save_reports_an_error_and_keeps_the_old_file() {
        let d = scratch_dir("fail");
        let path = d.join("head.oxb");
        std::fs::write(&path, b"what was saved before").unwrap();
        let fs = Picky { no_rename: true, ..Default::default() };
        let e = write_atomic(&fs, &path, b"new").unwrap_err();
        assert!(e.to_string().contains("head.oxb"), "the message names the file: {e}");
        assert_eq!(std::fs::read(&path).unwrap(), b"what was saved before", "old file untouched");
        assert_eq!(files_in(&d), vec!["head.oxb".to_string()], "no temporary file left");
        // Saving somewhere new that can't be finished: an error, no stray file.
        let fresh = d.join("new.oxb");
        assert!(write_atomic(&fs, &fresh, b"new").is_err());
        assert!(!fresh.exists());
        assert_eq!(files_in(&d), vec!["head.oxb".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tube_recipes_roundtrip_and_older_readers_skip_them() {
        use crate::{Piece, TubeRecipe, group_fingerprint};
        let points = vec![Vec3::ZERO, Vec3::new(0.5, 1.0, 0.0), Vec3::new(1.0, 2.0, 0.5)];
        let radii = vec![0.2, 0.1, 0.05];
        let mesh = primitives::tube(&points, &radii, 12, true);
        let (triangles, fingerprint) = group_fingerprint(&mesh, 0);
        let mut piece = Piece::new("Tube", mesh);
        let recipe = TubeRecipe {
            points,
            radii,
            sides: 12,
            caps: true,
            mirror: true,
            mirror_x: 0.25,
            shape: Default::default(),
            sharp: Vec::new(),
            profile: None,
            group: 0,
            triangles,
            fingerprint,
        };
        piece.tubes.push(recipe.clone());
        assert_eq!(piece.tube_recipe(), Some(&recipe));
        let bytes =
            encode(&SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"TUBE").count(), 1);
        let loaded = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        assert_eq!(loaded.tubes, vec![recipe]);
        assert!(loaded.tube_recipe().is_some(), "still matches the loaded mesh");
        // Moving a vertex (a brush) makes it an ordinary mesh again.
        let mut sculpted = loaded;
        sculpted.mesh.positions[3].x += 0.1;
        assert!(sculpted.tube_recipe().is_none());
        // An older reader skips TUBE like any unknown section.
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"TUBE").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        assert!(decode(&old).unwrap().pieces[0].tubes.is_empty());
        assert!(!bytes.windows(4).any(|w| w == b"TUBX"), "plain tubes need no TUBX");
    }

    #[test]
    fn tube_shapes_roundtrip_and_older_readers_dont_reopen_them_plain() {
        use crate::{Piece, TubeRecipe, group_fingerprint};
        use oxy_mesh::primitives::TubeShape;
        let points = vec![Vec3::ZERO, Vec3::new(0.0, 2.0, 0.0)];
        let radii = vec![0.3, 0.3];
        let shape =
            TubeShape { twist: 0.5, coil_size: 1.5, coil_turns: 2.0, braid: true, spread: 0.4, braid_turns: 3.0 };
        let mesh = primitives::tube_shaped(&points, &radii, 12, true, shape);
        let (triangles, fingerprint) = group_fingerprint(&mesh, 0);
        let mut piece = Piece::new("Braid", mesh);
        let recipe = TubeRecipe {
            points,
            radii,
            sides: 12,
            caps: true,
            mirror: false,
            mirror_x: 0.0,
            shape,
            sharp: Vec::new(),
            profile: None,
            group: 0,
            triangles,
            fingerprint,
        };
        piece.tubes.push(recipe.clone());
        let bytes =
            encode(&SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"TUBX").count(), 1);
        let loaded = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        assert_eq!(loaded.tubes, vec![recipe]);
        assert!(loaded.tube_recipe().is_some());
        // A reader without TUBX gets the plain recipe, which doesn't match
        // the braid's triangles: the braid stays ordinary triangles.
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"TUBX").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        let piece = decode(&old).unwrap().pieces.remove(0).into_piece();
        assert!(piece.tubes[0].shape.is_plain());
        assert!(piece.tube_recipe().is_none(), "an older reader must not reopen a braid as a plain tube");
        // Damaged numbers are refused rather than built.
        let mut bad = encode_tube_shapes(&[TubeRecipe {
            shape: TubeShape { coil_turns: f32::NAN, ..shape },
            ..piece.tubes[0].clone()
        }]);
        assert!(decode_tube_shapes(&bad).is_err());
        bad.truncate(10);
        assert!(decode_tube_shapes(&bad).is_err());
    }

    #[test]
    fn tube_corners_roundtrip_and_older_readers_dont_reopen_them_rounded() {
        use crate::{Piece, TubeRecipe, group_fingerprint};
        use oxy_mesh::primitives::TubeShape;
        let points = vec![Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0)];
        let radii = vec![0.1; 3];
        let sharp = vec![false, true, false];
        // Twisted as well, so both TUBE and TUBX must hide the real count.
        let shape = TubeShape { twist: 0.5, ..Default::default() };
        let mesh = primitives::tube_cornered(&points, &radii, &sharp, 12, true, shape);
        let (triangles, fingerprint) = group_fingerprint(&mesh, 0);
        let mut piece = Piece::new("Bent", mesh);
        let recipe = TubeRecipe {
            points,
            radii,
            sides: 12,
            caps: true,
            mirror: false,
            mirror_x: 0.0,
            shape,
            sharp,
            profile: None,
            group: 0,
            triangles,
            fingerprint,
        };
        piece.tubes.push(recipe.clone());
        let bytes =
            encode(&SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"TUBC").count(), 1);
        let loaded = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        assert_eq!(loaded.tubes, vec![recipe]);
        assert!(loaded.tube_recipe().is_some());
        // A reader without TUBC (4.0.0-beta.13) has the tube with no corners
        // and a count of 0, so it keeps the tube as ordinary triangles.
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"TUBC").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        let piece = decode(&old).unwrap().pieces.remove(0).into_piece();
        assert!(!piece.tubes[0].has_corners());
        assert!(piece.tube_recipe().is_none(), "an older reader must not reopen it with rounded corners");
        // Damaged flags are refused.
        let mut bad = encode_tube_corners(&[loaded.tubes[0].clone()]);
        bad[8] = 7;
        assert!(decode_tube_corners(&bad).is_err());
        // Tubes without corners write no TUBC.
        let mut plain = loaded.tubes[0].clone();
        plain.sharp = vec![false; 3];
        let mut p2 = Piece::new("Plain", primitives::cube(1, 1.0));
        p2.tubes.push(plain);
        let bytes =
            encode(&SceneData { pieces: vec![p2.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert!(!bytes.windows(4).any(|w| w == b"TUBC"));
    }

    #[test]
    fn tube_profiles_roundtrip_and_older_readers_dont_reopen_them_round() {
        use crate::{Piece, TubeRecipe, group_fingerprint};
        use glam::Vec2;
        use oxy_mesh::primitives::{TubeProfile, TubeShape};
        let points = vec![Vec3::ZERO, Vec3::new(0.0, 2.0, 0.0)];
        let radii = vec![0.3, 0.3];
        let profile = TubeProfile {
            points: vec![Vec2::new(-1.0, -0.5), Vec2::new(1.0, -0.5), Vec2::new(0.0, 1.0)],
            smooth: false,
            sharp: Vec::new(),
        };
        // Twisted too, so TUBE and TUBX must both hide the real count.
        let shape = TubeShape { twist: 0.25, ..Default::default() };
        let mesh = primitives::tube_profiled(&points, &radii, &[], 12, true, shape, Some(&profile));
        let (triangles, fingerprint) = group_fingerprint(&mesh, 0);
        let mut piece = Piece::new("Ridge", mesh);
        let recipe = TubeRecipe {
            points,
            radii,
            sides: 12,
            caps: true,
            mirror: false,
            mirror_x: 0.0,
            shape,
            sharp: Vec::new(),
            profile: Some(profile),
            group: 0,
            triangles,
            fingerprint,
        };
        assert_eq!(recipe.count_section(), 3);
        piece.tubes.push(recipe.clone());
        let bytes =
            encode(&SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"TUBP").count(), 1);
        let loaded = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        assert_eq!(loaded.tubes, vec![recipe]);
        assert!(loaded.tube_recipe().is_some());
        // Without TUBP (4.0.0-beta.13 and the first beta.14 builds) the tube
        // has no profile and a count of 0: it stays ordinary triangles.
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"TUBP").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        let piece = decode(&old).unwrap().pieces.remove(0).into_piece();
        assert!(piece.tubes[0].profile.is_none());
        assert!(piece.tube_recipe().is_none(), "an older reader must not reopen it round");
        // Damaged profiles are refused.
        let mut bad = encode_tube_profiles(&loaded.tubes);
        bad[8] = 9; // the smooth flag
        assert!(decode_tube_profiles(&bad).is_err());
        let mut two = loaded.tubes[0].clone();
        two.profile.as_mut().unwrap().points.truncate(2);
        assert!(decode_tube_profiles(&encode_tube_profiles(&[two])).is_err(), "a profile needs three points");
    }

    #[test]
    fn profile_corners_roundtrip_and_older_readers_dont_reopen_them_smooth() {
        use crate::{Piece, TubeRecipe, group_fingerprint};
        use glam::Vec2;
        use oxy_mesh::primitives::{TubeProfile, TubeShape};
        let points = vec![Vec3::ZERO, Vec3::new(0.0, 2.0, 0.0)];
        let radii = vec![0.3, 0.3];
        let profile = TubeProfile {
            points: vec![Vec2::new(-1.0, -0.4), Vec2::new(1.0, -0.4), Vec2::new(0.6, 0.6), Vec2::new(-0.6, 0.6)],
            smooth: true,
            sharp: vec![true, true, false, false],
        };
        let mesh = primitives::tube_profiled(&points, &radii, &[], 16, true, TubeShape::default(), Some(&profile));
        let (triangles, fingerprint) = group_fingerprint(&mesh, 0);
        let mut piece = Piece::new("D", mesh);
        let recipe = TubeRecipe {
            points,
            radii,
            sides: 16,
            caps: true,
            mirror: false,
            mirror_x: 0.0,
            shape: TubeShape::default(),
            sharp: Vec::new(),
            profile: Some(profile),
            group: 0,
            triangles,
            fingerprint,
        };
        assert_eq!(recipe.count_section(), 4);
        // A second, round tube in the same piece has no entry.
        let mut round = recipe.clone();
        round.profile = None;
        piece.tubes.push(recipe.clone());
        piece.tubes.push(round.clone());
        let bytes =
            encode(&SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert_eq!(bytes.windows(4).filter(|w| w == b"TPRC").count(), 1);
        let loaded = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        assert_eq!(loaded.tubes, vec![recipe, round]);
        assert!(loaded.tube_recipe().is_some());
        // Without TPRC (4.0.0-beta.14) the profile is smooth all round and
        // the count is 0: it stays ordinary triangles.
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"TPRC").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        let piece = decode(&old).unwrap().pieces.remove(0).into_piece();
        assert!(!piece.tubes[0].profile.as_ref().unwrap().has_corners());
        assert_eq!(piece.tubes[0].triangles, 0, "an older reader must not reopen it without its corners");
        // A profile without corners writes no section.
        let mut plain = loaded;
        plain.tubes[0].profile.as_mut().unwrap().sharp.clear();
        let bytes =
            encode(&SceneData { pieces: vec![plain.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() });
        assert!(!bytes.windows(4).any(|w| w == b"TPRC"));
    }

    #[test]
    fn open_curve_roundtrips_and_older_readers_skip_it() {
        use crate::SavedCurve;
        let mut s = sample();
        let mesh = &s.pieces[1].mesh;
        let curve = SavedCurve {
            points: vec![(Vec3::ZERO, Vec3::Y), (Vec3::new(1.0, 0.5, 0.0), Vec3::Z)],
            rest: vec![(Vec3::ZERO, Vec3::Y), (Vec3::X, Vec3::Z)],
            weights: vec![(1.0, 1.0), (1.5, 0.5)],
            closed: false,
            radius_pt: 60.0,
            world_per_pt: 0.004,
            strength: 1.0,
            falloff: 2,
            pressure: 1.0,
            vertices: mesh.positions.len() as u32,
            triangles: mesh.triangles.len() as u32,
        };
        s.pieces[1].curve = Some(Box::new(curve.clone()));
        let bytes = encode(&s);
        assert_eq!(bytes.windows(4).filter(|w| w == b"CURV").count(), 1);
        let d = decode(&bytes).unwrap();
        assert_eq!(d.pieces[0].curve, None);
        assert_eq!(d.pieces[1].curve.as_deref(), Some(&curve));
        let piece = d.pieces.into_iter().nth(1).unwrap().into_piece();
        assert!(piece.curve.as_ref().unwrap().fits(&piece.mesh));
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"CURV").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        assert!(decode(&old).unwrap().pieces.iter().all(|p| p.curve.is_none()));
    }

    #[test]
    fn symmetry_center_roundtrips_per_piece() {
        // Only pieces with their own centre write a SYMC section, and each
        // centre lands on the piece it follows.
        let mut s = sample();
        s.pieces[1].symmetry_center = Some(Vec3::new(0.4, -1.25, 3.0));
        let bytes = encode(&s);
        assert_eq!(bytes.windows(4).filter(|w| w == b"SYMC").count(), 1);
        let d = decode(&bytes).unwrap();
        assert_eq!(d.pieces[0].symmetry_center, None);
        assert_eq!(d.pieces[1].symmetry_center, Some(Vec3::new(0.4, -1.25, 3.0)));
        // A non-finite centre is dropped rather than poisoning every stroke.
        s.pieces[1].symmetry_center = Some(Vec3::new(f32::NAN, 0.0, 0.0));
        assert_eq!(decode(&encode(&s)).unwrap().pieces[1].symmetry_center, None);
    }

    #[test]
    fn roundtrip_is_exact() {
        let s = sample();
        let d = decode(&encode(&s)).unwrap();
        assert_eq!(d, s);
    }

    #[test]
    fn detects_corruption_and_truncation() {
        let mut bytes = encode(&sample());
        let n = bytes.len();
        assert!(decode(&bytes[..n / 2]).is_err());
        bytes[n - 10] ^= 0xff;
        assert!(matches!(decode(&bytes), Err(Error::Corrupt(_))));
        assert!(matches!(decode(b"not a file at all"), Err(Error::NotOxb)));
    }

    #[test]
    fn rejects_newer_versions_and_skips_unknown_sections() {
        let mut bytes = encode(&sample());
        bytes[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(decode(&bytes), Err(Error::UnsupportedVersion(99))));

        // Append an unknown section and bump the count.
        let mut bytes = encode(&sample());
        let count = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) + 1;
        bytes[16..20].copy_from_slice(&count.to_le_bytes());
        let raw = b"future data";
        bytes.extend_from_slice(b"XTRA");
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        let packed = lz4_flex::block::compress(raw);
        bytes.extend_from_slice(&(packed.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&crc32fast::hash(raw).to_le_bytes());
        bytes.extend_from_slice(&packed);
        assert_eq!(decode(&bytes).unwrap(), sample());
    }

    #[test]
    fn multires_roundtrip_and_old_reader_compatibility() {
        use crate::Piece;
        // A piece with two subdivision levels, sculpted at the top, viewed at level 1.
        let base = primitives::icosphere(1, 1.0);
        let mut mr = Multires::new(&base);
        let d = mr.add_level(&base);
        let mut m = oxy_mesh::Mesh::from_data(d);
        mr.bind(&m);
        let d = mr.add_level(&m);
        m = oxy_mesh::Mesh::from_data(d);
        mr.bind(&m);
        m.positions[5] += Vec3::Y * 0.3;
        let d = mr.set_level(&m, 1);
        m = oxy_mesh::Mesh::from_data(d);
        mr.bind(&m);
        let piece = Piece { multires: Some(mr), ..Piece::new("Head", m) };
        let scene = SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() };
        let bytes = encode(&scene);
        let back = decode(&bytes).unwrap();
        assert_eq!(back, scene);
        let restored = back.pieces[0].clone().into_piece();
        let mr = restored.multires.as_ref().unwrap();
        assert_eq!((mr.level_count(), mr.active()), (3, 1));
        assert!(mr.is_bound_to(&restored.mesh));

        // Without the MRES section (as an older reader sees it) the active level loads as a plain mesh.
        let mut plain = scene.clone();
        plain.pieces[0].multires = None;
        let old = decode(&encode(&plain)).unwrap();
        assert_eq!(old.pieces[0].mesh.triangles.len(), mr.triangle_count(1));
    }

    #[test]
    fn level_paint_survives_save_and_reopen_at_a_lower_level() {
        use crate::Piece;
        // Paint at the top of two levels, step down to the base, save.
        let base = primitives::icosphere(1, 1.0);
        let mut mr = Multires::new(&base);
        let mut m = oxy_mesh::Mesh::from_data(mr.add_level(&base));
        mr.bind(&m);
        m = oxy_mesh::Mesh::from_data(mr.add_level(&m));
        mr.bind(&m);
        m.color = m.positions.iter().map(|p| Vec3::new((p.x * 9.0).sin() * 0.5 + 0.5, 0.2, 0.7)).collect();
        m.refresh_all();
        let level_colors = |m: &oxy_mesh::Mesh| {
            let mut out = vec![Vec3::ZERO; m.vertex_count()];
            for (v, &s) in m.source_vertices().iter().enumerate() {
                out[s as usize] = m.color[v];
            }
            out
        };
        let painted = level_colors(&m);
        m = oxy_mesh::Mesh::from_data(mr.set_level(&m, 0));
        mr.bind(&m);
        let scene = SceneData {
            pieces: vec![Piece { multires: Some(mr), ..Piece::new("Head", m) }.to_data()],
            active: 0,
            alphas: Vec::new(),
            references: Vec::new(),
        };
        let bytes = encode(&scene);
        assert_eq!(bytes.windows(4).filter(|w| w == b"MRPT").count(), 1);
        // Reopen and step back up: the fine paint is there.
        let mut piece = decode(&bytes).unwrap().pieces.remove(0).into_piece();
        let mr = piece.multires.as_mut().unwrap();
        let top = oxy_mesh::Mesh::from_data(mr.set_level(&piece.mesh, 2));
        let worst =
            level_colors(&top).iter().zip(&painted).map(|(a, b)| (*a - *b).abs().max_element()).fold(0.0, f32::max);
        assert!(worst < 1e-4, "paint changed by {worst}");
        // An older reader (without MRPT) still opens the project.
        let mut skipped = bytes.clone();
        let at = skipped.windows(4).position(|w| w == b"MRPT").unwrap();
        skipped[at..at + 4].copy_from_slice(b"NOPE");
        assert!(decode(&skipped).is_ok());
    }

    #[test]
    fn quad_levels_roundtrip_in_mrcc_and_old_readers_skip_them() {
        use crate::Piece;
        use oxy_mesh::multires::Scheme;
        let base = primitives::plane(4, 1.0);
        assert_eq!(base.quad_and_triangle_counts(), (16, 0));
        let mut mr = Multires::new_catmull_clark(&base, None);
        let d = mr.add_level(&base);
        let mut m = oxy_mesh::Mesh::from_data(d);
        mr.bind(&m);
        let d = mr.add_level(&m);
        m = oxy_mesh::Mesh::from_data(d);
        mr.bind(&m);
        m.positions[7] += Vec3::Y * 0.2;
        let d = mr.set_level(&m, 1);
        m = oxy_mesh::Mesh::from_data(d);
        mr.bind(&m);
        let piece = Piece { multires: Some(mr), ..Piece::new("Sheet", m) };
        let scene = SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() };
        let bytes = encode(&scene);
        assert_eq!(bytes.windows(4).filter(|w| w == b"MRCC").count(), 1);
        assert_eq!(bytes.windows(4).filter(|w| w == b"MRES").count(), 0);
        let back = decode(&bytes).unwrap();
        assert_eq!(back, scene);
        let restored = back.pieces[0].clone().into_piece();
        let mr = restored.multires.as_ref().unwrap();
        assert_eq!((mr.scheme(), mr.level_count(), mr.active()), (Scheme::CatmullClark, 3, 1));
        assert!(mr.is_bound_to(&restored.mesh));
        assert_eq!(restored.mesh.quad_and_triangle_counts(), (64, 0), "level 1 is all quads");
        // An older reader skips MRCC and gets the active level as a plain mesh.
        let mut old = bytes.clone();
        let i = old.windows(4).position(|w| w == b"MRCC").unwrap();
        old[i..i + 4].copy_from_slice(b"ZZZZ");
        let plain = decode(&old).unwrap();
        assert!(plain.pieces[0].multires.is_none());
        assert_eq!(plain.pieces[0].mesh.triangles.len(), 128);
    }

    #[test]
    fn vertex_map_roundtrips_composes_and_old_readers_skip_it() {
        use crate::Piece;
        // A mesh written by another program, with its vertices in an order
        // OxyBrush will change when it loads it (reversed here).
        let mut theirs = primitives::icosphere(2, 1.0).to_data();
        let n = theirs.positions.len() as u32;
        theirs.positions.reverse();
        for t in &mut theirs.triangles {
            *t = t.map(|v| n - 1 - v);
        }
        let file = SceneData {
            pieces: vec![PieceData {
                name: "OxyCharacter Body".into(),
                visible: true,
                mesh: theirs.clone(),
                multires: None,
                vertex_map: None,
                symmetry_center: None,
                cage: None,
                tubes: Vec::new(),
                curve: None,
                strokes: None,
                material: None,
                instancers: Vec::new(),
                parent: None,
                link: None,
                own_level: None,
                spheres: None,
                skin_of: None,
                polygons: None,
            }],
            active: 0,
            alphas: Vec::new(),
            references: Vec::new(),
        };
        let points_at_theirs = |p: &Piece| {
            let map = p.current_origin().expect("origin while topology is unchanged");
            p.mesh.positions.iter().zip(map).all(|(q, &i)| *q == theirs.positions[i as usize])
        };

        // Loaded: every vertex knows its index in their file.
        let piece = decode(&encode(&file)).unwrap().pieces.remove(0).into_piece();
        assert!(points_at_theirs(&piece));

        // Saved: VMAP is written and round-trips exactly.
        let saved = SceneData { pieces: vec![piece.to_data()], active: 0, alphas: Vec::new(), references: Vec::new() };
        let bytes = encode(&saved);
        assert_eq!(bytes.windows(4).filter(|w| w == b"VMAP").count(), 1);
        let back = decode(&bytes).unwrap();
        assert_eq!(back, saved);
        assert_eq!(back.pieces[0].vertex_map.as_ref().unwrap().len(), n as usize);

        // Loaded and saved again: still points at their original file.
        let again = back.pieces[0].clone().into_piece();
        assert!(points_at_theirs(&again));

        // An older reader doesn't know VMAP and skips it like any unknown
        // section (tags aren't checksummed, so renaming it simulates that).
        let mut old = bytes.clone();
        let at = old.windows(4).position(|w| w == b"VMAP").unwrap();
        old[at..at + 4].copy_from_slice(b"ZZZZ");
        let skipped = decode(&old).unwrap();
        assert_eq!(skipped.pieces[0].vertex_map, None);
        assert_eq!(skipped.pieces[0].mesh, saved.pieces[0].mesh);

        // After a topology change the map no longer applies and isn't written.
        let mut changed = again;
        changed.mesh = oxy_mesh::Mesh::from_data(primitives::icosphere(3, 1.0).to_data());
        assert!(changed.current_origin().is_none());
        assert_eq!(changed.to_data().vertex_map, None);
        // Pieces made in the session have none either.
        assert_eq!(Piece::new("New", primitives::cube(2, 1.0)).to_data().vertex_map, None);
    }

    #[test]
    fn save_and_load_file_atomically() {
        let dir = std::env::temp_dir().join(format!("oxb-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("scene.oxb");
        save(&sample(), &p).unwrap();
        save(&sample(), &p).unwrap(); // overwrite works
        assert_eq!(load(&p).unwrap(), sample());
        let leftovers: Vec<_> =
            std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name() != "scene.oxb").collect();
        assert!(leftovers.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn piece_material_roundtrips_and_older_readers_skip_it() {
        use crate::Piece;
        let mut p = Piece::new("Jade", primitives::icosphere(1, 1.0));
        p.material = Some("preset:jade".into());
        let q = Piece::new("Plain", primitives::icosphere(1, 1.0));
        let bytes = encode(&SceneData {
            pieces: vec![p.to_data(), q.to_data()],
            active: 0,
            alphas: Vec::new(),
            references: Vec::new(),
        });
        assert_eq!(bytes.windows(4).filter(|w| w == b"MATL").count(), 1);
        let back = decode(&bytes).unwrap();
        assert_eq!(back.pieces[0].material.as_deref(), Some("preset:jade"));
        assert_eq!(back.pieces[1].material, None);
        assert_eq!(back.pieces[0].clone().into_piece().material.as_deref(), Some("preset:jade"));
        let mut old = bytes.clone();
        let i = old.windows(4).position(|w| w == b"MATL").unwrap();
        old[i..i + 4].copy_from_slice(b"ZZZZ");
        assert_eq!(decode(&old).unwrap().pieces[0].material, None);
    }

    #[test]
    fn surface_instances_roundtrip_and_older_readers_skip_them() {
        use crate::{Instancer, Piece, Scene};
        use oxy_mesh::instances::{Orient, Place, Settings, SizeMode};
        let mut host = Piece::new("Host", primitives::icosphere(1, 1.0));
        let mut source = Piece::new("Rivet", primitives::icosphere(0, 0.1));
        source.visible = false;
        let settings = Settings {
            place: Place::Group(3),
            orient: Orient::LongestEdge,
            direction: Vec3::new(0.0, 0.6, 0.8),
            size_mode: SizeMode::Absolute,
            size: 0.25,
            offset: -0.1,
            spin: 30.0,
            vary_size: 0.5,
            vary_spin: 1.0,
            seed: 77,
        };
        host.instancers.push(Instancer { source: source.id, settings });
        // The source comes after the host in the list: sources are found by
        // place, forwards or backwards.
        let mut scene = Scene::new(host);
        scene.pieces.push(source);
        let bytes = encode(&scene.to_data());
        assert_eq!(bytes.windows(4).filter(|w| w == b"INST").count(), 1);
        let back = Scene::from_data(decode(&bytes).unwrap()).unwrap();
        let ins = &back.pieces[0].instancers;
        assert_eq!(ins.len(), 1);
        assert_eq!(ins[0].source, back.pieces[1].id);
        assert_eq!(ins[0].settings, settings);
        assert!(!back.pieces[1].visible);
        // An older reader skips INST and keeps both pieces as they are.
        let mut old = bytes.clone();
        let i = old.windows(4).position(|w| w == b"INST").unwrap();
        old[i..i + 4].copy_from_slice(b"ZZZZ");
        let back = Scene::from_data(decode(&old).unwrap()).unwrap();
        assert_eq!(back.pieces.len(), 2);
        assert!(back.pieces[0].instancers.is_empty());
    }

    #[test]
    fn extended_instance_placements_preserve_legacy_modes() {
        use oxy_mesh::instances::{Place, Settings};
        let mut s = sample();
        let places = [Place::Mask(0.65), Place::Group(3), Place::Scatter(0.125)];
        s.pieces[0].instancers = places
            .into_iter()
            .map(|place| crate::SavedInstancer {
                source: 1,
                settings: Settings { place, seed: 42, ..Settings::default() },
            })
            .collect();
        let mut bytes = encode(&s);
        let back = decode(&bytes).unwrap();
        assert_eq!(back.pieces[0].instancers, s.pieces[0].instancers);
        let tag = bytes.windows(4).position(|w| w == b"INS2").unwrap();
        bytes[tag..tag + 4].copy_from_slice(b"ZZZZ");
        let legacy = decode(&bytes).unwrap();
        assert_eq!(legacy.pieces[0].instancers, s.pieces[0].instancers[1..2]);
    }

    #[test]
    fn instances_pointing_at_themselves_or_nowhere_are_dropped() {
        use crate::{Instancer, Piece, Scene};
        let mut host = Piece::new("Host", primitives::icosphere(1, 1.0));
        host.instancers.push(Instancer { source: host.id, settings: Default::default() });
        let scene = Scene::new(host);
        // to_data keeps it (index 0 is found); from_data drops a self-source.
        let back = Scene::from_data(decode(&encode(&scene.to_data())).unwrap()).unwrap();
        assert!(back.pieces[0].instancers.is_empty());
    }

    #[test]
    fn an_empty_scene_saves_and_opens() {
        let scene = SceneData { pieces: Vec::new(), active: 0, alphas: Vec::new(), references: Vec::new() };
        let bytes = encode(&scene);
        let back = decode(&bytes).unwrap();
        assert!(back.pieces.is_empty());
        assert_eq!(back.active, 0);
        // A project that has pieces still reads, including one saved before
        // empty scenes were allowed.
        let with = sample();
        assert_eq!(decode(&encode(&with)).unwrap().pieces.len(), with.pieces.len());
    }

    #[test]
    fn known_polygons_round_trip_in_oxb() {
        // Two quads, each folded too far to be guessed from its shape, in a
        // point order `Mesh::from_data` changes.
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 1.0),
        ];
        let triangles = vec![[0, 1, 2], [0, 2, 3], [1, 4, 5], [1, 5, 2]];
        let mesh = oxy_mesh::Mesh::new(positions, triangles);
        assert_eq!(mesh.quad_count(), 0, "not guessed from the shape");
        let mesh = oxy_mesh::Mesh::from_data(mesh.to_data());
        let at = |v: u32| mesh.source_vertices().iter().position(|&s| s == v).unwrap() as u32;
        let key = |a: u32, b: u32| [at(a).min(at(b)), at(a).max(at(b))];
        mesh.set_quad_diagonals([key(0, 2), key(1, 5)].into());
        mesh.set_ngon_edges(Default::default());
        let scene = crate::Scene::new(crate::Piece::new("Folded", mesh));
        let bytes = encode(&scene.to_data());
        assert_eq!(bytes.windows(4).filter(|w| w == b"POLY").count(), 1);
        let back = crate::Scene::from_data(decode(&bytes).unwrap()).unwrap();
        assert_eq!(back.pieces[0].mesh.quad_and_triangle_counts(), (2, 0));
        // Older readers skip it and guess, finding no quads.
        let mut skipped = bytes.clone();
        let at = skipped.windows(4).position(|w| w == b"POLY").unwrap();
        skipped[at..at + 4].copy_from_slice(b"NOPE");
        let old = crate::Scene::from_data(decode(&skipped).unwrap()).unwrap();
        assert_eq!(old.pieces[0].mesh.quad_count(), 0);
        // A piece whose quads are guessed writes no section.
        let guessed =
            crate::Scene::new(crate::Piece::new("Cube", oxy_mesh::Mesh::from_data(sample().pieces[0].mesh.clone())));
        assert_eq!(encode(&guessed.to_data()).windows(4).filter(|w| w == b"POLY").count(), 0);
        // Points out of range are refused, not trusted.
        assert!(decode_polygons(&[1, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 6).is_err());
    }

    #[test]
    fn parent_piece_index_round_trips_in_oxb() {
        let mut scene = sample();
        scene.pieces[1].parent = Some(0); // piece 1 is child of piece 0
        let bytes = encode(&scene);
        assert_eq!(bytes.windows(4).filter(|w| w == b"PARN").count(), 1);
        let back = decode(&bytes).unwrap();
        assert_eq!(back.pieces[0].parent, None);
        assert_eq!(back.pieces[1].parent, Some(0));

        // Unknown section skip keeps older readers happy:
        let mut skipped = bytes.clone();
        let at = skipped.windows(4).position(|w| w == b"PARN").unwrap();
        skipped[at..at + 4].copy_from_slice(b"NOPE");
        let decoded = decode(&skipped).unwrap();
        assert_eq!(decoded.pieces[1].parent, None);
    }

    #[test]
    fn linked_copies_round_trip_in_oxb_and_older_readers_get_plain_pieces() {
        // #327: piece 1 is a mirrored copy of piece 0.
        let mut scene = sample();
        let mirror = glam::Mat4::from_scale(glam::Vec3::new(-1.0, 1.0, 1.0));
        scene.pieces[1].link = Some((0, mirror));
        let bytes = encode(&scene);
        assert_eq!(bytes.windows(4).filter(|w| w == b"LINK").count(), 1);
        let back = decode(&bytes).unwrap();
        assert_eq!(back.pieces[0].link, None);
        assert_eq!(back.pieces[1].link, Some((0, mirror)));
        let opened = crate::Scene::from_data(back).unwrap();
        let link = opened.pieces[1].link.expect("linked");
        assert_eq!((link.source, link.transform), (opened.pieces[0].id, mirror));

        // An older reader skips it: both open as ordinary pieces.
        let mut skipped = bytes.clone();
        let at = skipped.windows(4).position(|w| w == b"LINK").unwrap();
        skipped[at..at + 4].copy_from_slice(b"NOPE");
        assert_eq!(decode(&skipped).unwrap().pieces[1].link, None);

        // A copy of itself, or a broken transform, isn't trusted.
        scene.pieces[1].link = Some((1, mirror));
        let opened = crate::Scene::from_data(decode(&encode(&scene)).unwrap()).unwrap();
        assert!(opened.pieces[1].link.is_none());
        scene.pieces[1].link = Some((0, glam::Mat4::ZERO));
        assert_eq!(decode(&encode(&scene)).unwrap().pieces[1].link, None);
    }

    #[test]
    fn a_saved_own_level_round_trips_and_older_readers_skip_it() {
        let mut scene = sample();
        scene.pieces[1].own_level = Some(3);
        let bytes = encode(&scene);
        assert_eq!(bytes.windows(4).filter(|w| w == b"LITE").count(), 1);
        let back = decode(&bytes).unwrap();
        assert_eq!((back.pieces[0].own_level, back.pieces[1].own_level), (None, Some(3)));
        assert_eq!(crate::Scene::from_data(back).unwrap().pieces[1].own_level, Some(3));
        let mut skipped = bytes.clone();
        let at = skipped.windows(4).position(|w| w == b"LITE").unwrap();
        skipped[at..at + 4].copy_from_slice(b"NOPE");
        assert_eq!(decode(&skipped).unwrap().pieces[1].own_level, None);
    }

    #[test]
    fn quad_subdivision_is_optional_and_skippable() {
        let arm = oxy_mesh::armature::Armature::new(Vec3::ZERO, 0.5, None);
        let mut recipe = crate::ArmatureRecipe::spheres(arm, 120, 0.3);
        recipe.quad_subdivision = Some(2);
        let mut scene = crate::Scene::from_data(sample()).unwrap();
        scene.pieces.push(crate::Piece::spheres_piece("OxySpheres", recipe.clone()));
        let mut bytes = encode(&scene.to_data());
        assert_eq!(decode(&bytes).unwrap().pieces[2].spheres.as_deref(), Some(&recipe));
        let at = bytes.windows(4).position(|w| w == b"ARQD").unwrap();
        bytes[at..at + 4].copy_from_slice(b"NOPE");
        recipe.quad_subdivision = None;
        assert_eq!(decode(&bytes).unwrap().pieces[2].spheres.as_deref(), Some(&recipe));
    }

    #[test]
    fn an_oxyspheres_piece_round_trips_and_older_readers_skip_it() {
        use oxy_mesh::armature::Armature;
        let mut arm = Armature::new(Vec3::ZERO, 0.5, Some(0.0));
        let a = arm.add_child(0, Vec3::new(1.0, 0.0, 0.0), 0.3);
        arm.add_child(a, Vec3::new(1.5, 0.5, 0.0), 0.2);
        let recipe = crate::ArmatureRecipe::spheres(arm, 120, 0.3);
        let mut scene = crate::Scene::from_data(sample()).unwrap();
        scene.pieces.push(crate::Piece::spheres_piece("OxySpheres", recipe.clone()));
        let bytes = encode(&scene.to_data());
        assert_eq!(bytes.windows(4).filter(|w| w == b"ARMS").count(), 1);
        let back = decode(&bytes).unwrap();
        assert_eq!(back.pieces[2].spheres.as_deref(), Some(&recipe));
        assert!(back.pieces[..2].iter().all(|p| p.spheres.is_none()));
        let opened = crate::Scene::from_data(back).unwrap();
        assert_eq!(opened.pieces.len(), 3);
        assert!(opened.pieces[2].is_spheres());
        let mut skipped = bytes.clone();
        let at = skipped.windows(4).position(|w| w == b"ARMS").unwrap();
        skipped[at..at + 4].copy_from_slice(b"NOPE");
        assert!(decode(&skipped).unwrap().pieces[2].spheres.is_none());
        // A broken tree is ignored, not trusted.
        let mut bad = recipe;
        bad.armature.spheres[1].parent = Some(2);
        bad.armature.spheres[2].parent = Some(1);
        scene.pieces[2].spheres = Some(Box::new(bad));
        assert!(decode(&encode(&scene.to_data())).unwrap().pieces[2].spheres.is_none());
    }

    #[test]
    fn an_oxyspheres_mesh_reference_is_optional_and_must_point_to_a_mesh() {
        let mut scene = crate::Scene::from_data(sample()).unwrap();
        let arm = oxy_mesh::armature::Armature::new(Vec3::ZERO, 0.5, None);
        let mut recipe = crate::ArmatureRecipe::spheres(arm, 100, 0.2);
        recipe.generated_mesh = Some(scene.pieces[1].id);
        scene.pieces.push(crate::Piece::spheres_piece("OxySpheres", recipe));
        let mut data = scene.to_data();
        let bytes = encode(&data);
        let opened = crate::Scene::from_data(decode(&bytes).unwrap()).unwrap();
        assert_eq!(opened.pieces[2].spheres.as_ref().unwrap().generated_mesh, Some(opened.pieces[1].id));

        let mut old = bytes;
        let tag = old.windows(4).position(|w| w == b"ARML").unwrap();
        old[tag..tag + 4].copy_from_slice(b"NOPE");
        let opened = crate::Scene::from_data(decode(&old).unwrap()).unwrap();
        assert_eq!(opened.pieces[2].spheres.as_ref().unwrap().generated_mesh, None);

        for invalid_index in [2, u64::MAX] {
            data.pieces[2].spheres.as_mut().unwrap().generated_mesh = Some(invalid_index);
            let opened = crate::Scene::from_data(decode(&encode(&data)).unwrap()).unwrap();
            assert_eq!(opened.pieces[2].spheres.as_ref().unwrap().generated_mesh, None);
        }
    }

    #[test]
    fn a_beta_23_3_mesh_with_its_spheres_gets_them_back_as_a_piece() {
        // Version 1: the spheres were kept on the mesh made from them.
        use oxy_mesh::armature::Armature;
        let arm = Armature::new(Vec3::ZERO, 0.5, None);
        let mut data = sample();
        let mesh = crate::Scene::from_data(data.clone()).unwrap().pieces[1].mesh.clone();
        let recipe = crate::ArmatureRecipe::new(arm, 100, 0.2, &mesh);
        // The section as 4.0.0-beta.23.3 wrote it reads as version 1.
        let (version, read) = decode_armature(&encode_armature(&recipe, 1)).unwrap().unwrap();
        assert_eq!((version, &read), (1, &recipe));
        data.pieces[1].skin_of = Some(Box::new(read));
        let back = data;
        let opened = crate::Scene::from_data(back).unwrap();
        assert_eq!(opened.pieces.len(), 3, "the spheres come back as a piece");
        let s = &opened.pieces[2];
        assert!(s.is_spheres() && !s.visible);
        assert_eq!(s.spheres.as_ref().unwrap().generated_mesh, Some(opened.pieces[1].id));
        assert!(!opened.pieces[1].is_spheres(), "the mesh stays a mesh");
    }

    #[test]
    fn placed_part_round_trips_in_oxb() {
        let mut scene = sample();
        let part = crate::SavedPlacedPart {
            id: 42,
            name: "DragonHorn".into(),
            group: 1,
            stroke_ids: vec![101, 102],
            anchor: Vec3::new(1.0, 2.0, 3.0),
            normal: Vec3::Y,
            tangent: Vec3::X,
            scale: 1.5,
            detail_size: 0.25,
            content: [7u8; 32],
            flags: 3,
            mirror_axis: 1,
            partner: 43,
            legacy_ids: false,
        };
        let strokes = crate::SavedStrokes {
            vertices: scene.pieces[0].mesh.positions.len() as u32,
            parts: vec![part.clone()],
            ..Default::default()
        };
        scene.pieces[0].strokes = Some(strokes);

        let bytes = encode(&scene);
        assert_eq!(bytes.windows(4).filter(|w| w == b"PART").count(), 1);
        let back = decode(&bytes).unwrap();
        let loaded = &back.pieces[0].strokes.as_ref().unwrap().parts;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], part);
        // A version 1 PART (beta.21) still reads, marked as having session ids.
        let mut raw = encode_placed_parts(std::slice::from_ref(&part));
        assert_eq!(raw[0], 2);
        raw[0] = 1;
        let old = decode_placed_parts(&raw).unwrap();
        assert!(old.len() == 1 && old[0].legacy_ids && old[0].stroke_ids == part.stroke_ids);
        // A newer version is skipped, not misread.
        raw[0] = 9;
        assert!(decode_placed_parts(&raw).unwrap().is_empty());

        // An older reader (where PART is renamed/unknown) skips PART gracefully
        let mut skipped = bytes.clone();
        let at = skipped.windows(4).position(|w| w == b"PART").unwrap();
        skipped[at..at + 4].copy_from_slice(b"NOPE");
        let decoded = decode(&skipped).unwrap();
        assert!(decoded.pieces[0].strokes.as_ref().is_none_or(|st| st.parts.is_empty()));
    }
}
