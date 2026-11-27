use super::{JobResult, STAMP_REPEATS};
use oxy_mesh::Mesh;
use oxy_sculpt::{Stroke, StrokeRecipe, StrokeSample};
use std::sync::{Arc, mpsc};

/// Avoid round trips to the worker for small, cheap projections.
pub(super) const BACKGROUND_TRIANGLES: usize = 32_768;

/// Read-only source and one in-flight preview. Input that arrives while
/// it runs is coalesced by the editor into the most recent pointer position.
pub struct SceneStamp {
    pub(super) piece: u64,
    pub(super) base: Arc<Mesh>,
    revision: u64,
    preview_rx: Option<mpsc::Receiver<StampResult>>,
    preview: Option<StampResult>,
}

pub(super) struct FinishingPreview {
    pub(super) doc: u64,
    pub(super) piece: u64,
    pub(super) mesh: Mesh,
}

pub struct StampResult {
    pub(super) mesh: Mesh,
    pub(super) touched: Vec<u32>,
    pub(super) recipe: Option<StrokeRecipe>,
    pub(super) paints: bool,
    dirty: Vec<u32>,
}

impl SceneStamp {
    pub(super) fn new(piece: u64, base: &Mesh, revision: u64) -> Self {
        Self { piece, base: Arc::new(base.clone()), revision, preview_rx: None, preview: None }
    }

    pub(super) fn busy(&self) -> bool {
        self.preview_rx.is_some()
    }

    pub(super) fn preview(&mut self) -> Option<&mut Mesh> {
        self.preview.as_mut().map(|p| &mut p.mesh)
    }

    pub(super) fn has_preview(&self) -> bool {
        self.preview.is_some()
    }

    pub(super) fn take_preview(&mut self) -> Option<Mesh> {
        self.preview.take().map(|result| result.mesh)
    }

    pub(super) fn clear_preview(&mut self) {
        self.preview = None;
    }

    pub(super) fn poll(&mut self) -> bool {
        let Some(rx) = &self.preview_rx else { return false };
        match rx.try_recv() {
            Ok(mut result) => {
                if let Some(old) = &self.preview {
                    // PREVIEW_ID's GPU buffers may still contain the prior,
                    // larger stamp wherever this one does not reach.
                    for &chunk in &old.dirty {
                        result.mesh.mark_dirty(chunk);
                    }
                } else {
                    result.mesh.mark_all_dirty();
                }
                self.preview = Some(result);
                self.preview_rx = None;
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.preview_rx = None;
                false
            }
        }
    }

    pub(super) fn submit(&mut self, stroke: Stroke, sample: StrokeSample) {
        let base = Arc::clone(&self.base);
        let (tx, rx) = mpsc::sync_channel(1);
        self.preview_rx = Some(rx);
        rayon::spawn_fifo(move || {
            let _ = tx.send(StampResult::evaluate(&base, stroke, sample));
        });
    }

    pub(super) fn matches(&self, mesh: &Mesh, revision: u64) -> bool {
        self.revision == revision
            && self.base.topology_version() == mesh.topology_version()
            && self.base.edit_version() == mesh.edit_version()
            && self.base.face_version() == mesh.face_version()
    }

    pub(super) fn finish(self, stroke: Stroke, sample: StrokeSample) -> JobResult {
        let result = StampResult::evaluate(&self.base, stroke, sample);
        JobResult::SceneStamp { source: Box::new(self), result: Box::new(result) }
    }
}

impl StampResult {
    pub(super) fn evaluate(base: &Mesh, mut stroke: Stroke, sample: StrokeSample) -> Self {
        let mut mesh = base.clone();
        mesh.take_dirty();
        let mut touched = Vec::new();
        stroke.stamp(&mut mesh, sample, STAMP_REPEATS, &mut |_, chunk| touched.push(chunk));
        touched.sort_unstable();
        touched.dedup();
        let dirty = mesh.take_dirty();
        for &chunk in &dirty {
            mesh.mark_dirty(chunk);
        }
        let paints = stroke.settings().also_paint;
        Self { mesh, touched, recipe: stroke.take_recipe(), paints, dirty }
    }
}
