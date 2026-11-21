//! Recipes from the Recipe Box after the drop (RFC 0026, phase 3).
//!
//! A placed recipe is a stroke group plus a placement record: where it
//! sits ([`PartFrame`]), which library file it came from, and, for a
//! mirrored pair, the other side. This module keeps those records right
//! through undo, saving and reopening, and adds what you can do with a
//! placed recipe:
//!
//! - **Move on surface:** move, turn and scale it. Its strokes come off
//!   (with every stroke made after the first of them), their recipes are
//!   mapped to the new frame, replayed in order, and the later strokes that
//!   aren't the recipe's go back as stored (the additive model of RFC
//!   0025). One undo step.
//! - **Linked pairs:** an Edit… or Edit path on one side is applied to the
//!   matching stroke on the other side in the same undo step; a move moves
//!   both, the other side staying the mirror image. Fade and delete are per
//!   side. **Unlink** stops it.
//! - **Save back to Recipe Box / Save as new recipe:** write the source
//!   side's current recipes, in its own frame, to the library, only when
//!   asked.
//!
//! In the interface the thing in the Recipe Box is a *recipe*; here, as in
//! RFC 0026, it is a *part*, because *recipe* also means one stroke's
//! recorded inputs (`StrokeRecipe`).

use super::*;
use oxy_sculpt::detail::remap_recipes;
use oxy_sculpt::{PartDetail, PartFrame, StoredChange};

/// `PlacedPart::flags`: edits on one side are applied to the other.
pub const FLAG_LINKED: u8 = 1;
/// `PlacedPart::flags`: the side placed from the Recipe Box (the one Save
/// back writes), whichever side of the plane it is on.
pub const FLAG_SOURCE: u8 = 2;
/// `PlacedPart::flags`: the frame is reflected (the mirror side of a pair).
pub const FLAG_MIRRORED: u8 = 4;

/// The share of a part's samples that may miss the surface before a drop or
/// a move is refused.
const MAX_MISSED_SHARE: f32 = 0.5;

impl PlacedPart {
    /// Where the part sits.
    pub fn frame(&self) -> PartFrame {
        PartFrame {
            anchor: self.anchor,
            normal: self.normal,
            tangent: self.tangent,
            scale: self.scale,
            mirrored: self.flags & FLAG_MIRRORED != 0,
        }
        .orthonormal()
    }

    fn set_frame(&mut self, f: &PartFrame) {
        self.anchor = f.anchor;
        self.normal = f.normal;
        self.tangent = f.tangent;
        self.scale = f.scale;
        if f.mirrored {
            self.flags |= FLAG_MIRRORED;
        } else {
            self.flags &= !FLAG_MIRRORED;
        }
    }
}

/// The unit normal of mirror plane `axis` (1 X, 2 Y, 3 Z; anything else X).
pub fn axis_vec(axis: u8) -> Vec3 {
    match axis {
        2 => Vec3::Y,
        3 => Vec3::Z,
        _ => Vec3::X,
    }
}

impl Living {
    pub fn part(&self, id: u64) -> Option<&PlacedPart> {
        self.placed_parts.iter().find(|p| p.id == id)
    }

    /// The placed part stroke `stroke` belongs to.
    pub fn part_of_stroke(&self, stroke: u64) -> Option<&PlacedPart> {
        self.placed_parts.iter().find(|p| p.stroke_ids.contains(&stroke))
    }

    /// The parts placed on `piece` as stroke group `group`.
    pub fn parts_in_group(&self, group: u32) -> Vec<&PlacedPart> {
        self.placed_parts.iter().filter(|p| p.group == group).collect()
    }

    /// A part's strokes that are on the list (not deleted, not undone), in
    /// the order they were made.
    pub fn part_listed(&self, part: &PlacedPart) -> Vec<u64> {
        self.strokes.iter().filter(|s| s.listed() && part.stroke_ids.contains(&s.id)).map(|s| s.id).collect()
    }

    /// The other side, when edits on `part` are applied to it now: the link
    /// is on and the other side still has strokes. Deleting every stroke on
    /// one side ends the link without changing the flag, so undoing the
    /// delete brings it back.
    pub fn linked_partner(&self, part: &PlacedPart) -> Option<&PlacedPart> {
        if part.flags & FLAG_LINKED == 0 || part.partner == 0 {
            return None;
        }
        let other = self.part(part.partner)?;
        (other.flags & FLAG_LINKED != 0 && !self.part_listed(other).is_empty()).then_some(other)
    }

    fn parts_of_piece(&self, piece: u64) -> Vec<PlacedPart> {
        self.placed_parts.iter().filter(|p| p.piece == piece).cloned().collect()
    }

    /// Forget placements whose strokes are all gone (baked by a remesh, a
    /// level change…): the shape stays on the piece, as for any stroke.
    pub(super) fn sync_parts(&mut self) {
        let Living { placed_parts, strokes, .. } = self;
        placed_parts.retain(|p| p.stroke_ids.iter().any(|id| strokes.iter().any(|s| s.id == *id)));
    }

    fn new_part_id(&mut self) -> u64 {
        // 0 means "no partner" in a record, so ids start at 1.
        self.next_part_id = self.next_part_id.max(1);
        let id = self.next_part_id;
        self.next_part_id += 1;
        id
    }
}

