/*
 * @quarto/components
 * Copyright (c) 2026 Posit, PBC
 *
 * The props contract for React components bound to Quarto document
 * elements (bd-react-components-a8flgldm, plan
 * claude-notes/plans/2026-10-13-react-components.md, D2).
 */

import type { ComponentType, ReactNode } from "react";
import type { Block, Inline } from "@quarto/pandoc-types";

/** The bound element's own attributes. */
export interface QuartoComponentAttrs {
  /** The element's id, or "" when it has none. */
  id: string;
  /** The element's classes, in source order. */
  classes: string[];
  /**
   * Every other attribute on the element as rendered (`data-*`, `aria-*`,
   * `title`, ...), minus the ones the runtime itself owns
   * (`data-quarto-component*`).
   */
  attributes: Record<string, string>;
}

/**
 * What a bound component receives. The same shape in `q2 render` output
 * (the page runtime mounts the component over the static markup) and in
 * the q2 preview (the renderer dispatches on the element), so one module
 * serves both.
 */
export interface QuartoComponentProps<P = Record<string, unknown>> {
  attrs: QuartoComponentAttrs;
  /**
   * JSON props: the declaration's `props:` in metadata, overlaid by the
   * element's `component-props` attribute and by a
   * `script.quarto-component-props` child (what `quarto.component` in Lua
   * emits). Always an object; `{}` when nothing was declared.
   */
  props: P;
  /**
   * The no-script content. In static HTML this is the element's original
   * DOM, reparented into the React tree (`content: dom`); in the preview
   * it is the registry-rendered subtree. Null for `content: none`.
   * Opaque: render it, do not inspect it.
   */
  children: ReactNode;
  /**
   * The bound node's Pandoc AST (`content: ast`), in the Pandoc JSON
   * shape: `{ t: "Div", c: [attr, blocks] }` or `{ t: "Span", c: [attr,
   * inlines] }`. Undefined for the other content modes in static HTML;
   * always present in the preview.
   */
  node?: Block | Inline;
  /**
   * The host element in static HTML; null in the preview, where React
   * owns the element. A component that reads `host` is static-only by
   * construction.
   */
  host: HTMLElement | null;
}

/** A component that can be bound to a document element. */
export type QuartoComponent<P = Record<string, unknown>> = ComponentType<
  QuartoComponentProps<P>
>;

/**
 * Identity helper for inference: `export default defineComponent<Props>(
 * ({ props, children }) => ...)` types the props without an annotation.
 */
export function defineComponent<P = Record<string, unknown>>(
  component: QuartoComponent<P>,
): QuartoComponent<P> {
  return component;
}

/** The attribute the runtime selects hosts by. */
export const HOST_ATTR = "data-quarto-component";
/** Set on a host once the runtime has mounted ("mounted") or given up ("failed"). */
export const STATE_ATTR = "data-quarto-component-state";
/** Dispatched (bubbling) on a host right after its component is rendered. */
export const MOUNTED_EVENT = "quarto-component:mounted";
