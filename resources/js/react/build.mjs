// Build the vendored React ES modules under this directory.
//
// Run through `cargo xtask vendor-react` (or `node resources/js/react/build.mjs`
// from the repo root). It reads `react` and `react-dom` from the repo's
// `node_modules` (the versions hub-client and preview-renderer pin) and
// writes four ES modules that the `components-resolve` transform ships
// under `site_libs/quarto-react/` and maps through the page's import map:
//
//   react.js              <- "react"
//   react-jsx-runtime.js  <- "react/jsx-runtime"
//   react-dom.js          <- "react-dom"
//   react-dom-client.js   <- "react-dom/client"
//
// Why a build step at all: React publishes CommonJS only, and a browser
// `import { useState } from "react"` needs real named exports. esbuild
// cannot enumerate a CommonJS module's exports statically, so each entry
// is generated here with an explicit `export const { ... } = React` list
// taken from the module at build time. `react` is bundled whole; the
// other three keep `react` (and `react-dom`) external so every module on
// the page shares one React instance, which is what hooks require.
//
// Output is minified, production mode. `build-info.json` records the
// versions so `vendored_react_matches_build_info` (components/runtime.rs)
// can pin them.

import { build } from "esbuild";
import { createRequire } from "node:module";
import { writeFileSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "../../..");
const require = createRequire(join(repoRoot, "package.json"));

const reactVersion = require("react/package.json").version;
const reactDomVersion = require("react-dom/package.json").version;

/** Named exports of a CommonJS module, excluding the interop marker. */
function namedExports(specifier) {
  const mod = require(specifier);
  return Object.keys(mod)
    .filter((k) => k !== "default" && k !== "__esModule")
    .sort();
}

/** An ESM entry that re-exports a CommonJS module with explicit names. */
function entrySource(specifier) {
  const names = namedExports(specifier);
  return [
    `import mod from ${JSON.stringify(specifier)};`,
    `export default mod;`,
    `export const { ${names.join(", ")} } = mod;`,
    "",
  ].join("\n");
}

/**
 * Resolve exactly these bare specifiers to the page's shared module.
 *
 * Two things make esbuild's own `external` option unsuitable here. It
 * externalizes every subpath too (`react` would cover `react/jsx-runtime`,
 * which must be bundled). And a CommonJS `require("react")` of an external
 * is left as a runtime `require` call, which a browser cannot satisfy.
 *
 * So each listed specifier resolves to a tiny ESM shim that re-exports the
 * external module: esbuild then links the CommonJS `require` to that shim
 * (copying the named exports at load time) and hoists one real
 * `import * from "react"` to the top of the output, which the page's import
 * map resolves to the vendored `react.js`. One React instance per page.
 */
function sharedExternal(names) {
  const SHIM = "quarto-shared-external";
  const escaped = names.map((n) => n.replace(/[/\\^$.*+?()[\]{}|]/g, "\\$&"));
  const filter = new RegExp(`^(${escaped.join("|")})$`);
  return {
    name: "shared-external",
    setup(b) {
      if (names.length === 0) return;
      b.onResolve({ filter }, (args) =>
        args.namespace === SHIM
          ? { path: args.path, external: true }
          : { path: args.path, namespace: SHIM },
      );
      b.onLoad({ filter: /.*/, namespace: SHIM }, (args) => ({
        contents: `import * as m from ${JSON.stringify(args.path)};\nexport * from ${JSON.stringify(args.path)};\nexport default m;\n`,
        loader: "js",
        resolveDir: repoRoot,
      }));
    },
  };
}

const MODULES = [
  { specifier: "react", out: "react.js", external: [] },
  { specifier: "react/jsx-runtime", out: "react-jsx-runtime.js", external: ["react"] },
  { specifier: "react-dom", out: "react-dom.js", external: ["react"] },
  { specifier: "react-dom/client", out: "react-dom-client.js", external: ["react", "react-dom"] },
];

for (const m of MODULES) {
  // The entry is fed through stdin and resolved from the repo root, so
  // `react` resolves against the workspace `node_modules` without a
  // temporary file.
  await build({
    stdin: {
      contents: entrySource(m.specifier),
      resolveDir: repoRoot,
      sourcefile: m.out.replace(/\.js$/, ".entry.js"),
      loader: "js",
    },
    bundle: true,
    format: "esm",
    platform: "browser",
    target: ["es2020"],
    minify: true,
    legalComments: "inline",
    define: { "process.env.NODE_ENV": '"production"' },
    plugins: [sharedExternal(m.external)],
    outfile: join(here, m.out),
    logLevel: "warning",
    banner: {
      js: `/* ${m.specifier}@${m.specifier.startsWith("react-dom") ? reactDomVersion : reactVersion} — vendored by resources/js/react/build.mjs */`,
    },
  });
  const bytes = readFileSync(join(here, m.out)).length;
  console.log(`${m.out}: ${bytes} bytes (${m.specifier})`);
}
writeFileSync(
  join(here, "build-info.json"),
  JSON.stringify({ react: reactVersion, "react-dom": reactDomVersion }, null, 2) + "\n",
);
console.log(`build-info.json: react ${reactVersion}, react-dom ${reactDomVersion}`);
