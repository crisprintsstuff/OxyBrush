# RFC 0024: Area lights, soft light falloff and the lit matcap

Status: Complete (accepted 2026-11-15)

## Summary

Three changes to the viewport lights ([RFC 0008](0008-viewport-lights.md)):

1. **Soft falloff for every light.** Light fades gently where a surface
   turns away from it, by the light's **Softness**. Before, a light without
   shadows ended in a hard line.
2. **Area lights.** A fourth kind of light: round, with a **Size**, facing
   an aim point. Its light is soft-edged, its highlights widen with its
   size, and its shadows are sharp where things touch and softer further
   away, as under a soft box.
3. **Light the matcap.** Keep the matcap as the material and let the lights
   brighten it and their shadows darken it, instead of the plain lit
   surface.

## Why

From a tester:

- Lights with shadows turned off looked unnatural. Asked which of three
  descriptions was closest, they chose "a hard edge where it falls off".
  A light with shadows gets a soft edge for free: its shadow map and the
  shadow's blur fade the light out just before the surface turns away. A
  light without shadows stopped exactly where the surface turns away, and
  the tone curve made that narrow band look like a line.
- "It would be really cool to have these lights and maintain the matcap."
- "Can we add area lights for the soft shadows?"

## Design

### Soft falloff

Each light fades out over a band on both sides of the terminator, `h` (a
sine) wide: `n·l` where `n·l ≥ h`, nothing where `n·l ≤ -h`, and the
quadratic `(n·l + h)² / 4h` in between. That is the usual smooth
approximation for a round light seen at angular radius `asin h` (as in
Lagarde and de Rousiers, "Moving Frostbite to Physically Based Rendering",
2014).

- For suns, points and spots, `h` comes from the light's **Softness**:
  about 2° at 0, 11° at the default 0.3, 34° at 1. Softness used to set
  only the shadow's edge, and was greyed out without shadows; now it is
  always available and sets both.
- For an area light, `h` is at least the light's own angular radius, so a
  big or near light wraps further round.

**Decision** (the maintainer chose from three descriptions of the
problem): "hard edge where it falls off".

### Area lights

- **Shape: round, with a size** (the maintainer chose the recommended
  option over a rectangle, which needs lookup tables to shade quickly).
  Size is the diameter, relative to the scene like light positions (0.2
  by default).
- **Facing:** it faces its aim point, and lights the half in front of it,
  most straight ahead (cosine). It has a position, an aim handle and
  falloff, like a spot without a cone.
- **Highlights:** the "representative point" of Karis, "Real Shading in
  Unreal Engine 4" (SIGGRAPH 2013). The highlight comes from the point of
  the light nearest the reflected ray, with the lobe widened by the
  light's angular size and scaled to keep its energy.
- **Shadows:** percentage-closer soft shadows (Fernando, NVIDIA 2005).
  Search the shadow map around the point for what blocks the light, take
  the blockers' mean distance, size the penumbra by similar triangles, and
  filter over it.
  - One perspective map, from the light toward its aim, wide enough to hold
    the scene.
  - Each sample is compared with the receiving surface where that sample
    lands (receiver plane depth bias, Isidoro, GDC 2006), so a wide search
    doesn't find the surface itself.
  - The samples (12 to search, 16 to filter, on a golden-angle spiral) are
    turned by a different angle at each pixel (interleaved gradient noise,
    Jimenez, SIGGRAPH 2014): fine grain instead of bands.

### Light the matcap

In Materials, **Lit surface** selects the matcap or a scene-wide custom
color. This panel also holds roughness and specular. Selecting a matcap
with viewport lights enabled switches to the matcap surface. Resetting
the lights preserves the material settings.

The lights are summed for a white surface
(sky and ground, each light's diffuse and its shadows). The matcap color
is multiplied by that, scaled so a surface facing the default key light
comes out about as the matcap alone, with values above 1 eased rather than
clipped. The lights' highlights are added on top. Rim light is left out,
since matcaps have their own. Exposure still applies.

**Decision:** "Matcap lit by the lights" (the recommended option) rather
than "matcap plus shadows only".

### Saving

The lighting line in the settings file goes to `v2`: the matcap switch
after the globals and each light's size after its other values. `v1`
lines still load, with no matcap lighting and the default size. The
project file doesn't store lights, so `.oxb` is unchanged.

## Cost

`shading_timing`, 327,680 triangles, 844×800, on llvmpipe (software
Vulkan, the only GPU here, so only the comparison means anything):

| Setup | ms per frame |
|---|---|
| Lights, one sun shadow map, AO, cavity | 162 |
| The same with the matcap lit | 167 |
| Area light with soft shadows in place of the sun | 194 |

The soft falloff adds a few instructions per light; an area light's
shadow reads the map 28 times per pixel instead of 12.

## Not in this plan

- Rectangular area lights.
- Point-light shadows (six maps).
- Settings for the matcap's gain.
