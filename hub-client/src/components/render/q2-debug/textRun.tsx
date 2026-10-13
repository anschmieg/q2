/**
 * Text runs for q2-debug: consecutive `Str` / `Space` / `SoftBreak` inlines
 * shown as one box, with character-level change highlighting.
 *
 * `renderInlineChildren` wraps the framework's `renderChildren`: it takes
 * the `<Node>` elements the framework produced and folds every maximal run
 * of text-like inlines into a `<TextRun>`. Other inlines (Emph, Link, ...)
 * stay as they are.
 *
 * When a run's text changes it paints the change *as the edit script
 * describes it* (see `setTextChangeLedger`): characters of inserted text
 * nodes and of a changed node's char diff sit in a `.q2-ins` span (held
 * green), deleted text nodes' characters in a `.q2-del` span at the
 * place they were removed from (held red, dropped after
 * `REMOVE_TTL_MS`). Text that merely moved — a paragraph split or merged,
 * a word wrapped in emphasis — is not a text change and paints nothing.
 * After `INSERT_TTL_MS` the run collapses to plain text.
 *
 * Run identity across renders: the run is keyed by the first of its nodes
 * that was already on screen last time, so an edit that adds or splits
 * words at the start of a run does not reset its history.
 */

import React, { useEffect, useReducer, useRef } from 'react';
import { Node, childKey } from '@quarto/preview-renderer/framework';
import type { BlockNode, InlineNode, NodeArgs } from '@quarto/preview-renderer/framework';
import { inlineStyle } from './styles';

const TEXT_TAGS = new Set(['Str', 'Space', 'SoftBreak']);
/**
 * Removed characters show for this long, then are dropped; inserted ones
 * keep their green span for `INSERT_TTL_MS`, then the run collapses back
 * to plain text. Collapsing matters beyond tidiness: it keeps a run free of
 * stale change markup when later edits re-key its segments.
 */
const REMOVE_TTL_MS = 1000;
const INSERT_TTL_MS = 1000;

/**
 * The colour holds are Web Animations API animations created on mount, not
 * CSS animations: a CSS animation restarts whenever its element is re-inserted
 * into the DOM, and the hosted-node layer *moves* a paragraph's host when
 * something wraps or unwraps around it — which would replay every hold in
 * the run. A WAAPI animation stays attached to its element through moves.
 */
const HOLDS: Record<'ins' | 'del', { keyframes: Keyframe[]; duration: number }> = {
    // Solid colours, held — no interpolation. Both keyframes are the same
    // on purpose: with a single keyframe WAAPI interpolates *from the
    // element's current style* to it, i.e. a fade in. The span is unwrapped
    // (ins) or dropped (del) by the run's timers when the hold ends.
    ins: { keyframes: [{ backgroundColor: '#8ee6a0' }, { backgroundColor: '#8ee6a0' }], duration: INSERT_TTL_MS },
    del: { keyframes: [{ backgroundColor: '#f29b9b' }, { backgroundColor: '#f29b9b' }], duration: REMOVE_TTL_MS },
};

function ChangedText({ kind, text }: { kind: 'ins' | 'del'; text: string }) {
    const ref = useRef<HTMLSpanElement>(null);
    useEffect(() => {
        const el = ref.current;
        if (!el || typeof el.animate !== 'function') return;
        const hold = HOLDS[kind];
        const anim = el.animate(hold.keyframes, { duration: hold.duration, fill: 'forwards' });
        return () => anim.cancel();
    }, [kind]);
    return <span ref={ref} className={kind === 'ins' ? 'q2-ins' : 'q2-del'}>{text}</span>;
}

export function textOf(node: InlineNode): string {
    switch (node.t) {
        case 'Str': return (node as { c: string }).c;
        case 'Space': return ' ';
        case 'SoftBreak': return '\n';
        default: return '';
    }
}

/**
 * `at` is the segment's offset in the new text (eq/ins) or in the old text
 * (del); with the run's change version it keys the segment, so dropping
 * the `del` segments later leaves the surviving spans' keys — and their
 * running holds — untouched.
 */
type Seg = { kind: 'eq' | 'ins' | 'del'; text: string; at: number };

