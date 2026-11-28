# RFC 0058 — Transmissive viewport materials

- **Status:** First slice
- **Scope:** Clear Glass and Frosted Glass for individual sculpt pieces.

## Workflow

Open **Materials**, choose the active piece as the target, then pick **Clear
Glass** or **Frosted Glass**. Adjust tint, roughness, and refraction while watching
the model, then apply the settings in one Undo step. Reseeding changes the fine
3D frost pattern. Projects save these settings with each piece.

## Rendering

The glass pass reads the opaque scene color and depth. It estimates a
screen-space offset from the refracted view ray. Fresnel reflectance makes
grazing surfaces more reflective. Roughness spreads the background sample.
The frost seed varies the roughness with 3D noise in model coordinates, so it
works on a sculpt without UVs or a texture. Opaque foreground depth still
occludes glass, and the sample stays inside the current pane.

This is a real-time approximation for the nearest glass surface. It samples
visible opaque geometry and reference backgrounds. Back-face refraction,
nested glass, caustics, and material export remain follow-up work.
Existing projects use their ordinary matcaps. Older readers fall back to the
scene material for the versioned glass keys saved in `MATL`.

## Acceptance

- The checkerboard behind a sculpt bends through Clear Glass and has softer
  edges through Frosted Glass when the view moves.
- Opaque geometry hides glass behind it. Other pieces retain their materials.
- A material assignment and Apply each undo in one step. Reopening restores
  the piece's glass settings.
- Opaque-only scenes keep the existing viewport path.

## References

- [Blender Glass BSDF manual](https://docs.blender.org/manual/en/5.0/render/shader_nodes/shader/glass.html)
- [Blender Noise Texture manual](https://docs.blender.org/manual/en/5.0/render/shader_nodes/textures/noise.html)
- [Nomad material guide, Refraction](https://nomadsculpt.com/manual/material#refraction)
