# quarto-components runtime

`quarto-components.js` is the client runtime for components bound to
document elements (bd-react-components-a8flgldm; plan
`claude-notes/plans/2026-10-13-react-components.md`). The
`components-resolve` transform (`crates/quarto-core/src/components/`) ships
it as a Project-scoped artifact under `site_libs/quarto-components/` on
every page that binds at least one component, and links it from `<head>`
as a module script, after the page's import map and manifest.

It is hand-written ES2020 with no build step: it imports `react` and
`react-dom/client` through the import map (the vendored copies in
`../react/`), reads the manifest, and for each
`[data-quarto-component]` host imports the component module, captures the
host's no-script children, and renders the component with the props
contract documented at the top of the file (`attrs`, `props`, `children`,
`node`, `host`). Any failure leaves the host as the server rendered it.

Not minified: it is small, and diff-reviewability wins (the same stance
as `../quarto-nav/`).
