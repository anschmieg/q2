/**
 * TEMPORARY DEBUGGING AID — remove before shipping.
 *
 * Watches the preview document with a MutationObserver and flashes every
 * element the moment it is inserted into the DOM, so you can see exactly
 * which DOM nodes React recreated during reconciliation (as opposed to
 * React component remounts, which the churn colors show). A node React
 * merely updated in place does not flash; a text-content change flashes
 * its parent in a second color.
 *
 * Elements under `[data-q2-debug-ignore]` (the DebugBar) are skipped.
 */

const STYLE_ID = 'q2-dom-flash-style';
const ADDED_CLASS = 'q2-dom-flash-added';
const TEXT_CLASS = 'q2-dom-flash-text';
const FLASH_MS = 900;

function ensureStyle(doc: Document): void {
    if (doc.getElementById(STYLE_ID) !== null) return;
    const style = doc.createElement('style');
    style.id = STYLE_ID;
    style.textContent = `
@keyframes q2-dom-flash-added { from { outline-color: #00d000; box-shadow: 0 0 0 3px #00d00080; } to { outline-color: transparent; box-shadow: 0 0 0 3px transparent; } }
@keyframes q2-dom-flash-text  { from { outline-color: #ff8800; box-shadow: 0 0 0 3px #ff880080; } to { outline-color: transparent; box-shadow: 0 0 0 3px transparent; } }
.${ADDED_CLASS} { outline: 2px solid transparent; animation: q2-dom-flash-added ${FLASH_MS}ms ease-out; }
.${TEXT_CLASS}  { outline: 2px solid transparent; animation: q2-dom-flash-text ${FLASH_MS}ms ease-out; }
`;
    doc.head.appendChild(style);
}

function ignored(node: Node): boolean {
    const el = node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement;
    return el !== null && el.closest('[data-q2-debug-ignore]') !== null;
}

function flash(el: Element, cls: string): void {
    // Restart the animation if the element is already flashing.
    el.classList.remove(cls);
    void (el as HTMLElement).offsetWidth;
    el.classList.add(cls);
    window.setTimeout(() => el.classList.remove(cls), FLASH_MS);
}

/** Start flashing DOM insertions under `root`. Returns a stop function. */
export function startDomFlash(root: Node): () => void {
    const doc = root.ownerDocument ?? document;
    ensureStyle(doc);
    const mo = new MutationObserver((records) => {
        for (const r of records) {
            if (r.type === 'childList') {
                for (const n of r.addedNodes) {
                    if (ignored(n)) continue;
                    if (n.nodeType === Node.ELEMENT_NODE) {
                        flash(n as Element, ADDED_CLASS);
                    } else if (n.nodeType === Node.TEXT_NODE && n.parentElement !== null) {
                        flash(n.parentElement, TEXT_CLASS);
                    }
                }
            } else if (r.type === 'characterData') {
                const parent = r.target.parentElement;
                if (parent !== null && ignored(parent) === false) flash(parent, TEXT_CLASS);
            }
        }
    });
    mo.observe(root, { childList: true, subtree: true, characterData: true });
    return () => mo.disconnect();
}
