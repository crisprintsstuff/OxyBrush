# RFC 0058 — Transmissive viewport materials

- **Status:** Viewport presets and thickness-based tint implemented
- **Scope:** Clear Glass and Frosted Glass for individual sculpt pieces.

## Workflow

Open **Materials**, choose the active piece as the target, then pick **Clear
Glass** or **Frosted Glass**. Adjust tint, roughness, and refraction while watching
the model, then apply the settings in one Undo step. For colored glass, raise
**Absorption** to make thick areas take on a deeper tint while thin edges stay
clearer. At zero, the tint is uniform. Reseeding changes the fine 3D frost
pattern. Projects save these settings with each piece.

## Rendering

The glass pass reads the opaque scene color and depth. It estimates a
screen-space offset from the refracted view ray. Fresnel reflectance makes
grazing surfaces more reflective. Roughness spreads the background sample.
The frost seed varies the roughness with 3D noise in model coordinates, so it
works on a sculpt without UVs or a texture. Opaque foreground depth still
occludes glass, and the sample stays inside the current pane.

Colored glass with absorption enabled gets a back-face depth pass per piece.
The difference from the visible entry depth estimates the path through one
closed shell. In scene units, the shader raises the linear tint to the power
of path length times Absorption using Beer–Lambert attenuation. A missing
or invalid exit and instanced copies use uniform tint. White tint transmits
the same color at all thicknesses.

This is a real-time approximation for the nearest glass surface. It samples
visible opaque geometry and reference backgrounds. Back-face refraction,
nested shells, caustics, and material export remain follow-up work.
Existing `glass:1` keys open with zero Absorption. New `glass:2` keys carry
the additional value. Older readers that do not recognize the key fall back
to the scene material.

## Acceptance

- The checkerboard behind a sculpt bends through Clear Glass and has softer
  edges through Frosted Glass when the view moves.
- Opaque geometry hides glass behind it. Other pieces retain their materials.
- With colored tint and absorption enabled, the center of a rounded piece
  absorbs more light than its thin rim. An overlapping glass piece does not
  supply the back depth, and an open sheet keeps uniform tint.
- A material assignment and Apply each undo in one step. Reopening restores
  the piece's glass settings.
- Opaque-only scenes keep the existing viewport path.

## References

- [Blender Glass BSDF manual](https://docs.blender.org/manual/en/5.0/render/shader_nodes/shader/glass.html)
- [Blender Noise Texture manual](https://docs.blender.org/manual/en/5.0/render/shader_nodes/textures/noise.html)
- [Nomad material guide, Refraction](https://nomadsculpt.com/manual/material#refraction)
- [Khronos KHR_materials_volume, Attenuation](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_materials_volume#attenuation)