/// Placement records read with a piece: new ids, and stroke references
/// turned into the ids the strokes got when they were loaded (`new_ids`,
/// by their place in the saved list). A `PART` from 4.0.0-beta.21
/// (version 1) stored the session's stroke ids, which mean nothing now: its
/// strokes are found by the stroke group, split by side of the mirror
/// plane for a pair, and the mirror side's frame (which that version saved
/// as the source side's) is rebuilt by reflection.
pub(super) fn adopt_saved_parts(
    living: &mut Living,
    piece: &Piece,
    saved: Vec<oxy_doc::SavedPlacedPart>,
    group_ids: &HashMap<u32, u32>,
    new_ids: &[Option<u64>],
) -> Vec<PlacedPart> {
    let center = piece.symmetry_center.unwrap_or(Vec3::ZERO);
    let ids: HashMap<u64, u64> = saved.iter().map(|p| (p.id, living.new_part_id())).collect();
    let mut out: Vec<PlacedPart> = saved
        .iter()
        .map(|p| PlacedPart {
            id: ids[&p.id],
            piece: piece.id,
            name: p.name.clone(),
            group: group_ids.get(&p.group).copied().unwrap_or(p.group),
            stroke_ids: if p.legacy_ids {
                Vec::new()
            } else {
                p.stroke_ids.iter().filter_map(|&i| new_ids.get(i as usize).copied().flatten()).collect()
            },
            anchor: p.anchor,
            normal: p.normal,
            tangent: p.tangent,
            scale: p.scale,
            detail_size: p.detail_size,
            content: p.content,
            flags: p.flags,
            mirror_axis: p.mirror_axis,
            partner: ids.get(&p.partner).copied().unwrap_or(0),
        })
        .collect();
    // Version 1: strokes by group, and the mirror side's frame.
    for k in 0..out.len() {
        if !saved[k].legacy_ids {
            continue;
        }
        let group = out[k].group;
        let in_group: Vec<&LivingStroke> =
            living.strokes.iter().filter(|s| s.piece == piece.id && s.group == Some(group)).collect();
        let partner = out.iter().position(|o| o.id == out[k].partner && o.group == group);
        let axis = axis_vec(out[k].mirror_axis);
        let source = out[k].flags & FLAG_SOURCE != 0;
        let source_frame = match partner {
            Some(j) if !source => out[j].frame(),
            _ => out[k].frame(),
        };
        let source_side = (source_frame.anchor - center).dot(axis) >= 0.0;
        out[k].stroke_ids = in_group
            .iter()
            .filter(|s| {
                if partner.is_none() {
                    return true;
                }
                let first = s.recipe.as_ref().and_then(|r| {
                    r.events.iter().find_map(|e| match e {
                        oxy_sculpt::RecipeEvent::Sample { sample, .. }
                        | oxy_sculpt::RecipeEvent::Stamp { sample, .. } => Some(sample.position),
                        _ => None,
                    })
                });
                let on_source_side = first.is_none_or(|p| ((p - center).dot(axis) >= 0.0) == source_side);
                on_source_side == source
            })
            .map(|s| s.id)
            .collect();
        if !source && partner.is_some() {
            let f = source_frame.reflect(center, axis);
            out[k].set_frame(&f);
        }
    }
    out
}

/// A stroke made by replaying `recipe` (a drop or a move): its stored
/// change at full strength.
fn replay_change(
    mesh: &mut oxy_mesh::Mesh,
    recipe: &StrokeRecipe,
    applied: f32,
    touch: &mut dyn FnMut(&oxy_mesh::Mesh, u32),
) -> StoredChange {
    let nothing = SurfaceChange { vertices: Vec::new(), deltas: Vec::new(), colors: Vec::new(), applied: 0.0 };
    resculpt(mesh, &nothing, &[], applied, recipe, touch)
}

fn samples_in(recipes: &[StrokeRecipe]) -> usize {
    recipes
        .iter()
        .flat_map(|r| r.events.iter())
        .filter(|e| matches!(e, oxy_sculpt::RecipeEvent::Sample { .. } | oxy_sculpt::RecipeEvent::Stamp { .. }))
        .count()
}

fn touch_chunks(
    history: &mut oxy_history::History,
    mesh: &oxy_mesh::Mesh,
    vertices: impl IntoIterator<Item = u32>,
    seen: &mut BTreeSet<u32>,
) {
    for v in vertices {
        if let Some(&c) = mesh.vertex_chunk_of(v)
            && seen.insert(c)
        {
            history.touch(mesh, c);
        }
    }
}

/// What Save back to Recipe Box would write, and what to ask first.
pub struct SaveBack {
    /// The library entry it replaces.
    pub file_name: String,
    /// The question to confirm, with any warning.
    pub question: String,
}

impl Editor {
    /// Drop the recipe being placed (RFC 0026). `choice` is the answer to
    /// Place one / Mirror pair (or, on the mirror plane, One centered copy /
    /// Mirror, halves match). One undo step: the strokes, their group and
    /// the placement records come and go together.
    pub fn commit_part_placement(&mut self, choice: PlaceChoice) -> Result<(), String> {
        let Some(placement) = self.part_placement.take() else {
            return Err("No recipe is being placed".into());
        };
        if let Some((part, _)) = placement.moving {
            return self.finish_part_move(part, placement.anchor, placement.normal, placement.twist, placement.scale);
        }
        if !self.living.enabled {
            self.status =
                "Living strokes is off. Turn it on in Preferences > General to save or place a recipe.".into();
            return Err(self.status.clone());
        }
        if self.dyntopo {
            self.status =
                "Dynamic detail is on, which changes the triangles, so this recipe can't be placed. Turn it off."
                    .into();
            return Err(self.status.clone());
        }
        let Some(index) = self.scene.index_of(placement.piece) else {
            return Err("The piece isn't there any more".into());
        };
        let name = placement.detail.name.clone();
        let mirrored = matches!(choice, PlaceChoice::MirrorPair | PlaceChoice::MirrorHalvesMatch);
        let piece = &self.scene.pieces[index];
        let pid = piece.id;
        let center = piece.symmetry_center.unwrap_or(Vec3::ZERO);
        let axis = axis_vec(placement.mirror_axis);
        let frame = PartFrame::from_twist(placement.anchor, placement.normal, placement.twist, placement.scale);
        let mirror_frame = mirrored.then(|| frame.reflect(center, axis));
        // Both sides are mapped on the surface as it is before the drop.
        let (source, missed) = placement.detail.project_frame(&frame, &piece.mesh);
        let (mirror, mirror_missed) =
            mirror_frame.map(|f| placement.detail.project_frame(&f, &piece.mesh)).unwrap_or_default();
        let total = samples_in(&placement.detail.recipes) * if mirrored { 2 } else { 1 };
        if total > 0 && (missed + mirror_missed) as f32 > total as f32 * MAX_MISSED_SHARE {
            self.status =
                "This recipe doesn't fit on the surface here. Scale it down, or drop it on a larger surface.".into();
            return Err(self.status.clone());
        }

        let groups_before = self.living.groups_of_piece(pid);
        let parts_before = self.living.parts_of_piece(pid);
        let group = self.living.next_group;
        self.living.next_group += 1;
        self.living.groups.push(StrokeGroup {
            id: group,
            piece: pid,
            name: name.clone(),
            strength: 1.0,
            visible: true,
        });

        let painted = self.scene.pieces[index].mesh.is_painted()
            || placement.detail.recipes.iter().any(|r| r.settings.kind == BrushKind::Paint);
        let doc = &mut self.documents[self.active_document];
        doc.history.begin_with(format!("Place {name}"), pid, &doc.scene.pieces[index].mesh, painted);
        let mut sides: Vec<Vec<u64>> = Vec::new();
        let mut moved: Vec<u32> = Vec::new();
        let mut colors = false;
        for recipes in [source, mirror] {
            let mut ids = Vec::new();
            for r in recipes {
                let history = &mut doc.history;
                let mesh = &mut doc.scene.pieces[index].mesh;
                let change = replay_change(mesh, &r, 1.0, &mut |m, c| history.touch(m, c));
                moved.extend_from_slice(&change.vertices);
                colors |= change.colors.iter().any(|c| *c != Vec3::ZERO);
                let id = doc.living.next_id;
                doc.living.next_id += 1;
                let mesh = &doc.scene.pieces[index].mesh;
                doc.living.strokes.push(LivingStroke {
                    id,
                    piece: pid,
                    brush: r.settings.kind,
                    made: SystemTime::now(),
                    entry: 0,
                    topology: mesh.topology_version(),
                    points: mesh.vertex_count(),
                    vertices: change.vertices,
                    deltas: change.deltas,
                    colors: if change.colors.iter().any(|c| *c != Vec3::ZERO) { change.colors } else { Vec::new() },
                    strength: 1.0,
                    deleted: false,
                    undone: false,
                    group: Some(group),
                    applied: 1.0,
                    recipe: Some(Arc::new(r)),
                    no_recipe: None,
                });
                ids.push(id);
            }
            sides.push(ids);
        }
        let mesh = &mut doc.scene.pieces[index].mesh;
        moved.sort_unstable();
        moved.dedup();
        mesh.update_after_move(&moved);
        if colors {
            mesh.mark_all_dirty();
        }
        self.history.commit();
        let Some(entry) = self.history.newest_id() else { return Ok(()) };
        let new_ids: Vec<u64> = sides.iter().flatten().copied().collect();
        for s in self.living.strokes.iter_mut().filter(|s| new_ids.contains(&s.id)) {
            s.entry = entry;
        }
        let source_id = self.living.new_part_id();
        let mirror_id = if mirrored { self.living.new_part_id() } else { 0 };
        let record = |id: u64, strokes: Vec<u64>, f: &PartFrame, flags: u8, partner: u64| {
            let mut p = PlacedPart {
                id,
                piece: pid,
                name: name.clone(),
                group,
                stroke_ids: strokes,
                anchor: f.anchor,
                normal: f.normal,
                tangent: f.tangent,
                scale: f.scale,
                detail_size: placement.detail.reference_radius,
                content: placement.content,
                flags,
                mirror_axis: placement.mirror_axis,
                partner,
            };
            p.set_frame(f);
            p
        };
        let mut sides = sides.into_iter();
        let source_strokes = sides.next().unwrap_or_default();
        let mirror_strokes = sides.next().unwrap_or_default();
        let link = if mirrored { FLAG_LINKED } else { 0 };
        let mut added = vec![record(source_id, source_strokes, &frame, FLAG_SOURCE | link, mirror_id)];
        if let Some(f) = mirror_frame {
            added.push(record(mirror_id, mirror_strokes, &f, FLAG_LINKED, source_id));
        }
        self.living.placed_parts.extend(added);
        let parts_after = self.living.parts_of_piece(pid);
        let groups_after = self.living.groups_of_piece(pid);
        self.living.edits.push(PanelEdit {
            entry,
            piece: pid,
            strokes: Vec::new(),
            groups: (groups_before, groups_after),
            data: Vec::new(),
            bodies: Vec::new(),
            parts: Some((parts_before, parts_after)),
        });
        self.living_enforce_budget();
        let missed = missed + mirror_missed;
        self.status = if missed > 0 {
            format!("Placed {name} ({missed} point(s) missed the surface and were left out)")
        } else {
            format!("Placed {name}")
        };
        Ok(())
    }

