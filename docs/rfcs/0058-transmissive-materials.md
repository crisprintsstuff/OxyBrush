# RFC 0058 — Transmissive viewport materials

- **Status:** Viewport presets, thickness-based tint, and opt-in exit refraction implemented
- **Scope:** Clear Glass and Frosted Glass for individual sculpt pieces.

## Workflow

See the [Glass materials guide](../guides/viewport-glass.md) for the pane and
sphere comparison.

Open **Materials**, choose the active piece as the target, then pick **Clear
Glass** or **Frosted Glass**. Adjust tint, roughness, and refraction while watching
the model, then apply the settings in one Undo step. For colored glass, raise
**Absorption** to make thick areas take on a deeper tint while thin edges stay
clearer. At zero, the tint is uniform. Reseeding changes the fine 3D frost
pattern. Turn on **Exit refraction** for glass with a closed surface. It
changes how flat panes and curved lenses bend the background. The setting
starts off and is saved with each piece.

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

Exit refraction adds view-space normals to the same back-face pass. A short
screen-space search intersects the inside ray with the far surface, then bends
the ray into air using the opposite index ratio. Absorption uses the traced
path length when available. Opaque scene depth supplies a backdrop distance,
and reference images use an estimated distance. Unreliable or missing exits,
surface instances, and rays that cannot reach the screen-space backdrop keep the
entry-only preview. Total internal reflection uses the existing studio
reflection. One closed shell per piece gives the most reliable result.

The preview samples visible opaque geometry and reference backgrounds. Nested
shells, multiple internal bounces, caustics, and material export remain
follow-up work. Existing `glass:1` and `glass:2` keys open with Exit refraction
off. A `glass:3` key enables it. Readers that lack support for the version use
the scene material.

## Acceptance

- The checkerboard behind a sculpt bends through Clear Glass and has softer
  edges through Frosted Glass when the view moves.
- A thin pane with parallel faces preserves the checkerboard more closely
  with Exit refraction enabled. A rounded piece still bends it, and frosting
  softens the image through the piece.
- Opaque geometry hides glass behind it. Other pieces retain their materials.
- With colored tint and absorption enabled, the center of a rounded piece
  absorbs more light than its thin rim. An overlapping glass piece does not
  supply the back depth, and an open sheet keeps uniform tint.
- A material assignment and Apply each undo in one step. Reopening restores
  the piece's glass settings.
- Opaque-only scenes keep the existing viewport path.

The Linux headless render job runs the glass and X-ray regression on
Mesa's software Vulkan adapter.

## References

- [Blender Glass BSDF manual](https://docs.blender.org/manual/en/5.0/render/shader_nodes/shader/glass.html)
- [Blender Noise Texture manual](https://docs.blender.org/manual/en/5.0/render/shader_nodes/textures/noise.html)
- [Nomad material guide, Refraction](https://nomadsculpt.com/manual/material#refraction)
- [Khronos KHR_materials_volume, Attenuation](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_materials_volume#attenuation)
- [Physically Based Rendering, fourth edition, Specular Reflection and Transmission](https://pbr-book.org/4ed/Reflection_Models/Specular_Reflection_and_Transmission)
