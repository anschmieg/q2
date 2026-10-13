/**
 * Entry point for the q2-debug renderer iframe.
 *
 * Loaded by `/q2-debug.html` and handles the postMessage protocol with
 * the parent (Q2DebugIframe). Mounts the framework's <Ast> with
 * q2-debug's registry as the format-side defaults, layered with any
 * user-TSX overrides loaded via LOAD_CUSTOM_COMPONENTS.
 */

import { createRoot } from 'react-dom/client';
import React from 'react';
import { Deck, Slide } from '@revealjs/react';
// Reveal CSS comes from the VENDORED copy — the same files `q2 render`
// embeds and `q2-preview/RevealDeck.tsx` imports — not from npm
// `reveal.js/*.css`, so the debug surface can't drift from render's
// deck styling (bd-4b7f1hr7; same convergence as bd-ibqkf9ry). The
// vendored↔npm byte-identity test in `revealjs/assemble.rs` keeps
// these in sync with the pinned npm package.
// bd-dg8x84bu: scoped reset, never the global Meyer reset — this debug iframe
// renders arbitrary content via <Ast> (not only decks), so a global reset would
// zero font-style on <em>/<i> here too. reset-scoped.css confines it to .reveal.
import '../../../../../resources/revealjs/reset-scoped.css';
import '../../../../../resources/revealjs/reveal.css';
import '../../../../../resources/revealjs/theme/white.css';
import '../../../../../resources/revealjs/quarto-reveal.css';
import katex from 'katex';
import 'katex/dist/katex.min.css';
import './changed.css';

import {
    Ast,
    Node,
    renderChildren,
    renderNode,
    inlinesToPlainText,
    blocksToPlainText,
} from '@quarto/preview-renderer/framework';
import type { FormatRegistry } from '@quarto/preview-renderer/framework';
import {
    Block,
    Inline,
    Para, Plain, Header, CodeBlock, BulletList, OrderedList,
    BlockQuote, Div, HorizontalRule, RawBlock, Figure,
    Str, Space, SoftBreak, LineBreak,
    Emph, Strong, Code, Link, Image, Span, Quoted,
    blockStyle, inlineStyle,
    q2DebugRegistry,
} from '.';
import { buildCustomRegistry, type ComponentExports } from '@quarto/preview-renderer/utils/customRegistry';
import {
    makeIframeMessageDispatcher,
    type IframeMessage,
} from '../iframeMessageDispatch';
import type { PandocAST } from '@quarto/preview-renderer/framework';
import { setTextChangeLedger } from './textRun';
import { enableStableNodeKeys, enableHostedNodes, hostOf } from '@quarto/preview-renderer/framework';
import { reconcileAst, describeOp, type Op, type Touched } from '@quarto/preview-renderer/astdiff';

// Incremental AST updates (see `updateAst`): the iframe keeps one AST
// object across renders and patches it in place, so React children are
// keyed by node identity rather than by index, and every node's DOM lives
// in a persistent host that is *moved* (not rebuilt) when the structure
// above it changes — see `framework/hostedNodes.tsx`.
enableStableNodeKeys();
enableHostedNodes();

// Set the renderer-surface global at module top so importing this
// module is sufficient to populate `window.__REACT_AST_DEBUG_RENDERER__`
// — the framework-primitive parity test (Plan 2A item 14) relies on
// being able to read both globals via plain module imports without
// firing `LOAD_CUSTOM_COMPONENTS` setup messages. `React`, `katex`,
// and `RevealReact` stay lazy in `loadCustomComponents` since they're
// tied to dynamic user-TSX imports (the right time to set them).
(window as any).__REACT_AST_DEBUG_RENDERER__ = {
    renderChildren, renderNode, Node,
    inlinesToPlainText, blocksToPlainText,
    Block, Inline,
    Para, Plain, Header, CodeBlock, BulletList, OrderedList,
    BlockQuote, Div, HorizontalRule, RawBlock, Figure,
    Str, Space, SoftBreak, LineBreak,
    Emph, Strong, Code, Link, Image, Span, Quoted,
    q2DebugRegistry, blockStyle, inlineStyle,
};

let root: ReturnType<typeof createRoot> | null = null;
let customRegistry: Record<string, React.ComponentType<any>> = {};

/**
 * The AST currently on screen, patched in place on every update (see
 * `updateAst`). Reset when the document path changes, since a diff
 * against a different document would be large and meaningless.
 */
let currentAst: PandocAST | null = null;
let currentAstPath: string | null = null;

/** Last edit script applied, exposed for debugging / e2e inspection. */
declare global {
  interface Window {
    __Q2_DEBUG_AST_OPS__?: Op[];
  }
}

