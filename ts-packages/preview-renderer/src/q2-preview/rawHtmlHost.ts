/**
 * Stable host for raw-HTML blocks that embed an <iframe>.
 *
 * Problem. A React remount of `RawBlock` (a sibling shift, a new wrapping
 * Div, sectionize kicking in) tears down its injected HTML and re-injects
 * it, so any <iframe> inside reloads. Moving the iframe element between
 * parents would not help: the browser discards an iframe's browsing
 * context as soon as the element leaves the document, so `appendChild`
 * into a new parent reloads it just the same. The only way to keep the
 * iframe alive is to never detach it.
 *
 * Approach. The raw HTML is injected exactly once into a wrapper element
 * that lives under a single host `<div>` appended to `document.body` and
 * is never moved. The React tree renders a placeholder `<div>` in the
 * block's normal flow position. The hosted wrapper is absolutely
 * positioned over the placeholder (document coordinates, so scrolling
 * needs no updates) and the placeholder's height is set from the hosted
 * content's natural height, so layout around it is unchanged.
 *
 * Wrappers are pooled by their HTML string. On unmount a wrapper is
 * hidden and parked, not removed; a remount with the same HTML reclaims
 * it (React runs unmount cleanups before the replacement's layout effect
 * in the same commit, so a remount always finds its own wrapper). A
 * parked wrapper that nobody reclaims within `PARK_TTL_MS` is removed.
 */

const HOST_ID = 'q2-raw-html-host';
const PARK_TTL_MS = 3000;

let hostEl: HTMLDivElement | null = null;
const parked = new Map<string, HTMLDivElement[]>();
const parkTimers = new WeakMap<HTMLDivElement, number>();

function host(): HTMLDivElement {
    if (hostEl !== null && hostEl.isConnected === true) return hostEl;
    const el = document.createElement('div');
    el.id = HOST_ID;
    // Zero-size, non-clipping anchor at the document origin; children
    // are positioned in document coordinates relative to it.
    el.style.position = 'absolute';
    el.style.top = '0';
    el.style.left = '0';
    el.style.width = '0';
    el.style.height = '0';
    el.style.overflow = 'visible';
    document.body.appendChild(el);
    hostEl = el;
    return el;
}

/** True when the raw HTML should take the hosted path. */
export function needsStableHost(html: string): boolean {
    return /<iframe[\s>]/i.test(html);
}

/** Take a wrapper for `html`, reclaiming a parked one if available. */
export function acquireHosted(html: string): HTMLDivElement {
    const pool = parked.get(html);
    const reclaimed = pool !== undefined ? pool.pop() : undefined;
    if (reclaimed !== undefined) {
        const timer = parkTimers.get(reclaimed);
        if (timer !== undefined) window.clearTimeout(timer);
        parkTimers.delete(reclaimed);
        reclaimed.style.display = '';
        return reclaimed;
    }
    const el = document.createElement('div');
    el.className = 'q2-raw-hosted';
    el.style.position = 'absolute';
    el.style.boxSizing = 'border-box';
    el.innerHTML = html;
    host().appendChild(el);
    return el;
}

/** Park a wrapper: hide it (display:none keeps an iframe alive) and schedule removal. */
export function releaseHosted(html: string, el: HTMLDivElement): void {
    el.style.display = 'none';
    let pool = parked.get(html);
    if (pool === undefined) {
        pool = [];
        parked.set(html, pool);
    }
    pool.push(el);
    const timer = window.setTimeout(() => {
        const current = parked.get(html);
        if (current !== undefined) {
            const i = current.indexOf(el);
            if (i !== -1) current.splice(i, 1);
            if (current.length === 0) parked.delete(html);
        }
        parkTimers.delete(el);
        el.remove();
    }, PARK_TTL_MS);
    parkTimers.set(el, timer);
}

/**
 * Keep `hosted` glued to `placeholder`: position/width follow the
 * placeholder, the placeholder's height follows the hosted content.
 * Returns a disposer.
 */
export function bindHosted(placeholder: HTMLElement, hosted: HTMLDivElement): () => void {
    let frame: number | null = null;

    const sync = () => {
        frame = null;
        if (placeholder.isConnected === false) return;
        const rect = placeholder.getBoundingClientRect();
        const left = rect.left + window.scrollX;
        const top = rect.top + window.scrollY;
        hosted.style.left = `${left}px`;
        hosted.style.top = `${top}px`;
        hosted.style.width = `${rect.width}px`;
        const height = hosted.offsetHeight;
        const current = placeholder.style.height;
        if (current !== `${height}px`) placeholder.style.height = `${height}px`;
    };
    const schedule = () => {
        if (frame !== null) return;
        frame = window.requestAnimationFrame(sync);
    };

    sync();

    const disposers: Array<() => void> = [];
    if (typeof ResizeObserver !== 'undefined') {
        const ro = new ResizeObserver(schedule);
        ro.observe(placeholder);
        ro.observe(hosted);
        ro.observe(document.documentElement);
        disposers.push(() => ro.disconnect());
    }
    if (typeof MutationObserver !== 'undefined') {
        // Layout shifts that leave the document height unchanged are not
        // seen by the ResizeObserver above; a coalesced mutation watch
        // catches the common ones (content above the block changing).
        const mo = new MutationObserver(schedule);
        mo.observe(document.body, { childList: true, subtree: true, characterData: true, attributes: true });
        disposers.push(() => mo.disconnect());
    }
    window.addEventListener('resize', schedule);
    disposers.push(() => window.removeEventListener('resize', schedule));

    return () => {
        if (frame !== null) window.cancelAnimationFrame(frame);
        for (const d of disposers) d();
        placeholder.style.height = '';
    };
}
