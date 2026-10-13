# Vendored React ES modules

React 19 as four browser ES modules, shipped under `site_libs/quarto-react/`
by the `components-resolve` transform (`crates/quarto-core/src/components/`)
when a page binds a component (bd-react-components-a8flgldm), and resolved
through the page's import map:

| file                   | import map key       |
| ---------------------- | -------------------- |
| `react.js`             | `react`              |
| `react-jsx-runtime.js` | `react/jsx-runtime`  |
| `react-dom.js`         | `react-dom`          |
| `react-dom-client.js`  | `react-dom/client`   |

`build-info.json` records the npm versions the files were built from.

## Why a build step

React publishes CommonJS only, and `import { useState } from "react"` in a
browser needs real named exports. `build.mjs` generates an entry per module
with an explicit `export const { ... } = React` list taken from the package at
build time, bundles it with esbuild (production, minified), and keeps `react`
(and `react-dom`) external in the subpath modules so every module on the page
shares one React instance, which hooks require. See the comments in
`build.mjs` for the resolver shim that turns React's own `require("react")`
into that external import.

## Rebuilding

```bash
cargo xtask vendor-react
```

reads `react` and `react-dom` from the repo's `node_modules` (the versions
`hub-client/package.json` pins), rewrites the four files and
`build-info.json`, and runs `check.mjs`, which links the files the way an
import map would and renders a stateful component under jsdom. Commit the
result. The Rust side pins the versions (`vendored_react_matches_build_info`
in `components/runtime.rs`), so a React bump that forgets this step fails the
test suite.
