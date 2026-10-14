/**
 * Content-derived React keys for AST node lists.
 *
 * Keying children by array index means an insertion or removal shifts every
 * later sibling onto a different element. React then reconciles each slot
 * against whatever node now sits there: same tag → a re-render with foreign
 * props (and any per-instance state, such as an open editor, lands on the
 * wrong node); different tag → unmount + remount. Either way the whole tail
 * of the list churns.
 *
 * Nothing in the AST is a stable per-node id — the source-info pool index
 * (`s`) and the location (`l`) both shift with any edit above the node — so
 * the key is derived from what the node *is*: its tag, its attribute id
 * when it has one, and a hash of a bounded prefix of its own content
 * (attributes + the first ~64 characters of text). That stays fixed while
 * siblings move and only changes when the node's own head is edited.
 *
 * Siblings that are byte-identical in that prefix (`Space`, repeated
 * words) get an occurrence suffix so keys stay unique within one parent.
 */

import { getDebugSettings } from '../q2-preview/debugSettings';

const PREFIX_BUDGET = 64;

/**
 * TEMPORARY DEBUGGING SWITCH: when off, reproduces the pre-change behavior
 * (array-index keys) so the churn colors show what it cost; when on, keys
 * by content identity. Toggled from the in-preview DebugBar.
 */
export function useContentKeys(): boolean {
    return getDebugSettings().contentKeys;
}

/** Node fields that carry identity; `s` / `l` are deliberately excluded. */
const IDENTITY_FIELDS = ['t', 'type_name', 'attr', 'c', 'slots', 'itemAttr'] as const;

/** Appends a bounded textual prefix of `value` to `acc`. Returns the new accumulator. */
function walk(value: unknown, acc: string): string {
    if (acc.length >= PREFIX_BUDGET) return acc;
    if (typeof value === 'string') return acc + value + '\u0001';
    if (typeof value === 'number' || typeof value === 'boolean') return acc + String(value) + '\u0001';
    if (Array.isArray(value)) {
        for (const item of value) {
            acc = walk(item, acc);
            if (acc.length >= PREFIX_BUDGET) break;
        }
        return acc;
    }
    if (value !== null && typeof value === 'object') {
        const obj = value as Record<string, unknown>;
        // Slots are keyed objects; include the slot name so reordering
        // slot contents changes the prefix.
        if (obj.kind !== undefined && obj.value !== undefined && obj.t === undefined) {
            return walk(obj.value, acc);
        }
        for (const field of IDENTITY_FIELDS) {
            if (obj[field] !== undefined) {
                acc = walk(obj[field], acc);
                if (acc.length >= PREFIX_BUDGET) break;
            }
        }
        return acc;
    }
    return acc;
}

/** FNV-1a 32-bit, base36. Short, fast, good enough for sibling keys. */
function hash(s: string): string {
    let h = 0x811c9dc5;
    for (let i = 0; i < s.length; i++) {
        h ^= s.charCodeAt(i);
        h = Math.imul(h, 0x01000193);
    }
    return (h >>> 0).toString(36);
}

/** The attribute id of a node, if its shape carries one. */
function nodeId(node: unknown): string {
    const n = node as { t?: string; attr?: unknown[]; c?: unknown };
    if (Array.isArray(n.attr) && typeof n.attr[0] === 'string') return n.attr[0];
    const c = n.c;
    if (!Array.isArray(c)) return '';
    if (n.t === 'Header' && Array.isArray(c[1]) && typeof c[1][0] === 'string') return c[1][0];
    if (Array.isArray(c[0]) && typeof c[0][0] === 'string' && Array.isArray(c[0][1])) return c[0][0];
    return '';
}

/** Identity string for one node: `Tag#id:hash` (the `#id` part only when present). */
export function nodeIdentity(node: unknown): string {
    const n = node as { t?: string; type_name?: string } | null | undefined;
    if (n === null || n === undefined) return 'null';
    const tag = n.type_name ?? n.t ?? 'node';
    const id = nodeId(n);
    return `${tag}${id !== '' ? `#${id}` : ''}:${hash(walk(n, ''))}`;
}

/**
 * Unique keys for a list of sibling nodes. Identical identities are
 * disambiguated by occurrence order (`~1`, `~2`, …).
 */
export function childKeys(nodes: readonly unknown[]): string[] {
    if (useContentKeys() === false) return nodes.map((_, i) => String(i));
    const seen = new Map<string, number>();
    return nodes.map((node) => {
        const base = nodeIdentity(node);
        const n = seen.get(base) ?? 0;
        seen.set(base, n + 1);
        return n === 0 ? base : `${base}~${n}`;
    });
}

/**
 * Keys for list items (`BlockNode[][]`): an item is identified by its
 * blocks taken together, so a one-block item and the same block inside
 * a two-block item differ.
 */
export function itemKeys(items: readonly (readonly unknown[])[]): string[] {
    if (useContentKeys() === false) return items.map((_, i) => String(i));
    const seen = new Map<string, number>();
    return items.map((item) => {
        const base = item.length === 0 ? 'empty' : item.map(nodeIdentity).join('+');
        const n = seen.get(base) ?? 0;
        seen.set(base, n + 1);
        return n === 0 ? base : `${base}~${n}`;
    });
}

/** `nodes.map` with a precomputed stable key handed to the callback. */
export function mapKeyed<T, R>(
    nodes: readonly T[],
    fn: (node: T, index: number, key: string) => R,
): R[] {
    const keys = childKeys(nodes);
    return nodes.map((node, i) => fn(node, i, keys[i]));
}

/** `items.map` for `BlockNode[][]`-shaped lists, with a precomputed stable key. */
export function mapItemsKeyed<T extends readonly unknown[], R>(
    items: readonly T[],
    fn: (item: T, index: number, key: string) => R,
): R[] {
    const keys = itemKeys(items);
    return items.map((item, i) => fn(item, i, keys[i]));
}
