"use strict";

const { clamp, dot, subtract, cross, unit, sphere, sample, paint, weight, blend, toObj } = SculptVariants;
const { directions, faces } = sphere();
const bases = {
  a: directions.map((direction) => sample(direction, false)),
  b: directions.map((direction) => sample(direction, true)),
};
const get = (id) => document.getElementById(id);
const views = ["a", "mix", "b"].map((id) => {
  const canvas = get(id);
  return { canvas, id, ctx: canvas.getContext("2d"), visible: [] };
});
const controls = ["blend", "radius", "orbit", "show-map"].map(get);
const [blendControl, radiusControl, orbitControl, mapControl] = controls;
const undo = get("undo");
const history = [];
const brushes = [...document.querySelectorAll("[data-brush]")];
let brush = "b";
let yaw = -17 * Math.PI / 180;
let pitch = -0.11;
let queued = false;
let gesture;
let hover;

function freshSample() {
  const field = () => new Float64Array(directions.length);
  return { a: field(), b: field(), keepA: field(), keepB: field() };
}

let state = freshSample();

function meshPositions(id) {
  return bases[id].map((point, i) =>
    point.map((value, axis) => value + directions[i][axis] * state[id][i]));
}

function composite() {
  const a = meshPositions("a");
  const b = meshPositions("b");
  const global = Number(blendControl.value) / 100;
  const weights = directions.map((_, i) => weight(state.keepA[i], state.keepB[i], global));
  const vertices = a.map((point, i) => blend(point, b[i], weights[i]));
  return { a, mix: vertices, b, weights };
}

function rotate([x, y, z]) {
  const cy = Math.cos(yaw);
  const sy = Math.sin(yaw);
  const cx = Math.cos(pitch);
  const sx = Math.sin(pitch);
  const depth = -sy * x + cy * z;
  return [cy * x + sy * z, cx * y - sx * depth, sx * y + cx * depth];
}

function vertexNormals(vertices) {
  const normals = vertices.map(() => [0, 0, 0]);
  for (const [a, b, c] of faces) {
    const normal = cross(subtract(vertices[b], vertices[a]), subtract(vertices[c], vertices[a]));
    for (const vertex of [a, b, c]) {
      for (let axis = 0; axis < 3; axis++) normals[vertex][axis] += normal[axis];
    }
  }
  return normals.map(unit);
}

const light = unit([-0.42, 0.56, 0.78]);
const specular = unit([-0.22, 0.40, 1.0]);
const clay = [203, 148, 107];
const mapA = [102, 196, 224];
const mapB = [238, 136, 93];

function edge(ax, ay, bx, by, x, y) {
  return (bx - ax) * (y - ay) - (by - ay) * (x - ax);
}

function draw(view, vertices, weights, scale) {
  const { canvas, ctx } = view;
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  const pixelWidth = Math.round(width * ratio);
  const pixelHeight = Math.round(height * ratio);
  if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
    canvas.width = pixelWidth;
    canvas.height = pixelHeight;
  }
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, width, height);
  view.scale = scale;
  const projected = vertices.map((point) => {
    const [x, y, z] = rotate(point);
    return [width / 2 + x * scale, height * 0.57 - y * scale, z];
  });
  const normals = vertexNormals(vertices).map(rotate);
  const visible = [];
  for (const ids of faces) {
    const [a, b, c] = ids.map((id) => projected[id]);
    const area = edge(a[0], a[1], b[0], b[1], c[0], c[1]);
    if (area < 0) visible.push({ ids, a, b, c, area, depth: (a[2] + b[2] + c[2]) / 3 });
  }
  visible.sort((left, right) => left.depth - right.depth);
  view.visible = visible;
  const showMap = mapControl.checked && view.id === "mix";
  ctx.lineWidth = 0.65;
  for (const { ids, a, b, c } of visible) {
    const normal = unit([0, 1, 2].map((axis) => ids.reduce((sum, id) => sum + normals[id][axis], 0)));
    const diffuse = 0.47 + 0.47 * Math.max(0, dot(normal, light));
    const shine = 46 * Math.max(0, dot(normal, specular)) ** 25;
    const color = showMap ? blend(mapA, mapB, ids.reduce((sum, id) => sum + weights[id], 0) / 3) : clay;
    const fill = "rgb(" + color.map((channel) => Math.round(channel * diffuse + shine)).join(",") + ")";
    ctx.beginPath();
    ctx.moveTo(a[0], a[1]);
    ctx.lineTo(b[0], b[1]);
    ctx.lineTo(c[0], c[1]);
    ctx.closePath();
    ctx.fillStyle = ctx.strokeStyle = fill;
    ctx.stroke();
    ctx.fill();
  }
  if (hover?.view === view && hit(view, hover.x, hover.y)) {
    ctx.beginPath();
    ctx.arc(hover.x, hover.y, Number(radiusControl.value) * scale / 100, 0, Math.PI * 2);
    ctx.strokeStyle = "#f6e8ce";
    ctx.lineWidth = 1.4;
    ctx.stroke();
  }
}

function render() {
  queued = false;
  get("blend-value").value = blendControl.value + "% B";
  get("radius-value").value = radiusControl.value + "%";
  undo.disabled = history.length === 0;
  // Identical camera and units per pixel in every pane.
  const scale = Math.min(...views.map(({ canvas }) =>
    Math.min(canvas.clientWidth / 2.57, canvas.clientHeight / 3.2)));
  const positions = composite();
  views.forEach((view) => draw(view, positions[view.id], positions.weights, scale));
}

