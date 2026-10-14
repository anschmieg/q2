/**
 * Timeout policy for the local render checks.
 *
 * The WASM VFS is instance-global, so renders serialize on a promise
 * chain (see local-render.ts). A render that wedges would block every
 * later render behind it; the timeouts below at least let the tool
 * RESPOND — the write tools degrade to a pointer at get_errors, and
 * get_errors fails cleanly. Unwedging the chain itself is not possible
 * in-process (the WASM host cannot be terminated); a wedged host needs
 * a server restart.
 */

/**
 * Write-tool render checks hold up a write response, so they get a
 * short leash: the write has already landed, the check is a courtesy.
 */
export const RENDER_CHECK_TIMEOUT_MS = 30_000;

/** A get_errors render gets longer — the caller asked for the render. */
export const GET_ERRORS_TIMEOUT_MS = 60_000;

/**
 * Race `promise` against a timeout. On timeout the returned promise
 * rejects with an error naming `label`; the input promise is left to
 * settle on its own (its settlement is consumed, never unhandled).
 */
export function withTimeout<T>(promise: Promise<T>, ms: number, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const t = setTimeout(() => reject(new Error(`${label} timed out after ${ms} ms`)), ms);
    promise.then(
      (v) => {
        clearTimeout(t);
        resolve(v);
      },
      (e) => {
        clearTimeout(t);
        reject(e);
      },
    );
  });
}
