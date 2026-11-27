# RFC 0057 — Inspect imported meshes

- **Status:** Proposed with an implementation for review.
- **Scope:** Surface topology of mesh pieces as they enter the editor.

## Concept

A sculpt may open successfully and still fail a remesher or slicer. Give the
artist an inspection report at import, while the piece names still identify
where to look. Show findings for each affected piece in a batch. Keep
imports editable and undoable, including intentionally open surfaces.

## Checks and presentation

Count boundary edges, edges shared by more than two faces, shared edges with
inconsistent winding, duplicate indexed triangles, and zero-area triangles
in the mesh that OxyBrush decoded. Display an **Inspect import** button for
findings from the latest successful import. The report names each affected
piece and can select it for closer inspection and manual repair. A boundary
can be intentional on a sheet.

Use the mesh's vertex-to-triangle adjacency to inspect one vertex's incident
edges at a time. Large files share the existing background import worker.
Report data is per tab and describes the imported shape. A mesh can change
afterward, which is why the report identifies it as an import snapshot.
Structural file errors still use the import errors from the format reader.

Self-intersections, disconnected vertex fans, and solid printability require
additional checks. This first pass diagnoses indexed edges and triangles.
Automatic repair and different choices of weld tolerance are separate
decisions.

## Acceptance criteria

- A closed box imports quietly. A sheet, non-manifold edge, reversed face,
  duplicate face, or collinear triangle produces the corresponding finding.
- Report every affected piece with its current scene name. Keep geometry and
  the active piece intact while checking, and take the artist to the selected
  row's piece on request.
- A large batch still uses the import worker. The report follows its tab,
  and Undo can remove the imported pieces. Fitting a mesh to the scene
  preserves the reported piece across the two import undo steps.