/**
 * Change highlighting (changed.css), driven by the edit script.
 *
 * After each reconciled render the nodes the ops touched (see
 * `astdiff`'s `Touched`) are mapped to their DOM through the hosted-node
 * store and given a class:
 *  - `q2-flash-inserted`: the node's own box, and every box inside it for a
 *    subtree insert — all of it is new;
 *  - `q2-flash-changed`: a node whose attributes, properties or text
 *    changed, or that was split or merged (its own box only);
 *  - `q2-flash-deleted`: a static ghost of a deleted node, left in its
 *    place for the highlight duration (see `ghostRemoved`);
 *  - `q2-flash-removed`: a container that had a child unwrapped out of
 *    it — the children stay on screen, so there is nothing to ghost.
 * Text inside a run paints its own character-level diff (textRun.tsx).
 *
 * The class comes off on `animationend`. Node markup is rendered through
 * portals in the commits that follow `root.render`, so the painting is
 * retried over a few animation frames until every host has content.
 */
const FLASH_CLASSES = ['q2-flash-inserted', 'q2-flash-changed', 'q2-flash-removed'];

function isTextLike(node: unknown): boolean {
  const t = (node as { t?: unknown } | null)?.t;
  return t === 'Str' || t === 'Space' || t === 'SoftBreak';
}
const HIGHLIGHT_MS = 1000;

/**
 * A deleted node is gone from the AST, and React drops its DOM on the next
 * commit. To show it going, snapshot its box *before* rendering: a static
 * clone is left in its place (React ignores DOM it doesn't own), held solid
 * red for the highlight duration, then removed.
 *
 * The hold is a Web Animations API animation rather than a CSS one on
 * purpose: a CSS animation restarts whenever its element is re-inserted
 * into the DOM, and the ghost lives inside the parent's markup, which the
 * hosted-node layer may move during the very same update (a wrap or
 * unwrap next to the deletion). A WAAPI animation stays attached to the
 * element and keeps running through such moves.
 */
function ghostRemoved(nodes: unknown[]) {
  for (const node of nodes) {
    const host = hostOf(node);
    const box = host?.firstElementChild;
    const anchor = host?.parentElement;
    if (!box || !anchor || !anchor.parentNode) continue;
    const ghost = box.cloneNode(true) as HTMLElement;
    ghost.classList.remove(...FLASH_CLASSES);
    ghost.classList.add('q2-flash-deleted');
    ghost.setAttribute('data-q2-ghost', '');
    ghost.setAttribute('aria-hidden', 'true');
    ghost.style.pointerEvents = 'none';
    anchor.parentNode.insertBefore(ghost, anchor);
    // Held solid red for the highlight duration, then removed — no fade.
    // (Two identical keyframes: a single one would interpolate from the
    // element's current style, i.e. fade in.)
    const red = { backgroundColor: '#f29b9b', boxShadow: '0 0 0 2px #e03131' };
    const hold = ghost.animate([red, red], { duration: HIGHLIGHT_MS, fill: 'forwards' });
    const done = () => ghost.remove();
    hold.finished.then(done, done);
    // Fallback in case the animation never settles (reduced motion, hidden tab).
    window.setTimeout(done, HIGHLIGHT_MS * 2);
  }
}

function flash(el: Element, cls: string) {
  if (el.classList.contains(cls)) {
    // Restart a running animation: drop the class, force a reflow, re-add.
    el.classList.remove(cls);
    void (el as HTMLElement).offsetWidth;
  }
  el.classList.add(cls);
}

/** The element a node's component rendered at the top of its host. */
function boxOf(node: unknown, rootElement: HTMLElement): Element | null {
  if (node === currentAst) return rootElement.querySelector('.pandoc-content-debug');
  const host = hostOf(node);
  return host?.firstElementChild ?? null;
}

function paintTouched(touched: Touched, rootElement: HTMLElement, attempt = 0) {
  let missing = false;
  const paint = (nodes: unknown[], cls: string, subtree: boolean) => {
    for (const node of nodes) {
      const box = boxOf(node, rootElement);
      if (!box) { missing = true; continue; }
      flash(box, cls);
      if (subtree) {
        for (const inner of box.querySelectorAll('q2-host > *')) flash(inner, cls);
      }
    }
  };
  paint(touched.insertedSubtrees, 'q2-flash-inserted', true);
  paint(touched.createdNodes, 'q2-flash-inserted', false);
  paint(touched.changedNodes, 'q2-flash-changed', false);
  paint(touched.removedFrom, 'q2-flash-removed', false);
  if (missing && attempt < 5) {
    requestAnimationFrame(() => paintTouched(touched, rootElement, attempt + 1));
  }
}

let flashListenerInstalled = false;
function ensureFlashListener(rootElement: HTMLElement) {
  if (flashListenerInstalled) return;
  flashListenerInstalled = true;
  rootElement.addEventListener('animationend', (e) => {
    if (e.target instanceof Element) e.target.classList.remove(...FLASH_CLASSES);
  });
}

// Stable callbacks so re-renders hand `<Ast>` identical props.
const onNavigateToDocument = (path: string, anchor: string | null) => {
  window.parent.postMessage({ type: 'NAVIGATE_TO_DOCUMENT', path, anchor }, '*');
};
const setAst = (newAst: PandocAST) => {
  window.parent.postMessage({ type: 'SET_AST', ast: newAst }, '*');
};

interface UpdateAstPayload {
  astJson: string;
  currentFilePath: string;
}