/** Char-level diff as a flat segment list (LCS; whole-replace past a size cap). */
export function charDiff(oldText: string, newText: string): Seg[] {
    const a = Array.from(oldText);
    const b = Array.from(newText);
    if (a.length * b.length > 400_000) {
        return [{ kind: 'del', text: oldText, at: 0 }, { kind: 'ins', text: newText, at: 0 }];
    }
    const n = a.length;
    const m = b.length;
    const dp = new Uint32Array((n + 1) * (m + 1));
    const at = (i: number, j: number) => i * (m + 1) + j;
    for (let i = n - 1; i >= 0; i--) {
        for (let j = m - 1; j >= 0; j--) {
            dp[at(i, j)] = a[i] === b[j] ? dp[at(i + 1, j + 1)] + 1 : Math.max(dp[at(i + 1, j)], dp[at(i, j + 1)]);
        }
    }
    const segs: Seg[] = [];
    const push = (kind: Seg['kind'], ch: string, pos: number) => {
        const last = segs[segs.length - 1];
        if (last && last.kind === kind) last.text += ch;
        else segs.push({ kind, text: ch, at: pos });
    };
    let i = 0;
    let j = 0;
    while (i < n || j < m) {
        if (i < n && j < m && a[i] === b[j]) { push('eq', a[i], j); i++; j++; }
        else if (j < m && (i >= n || dp[at(i, j + 1)] >= dp[at(i + 1, j)])) { push('ins', b[j], j); j++; }
        else { push('del', a[i], i); i++; }
    }
    return segs;
}

/**
 * Text-level facts from the last applied edit script, set by the renderer
 * entry before each render (from `astdiff`'s `Touched`).
 */
export interface TextChangeLedger {
    /** Text-like inline nodes that were inserted. */
    inserted: Set<unknown>;
    /** Nodes whose text changed, with the old and new text. */
    changed: Map<unknown, { old: string; new: string }>;
    /** Deleted text, with the siblings that surrounded it at that moment. */
    removed: { node: unknown; text: string; before: unknown | null; after: unknown | null }[];
}

let ledger: TextChangeLedger = { inserted: new Set(), changed: new Map(), removed: [] };

export function setTextChangeLedger(next: TextChangeLedger): void {
    ledger = next;
}

/** Segments for `nodes` according to the ledger; plain text if it says nothing. */
export function segmentsFromLedger(nodes: InlineNode[], ledger: TextChangeLedger): Seg[] {
    const inRun = new Set<unknown>(nodes);
    const segs: Seg[] = [];
    let pos = 0;
    const push = (kind: Seg['kind'], text: string, at: number) => {
        if (text.length === 0) return;
        const last = segs[segs.length - 1];
        // Deleted text occupies no new-text offset, so consecutive deletions
        // share `at`; merge them. Other kinds merge when contiguous.
        const joins = last && last.kind === kind && (kind === 'del' ? last.at === at : last.at + last.text.length === at);
        if (joins) last.text += text;
        else segs.push({ kind, text, at });
    };
    // A deletion's neighbours were recorded at the moment it happened; a
    // neighbour may itself have been deleted since, so resolve neighbours
    // through other removals to nodes that still exist.
    const removedByNode = new Map(ledger.removed.map((r) => [r.node, r]));
    const resolve = (node: unknown | null, side: 'before' | 'after'): unknown | null => {
        let cur = node;
        for (let guard = 0; cur !== null && removedByNode.has(cur) && guard < 1000; guard++) cur = removedByNode.get(cur)![side];
        return cur;
    };
    const placed = ledger.removed.map((r) => ({ text: r.text, before: resolve(r.before, 'before'), after: resolve(r.after, 'after') }));
    // Deleted text goes before the node that followed it, or after the node
    // that preceded it when the follower is not in this run.
    const delBefore = (node: unknown) => {
        for (const r of placed) if (r.before === node) push('del', r.text, pos);
    };
    const delAfter = (node: unknown) => {
        for (const r of placed) if (r.after === node && !(r.before !== null && inRun.has(r.before))) push('del', r.text, pos);
    };
    for (const node of nodes) {
        delBefore(node);
        const text = textOf(node);
        const change = ledger.changed.get(node);
        if (ledger.inserted.has(node)) {
            push('ins', text, pos);
        } else if (change) {
            for (const seg of charDiff(change.old, change.new)) {
                push(seg.kind, seg.text, pos + (seg.kind === 'del' ? seg.at : seg.at));
            }
        } else {
            push('eq', text, pos);
        }
        pos += text.length;
        delAfter(node);
    }
    return segs;
}

