/*
 * quarto-components.js
 * Copyright (c) 2026 Posit, PBC
 *
 * Client runtime for components bound to document elements
 * (bd-react-components-a8flgldm). Mounts each bound element's React
 * component over the markup `q2 render` already produced.
 */

// The page carries, in its <head>, an import map (so `react`,
// `react-dom/client` and `react/jsx-runtime` resolve to the vendored
// copies under site_libs/quarto-react/) and a manifest:
//
//   <script type="application/json" id="quarto-components-manifest">
//   {"components": {"<name>": {"module": "<page-relative url>",
//                              "export": "default", "content": "dom",
//                              "props": {...}}}}
//   </script>
//
// Every element with `data-quarto-component="<name>"` is a host. For each
// host the runtime imports the module, captures the host's existing
// children (the no-script rendering), and renders the component into the
// host with these props:
//
//   attrs    { id, classes, attributes }  the host's own attributes
//   props    the manifest `props` overlaid by the host's
//            `data-quarto-component-props` JSON and by a child
//            <script type="application/json" class="quarto-component-props">
//   children the captured no-script content, as a React element
//            (`content: dom`); null for `content: none`
//   node     the bound node's Pandoc AST when `content: ast` attached one
//   host     the host element
//
// Failure at any step (module missing, export missing, render throwing)
// is logged and leaves the host as the server rendered it: the no-script
// version is also the error version.

import * as React from "react";
import { createRoot } from "react-dom/client";

const MANIFEST_ID = "quarto-components-manifest";
const HOST_ATTR = "data-quarto-component";
const PROPS_ATTR = "data-quarto-component-props";
const STATE_ATTR = "data-quarto-component-state";
const PROPS_SCRIPT_CLASS = "quarto-component-props";
const AST_SCRIPT_CLASS = "quarto-component-ast";
const CONTENT_CLASS = "quarto-component-content";
const OWN_ATTRS = new Set(["id", "class", HOST_ATTR, PROPS_ATTR, STATE_ATTR]);

function log(message, ...rest) {
  console.error(`quarto-components: ${message}`, ...rest);
}

function readManifest() {
  const el = document.getElementById(MANIFEST_ID);
  if (!el) return { components: {} };
  try {
    const parsed = JSON.parse(el.textContent || "{}");
    return { components: {}, ...parsed };
  } catch (e) {
    log("the manifest is not valid JSON", e);
    return { components: {} };
  }
}

function parseJson(text, what) {
  if (text == null || text.trim() === "") return {};
  try {
    const value = JSON.parse(text);
    return value && typeof value === "object" && !Array.isArray(value) ? value : {};
  } catch (e) {
    log(`${what} is not valid JSON`, e);
    return {};
  }
}

/** The host's attributes, minus the ones the runtime itself owns. */
function attrsOf(host) {
  const attributes = {};
  for (const { name, value } of host.attributes) {
    if (!OWN_ATTRS.has(name)) attributes[name] = value;
  }
  return {
    id: host.id || "",
    classes: Array.from(host.classList),
    attributes,
  };
}

/**
 * Renders captured DOM nodes inside a React tree. The wrapper element is
 * `display: contents`, so it takes no part in layout; a span is used when
 * the host is inline.
 */
function StaticContent({ nodes, inline }) {
  const ref = React.useRef(null);
  React.useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return undefined;
    for (const n of nodes) el.appendChild(n);
    return () => {
      for (const n of nodes) if (n.parentNode === el) el.removeChild(n);
    };
  }, [nodes]);
  return React.createElement(inline ? "span" : "div", {
    ref,
    className: CONTENT_CLASS,
    style: { display: "contents" },
  });
}

function takeChildScript(host, className) {
  for (const child of Array.from(host.children)) {
    if (child.tagName === "SCRIPT" && child.classList.contains(className)) {
      host.removeChild(child);
      return child.textContent;
    }
  }
  return null;
}

async function mount(host, manifest) {
  const name = host.getAttribute(HOST_ATTR);
  const spec = manifest.components[name];
  if (!spec) {
    log(`no component named "${name}" in the page manifest`);
    return;
  }
  // `import()` resolves a relative specifier against this module's own
  // URL, not the page's; the manifest URLs are page-relative.
  const moduleUrl = new URL(spec.module, document.baseURI).href;
  let module;
  try {
    module = await import(moduleUrl);
  } catch (e) {
    log(`could not load "${name}" from ${moduleUrl}`, e);
    return;
  }
  const exportName = spec.export || "default";
  const Component = module[exportName];
  if (typeof Component !== "function") {
    log(`"${name}": ${moduleUrl} has no export "${exportName}" that is a component`);
    return;
  }

  const propsScript = takeChildScript(host, PROPS_SCRIPT_CLASS);
  const astScript = takeChildScript(host, AST_SCRIPT_CLASS);
  const props = {
    ...(spec.props || {}),
    ...parseJson(host.getAttribute(PROPS_ATTR), `"${name}" ${PROPS_ATTR}`),
    ...parseJson(propsScript, `"${name}" props script`),
  };
  let node;
  if (astScript != null) {
    try {
      node = JSON.parse(astScript);
    } catch (e) {
      log(`"${name}" AST payload is not valid JSON`, e);
    }
  }

  const nodes = Array.from(host.childNodes);
  const inline = host.tagName === "SPAN";
  const children =
    spec.content === "none"
      ? null
      : React.createElement(StaticContent, { nodes, inline });
  for (const n of nodes) host.removeChild(n);

  const restore = () => {
    host.replaceChildren(...nodes);
    host.setAttribute(STATE_ATTR, "failed");
  };
  const root = createRoot(host, {
    onUncaughtError(error) {
      log(`"${name}" threw while rendering; restored the static content`, error);
      root.unmount();
      restore();
    },
  });
  host.setAttribute(STATE_ATTR, "mounted");
  root.render(
    React.createElement(Component, { attrs: attrsOf(host), props, children, node, host }),
  );
  host.dispatchEvent(
    new CustomEvent("quarto-component:mounted", { bubbles: true, detail: { name } }),
  );
}

function init() {
  const manifest = readManifest();
  const hosts = document.querySelectorAll(`[${HOST_ATTR}]`);
  for (const host of hosts) {
    mount(host, manifest).catch((e) => log("unexpected failure", e));
  }
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init, { once: true });
} else {
  init();
}
