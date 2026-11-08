# RFC 0008: Viewport lights and shadows

- **Status:** proposed for the maintainer; implemented in the same change for review
- **Requested by:** users on the project's Discord. A community member built
  lights into a private copy of OxyBrush and shared a description of how it
  behaves, with a View-menu layout the maintainer liked. Only that
  description was used; none of that build's code was seen (see
  [CLEAN_ROOM.md](../CLEAN_ROOM.md)).

## Problem

The viewport shades with a matcap: lighting baked into a sphere image that
turns with the camera. It is fast and reads form well while sculpting, but
you can't place a light, light the model from the side or from behind, or
see how the model throws shadows. Those matter when checking a sculpt for
printing or presentation, and when judging forms under a light that stays
put as you orbit.

## Decision

An optional **lit** viewport, next to the matcap, which stays the default.

**Where it lives** (View > Shading, as the community build had it):
- **Light the viewport** switches between the matcap and the lights.
- **Lights…** opens the Lights window.
- Both are also in the command palette. The choice and the whole rig are
  remembered between sessions.

**The rig:** four lights, each one a **Sun**, **Point** or **Spot**. Each
light has:
- on/off;
- intensity and color, with **Warm**, **Daylight** and **Cool** color
  buttons (black-body colors);
- **Follow the view**, which keeps the light fixed relative to the camera so
  the model turns under it;
- for a sun: direction (azimuth, elevation);
- for a point or spot: position and distance falloff;
- for a spot: cone, soft edge, and an aim point (with **Aim at the model**);
- for a sun or spot: shadows, and how soft their edges are.

**Global settings:** exposure, sky and ground light (color and strength),
rim light, the surface (color, roughness, specular), shadows on/off, and
shadow detail (1024, 2048 or 4096 px). **Reset lights** puts everything
back.

**Defaults:** a warm key light from the upper right and a cool fill, both
following the view; the key casts shadows. Two more lights are set up but
off: a back light and a spot.

**Positions** are stored relative to the visible scene (its middle and
longest side), so a rig fits any model, small or large, and keeps working
when the model grows.

**Markers:** while the Lights window is open, each light that's on shows a
numbered dot over the viewport. Drag it to move the light. A sun's dot sits
on a sphere around the scene, and dragging it turns the sun's direction. A
spot also shows a diamond where it aims, which can be dragged too. Markers
that would be off screen stay at the viewport's edge. The editor hit-tests
them like curve handles, so dragging one never sculpts, and windows stay
on top of them.
- The community build showed markers while **L** was held. In OxyBrush, L
  already switches the mask box to a lasso, so markers show with the window
  instead.

## How it works

- **Lighting model:** `oxy_render::lighting` holds the rig, resolves it to
  view-space vectors for the shader, builds the shadow-map matrices, and
  reads and writes the settings line. It has no GPU calls and is
  unit-tested.
- **Shading:** the lit branch replaces only the matcap lookup, so face-group
  colors, paint, masks, the preview glaze, inactive-piece dimming, AO and
  cavity all apply on top exactly as before. Per pixel and per light:
  - diffuse plus GGX specular (Walter et al., EGSR 2007) with Schlick
    Fresnel and Smith–Schlick visibility;
  - a sky/ground term and a rim term;
  - Narkowicz's fit of the ACES tone curve (2015).
- **Shadows:** depth-only shadow maps for suns (orthographic, fitted to the
  scene's bounding sphere) and spots (perspective, fitted to the cone).
  - The maps live in one texture array, allocated at the chosen size only
    while a light casts shadows (a 1×1 stand-in otherwise).
  - Edges are softened with a 12-tap disk filter on hardware depth
    comparison.
  - Self-shadowing is avoided with a normal offset in the shader plus a
    slope-scaled depth bias.
  - **Point lights cast no shadows** in this version: they would need six
    maps each (a cube).
- The **uniform block** grows by 592 bytes. With lights off, nothing else
  changes in the frame.

## Performance

`cargo run --release -p oxy-render --example shading_timing`, a
330k-triangle sphere. Measured on the cloud test machine's software
renderer (lavapipe, CPU only), so absolute numbers are far slower than any
GPU; the comparison is what matters:

| Viewport | ms/frame |
|---|---|
| Matcap + AO + cavity (default) | 133 |
| Lights, no shadows, + AO + cavity | 136 |
| Lights + one shadow map + AO + cavity | 188 |

Lit shading costs about as much as the matcap. Each shadow-casting light
draws the visible pieces once more, depth only. That's why shadows are
opt-in per light and globally, and why point lights don't cast them yet.

## Not in this version

- shadows from point lights;
- more than four lights;
- per-piece materials;
- environment (HDRI) lighting;
- a path-traced render;
- bloom and other post effects.

The plugin API (RFC 0007, later phases) is the intended route for the last
two.

## Verification

- **Unit tests:** light placement relative to the scene and to the camera;
  angle round trips; the sun shadow map covers the scene and preserves depth
  order; the spot map centers on its aim; packing; the settings line round
  trip, including rejecting damaged lines; black-body colors.
- **Shaders** are validated with naga.
- **Editor test:** dragging a point light's marker moves the light in the
  view plane, and dragging a sun's marker turns it, without sculpting.
- **Headless screenshots** (lavapipe) of the lit viewport, the Lights window
  with its markers, and a ball over a plane with a high sun. The only pixels
  that change when shadows are switched on are the ball's soft shadow on
  the plane.
- **Not run on a real GPU,** on Windows or on macOS.
