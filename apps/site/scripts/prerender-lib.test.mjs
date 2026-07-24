import assert from "node:assert/strict";
import test from "node:test";
import { injectPrerenderedRoot } from "./prerender-lib.mjs";

test("injects server markup into the unique site root", () => {
  assert.equal(
    injectPrerenderedRoot(
      '<div id="root"></div>',
      "<main><h1>明窗净几，字字轻语。</h1></main>"
    ),
    '<div id="root"><main><h1>明窗净几，字字轻语。</h1></main></div>'
  );
});

test("rejects a missing site root", () => {
  assert.throws(
    () => injectPrerenderedRoot("<body></body>", "<main />"),
    /exactly one root/u
  );
});

test("rejects duplicate site roots", () => {
  assert.throws(
    () => injectPrerenderedRoot(
      '<div id="root"></div><div id="root"></div>',
      "<main />"
    ),
    /exactly one root/u
  );
});

test("rejects an empty root beside a populated root", () => {
  assert.throws(
    () => injectPrerenderedRoot(
      '<div id="root"></div><div id="root"><main>Existing</main></div>',
      "<main />"
    ),
    /exactly one root/u
  );
});

test("injects replacement sequences byte-for-byte", () => {
  assert.equal(
    injectPrerenderedRoot('<div id="root"></div>', "<main>$&</main>"),
    '<div id="root"><main>$&</main></div>'
  );
});