    /// Start Move on surface for placed part `part_id`: the next click on
    /// the model moves it there (with the Twist and Size from the Move
    /// window), or Apply keeps it where it is with a new twist or size.
    pub fn begin_part_move(&mut self, part_id: u64) -> Result<(), String> {
        let Some(part) = self.living.part(part_id).cloned() else {
            return Err("That recipe isn't placed any more".into());
        };
        if self.dyntopo {
            self.status =
                "Dynamic detail is on, which changes the triangles, so a recipe can't be moved. Turn it off.".into();
            return Err(self.status.clone());
        }
        let frame = part.frame();
        self.part_placement = Some(crate::parts::PartPlacement {
            detail: PartDetail {
                name: part.name.clone(),
                reference_radius: part.detail_size,
                recipes: Vec::new(),
                alphas: Vec::new(),
            },
            anchor: frame.anchor,
            normal: frame.normal,
            tangent: frame.tangent,
            twist: 0.0,
            scale: frame.scale,
            piece: part.piece,
            choice_open: false,
            on_plane: false,
            mirror_axis: part.mirror_axis,
            content: part.content,
            moving: Some((part_id, frame)),
        });
        self.status =
            format!("Move {}: click on the model where it should go, or set Twist and Size and Apply", part.name);
        Ok(())
    }

    /// Finish Move on surface: the part's new anchor and normal (a click,
    /// or the old ones for Apply), its extra `twist` (radians) and `scale`.
    fn finish_part_move(
        &mut self,
        part_id: u64,
        anchor: Vec3,
        normal: Vec3,
        twist: f32,
        scale: f32,
    ) -> Result<(), String> {
        let Some(part) = self.living.part(part_id) else { return Err("That recipe isn't placed any more".into()) };
        let from = part.frame();
        // Carry the tangent onto the new surface, then turn it.
        let n = normal.normalize_or(from.normal);
        let carried = (from.tangent - n * from.tangent.dot(n)).normalize_or(from.tangent);
        let b = n.cross(carried);
        let (sin_t, cos_t) = twist.sin_cos();
        let to = PartFrame {
            anchor,
            normal: n,
            tangent: (carried * cos_t + b * sin_t).normalize_or(carried),
            scale: scale.clamp(0.01, 100.0),
            mirrored: from.mirrored,
        }
        .orthonormal();
        self.move_part(part_id, to)
    }

