/**
 * The q2-debug document: a DOM tree that mirrors the AST by node identity,
 * maintained from the edit scripts `reconcileAst` applies. No React.
 *
 * Every AST node owns one box element (`renderNode`), remembered in a
 * WeakMap keyed by the node object. Consecutive Str/Space/SoftBreak
 * inlines are shown as one "Text:" run instead of individual boxes.
 *
 * On each update:
 *  1. `reconcileAst` diffs the fresh parse against the retained AST and
 *     patches the AST in place; untouched node objects survive.
 *  2. Boxes of nodes whose own fields changed are updated in place (label,
 *     text); the element itself is kept.
 *  3. Containers whose child lists changed get their slot contents
 *     re-placed from the AST: existing boxes are moved into position,
 *     new nodes get new boxes, boxes of nodes no longer listed drop out.
 *     Nothing else in the DOM is touched.
 *  4. Highlights are painted from the same report: inserted (green),
 *     changed (yellow), a child unwrapped out (red outline), and a ghost of
 *     each deleted node held in place (red) before it goes. Text runs paint
 *     character-level changes from the script's text facts.
 */

import { reconcileAst, describe, type Elem, type ReconcileResult, type Touched } from '@quarto/preview-renderer/astdiff';
import type { PandocAST } from '@quarto/preview-renderer/framework';
import { NODE_CLASS, refreshNode, renderNode } from './render';
import { emptyLedger, isTextLike, ledgerFrom, segmentsFromLedger, textOf, type Seg, type TextChangeLedger } from './textDiff';

const HIGHLIGHT_MS = 1000;
const FLASH_CLASSES = ['q2-flash-inserted', 'q2-flash-changed', 'q2-flash-removed'];
const GHOST_ATTR = 'data-q2-ghost';
const RUN_ATTR = 'data-q2-run';

/** Solid colour held for the highlight duration. Two identical keyframes on purpose: one would fade in. */
function hold(el: Element, style: Keyframe): Animation | null {
    if (typeof el.animate !== 'function') return null;
    return el.animate([style, style], { duration: HIGHLIGHT_MS, fill: 'forwards' });
}

interface RunRecord {
    el: HTMLElement;
    nodes: unknown[];
    text: string;
}

export class DebugDocument {
    private readonly container: HTMLElement;
    private ast: PandocAST | null = null;
    private readonly roots = new WeakMap<object, HTMLElement>();
    private readonly slots = new WeakMap<object, Record<string, HTMLElement>>();
    private readonly elems = new WeakMap<object, Elem>();
    private readonly runKeyOf = new WeakMap<object, object>();
    private readonly runs = new WeakMap<object, RunRecord>();
    private ledger: TextChangeLedger = emptyLedger();

    constructor(container: HTMLElement) {
        this.container = container;
        container.addEventListener('animationend', (e) => {
            if (e.target instanceof Element) e.target.classList.remove(...FLASH_CLASSES);
        });
    }

    /** Forget the retained AST; the next update renders from scratch. */
    reset(): void {
        this.ast = null;
        this.container.replaceChildren();
    }

    update(fresh: PandocAST): ReconcileResult {
        const result = reconcileAst(this.ast, fresh);
        this.ast = result.ast;
        if (!result.reused) {
            this.ledger = emptyLedger();
            this.container.replaceChildren();
            this.placeList(this.container, this.ast.blocks, 'block');
            return result;
        }
        this.ledger = ledgerFrom(result.touched);
        this.ghost(result.touched.removedNodes);
        this.patch(result.touched);
        this.paint(result.touched);
        return result;
    }

    /** The box element of `node`, if rendered. */
    boxOf(node: unknown): HTMLElement | undefined {
        if (node === this.ast) return this.container;
        return node !== null && typeof node === 'object' ? this.roots.get(node) : undefined;
    }

    // --- building and placing -------------------------------------------

    private ensureBox(node: unknown, elem: Elem): HTMLElement {
        const key = node as object;
        const existing = this.roots.get(key);
        if (existing) return existing;
        const { root, slots } = renderNode(node, elem);
        this.roots.set(key, root);
        this.slots.set(key, slots);
        this.elems.set(key, elem);
        this.placeChildren(node);
        return root;
    }

    /** Update `node`'s own markup in place and re-place its children. */
    private refreshBox(node: unknown): void {
        const key = node as object;
        const elem = this.elems.get(key);
        const root = this.roots.get(key);
        const slots = this.slots.get(key);
        if (elem === undefined || root === undefined || slots === undefined) return;
        refreshNode(node, elem, { root, slots });
        this.placeChildren(node);
    }

    /** Re-place `node`'s children from the AST into its slot containers. */
    private placeChildren(node: unknown): void {
        if (node === this.ast) {
            this.placeList(this.container, this.ast!.blocks, 'block');
            return;
        }
        const key = node as object;
        const elem = this.elems.get(key);
        const slotEls = this.slots.get(key);
        if (elem === undefined || slotEls === undefined) return;
        for (const slot of describe(node, elem).slots) {
            const el = slotEls[slot.name];
            if (el) this.placeList(el, slot.read(), slot.elem);
        }
    }

    private placeList(el: HTMLElement, children: unknown[], elem: Elem): void {
        const desired: HTMLElement[] = [];
        if (elem === 'inline') {
            const usedKeys = new Set<object>();
            let run: unknown[] = [];
            const flush = () => {
                if (run.length > 0) desired.push(this.runElement(run, usedKeys));
                run = [];
            };
            for (const child of children) {
                if (isTextLike(child)) {
                    run.push(child);
                } else {
                    flush();
                    desired.push(this.ensureBox(child, elem));
                }
            }
            flush();
        } else {
            for (const child of children) desired.push(this.ensureBox(child, elem));
        }
        syncChildren(el, desired);
    }

