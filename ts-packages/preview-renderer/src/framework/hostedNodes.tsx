/**
 * Hosted nodes: persistent DOM per AST node, moved rather than rebuilt.
 *
 * React never moves DOM between parents — when a node is wrapped in a new
 * container, React unmounts the child's subtree and mounts it again under
 * the wrapper. For a renderer that patches its AST in place (q2-debug via
 * `astdiff/reconcileAst`) that would rebuild the DOM of every wrapped
 * node, defeating the point of keeping node identity.
 *
 * With hosted nodes enabled, `<Node>` no longer renders a node's markup in
 * place. Instead:
 *
 *  - every AST node gets a persistent `<q2-host>` element (display:
 *    contents), owned by a module-level store keyed by node identity;
 *  - the node's markup is rendered into that host through a React portal
 *    from a flat `<HostedPortals>` list mounted once at the framework root,
 *    so a node's portal fiber has the same parent no matter where the node
 *    sits in the AST, and survives any restructuring above it;
 *  - in the parent's markup, `<Node>` renders an empty `<q2-anchor>` and a
 *    layout effect moves the child's host into it. Re-anchoring is a DOM
 *    `appendChild` of the existing element: the subtree is untouched.
 *
 * A host whose anchor disappears is released on a zero-delay timer; if it
 * has been re-anchored by then (wrap, split, merge, parent re-render) the
 * release is cancelled, otherwise its portal is unmounted.
 *
 * Opt in with `enableHostedNodes()` from the renderer entry (q2-debug).
 * q2-preview keeps the default inline rendering.
 */

import React, { useLayoutEffect, useRef, useSyncExternalStore } from 'react';
import { createPortal } from 'react-dom';
import type { BlockNode, InlineNode } from './types';

export interface HostedNodeProps {
    node: BlockNode | InlineNode;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setLocalAst: (newNode: BlockNode | InlineNode) => void;
}

interface Entry {
    node: object;
    props: HostedNodeProps;
    host: HTMLElement;
    key: string;
    /** Bumped when `props` change; subscribed to by that node's portal. */
    version: number;
    listeners: Set<() => void>;
    /** Pending release timer, if the anchor went away. */
    removal: number | null;
}

let enabled = false;

export function enableHostedNodes(): void {
    enabled = true;
}

export function hostedNodesEnabled(): boolean {
    return enabled;
}

const entries = new Map<object, Entry>();

/** The persistent host element of `node`, if it is (still) rendered. */
export function hostOf(node: unknown): HTMLElement | undefined {
    return node !== null && typeof node === 'object' ? entries.get(node)?.host : undefined;
}
const structureListeners = new Set<() => void>();
let structureVersion = 0;
let nextKey = 1;

function notifyStructure(): void {
    structureVersion++;
    for (const l of structureListeners) l();
}

function subscribeStructure(cb: () => void): () => void {
    structureListeners.add(cb);
    return () => { structureListeners.delete(cb); };
}

function register(props: HostedNodeProps): Entry {
    const node = props.node as object;
    let entry = entries.get(node);
    if (entry) {
        entry.props = props;
        entry.version++;
        if (entry.removal !== null) {
            window.clearTimeout(entry.removal);
            entry.removal = null;
        }
        for (const l of entry.listeners) l();
        return entry;
    }
    const host = document.createElement('q2-host');
    host.style.display = 'contents';
    entry = { node, props, host, key: `h${nextKey++}`, version: 0, listeners: new Set(), removal: null };
    entries.set(node, entry);
    notifyStructure();
    return entry;
}

function release(node: object): void {
    const entry = entries.get(node);
    if (!entry || entry.removal !== null) return;
    entry.removal = window.setTimeout(() => {
        entry.removal = null;
        if (!entry.host.isConnected) {
            entries.delete(node);
            notifyStructure();
        }
    }, 0);
}

/**
 * Placeholder rendered in the parent's markup. On every commit it makes
 * sure the node's host sits inside this anchor and refreshes the props
 * (`setLocalAst` closures change with each parent render).
 *
 * Only an anchor that is itself in the document may claim the host. When
 * a node moves (unwrap, split, merge), the anchor in its *old* parent's
 * markup lives on inside a detached subtree until that parent's portal is
 * released, and it still re-renders with the document in the meantime.
 * Letting it register would overwrite the props with stale closures and
 * pull the host back out of the live tree — which then looks like the
 * node vanished once the release timer finds it disconnected.
 */
export function HostAnchor(props: HostedNodeProps) {
    const ref = useRef<HTMLElement>(null);
    useLayoutEffect(() => {
        const anchor = ref.current;
        if (!anchor || !anchor.isConnected) return;
        const entry = register(props);
        if (entry.host.parentNode !== anchor) anchor.appendChild(entry.host);
        return () => release(props.node as object);
    });
    return React.createElement('q2-anchor', { ref, style: { display: 'contents' } });
}

function PortalContent({ entry, render }: { entry: Entry; render: (props: HostedNodeProps) => React.ReactNode }) {
    useSyncExternalStore(
        (cb) => {
            entry.listeners.add(cb);
            return () => { entry.listeners.delete(cb); };
        },
        () => entry.version,
    );
    return <>{render(entry.props)}</>;
}

/**
 * The flat portal list. Mount once inside the framework's context
 * providers so every node's markup sees the registry and attribution
 * contexts. `render` turns a node's props into its markup (the
 * dispatcher that `<Node>` used to render inline).
 */
export function HostedPortals({ render }: { render: (props: HostedNodeProps) => React.ReactNode }) {
    useSyncExternalStore(subscribeStructure, () => structureVersion);
    return (
        <>
            {[...entries.values()].map((entry) =>
                createPortal(<PortalContent entry={entry} render={render} />, entry.host, entry.key),
            )}
        </>
    );
}