    /// Move, turn and scale placed part `part_id` to frame `to` (RFC 0026:
    /// Move on surface), and its linked mirror side to the mirror of `to`.
    /// One undo step. Strokes made after the part's first stroke that
    /// aren't the part's stay as they were stored.
    pub fn move_part(&mut self, part_id: u64, to: PartFrame) -> Result<(), String> {
        let Some(part) = self.living.part(part_id).cloned() else {
            return Err("That recipe isn't placed any more".into());
        };
        let pid = part.piece;
        let Some(index) = self.scene.index_of(pid) else { return Err("The piece isn't there any more".into()) };
        let center = self.scene.pieces[index].symmetry_center.unwrap_or(Vec3::ZERO);
        let axis = axis_vec(part.mirror_axis);
        let mut sides: Vec<(PlacedPart, PartFrame)> = vec![(part.clone(), to)];
        if let Some(other) = self.living.linked_partner(&part).cloned() {
            sides.push((other, to.reflect(center, axis)));
        }
        // The strokes that move, in the order they were made.
        let mut moving: Vec<(usize, u64, usize)> = Vec::new(); // (list index, stroke id, side)
        for (k, (p, _)) in sides.iter().enumerate() {
            for id in self.living.part_listed(p) {
                let at = self.living.strokes.iter().position(|s| s.id == id).expect("listed");
                if self.living.strokes[at].recipe.is_none() {
                    self.status = format!("A stroke of {} was baked, so it can't be moved", part.name);
                    return Err(self.status.clone());
                }
                moving.push((at, id, k));
            }
        }
        if moving.is_empty() {
            self.status = format!("{} has no strokes left to move", part.name);
            return Err(self.status.clone());
        }
        moving.sort_unstable();
        let first = moving[0].0;
        let moving_ids: Vec<u64> = moving.iter().map(|m| m.1).collect();
        let later: Vec<SurfaceChange> = self.living.strokes[first..]
            .iter()
            .filter(|s| s.piece == pid && !s.undone && s.applied != 0.0 && !moving_ids.contains(&s.id))
            .map(surface_of)
            .collect();
        let own: Vec<(SurfaceChange, EditBody)> = moving
            .iter()
            .map(|m| {
                let s = &self.living.strokes[m.0];
                (surface_of(s), self.edit_body(s.id).expect("has a recipe"))
            })
            .collect();
        let painted = self.scene.pieces[index].mesh.is_painted() || own.iter().any(|o| !o.0.colors.is_empty());
        let unit = part.detail_size.max(1e-4);

        let doc = &mut self.documents[self.active_document];
        doc.history.begin_with(format!("Move {}", part.name), pid, &doc.scene.pieces[index].mesh, painted);
        let mut seen = BTreeSet::new();
        {
            let mesh = &doc.scene.pieces[index].mesh;
            touch_chunks(&mut doc.history, mesh, own.iter().flat_map(|o| o.0.vertices.iter().copied()), &mut seen);
            touch_chunks(&mut doc.history, mesh, later.iter().flat_map(|l| l.vertices.iter().copied()), &mut seen);
        }
        let mesh = &mut doc.scene.pieces[index].mesh;
        for (c, _) in &own {
            add_change(mesh, &c.vertices, &c.deltas, &c.colors, -c.applied);
        }
        for c in &later {
            add_change(mesh, &c.vertices, &c.deltas, &c.colors, -c.applied);
        }
        // Map every recipe on the surface without them, as a drop does.
        let mut mapped: Vec<Option<StrokeRecipe>> = vec![None; moving.len()];
        let mut missed = 0;
        let mut total = 0;
        for (k, (p, target)) in sides.iter().enumerate() {
            let rows: Vec<usize> = (0..moving.len()).filter(|&i| moving[i].2 == k).collect();
            let recipes: Vec<StrokeRecipe> = rows.iter().map(|&i| own[i].1.recipe.as_ref().clone()).collect();
            total += samples_in(&recipes);
            let (out, m) = remap_recipes(&recipes, &p.frame(), target, unit, mesh);
            missed += m;
            for (&i, r) in rows.iter().zip(out) {
                mapped[i] = Some(r);
            }
        }
        if total > 0 && missed as f32 > total as f32 * MAX_MISSED_SHARE {
            for c in later.iter().rev() {
                add_change(mesh, &c.vertices, &c.deltas, &c.colors, c.applied);
            }
            for (c, _) in own.iter().rev() {
                add_change(mesh, &c.vertices, &c.deltas, &c.colors, c.applied);
            }
            doc.history.cancel();
            self.status = format!(
                "{} doesn't fit on the surface there. Scale it down, or move it to a larger surface.",
                part.name
            );
            return Err(self.status.clone());
        }
        let mut bodies = Vec::with_capacity(moving.len());
        let mut moved: Vec<u32> = own.iter().flat_map(|o| o.0.vertices.iter().copied()).collect();
        for (i, recipe) in mapped.into_iter().enumerate() {
            let recipe = recipe.expect("mapped");
            let history = &mut doc.history;
            let change = replay_change(mesh, &recipe, own[i].0.applied, &mut |m, c| history.touch(m, c));
            moved.extend_from_slice(&change.vertices);
            let after = EditBody {
                vertices: change.vertices,
                deltas: change.deltas,
                colors: change.colors,
                brush: recipe.settings.kind,
                recipe: Arc::new(recipe),
            };
            bodies.push((moving[i].1, own[i].1.clone(), after));
        }
        for c in &later {
            add_change(mesh, &c.vertices, &c.deltas, &c.colors, c.applied);
        }
        moved.sort_unstable();
        moved.dedup();
        mesh.update_after_move(&moved);
        if painted {
            mesh.mark_all_dirty();
        }
        self.history.commit();
        for (id, _, after) in &bodies {
            self.write_edit_body(*id, after);
        }
        let parts_before = self.living.parts_of_piece(pid);
        for (p, target) in &sides {
            if let Some(rec) = self.living.placed_parts.iter_mut().find(|r| r.id == p.id) {
                rec.set_frame(target);
            }
        }
        let parts_after = self.living.parts_of_piece(pid);
        if let Some(entry) = self.history.newest_id() {
            let groups = self.living.groups_of_piece(pid);
            self.living.edits.push(PanelEdit {
                entry,
                piece: pid,
                strokes: Vec::new(),
                groups: (groups.clone(), groups),
                data: Vec::new(),
                bodies,
                parts: Some((parts_before, parts_after)),
            });
        }
        let others = later.len();
        self.status = match (others, missed) {
            (0, 0) => format!("Moved {}", part.name),
            (0, m) => format!("Moved {} ({m} point(s) missed the surface and were left out)", part.name),
            (_, _) => format!("Moved {}. Strokes made after this recipe stay as they were.", part.name),
        };
        Ok(())
    }

    /// Stop applying edits on one side of a mirrored pair to the other (RFC
    /// 0026: Unlink). Nothing on the surface changes. One undo step.
    pub fn unlink_part(&mut self, part_id: u64) -> Result<(), String> {
        let Some(part) = self.living.part(part_id).cloned() else {
            return Err("That recipe isn't placed any more".into());
        };
        if part.flags & FLAG_LINKED == 0 {
            return Err(format!("{} isn't linked", part.name));
        }
        let pid = part.piece;
        let Some(index) = self.scene.index_of(pid) else { return Err("The piece isn't there any more".into()) };
        let parts_before = self.living.parts_of_piece(pid);
        for rec in self.living.placed_parts.iter_mut().filter(|r| r.id == part.id || r.id == part.partner) {
            rec.flags &= !FLAG_LINKED;
        }
        let parts_after = self.living.parts_of_piece(pid);
        // An undo step that changes no points snapshots one chunk.
        let doc = &mut self.documents[self.active_document];
        let mesh = &doc.scene.pieces[index].mesh;
        doc.history.begin_with(format!("Unlink {}", part.name), pid, mesh, false);
        let chunk = part
            .stroke_ids
            .iter()
            .find_map(|id| doc.living.strokes.iter().find(|s| s.id == *id))
            .and_then(|s| s.vertices.first())
            .and_then(|&v| mesh.vertex_chunk_of(v).copied())
            .unwrap_or(0);
        if !mesh.chunks.is_empty() {
            doc.history.touch(mesh, chunk);
        }
        self.history.commit();
        if let Some(entry) = self.history.newest_id() {
            let groups = self.living.groups_of_piece(pid);
            self.living.edits.push(PanelEdit {
                entry,
                piece: pid,
                strokes: Vec::new(),
                groups: (groups.clone(), groups),
                data: Vec::new(),
                bodies: Vec::new(),
                parts: Some((parts_before, parts_after)),
            });
        }
        self.status = format!("Unlinked {}: edits on one side no longer change the other", part.name);
        Ok(())
    }