// Shared dispatcher gates UPDATE_AST on the in-flight
// LOAD_CUSTOM_COMPONENTS promise so two UPDATE_ASTs queued during
// component load run in arrival order. See
// `../iframeMessageDispatch.ts` for the rationale (the previous
// setInterval-polling pattern was phase-racy).
const dispatch = makeIframeMessageDispatcher({
  loadCustomComponents,
  updateAst: (payload) => updateAst(payload as UpdateAstPayload),
});

window.addEventListener('message', (event) => {
  // In production, verify event.origin for security
  dispatch(event.data as IframeMessage);
});

/**
 * Load custom components from transpiled JS code using dynamic imports.
 */
async function loadCustomComponents(componentsCode: Record<string, string>) {
  // Make React, RevealReact, and katex available as globals for user
  // TSX modules. The renderer-surface global is set at module top
  // (see above); these three remain lazy because they're tied to
  // dynamic user-TSX imports — `loadCustomComponents` is the right
  // moment to materialize them.
  (window as any).React = React;
  (window as any).RevealReact = { Deck, Slide };
  (window as any).katex = katex;

  const loadedModules: ComponentExports[] = [];
  for (const [componentName, code] of Object.entries(componentsCode)) {
    try {
      const blob = new Blob([code], { type: 'application/javascript' });
      const url = URL.createObjectURL(blob);
      try {
        const module = await import(/* @vite-ignore */ url);
        loadedModules.push(module as ComponentExports);
        console.log(`[Q2DebugIframe] Loaded custom component: ${componentName}`);
      } finally {
        URL.revokeObjectURL(url);
      }
    } catch (err) {
      console.error(`[Q2DebugIframe] Failed to load custom component ${componentName}:`, err);
    }
  }

  customRegistry = buildCustomRegistry(loadedModules);
}

/**
 * Render an AST update.
 *
 * Instead of handing React a brand-new AST each time, the fresh parse is
 * diffed against the AST already on screen (`currentAst`) and the
 * resulting edit script is applied to it in place. Untouched nodes keep
 * their object identity, so:
 *  - React (keyed by identity via `enableStableNodeKeys`) re-renders only
 *    the changed nodes and leaves everyone else's DOM alone;
 *  - anything holding a reference to a node (`currentAst.blocks[3]`)
 *    keeps pointing at the same content across renders.
 *
 * On the first render, or if reconciliation can't reproduce the fresh
 * parse, the fresh AST is used as-is.
 */
function updateAst(payload: UpdateAstPayload) {
  const {
    astJson,
    currentFilePath,
  } = payload;

  // Merge q2-debug defaults with any user-TSX overrides. The cast asserts
  // the merged result satisfies the FormatRegistry contract; the override
  // side is babel-transpiled user code and runtime-trusted.
  const mergedRegistry: FormatRegistry = {
    ...q2DebugRegistry,
    ...customRegistry,
  } as FormatRegistry;

  const rootElement = document.getElementById('root');
  if (!rootElement) {
    console.error('Root element not found');
    return;
  }

  try {
    let fresh: PandocAST;
    try {
      fresh = JSON.parse(astJson) as PandocAST;
    } catch (err) {
      throw new Error(`Failed to parse AST: ${err instanceof Error ? err.message : String(err)}`);
    }

    if (currentAstPath !== currentFilePath) {
      currentAst = null;
      currentAstPath = currentFilePath;
    }
    setTextChangeLedger({ inserted: new Set(), changed: new Map(), removed: [] });
    const result = reconcileAst(currentAst, fresh);
    currentAst = result.ast;
    window.__Q2_DEBUG_AST_OPS__ = result.ops;
    if (result.reused) {
      console.debug(`[q2-debug] applied ${result.ops.length} AST op(s)`, result.ops.map(describeOp));
      ensureFlashListener(rootElement);
      const touched = result.touched;
      // Deleted nodes still have their DOM until the render below commits.
      ghostRemoved(touched.removedNodes);
      // Text runs paint character changes from the script, not by guessing.
      setTextChangeLedger({
        inserted: new Set(touched.insertedSubtrees.filter(isTextLike)),
        changed: new Map(touched.textChanges.map((c) => [c.node, { old: c.old, new: c.new }])),
        removed: touched.removedText,
      });
      requestAnimationFrame(() => paintTouched(touched, rootElement));
    }

    // Create root only once
    if (!root) {
      root = createRoot(rootElement);
    }

    // Render the Ast component. `currentAst` is usually the same object as
    // last time; React still walks the tree, but identity-keyed children
    // let it keep the DOM of every node the edit script didn't touch.
    root.render(
      <Ast
        ast={currentAst}
        currentFilePath={currentFilePath}
        onNavigateToDocument={onNavigateToDocument}
        setAst={setAst}
        registry={mergedRegistry}
      />
    );
  } catch (err) {
    console.error('Failed to render AST:', err);
    rootElement.innerHTML = `
      <div style="padding: 20px; color: red;">
        <strong>Render Error:</strong>
        <pre>${err instanceof Error ? err.message : String(err)}</pre>
      </div>
    `;
  }
}

// Signal that the iframe is ready to receive messages
window.parent.postMessage({ type: 'IFRAME_READY' }, '*');