    // --- text runs --------------------------------------------------------

    /**
     * A run is keyed by whichever of its nodes was in a run before, so a
     * word added at its start doesn't make it a different run. `used`
     * keeps two runs in one slot from sharing a key.
     */
    private runKey(nodes: unknown[], used: Set<object>): object {
        let key: object | undefined;
        for (const n of nodes) {
            const k = this.runKeyOf.get(n as object);
            if (k !== undefined && !used.has(k)) { key = k; break; }
        }
        if (key === undefined || used.has(key)) key = nodes[0] as object;
        used.add(key);
        for (const n of nodes) this.runKeyOf.set(n as object, key);
        return key;
    }

    private runElement(nodes: unknown[], used: Set<object>): HTMLElement {
        const key = this.runKey(nodes, used);
        const text = nodes.map(textOf).join('');
        const rec = this.runs.get(key);
        if (rec && rec.text === text && rec.nodes.length === nodes.length && rec.nodes.every((n, i) => n === nodes[i])) {
            return rec.el;
        }
        const el = buildRun(segmentsFromLedger(nodes, this.ledger), text);
        this.runs.set(key, { el, nodes: nodes.slice(), text });
        return el;
    }

    // --- highlighting -----------------------------------------------------

    /** Leave a static copy of each deleted node's box in place, held red, then drop it. */
    private ghost(nodes: unknown[]): void {
        for (const node of nodes) {
            const box = this.roots.get(node as object);
            if (!box || !box.isConnected) continue;
            const ghost = box.cloneNode(true) as HTMLElement;
            ghost.setAttribute(GHOST_ATTR, '');
            ghost.setAttribute('aria-hidden', 'true');
            ghost.style.pointerEvents = 'none';
            ghost.querySelectorAll(`[${GHOST_ATTR}]`).forEach((g) => g.remove());
            box.before(ghost);
            const done = () => ghost.remove();
            const anim = hold(ghost, { backgroundColor: '#f29b9b', boxShadow: '0 0 0 2px #e03131' });
            if (anim) anim.finished.then(done, done);
            window.setTimeout(done, HIGHLIGHT_MS * 2);
        }
    }

    private patch(touched: Touched): void {
        for (const node of touched.changedNodes) this.refreshBox(node);
        for (const node of touched.relisted) this.placeChildren(node);
    }

    private paint(touched: Touched): void {
        const flash = (el: Element, cls: string) => {
            if (el.classList.contains(cls)) {
                el.classList.remove(cls);
                void (el as HTMLElement).offsetWidth;
            }
            el.classList.add(cls);
        };
        for (const node of touched.insertedSubtrees) {
            const box = this.boxOf(node);
            if (!box) continue;
            flash(box, 'q2-flash-inserted');
            for (const inner of box.querySelectorAll(`.${NODE_CLASS}`)) flash(inner, 'q2-flash-inserted');
        }
        for (const node of touched.createdNodes) {
            const box = this.boxOf(node);
            if (box) flash(box, 'q2-flash-inserted');
        }
        for (const node of touched.changedNodes) {
            const box = this.boxOf(node);
            if (box) flash(box, 'q2-flash-changed');
        }
        for (const node of touched.removedFrom) {
            const box = this.boxOf(node);
            if (box) flash(box, 'q2-flash-removed');
        }
    }
}

/**
 * Make `el`'s element children equal `desired`, in order, moving existing
 * elements rather than recreating them. Ghosts (`data-q2-ghost`) are left
 * where they are and never counted.
 */
function syncChildren(el: HTMLElement, desired: HTMLElement[]): void {
    const isGhost = (n: Element | null) => n !== null && n.hasAttribute(GHOST_ATTR);
    const nextReal = (n: Element | null): Element | null => {
        while (isGhost(n)) n = n!.nextElementSibling;
        return n;
    };
    let cursor = nextReal(el.firstElementChild);
    for (const want of desired) {
        if (cursor === want) {
            cursor = nextReal(cursor.nextElementSibling);
        } else {
            el.insertBefore(want, cursor);
        }
    }
    while (cursor !== null) {
        const next = nextReal(cursor.nextElementSibling);
        cursor.remove();
        cursor = next;
    }
}

function buildRun(segs: Seg[], text: string): HTMLElement {
    const el = document.createElement('span');
    el.className = `${NODE_CLASS} inline run`;
    el.setAttribute(RUN_ATTR, '');
    const label = document.createElement('strong');
    label.className = 'label';
    label.textContent = 'Text:';
    el.append(label, ' ');
    let marked = false;
    for (const seg of segs) {
        if (seg.kind === 'eq') {
            el.append(seg.text);
            continue;
        }
        marked = true;
        const span = document.createElement('span');
        span.className = seg.kind === 'ins' ? 'q2-ins' : 'q2-del';
        span.textContent = seg.text;
        el.append(span);
        hold(span, { backgroundColor: seg.kind === 'ins' ? '#8ee6a0' : '#f29b9b' });
    }
    if (marked) {
        // Collapse to plain text when the highlight ends: removed characters
        // go, inserted ones lose their span.
        window.setTimeout(() => el.replaceChildren(label, ' ', text), HIGHLIGHT_MS);
    }
    return el;
}
