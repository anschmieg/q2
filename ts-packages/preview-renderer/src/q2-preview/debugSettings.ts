/**
 * TEMPORARY DEBUGGING AID — remove before shipping.
 *
 * Tiny external store for the churn-debugging switches so a UI toggle
 * inside the preview can flip them at runtime. Consumers that render
 * from these values subscribe via `useDebugSettings`; PreviewRoot keys
 * the document on the version so a flip rebuilds the whole tree.
 */
import { useSyncExternalStore } from 'react';

export interface DebugSettings {
    /** Key sibling nodes by content identity (true) or array index (false). */
    contentKeys: boolean;
    /** Paint every render component instance with a random background. */
    churnColors: boolean;
    /** Flash every DOM element the moment it is inserted (MutationObserver). */
    domFlash: boolean;
}

let settings: DebugSettings = { contentKeys: false, churnColors: true, domFlash: false };
let version = 0;
const listeners = new Set<() => void>();

export function getDebugSettings(): DebugSettings {
    return settings;
}

export function setDebugSettings(patch: Partial<DebugSettings>): void {
    settings = { ...settings, ...patch };
    version += 1;
    for (const l of listeners) l();
}

function subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
}

export function useDebugSettings(): DebugSettings & { version: number } {
    const v = useSyncExternalStore(subscribe, () => version, () => version);
    return { ...settings, version: v };
}
