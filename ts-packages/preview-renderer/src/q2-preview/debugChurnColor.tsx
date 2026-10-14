import React, { useState } from 'react';
import { nodeIdentity, useContentKeys } from '../framework/nodeKey';
import { getDebugSettings } from './debugSettings';

/**
 * TEMPORARY DEBUGGING AID — remove before shipping.
 *
 * Wraps a render component so that each *instance* picks a random
 * pastel background the first time it mounts and keeps it for as long
 * as it stays mounted. A re-render keeps the color; a remount (React
 * churn — the component was torn down and recreated) picks a new one.
 * Watching the preview flicker therefore shows exactly which pieces
 * of the tree are being recreated instead of updated.
 *
 * Blocks are wrapped in a `<div>`, inlines in a `<span>`, so the
 * background has a box to paint. This changes the DOM shape slightly
 * (one extra element per node), which is fine for a debugging pass but
 * will break DOM-shape parity tests — do not leave it enabled.
 */
/** Compile-time kill switch; the runtime toggle lives in debugSettings.ts. */
export const DEBUG_CHURN_COLORS: boolean = true;

function randomPastel(): string {
    const hue = Math.floor(Math.random() * 360);
    return `hsl(${hue} 85% 85%)`;
}

export function withChurnColor<P extends object>(
    Component: (props: P) => React.ReactNode,
    as: 'div' | 'span',
    /** Return true to render this instance unpainted (no wrapper at all). */
    skip?: (props: P) => boolean,
): (props: P) => React.ReactNode {
    if (DEBUG_CHURN_COLORS === false) {
        return Component;
    }
    const name = (Component as { displayName?: string; name?: string }).displayName
        ?? (Component as { name?: string }).name
        ?? 'Component';
    const Wrapped = (props: P) => {
        // Lazy initializer runs once per mount, so the color only changes
        // when React creates a fresh instance.
        const [color] = useState(randomPastel);
        if (getDebugSettings().churnColors === false) {
            return <Component {...props} />;
        }
        if (skip !== undefined && skip(props) === true) {
            return <Component {...props} />;
        }
        const Tag = as;
        // Shows what the node is keyed by: its content identity when
        // USE_CONTENT_KEYS is on, otherwise a reminder that it's positional.
        const keyInfo = useContentKeys()
            ? nodeIdentity((props as { node?: unknown }).node)
            : 'index';
        return (
            <Tag
                data-churn-component={name}
                data-churn-key={keyInfo}
                title={`${name} · key: ${keyInfo}`}
                style={{ backgroundColor: color }}
            >
                <Component {...props} />
            </Tag>
        );
    };
    Wrapped.displayName = `ChurnColor(${name})`;
    return Wrapped;
}

/** True for a Pandoc Div carrying the `section` class (renders as `<section>`). */
export function isSectionDiv(props: { node?: { t?: string; c?: unknown } }): boolean {
    const node = props.node;
    if (node === undefined || node.t !== 'Div') return false;
    const attr = (node.c as [[string, string[], unknown]] | undefined)?.[0];
    return Array.isArray(attr) && Array.isArray(attr[1]) && attr[1].includes('section');
}

/** Apply `withChurnColor` to every entry of a component map. */
export function withChurnColors<T extends Record<string, (props: any) => React.ReactNode>>(
    components: T,
    as: 'div' | 'span',
): T {
    const out: Record<string, (props: any) => React.ReactNode> = {};
    for (const [key, Component] of Object.entries(components)) {
        out[key] = withChurnColor(Component, as);
    }
    return out as T;
}
