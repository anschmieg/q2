// Sanity check for the vendored React ES modules (run by `cargo xtask
// vendor-react` after `build.mjs`, or directly with `node` from the repo
// root). Links the four files the way a browser import map would (esbuild
// `alias`), then renders a stateful component under jsdom, clicks it, and
// checks that the state update rendered: one React instance, hooks,
// events and the JSX runtime all work together.

import { createRequire } from "node:module";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "../../..");
const require = createRequire(join(repoRoot, "package.json"));
const { build } = require("esbuild");
const { JSDOM } = require("jsdom");

const program = `
import * as React from "react";
import { useState } from "react";
import * as ReactDOM from "react-dom";
import { createRoot } from "react-dom/client";
import { jsx } from "react/jsx-runtime";

export async function run(document) {
  function Counter() {
    const [n, setN] = useState(41);
    return jsx("button", { id: "b", onClick: () => setN(n + 1), children: "n=" + n });
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  createRoot(host).render(jsx(Counter, {}));
  await new Promise((r) => setTimeout(r, 20));
  const before = host.innerHTML;
  host.querySelector("#b").dispatchEvent(
    new document.defaultView.MouseEvent("click", { bubbles: true }),
  );
  await new Promise((r) => setTimeout(r, 20));
  return { react: React.version, reactDom: ReactDOM.version, before, after: host.innerHTML };
}
`;

const work = mkdtempSync(join(tmpdir(), "quarto-react-check-"));
try {
  const out = join(work, "bundle.mjs");
  await build({
    stdin: { contents: program, resolveDir: here, loader: "js" },
    bundle: true,
    format: "esm",
    platform: "browser",
    alias: {
      react: join(here, "react.js"),
      "react/jsx-runtime": join(here, "react-jsx-runtime.js"),
      "react-dom": join(here, "react-dom.js"),
      "react-dom/client": join(here, "react-dom-client.js"),
    },
    outfile: out,
    logLevel: "warning",
  });

  const dom = new JSDOM("<!doctype html><html><body></body></html>", {
    pretendToBeVisual: true,
  });
  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  Object.defineProperty(globalThis, "navigator", {
    value: dom.window.navigator,
    configurable: true,
  });

  const result = await (await import(pathToFileURL(out).href)).run(dom.window.document);
  const info = require(join(here, "build-info.json"));
  const problems = [];
  if (result.react !== info.react) problems.push(`react ${result.react} != build-info ${info.react}`);
  if (result.reactDom !== info["react-dom"]) {
    problems.push(`react-dom ${result.reactDom} != build-info ${info["react-dom"]}`);
  }
  if (result.before !== '<button id="b">n=41</button>') problems.push(`first render: ${result.before}`);
  if (result.after !== '<button id="b">n=42</button>') problems.push(`after click: ${result.after}`);
  if (problems.length > 0) {
    console.error("vendored React check failed:\n  " + problems.join("\n  "));
    process.exit(1);
  }
  console.log(`vendored React ${result.react} ok: hooks, events and jsx-runtime render under jsdom`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
