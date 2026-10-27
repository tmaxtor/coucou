// Run the actual hub animation lifecycle with disconnected canvases. The hero
// module uses a TypeScript parameter property, so transpile that module only.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { registerHooks } from "node:module";
import ts from "typescript";

registerHooks({
  load(url, context, nextLoad) {
    if (url.endsWith("/src/hub/hero.ts")) {
      return {
        format: "module",
        source: ts.transpileModule(readFileSync(new URL(url), "utf8"), {
          compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
        }).outputText,
        shortCircuit: true,
      };
    }
    return nextLoad(url, context);
  },
});

const listeners = new Map();
const frames = new Map();
let nextFrame = 0;
let boundsReads = 0;
window.addEventListener = (name, handler) => {
  const handlers = listeners.get(name) ?? [];
  handlers.push(handler);
  listeners.set(name, handlers);
};
globalThis.document = { visibilityState: "visible", addEventListener() {} };
window.innerWidth = 1000;
window.innerHeight = 700;
globalThis.requestAnimationFrame = (fn) => {
  frames.set(++nextFrame, fn);
  return nextFrame;
};
globalThis.cancelAnimationFrame = (id) => frames.delete(id);

const { liveMochi } = await import("../src/hub/hero.ts");
function tick(now) {
  const ready = [...frames.values()];
  frames.clear();
  for (const fn of ready) fn(now);
}
function canvas(connected) {
  return {
    isConnected: connected, width: 100, height: 100, clientWidth: 100, clientHeight: 100,
    addEventListener() {},
    getContext: () => null,
    getBoundingClientRect() {
      boundsReads++;
      return { left: 0, top: 0, right: 100, bottom: 100, width: 100, height: 100 };
    },
  };
}

test("repeated empty states leave one shared mouse listener and no detached-canvas work", () => {
  const canvases = Array.from({ length: 200 }, () => canvas(true));
  for (const el of canvases) liveMochi(el);
  assert.equal(listeners.get("pointermove").length, 1);
  assert.equal(frames.size, 1);
  tick(1000);
  for (const el of canvases) el.isConnected = false;
  tick(2000);
  assert.equal(frames.size, 0, "the empty animation loop stops");
  const before = boundsReads;
  for (const handler of listeners.get("pointermove")) handler({ clientX: 30, clientY: 50 });
  assert.equal(boundsReads, before, "mouse movement never touches removed canvases");
});

test("canvases built but never mounted also stop keeping the loop alive", () => {
  liveMochi(canvas(false));
  tick(3000);
  assert.equal(frames.size, 1);
  tick(8001);
  assert.equal(frames.size, 0);
  assert.equal(listeners.get("pointermove").length, 1);
});
