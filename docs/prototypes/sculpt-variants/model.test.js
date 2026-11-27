"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");
const { blend, cross, dot, paint, sample, sphere, subtract, toObj, weight } = require("./model.js");

test("the slider selects exact endpoints and interpolates without painting", () => {
  const a = [1, 2, 3];
  const b = [3, 4, 5];
  assert.strictEqual(blend(a, b, weight(0, 0, 0)), a);
  assert.strictEqual(blend(a, b, weight(0, 0, 1)), b);
  assert.deepEqual(blend(a, b, weight(0, 0, 0.5)), [2, 3, 4]);
});

test("paint locks an endpoint and erase restores the global slider", () => {
  const [a, b] = paint(0, 0, 1, "a");
  const [newA, newB] = paint(a, b, 1, "b");
  for (const global of [0, 0.5, 1]) {
    assert.equal(weight(a, b, global), 0);
    assert.equal(weight(newA, newB, global), 1);
    assert.equal(weight(...paint(newA, newB, 1, "erase"), global), global);
  }
});

test("feathered paint keeps both choices bounded and retains the live source link", () => {
  let weights = [0, 0];
  for (let i = 0; i < 500; i++) {
    weights = paint(...weights, (i % 17) / 17, i % 2 ? "a" : "b");
    assert.ok(weights[0] >= 0 && weights[1] >= 0 && weights[0] + weights[1] <= 1 + Number.EPSILON);
  }
  const keepA = paint(0, 0, 1, "a");
  const weightB = weight(...keepA, 0.5);
  assert.deepEqual(blend([8, 2, 1], [5, 5, 5], weightB), [8, 2, 1]);
});

test("the sample and OBJ preserve shared closed, outward-oriented connectivity", () => {
  const { directions, faces } = sphere();
  const edges = new Map();
  for (const [a, b, c] of faces) {
    const normal = cross(subtract(directions[b], directions[a]), subtract(directions[c], directions[a]));
    assert.ok(dot(normal, directions[a]) > 0);
    for (const [left, right] of [[a, b], [b, c], [c, a]]) {
      const key = left < right ? left + "," + right : right + "," + left;
      const entry = edges.get(key) || { count: 0, balance: 0 };
      entry.count++;
      entry.balance += left < right ? 1 : -1;
      edges.set(key, entry);
    }
  }
  assert.equal(directions.length - edges.size + faces.length, 2);
  assert.ok([...edges.values()].every(({ count, balance }) => count === 2 && balance === 0));
  const a = directions.map((direction) => sample(direction, false));
  const b = directions.map((direction) => sample(direction, true));
  assert.notDeepEqual(a, b);
  const obj = toObj(a.map((position, i) => blend(position, b[i], 0.33)), faces);
  assert.equal(obj.match(/^v /gm).length, directions.length);
  assert.equal(obj.match(/^f /gm).length, faces.length);
  assert.ok(!obj.includes("NaN"));
});
