/**
 * Entry point for the q2-debug renderer iframe.
 *
 * Loaded by `/q2-debug.html`; speaks the postMessage protocol with the
 * parent (`Q2DebugIframe`). The document is rendered without React: the
 * fresh parse is diffed against the AST already on screen, the edit script
 * is applied to that AST in place, and the DOM is patched to match —
 * see `dom/document.ts`. Untouched nodes keep both their AST objects and
 * their DOM, and every highlight is derived from the script.
 */

import './dom/debug.css';
import type { PandocAST } from '@quarto/preview-renderer/framework';
import { describeOp, type Op } from '@quarto/preview-renderer/astdiff';
import {
    makeIframeMessageDispatcher,
    type IframeMessage,
} from '../iframeMessageDispatch';
import { DebugDocument } from './dom/document';

interface UpdateAstPayload {
  astJson: string;
  currentFilePath: string;
}

/** Last edit script applied, exposed for debugging / e2e inspection. */
declare global {
  interface Window {
    __Q2_DEBUG_AST_OPS__?: Op[];
  }
}

let doc: DebugDocument | null = null;
let currentPath: string | null = null;

// Shared dispatcher gates UPDATE_AST on the in-flight LOAD_CUSTOM_COMPONENTS
// promise so messages run in arrival order (see ../iframeMessageDispatch.ts).
const dispatch = makeIframeMessageDispatcher({
  loadCustomComponents: async (componentsCode) => {
    // User render components are React components; this renderer has no
    // React. Accepted and ignored so the parent's protocol is unchanged.
    const names = Object.keys(componentsCode);
    if (names.length > 0) {
      console.warn('[q2-debug] custom render components are not supported by the DOM renderer; ignoring', names);
    }
  },
  updateAst: (payload) => updateAst(payload as UpdateAstPayload),
});

window.addEventListener('message', (event) => {
  // In production, verify event.origin for security
  dispatch(event.data as IframeMessage);
});

function updateAst(payload: UpdateAstPayload) {
  const { astJson, currentFilePath } = payload;
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
    if (!doc) {
      const container = document.createElement('div');
      container.className = 'pandoc-content-debug';
      rootElement.replaceChildren(container);
      doc = new DebugDocument(container);
    }
    if (currentPath !== currentFilePath) {
      // A diff against a different document would be large and meaningless.
      doc.reset();
      currentPath = currentFilePath;
    }
    const result = doc.update(fresh);
    window.__Q2_DEBUG_AST_OPS__ = result.ops;
    if (result.reused) {
      console.debug(`[q2-debug] applied ${result.ops.length} AST op(s)`, result.ops.map(describeOp));
    }
  } catch (err) {
    console.error('Failed to render AST:', err);
    doc = null;
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