    /// For an edit of stroke `id` from `old` to `new`, the matching stroke
    /// on the linked other side and the recipe it gets: the same settings,
    /// and, when the path changed, the path mapped through the mirror.
    fn linked_edit(&self, id: u64, old: &StrokeRecipe, new: &StrokeRecipe) -> Option<(u64, StrokeRecipe)> {
        let part = self.living.part_of_stroke(id)?;
        let other = self.living.linked_partner(part)?;
        let k = part.stroke_ids.iter().position(|s| *s == id)?;
        let twin = *other.stroke_ids.get(k)?;
        let stroke = self.living.strokes.iter().find(|s| s.id == twin && s.listed())?;
        let mut recipe = stroke.recipe.as_ref()?.as_ref().clone();
        if old.events != new.events {
            let index = self.scene.index_of(part.piece)?;
            let mesh = &self.scene.pieces[index].mesh;
            let (out, _) =
                remap_recipes(std::slice::from_ref(new), &part.frame(), &other.frame(), part.detail_size, mesh);
            recipe = out.into_iter().next()?;
        }
        recipe.settings = new.settings;
        recipe.invert = new.invert;
        recipe.alpha = new.alpha.clone();
        recipe.paint = new.paint;
        Some((twin, recipe))
    }

    /// Inside an open undo step (`history.begin_with` done): re-sculpt the
    /// linked other side's stroke for an edit of `id` from `old` to `new`.
    ///
    /// RETURNS: the other stroke's id and its bodies before and after, for
    /// the step's panel record.
    pub(super) fn resculpt_linked_in_step(
        &mut self,
        id: u64,
        old: &StrokeRecipe,
        new: &StrokeRecipe,
    ) -> Option<(u64, EditBody, EditBody)> {
        let (twin, recipe) = self.linked_edit(id, old, new)?;
        let from = self.edit_body(twin)?;
        let stroke = self.living.strokes.iter().find(|s| s.id == twin)?;
        let pid = stroke.piece;
        let applied = stroke.applied;
        let own = surface_of(stroke);
        let later = self.later_changes(twin);
        let index = self.scene.index_of(pid)?;
        let doc = &mut self.documents[self.active_document];
        let history = &mut doc.history;
        let mesh = &mut doc.scene.pieces[index].mesh;
        let change = resculpt(mesh, &own, &later, applied, &recipe, &mut |m, c| history.touch(m, c));
        let after = EditBody {
            vertices: change.vertices,
            deltas: change.deltas,
            colors: change.colors,
            brush: recipe.settings.kind,
            recipe: Arc::new(recipe),
        };
        self.write_edit_body(twin, &after);
        Some((twin, from, after))
    }

    /// The source side of placed part `part_id` and its recipes for saving,
    /// or why it can't be saved.
    fn source_recipes(&self, part_id: u64) -> Result<(PlacedPart, Vec<StrokeRecipe>, bool), String> {
        let part = self.living.part(part_id).ok_or("That recipe isn't placed any more")?;
        let source = if part.flags & FLAG_SOURCE != 0 {
            part
        } else {
            match self.living.part(part.partner) {
                Some(p) if p.flags & FLAG_SOURCE != 0 => p,
                _ => {
                    return Err(format!(
                        "The side of {} placed from the Recipe Box was deleted, so it can't be saved back. Save this side as a new recipe instead.",
                        part.name
                    ));
                }
            }
        };
        let mut recipes = Vec::new();
        let mut faded = false;
        let mut present = 0;
        for id in &source.stroke_ids {
            let Some(s) = self.living.strokes.iter().find(|s| s.id == *id) else {
                return Err(format!(
                    "Some strokes of {} were baked (a remesh or a level change), so it can't be saved from the piece any more.",
                    source.name
                ));
            };
            if !s.listed() {
                continue; // deleted on purpose: left out
            }
            present += 1;
            let Some(r) = &s.recipe else {
                return Err(format!("A stroke of {} wasn't recorded, so it can't be saved.", source.name));
            };
            recipes.push(r.as_ref().clone());
            faded |= (self.living.effective(s) - 1.0).abs() > 1e-4;
        }
        if present == 0 {
            return Err(format!(
                "Every stroke of the side of {} placed from the Recipe Box was deleted, so it can't be saved back.",
                source.name
            ));
        }
        Ok((source.clone(), recipes, faded))
    }

    /// What Save back to Recipe Box for `part_id` would replace, and the
    /// question to ask before it does.
    pub fn part_save_back_check(&self, part_id: u64) -> Result<SaveBack, String> {
        let (source, _, faded) = self.source_recipes(part_id)?;
        let entry = self
            .parts_library
            .entries
            .iter()
            .find(|e| source.content != [0; 32] && e.hash == source.content)
            .or_else(|| self.parts_library.entries.iter().find(|e| e.name == source.name))
            .ok_or_else(|| {
                format!("{} isn't in the Recipe Box any more. Use Save as new recipe instead.", source.name)
            })?;
        let mut question = format!("Replace {} in the Recipe Box with this version?", entry.name);
        if faded {
            question.push_str(" Fades aren't saved: the recipe keeps its strokes at full strength.");
        }
        Ok(SaveBack { file_name: entry.file_name.clone(), question })
    }

    /// Save back to Recipe Box, after the question from
    /// [`Self::part_save_back_check`] was answered yes: replace the library
    /// file with the source side's current recipes, in its own frame.
    pub fn part_save_back(&mut self, part_id: u64) -> Result<PathBuf, String> {
        let check = self.part_save_back_check(part_id)?;
        let (source, recipes, _) = self.source_recipes(part_id)?;
        let detail = PartDetail::from_placed(&source.name, &recipes, &source.frame(), source.detail_size)?;
        let dir = self.parts_dir.clone().ok_or("The Recipe Box folder isn't available")?;
        let hash = self
            .parts_library
            .replace_part(&dir, &check.file_name, detail)
            .map_err(|e| format!("Couldn't save the recipe: {e}"))?;
        // Both sides now come from the new version of the file.
        let ids = [source.id, source.partner];
        for rec in self.living.placed_parts.iter_mut().filter(|r| ids.contains(&r.id)) {
            rec.content = hash;
        }
        self.status = format!("Saved {} back to the Recipe Box", source.name);
        Ok(dir.join(check.file_name))
    }