function schedule() {
  if (queued) return;
  queued = true;
  requestAnimationFrame(render);
}

function hit(view, x, y) {
  for (let index = view.visible.length - 1; index >= 0; index--) {
    const { ids, a, b, c, area } = view.visible[index];
    const u = edge(b[0], b[1], c[0], c[1], x, y) / area;
    if (u < 0) continue;
    const v = edge(c[0], c[1], a[0], a[1], x, y) / area;
    const w = 1 - u - v;
    if (v < 0 || w < 0) continue;
    // Use the corresponding point on the shared sphere for both sculpting and painting.
    return unit([0, 1, 2].map((axis) =>
      directions[ids[0]][axis] * u + directions[ids[1]][axis] * v + directions[ids[2]][axis] * w));
  }
  return null;
}

function setStatus(message) {
  get("status").textContent = message;
}

function remember() {
  const snapshot = Object.fromEntries(Object.entries(state).map(([key, field]) => [key, field.slice()]));
  if (history.length === 32) history.shift();
  history.push(snapshot);
}

function dab(view, x, y) {
  const center = hit(view, x, y);
  if (!center) return;
  const radius = Number(radiusControl.value) / 100;
  const sculptSign = Number(get("sculpt-mode").value);
  if (!gesture.edited) remember();
  gesture.edited = true;
  directions.forEach((direction, i) => {
    const distance = Math.sqrt(Math.max(0, 2 - 2 * dot(center, direction)));
    if (distance >= radius) return;
    const edgeFraction = 1 - distance / radius;
    // A solid center locks the choice. Feather the transition around it.
    const t = view.id === "mix" ? Math.min(1, edgeFraction / 0.76) : edgeFraction;
    const falloff = t * t * (3 - 2 * t);
    if (view.id === "mix") {
      [state.keepA[i], state.keepB[i]] = paint(state.keepA[i], state.keepB[i], falloff, brush);
    } else {
      state[view.id][i] = clamp(state[view.id][i] + sculptSign * falloff * 0.034, -0.20, 0.34);
    }
  });
  schedule();
}

function point(event, canvas) {
  const box = canvas.getBoundingClientRect();
  return [event.clientX - box.left, event.clientY - box.top];
}

function orbit(deltaX, deltaY) {
  yaw = ((yaw + deltaX * 0.010 + 3 * Math.PI) % (2 * Math.PI)) - Math.PI;
  pitch = clamp(pitch + deltaY * 0.010, -1.2, 1.2);
  orbitControl.value = Math.round(yaw * 180 / Math.PI);
}

for (const view of views) {
  const { canvas } = view;
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  canvas.addEventListener("pointerdown", (event) => {
    if (gesture || ![0, 2].includes(event.button)) return;
    const [x, y] = point(event, canvas);
    gesture = { view, pointer: event.pointerId, x, y, orbit: event.button === 2, edited: false };
    canvas.setPointerCapture(event.pointerId);
    if (!gesture.orbit) dab(view, x, y);
    event.preventDefault();
  });
  canvas.addEventListener("pointermove", (event) => {
    const [x, y] = point(event, canvas);
    hover = { view, x, y };
    if (gesture?.view === view && gesture.pointer === event.pointerId) {
      const dx = x - gesture.x;
      const dy = y - gesture.y;
      if (gesture.orbit) {
        orbit(dx, dy);
      } else {
        // Advance by fixed screen distance, carrying the remainder across events.
        const distance = Math.hypot(dx, dy);
        const step = Math.max(3, Number(radiusControl.value) * view.scale / 630);
        for (let moved = step; moved <= distance; moved += step) {
          const fraction = moved / distance;
          dab(view, gesture.x + dx * fraction, gesture.y + dy * fraction);
        }
        const used = distance > 0 ? Math.floor(distance / step) * step / distance : 0;
        gesture.x += dx * used;
        gesture.y += dy * used;
      }
      if (gesture.orbit) Object.assign(gesture, { x, y });
    }
    schedule();
  });
  canvas.addEventListener("pointerleave", () => {
    hover = null;
    schedule();
  });
  canvas.addEventListener("lostpointercapture", (event) => {
    if (gesture?.view !== view || gesture.pointer !== event.pointerId) return;
    if (gesture.edited) setStatus(view.id === "mix" ? "Mix updated." : "Variant " + view.id.toUpperCase() + " updated.");
    gesture = undefined;
  });
}
for (const button of brushes) {
  button.addEventListener("click", () => {
    brush = button.dataset.brush;
    brushes.forEach((item) => item.setAttribute("aria-pressed", String(item === button)));
  });
}
controls.forEach((control) => control.addEventListener("input", schedule));
orbitControl.addEventListener("input", () => { yaw = Number(orbitControl.value) * Math.PI / 180; });
undo.addEventListener("click", () => {
  if (history.length === 0 || gesture) return;
  state = history.pop();
  setStatus("Last edit undone.");
  schedule();
});
get("reset").addEventListener("click", () => {
  if (gesture) return;
  remember();
  state = freshSample();
  setStatus("Sample reset.");
  schedule();
});
get("export").addEventListener("click", () => {
  const url = URL.createObjectURL(new Blob([toObj(composite().mix, faces)], { type: "text/plain" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = "sculpt-variants-mix.obj";
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  setStatus("Exported the current mix.");
});
const observer = new ResizeObserver(schedule);
views.forEach(({ canvas }) => observer.observe(canvas));
schedule();
