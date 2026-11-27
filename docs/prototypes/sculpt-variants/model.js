"use strict";

const SculptVariants = (() => {
  const clamp = (value, low, high) => Math.min(high, Math.max(low, value));
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const subtract = (a, b) => a.map((value, axis) => value - b[axis]);
  const cross = ([x, y, z], [u, v, w]) => [y * w - z * v, z * u - x * w, x * v - y * u];
  const unit = (v) => {
    const length = Math.hypot(...v);
    return length > 0 ? v.map((value) => value / length) : [0, 0, 0];
  };

  function sphere(rings = 36, segments = 64) {
    const directions = [[0, 1, 0]];
    for (let row = 1; row < rings; row++) {
      const theta = Math.PI * row / rings;
      for (let segment = 0; segment < segments; segment++) {
        const phi = 2 * Math.PI * segment / segments;
        directions.push([Math.sin(theta) * Math.sin(phi), Math.cos(theta), Math.sin(theta) * Math.cos(phi)]);
      }
    }
    const bottom = directions.push([0, -1, 0]) - 1;
    const faces = [];
    const index = (row, segment) => 1 + (row - 1) * segments + segment % segments;
    for (let segment = 0; segment < segments; segment++) {
      faces.push([0, index(1, segment), index(1, segment + 1)]);
      for (let row = 1; row < rings - 1; row++) {
        const a = index(row, segment);
        const b = index(row, segment + 1);
        const c = index(row + 1, segment);
        const d = index(row + 1, segment + 1);
        faces.push([a, c, b], [b, c, d]);
      }
      faces.push([index(rings - 1, segment), bottom, index(rings - 1, segment + 1)]);
    }
    for (const face of faces) {
      const [a, b, c] = face.map((i) => directions[i]);
      if (dot(cross(subtract(b, a), subtract(c, a)), a) < 0) {
        [face[1], face[2]] = [face[2], face[1]];
      }
    }
    return { directions, faces };
  }

  function sample(direction, variant) {
    const [x, y, z] = direction;
    const gx = Math.abs(x);
    // Broad clay forms modeled as smooth displacements on a common closed mesh.
    const patch = (px, py, wx, wy) =>
      Math.exp(-(((gx - px) / wx) ** 2 + ((y - py) / wy) ** 2));
    const front = clamp(z * 3, 0, 1);
    const eye = patch(0.41, 0.13, 0.19, 0.11);
    const brow = patch(0.38, 0.29, 0.29, 0.12);
    const muzzle = patch(0.04, -0.29, 0.47, 0.23);
    const nose = patch(0, -0.03, 0.19, 0.25);
    const mouth = patch(0.04, -0.44, 0.38, 0.041);
    const cheek = patch(0.60, -0.12, 0.23, 0.33);
    const crest = patch(0.02, 0.70, 0.22, 0.31);
    const temple = patch(0.72, 0.48, 0.21, 0.24);
    const width = 0.82 + 0.14 * (1 - y * y);
    const common = front * (0.20 * nose - 0.16 * eye + 0.13 * brow + 0.15 * muzzle - 0.04 * mouth);
    const broad = front * (0.19 * cheek + 0.08 * brow);
    const crested = front * (0.25 * crest + 0.12 * temple + 0.16 * muzzle - 0.06 * cheek);
    return [
      x * (width + (variant ? -0.13 * cheek + 0.36 * temple : 0.28 * cheek)),
      y * 1.12 + (variant ? 0.30 * crest * front + 0.17 * temple : 0),
      z * 0.74 + common + (variant ? crested : broad),
    ];
  }

  function paint(keepA, keepB, amount, brush) {
    const remaining = 1 - amount;
    return [
      keepA * remaining + (brush === "a" ? amount : 0),
      keepB * remaining + (brush === "b" ? amount : 0),
    ];
  }

  function weight(keepA, keepB, global) {
    return keepB + (1 - keepA - keepB) * global;
  }

  function blend(a, b, weight) {
    if (weight === 0) return a;
    if (weight === 1) return b;
    return a.map((value, axis) => value + (b[axis] - value) * weight);
  }

  function toObj(vertices, faces) {
    return [
      "# OxyBrush Sculpt Variants",
      "o Sculpt_Variants_Mix",
      ...vertices.map((vertex) => "v " + vertex.map((value) => value.toFixed(7)).join(" ")),
      "s 1",
      ...faces.map((face) => "f " + face.map((index) => index + 1).join(" ")),
      "",
    ].join("\n");
  }

  return { clamp, dot, subtract, cross, unit, sphere, sample, paint, weight, blend, toObj };
})();

if (typeof module !== "undefined") module.exports = SculptVariants;