    /// Save as new recipe: the source side's current recipes as a new entry
    /// named `name` (a free variant of it, so nothing is replaced).
    pub fn part_save_as_new(&mut self, part_id: u64, name: &str, category: &str) -> Result<PathBuf, String> {
        let part = self.living.part(part_id).cloned().ok_or("That recipe isn't placed any more")?;
        // The source side when it's there; this side when it was deleted.
        let (frame, size, recipes) = match self.source_recipes(part_id) {
            Ok((source, recipes, _)) => (source.frame(), source.detail_size, recipes),
            Err(_) => {
                let recipes: Vec<StrokeRecipe> = self
                    .living
                    .part_listed(&part)
                    .iter()
                    .filter_map(|id| self.living.strokes.iter().find(|s| s.id == *id))
                    .filter_map(|s| s.recipe.as_ref().map(|r| r.as_ref().clone()))
                    .collect();
                (part.frame(), part.detail_size, recipes)
            }
        };
        let name = name.trim();
        let name = self.parts_library.free_name(if name.is_empty() { &part.name } else { name });
        let detail = PartDetail::from_placed(&name, &recipes, &frame, size)?;
        let dir = self.parts_dir.clone().ok_or("The Recipe Box folder isn't available")?;
        let path = self
            .parts_library
            .save_part(&dir, detail, category)
            .map_err(|e| format!("Couldn't save the recipe: {e}"))?;
        self.status = format!("Saved {name} to the Recipe Box");
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parts::PlaceChoice;
    use oxy_sculpt::{BrushSettings, RecipeEvent, StrokeSample, Symmetry};

    fn editor() -> Editor {
        let mut e = Editor::for_tests();
        e.symmetry.x = false;
        e.living.enabled = true;
        e.scene.active_mut().symmetry_center = Some(Vec3::ZERO);
        e
    }

    /// An asymmetric L: a long leg along local x and a short one along +y,
    /// so a mirror image and a turned copy differ.
    fn l_detail() -> PartDetail {
        let event = |x: f32, y: f32| RecipeEvent::Sample {
            sample: StrokeSample { position: Vec3::new(x, y, 0.0), normal: Vec3::Z, pressure: 1.0 },
            spacing: 1.0,
            strength: 1.0,
            max_dabs: u32::MAX,
        };
        let recipe = |events: Vec<RecipeEvent>| StrokeRecipe {
            settings: BrushSettings { radius: 0.04, strength: 0.6, ..BrushSettings::default() },
            invert: false,
            symmetry: Symmetry::default(),
            center: Vec3::ZERO,
            view: Vec3::ZERO,
            grab_through: false,
            paint: Vec3::ONE,
            alpha: None,
            events,
        };
        PartDetail {
            name: "Ell".into(),
            reference_radius: 0.04,
            recipes: vec![
                recipe((0..=8).map(|k| event(k as f32 * 0.5, 0.0)).collect()),
                recipe((0..=3).map(|k| event(4.0, k as f32 * 0.5)).collect()),
            ],
            alphas: Vec::new(),
        }
    }

    fn surface_point(e: &Editor, dir: Vec3) -> (Vec3, Vec3) {
        let d = dir.normalize();
        let hit = e.scene.active().mesh.raycast(d * 3.0, -d).expect("hits the sphere");
        (hit.point, hit.normal)
    }

    fn place(e: &mut Editor, at: Vec3, choice: PlaceChoice) {
        e.begin_part_placement(l_detail()).unwrap();
        // At its own size, so the whole L lands on the unit sphere.
        e.part_placement.as_mut().unwrap().scale = 1.0;
        let (p, n) = surface_point(e, at);
        let pid = e.scene.active().id;
        e.update_part_placement(p, n, pid);
        e.commit_part_placement(choice).unwrap();
    }

    fn first_sample(e: &Editor, stroke: u64) -> Vec3 {
        let s = e.living.strokes.iter().find(|s| s.id == stroke).unwrap();
        s.recipe
            .as_ref()
            .unwrap()
            .events
            .iter()
            .find_map(|ev| match ev {
                RecipeEvent::Sample { sample, .. } => Some(sample.position),
                _ => None,
            })
            .unwrap()
    }

    fn positions(e: &Editor) -> Vec<Vec3> {
        e.scene.active().mesh.positions.clone()
    }

    fn close(a: &[Vec3], b: &[Vec3]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (*x - *y).length() < 1e-5)
    }

    #[test]
    fn dropping_and_moving_follow_the_pointer_in_the_viewport() {
        let mut e = editor();
        // Panels around the viewport: it doesn't start at the window corner.
        let [_, _, w, h] = e.canvas;
        e.canvas = [180.0, 90.0, w, h];
        let centre = glam::Vec2::new(180.0 + w * 0.5, 90.0 + h * 0.5);
        // Hovering a drop over the model puts it under the pointer.
        e.begin_part_placement(l_detail()).unwrap();
        e.part_placement.as_mut().unwrap().scale = 1.0;
        e.pointer_moved(centre);
        let want = e.surface_hit_at(centre).expect("the model is under the middle of the viewport").point;
        assert!(e.part_placement.as_ref().unwrap().anchor.distance(want) < 1e-5, "the drop is under the pointer");
        e.commit_part_placement(PlaceChoice::One).unwrap();
        // Move on surface: a click elsewhere on the model moves it there.
        let id = e.living.placed_parts[0].id;
        e.begin_part_move(id).unwrap();
        let at = centre + glam::Vec2::new(w * 0.08, -h * 0.1);
        let target = e.surface_hit_at(at).expect("on the model").point;
        let revision = e.history.revision();
        e.pointer = Some(at);
        e.pointer_down(crate::editor::PointerButton::Primary);
        assert!(!e.is_stroking(), "the click is the move's, not a stroke");
        e.pointer_up();
        assert!(e.part_placement.is_none(), "the move finished");
        assert!(e.living.part(id).unwrap().anchor.distance(target) < 1e-5, "it moved under the pointer");
        assert_eq!(e.history.revision(), revision + 1, "one undo step");
    }