export function TextRun({ nodes }: { nodes: InlineNode[] }) {
    const text = nodes.map(textOf).join('');
    // What we showed last time, and how the current text differs from it.
    // `version` bumps on every text change and goes into the segment keys,
    // so each change renders *new* spans. Reusing a span (same position,
    // new text) would skip the CSS animation, which only plays on creation.
    const plain = (t: string): Seg[] => [{ kind: 'eq', text: t, at: 0 }];
    const shown = useRef<{ text: string; segs: Seg[]; version: number; changedAt: number }>({ text, segs: plain(text), version: 0, changedAt: 0 });
    if (shown.current.text !== text) {
        shown.current = { text, segs: segmentsFromLedger(nodes, ledger), version: shown.current.version + 1, changedAt: Date.now() };
    }
    const [, bump] = useReducer((x: number) => x + 1, 0);
    const hasDel = shown.current.segs.some((s) => s.kind === 'del');
    const hasIns = shown.current.segs.some((s) => s.kind === 'ins');
    const version = shown.current.version;
    useEffect(() => {
        if (!hasDel && !hasIns) return;
        // Timers are anchored to when the text changed, so the re-render
        // that drops the removed characters doesn't push the collapse out.
        const elapsed = Date.now() - shown.current.changedAt;
        const timers: number[] = [];
        if (hasDel) {
            timers.push(window.setTimeout(() => {
                // Drop removed characters; keys of the rest are unchanged, so
                // their spans (and running holds) survive this re-render.
                shown.current = { ...shown.current, segs: shown.current.segs.filter((s) => s.kind !== 'del') };
                bump();
            }, Math.max(0, REMOVE_TTL_MS - elapsed)));
        }
        timers.push(window.setTimeout(() => {
            // Collapse to plain text.
            shown.current = { ...shown.current, text, segs: plain(text), version: shown.current.version + 1 };
            bump();
        }, Math.max(0, INSERT_TTL_MS - elapsed)));
        return () => timers.forEach((t) => window.clearTimeout(t));
    }, [text, version, hasDel, hasIns]);

    return (
        <span style={{ ...inlineStyle, whiteSpace: 'pre-wrap' }} data-q2-run="">
            <strong>Text:</strong>{' '}
            {shown.current.segs.map((seg) => {
                // Offset-based so dropping `del` segments keeps the others'
                // keys. Unique because adjacent same-kind segments merge.
                const key = `${version}:${seg.kind === 'del' ? 'd' : 'n'}${seg.at}`;
                return seg.kind === 'eq'
                    ? <React.Fragment key={key}>{seg.text}</React.Fragment>
                    : <ChangedText key={key} kind={seg.kind} text={seg.text} />;
            })}
        </span>
    );
}

function isTextNodeElement(el: React.ReactNode): el is React.ReactElement<{ node: InlineNode }> {
    return React.isValidElement(el)
        && el.type === Node
        && TEXT_TAGS.has(((el.props as { node?: { t?: string } }).node?.t) ?? '');
}

/** The run key each node was last rendered under. */
const runKeyOf = new WeakMap<object, React.Key>();

/**
 * Key a run by the key of whichever of its nodes was in a run before, so
 * words added or split off at the start don't reset the run's history.
 * (The framework's `childKey` can't tell us that: it has keyed every
 * child, new or not, by the time the run is assembled. And a render pass
 * may happen twice per update, so "seen in a previous render" is not
 * "seen in a previous update" either — hence remembering the key itself.)
 * `used` keeps two runs under one parent from sharing a key when a run is
 * split in two by an inserted inline.
 */
function runKey(nodes: InlineNode[], used: Set<React.Key>): React.Key {
    let key: React.Key | undefined;
    for (const n of nodes) {
        const k = runKeyOf.get(n);
        if (k !== undefined && !used.has(k)) { key = k; break; }
    }
    if (key === undefined) {
        key = childKey(nodes[0], 0);
        if (used.has(key)) key = `${key}-${used.size}`;
    }
    used.add(key);
    for (const n of nodes) runKeyOf.set(n, key);
    return key;
}

/**
 * `renderChildren` for inline containers, with text-like inlines folded
 * into `<TextRun>`s.
 */
export function renderInlineChildren<T extends BlockNode | InlineNode>(
    args: NodeArgs<T>,
    renderChildren: (a: NodeArgs<T>) => React.ReactNode,
): React.ReactNode {
    const rendered = renderChildren(args);
    const items = React.Children.toArray(rendered);
    const out: React.ReactNode[] = [];
    const used = new Set<React.Key>();
    let run: InlineNode[] = [];
    const flush = () => {
        if (run.length === 0) return;
        out.push(<TextRun key={runKey(run, used)} nodes={run} />);
        run = [];
    };
    for (const item of items) {
        if (isTextNodeElement(item)) {
            run.push(item.props.node);
        } else {
            flush();
            out.push(item);
        }
    }
    flush();
    return out;
}