    #[test]
    fn a_drop_is_one_undo_step_with_its_records() {
        let mut e = editor();
        let base = positions(&e);
        place(&mut e, Vec3::new(0.3, 1.0, 0.2), PlaceChoice::One);
        assert_eq!(e.living.placed_parts.len(), 1);
        let part = e.living.placed_parts[0].clone();
        assert!(part.id != 0 && part.flags & FLAG_SOURCE != 0);
        assert_eq!(e.living.part_listed(&part).len(), 2);
        assert!(!close(&positions(&e), &base), "sculpted");
        e.undo();
        e.living_sync();
        assert!(close(&positions(&e), &base));
        assert!(e.living.placed_parts.is_empty(), "the record goes with the step");
        assert!(e.living.listed(e.scene.active().id).is_empty(), "and its strokes");
        e.redo();
        assert_eq!(e.living.placed_parts.len(), 1);
        assert_eq!(e.living.part_listed(&e.living.placed_parts[0]).len(), 2);
    }

    #[test]
    fn moving_a_recipe_resculpts_it_in_one_undo_step_and_keeps_later_strokes() {
        let mut e = editor();
        place(&mut e, Vec3::new(0.3, 1.0, 0.2), PlaceChoice::One);
        let part = e.living.placed_parts[0].clone();
        let placed = positions(&e);
        // A later stroke elsewhere: it stays as stored.
        let (q, qn) = surface_point(&e, Vec3::new(-0.2, 0.3, 1.0));
        let mut later_recipe = l_detail().recipes[0].clone();
        later_recipe.events = vec![RecipeEvent::Sample {
            sample: StrokeSample { position: q, normal: qn, pressure: 1.0 },
            spacing: 1.0,
            strength: 1.0,
            max_dabs: u32::MAX,
        }];
        e.living.strokes.len();
        let before_later = e.living.strokes.len();
        {
            // Sculpt it as a living stroke with a recipe, like a brush stroke.
            let pid = e.scene.active().id;
            let doc = &mut e.documents[e.active_document];
            let mesh = &doc.scene.pieces[0].mesh;
            doc.history.begin_with("Later", pid, mesh, false);
            let history = &mut doc.history;
            let mesh = &mut doc.scene.pieces[0].mesh;
            later_recipe.replay(mesh, &mut |m, c| history.touch(m, c));
            e.history.commit();
            e.living_record(BrushKind::Draw, Some(later_recipe));
        }
        assert_eq!(e.living.strokes.len(), before_later + 1);
        let later = e.living.strokes.last().unwrap().clone();
        let to = PartFrame::from_twist(
            surface_point(&e, Vec3::new(1.0, 0.4, -0.3)).0,
            surface_point(&e, Vec3::new(1.0, 0.4, -0.3)).1,
            0.7,
            part.scale * 1.3,
        );
        e.move_part(part.id, to).unwrap();
        assert!(e.status.contains("stay as they were"), "{}", e.status);
        let moved = e.living.part(part.id).unwrap().clone();
        assert!(moved.anchor.distance(to.anchor) < 1e-6 && (moved.scale - to.scale).abs() < 1e-6);
        assert!(first_sample(&e, part.stroke_ids[0]).distance(to.anchor) < 0.02, "the strokes went with it");
        let r = e.living.strokes.iter().find(|s| s.id == part.stroke_ids[0]).unwrap();
        assert!((r.recipe.as_ref().unwrap().settings.radius - 0.04 * to.scale).abs() < 1e-5, "and grew");
        let kept = e.living.strokes.iter().find(|s| s.id == later.id).unwrap();
        assert_eq!(kept.deltas, later.deltas, "the later stroke's stored change is untouched");
        let after_move = positions(&e);
        // One step back: as before the move, records and all.
        e.undo();
        assert!(close(
            &positions(&e),
            &placed.iter().zip(&later_with(&e, &later)).map(|(p, d)| *p + *d).collect::<Vec<_>>()
        ));
        assert!(e.living.part(part.id).unwrap().anchor.distance(part.anchor) < 1e-6);
        e.redo();
        assert!(close(&positions(&e), &after_move));
    }

    /// `placed` plus the later stroke's change, point by point (the surface
    /// before the move had both).
    fn later_with(e: &Editor, later: &LivingStroke) -> Vec<Vec3> {
        let mut d = vec![Vec3::ZERO; e.scene.active().mesh.vertex_count()];
        for (v, delta) in later.vertices.iter().zip(&later.deltas) {
            d[*v as usize] = *delta * later.applied;
        }
        d
    }

    #[test]
    fn a_mirrored_pair_is_a_mirror_image_and_moves_and_edits_together() {
        let mut e = editor();
        place(&mut e, Vec3::new(0.5, 0.8, 0.3), PlaceChoice::MirrorPair);
        assert_eq!(e.living.placed_parts.len(), 2);
        let source = e.living.placed_parts.iter().find(|p| p.flags & FLAG_SOURCE != 0).unwrap().clone();
        let mirror = e.living.part(source.partner).unwrap().clone();
        assert!(mirror.flags & FLAG_MIRRORED != 0 && mirror.partner == source.id);
        assert!(e.living.linked_partner(&source).is_some() && e.living.linked_partner(&mirror).is_some());
        // Each stroke's samples are the reflection of its twin's.
        for (a, b) in source.stroke_ids.iter().zip(&mirror.stroke_ids) {
            let (p, q) = (first_sample(&e, *a), first_sample(&e, *b));
            assert!(Vec3::new(-p.x, p.y, p.z).distance(q) < 0.01, "{p} mirrors to {q}");
        }
        // Moving the source moves the mirror side to the mirror of it.
        let (p, n) = surface_point(&e, Vec3::new(0.6, 0.2, 0.9));
        e.move_part(source.id, PartFrame::from_twist(p, n, 0.3, source.scale)).unwrap();
        let s2 = e.living.part(source.id).unwrap().clone();
        let m2 = e.living.part(mirror.id).unwrap().clone();
        assert!(Vec3::new(-s2.anchor.x, s2.anchor.y, s2.anchor.z).distance(m2.anchor) < 1e-5);
        for (a, b) in s2.stroke_ids.iter().zip(&m2.stroke_ids) {
            let (p, q) = (first_sample(&e, *a), first_sample(&e, *b));
            assert!(Vec3::new(-p.x, p.y, p.z).distance(q) < 0.01, "after the move, {p} mirrors to {q}");
        }
        // Edit… on the mirror side changes the source side too, in one step.
        let twin = m2.stroke_ids[0];
        let partner_stroke = s2.stroke_ids[0];
        let radius = |e: &Editor, id: u64| {
            e.living.strokes.iter().find(|s| s.id == id).unwrap().recipe.as_ref().unwrap().settings.radius
        };
        let r0 = radius(&e, partner_stroke);
        e.living_open_edit(twin).unwrap();
        e.living_edit_setting(StrokeSetting::Radius(r0 * 1.5), false);
        assert!((radius(&e, twin) - r0 * 1.5).abs() < 1e-6);
        assert!((radius(&e, partner_stroke) - r0 * 1.5).abs() < 1e-6, "the link carried the edit");
        e.living_close_edit();
        e.undo();
        assert!(
            (radius(&e, twin) - r0).abs() < 1e-6 && (radius(&e, partner_stroke) - r0).abs() < 1e-6,
            "one undo step"
        );
        // Fading one side doesn't touch the other.
        e.living.selected = [twin].into();
        e.living_set_strength(0.0);
        let other = e.living.strokes.iter().find(|s| s.id == partner_stroke).unwrap();
        assert_eq!(other.strength, 1.0, "fade is per side");
    }

    #[test]
    fn unlinking_stops_linked_edits_and_undo_links_again() {
        let mut e = editor();
        place(&mut e, Vec3::new(0.5, 0.8, 0.3), PlaceChoice::MirrorPair);
        let source = e.living.placed_parts.iter().find(|p| p.flags & FLAG_SOURCE != 0).unwrap().clone();
        let before = positions(&e);
        e.unlink_part(source.id).unwrap();
        assert!(close(&positions(&e), &before), "nothing moves");
        assert!(e.living.linked_partner(&source).is_none());
        assert!(e.living.part(source.id).unwrap().flags & FLAG_LINKED == 0);
        let a = source.stroke_ids[0];
        let b = e.living.part(source.partner).unwrap().stroke_ids[0];
        e.living_open_edit(a).unwrap();
        let r0 = e.living.strokes.iter().find(|s| s.id == b).unwrap().recipe.as_ref().unwrap().settings.radius;
        e.living_edit_setting(StrokeSetting::Radius(r0 * 2.0), false);
        e.living_close_edit();
        let rb = e.living.strokes.iter().find(|s| s.id == b).unwrap().recipe.as_ref().unwrap().settings.radius;
        assert!((rb - r0).abs() < 1e-6, "unlinked: the other side keeps its own");
        e.undo(); // the edit
        e.undo(); // the unlink
        assert!(e.living.part(source.id).unwrap().flags & FLAG_LINKED != 0, "undo links them again");
        // Both recipes are still there.
        assert!(e.living.strokes.iter().filter(|s| s.recipe.is_some()).count() >= 4);
    }

    #[test]
    fn placed_recipes_survive_save_and_reopen() {
        let mut e = editor();
        place(&mut e, Vec3::new(-0.5, 0.8, 0.3), PlaceChoice::MirrorPair);
        let bytes = oxy_doc::format::encode(&e.scene_data());
        let mut f = Editor::for_tests();
        f.living.enabled = true;
        let scene = oxy_doc::Scene::from_data(oxy_doc::format::decode(&bytes).unwrap()).unwrap();
        f.replace_document(scene, None);
        assert_eq!(f.living.placed_parts.len(), 2);
        for p in &f.living.placed_parts {
            assert!(p.id != 0);
            let listed = f.living.part_listed(p);
            assert_eq!(listed.len(), 2, "each side finds its own strokes again");
            assert!(listed.iter().all(|id| f.living.strokes.iter().any(|s| s.id == *id && s.recipe.is_some())));
        }
        let source = f.living.placed_parts.iter().find(|p| p.flags & FLAG_SOURCE != 0).unwrap().clone();
        assert!(f.living.linked_partner(&source).is_some(), "still linked");
        assert!(f.living.part(source.partner).unwrap().flags & FLAG_MIRRORED != 0);
        // And it still moves.
        let (p, n) = surface_point(&f, Vec3::new(-0.7, 0.3, 0.7));
        f.move_part(source.id, PartFrame::from_twist(p, n, 0.0, source.scale)).unwrap();
    }

    #[test]
    fn save_back_writes_only_when_asked_and_keeps_the_source_hand() {
        let dir = std::env::temp_dir().join(format!("oxy_recipe_save_back_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut e = editor();
        e.parts_dir = Some(dir.clone());
        let path = e.parts_library.save_part(&dir, l_detail(), "Shapes").unwrap();
        let hash = e.parts_library.entries[0].hash;
        // Placed as a pair from the negative side.
        e.begin_part_placement(l_detail()).unwrap();
        e.part_placement.as_mut().unwrap().content = hash;
        e.part_placement.as_mut().unwrap().scale = 1.0;
        let (p, n) = surface_point(&e, Vec3::new(-0.5, 0.8, 0.3));
        let pid = e.scene.active().id;
        e.update_part_placement(p, n, pid);
        e.commit_part_placement(PlaceChoice::MirrorPair).unwrap();
        let source = e.living.placed_parts.iter().find(|p| p.flags & FLAG_SOURCE != 0).unwrap().clone();
        assert_eq!(source.content, hash);
        let mirror = e.living.part(source.partner).unwrap().clone();
        // Edit the mirror side: the file in the box doesn't change.
        let on_disk = std::fs::read(&path).unwrap();
        e.living_open_edit(mirror.stroke_ids[1]).unwrap();
        e.living_edit_setting(StrokeSetting::Radius(0.06), false);
        e.living_close_edit();
        assert_eq!(std::fs::read(&path).unwrap(), on_disk, "an edit never writes the box");
        // Save back: the source side's recipes, in the source's own hand.
        let check = e.part_save_back_check(mirror.id).unwrap();
        assert!(check.question.starts_with("Replace Ell"), "{}", check.question);
        e.part_save_back(mirror.id).unwrap();
        let saved = PartDetail::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.recipes.len(), 2);
        assert!((saved.recipes[1].settings.radius - 0.06).abs() < 1e-5, "the linked edit is in it");
        let short_leg_end = |d: &PartDetail| match d.recipes[1].events.last().unwrap() {
            RecipeEvent::Sample { sample, .. } => sample.position,
            _ => unreachable!(),
        };
        let (was, now) = (short_leg_end(&l_detail()), short_leg_end(&saved));
        assert!(now.y > 0.5 && (now.y - was.y).abs() < 0.2, "the short leg still points +y: {was} vs {now}");
        let new_hash = e.parts_library.entries[0].hash;
        assert_ne!(new_hash, hash);
        assert!(e.living.placed_parts.iter().all(|p| p.content == new_hash), "both sides follow the new file");
        // Save as new never replaces another recipe with the same name.
        let other = e.part_save_as_new(source.id, "Ell", "Shapes").unwrap();
        assert_ne!(other, path);
        assert_eq!(e.parts_library.entries.len(), 2);
        // With the source side's strokes all deleted, save back refuses.
        e.living.selected = source.stroke_ids.iter().copied().collect();
        e.living_delete_selected();
        assert!(e.part_save_back_check(mirror.id).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
