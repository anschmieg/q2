import React, { useContext } from 'react';
import { RegistryContext } from './RegistryContext';
import { liItemAttrProps } from './listItemAttr';
import { isAtomicSourceInfo, ATOMIC_KINDS } from '../utils/sourceInfo';
import { isAtomicCustomNode } from '../utils/atomicCustomNodes';
import { mapKeyed, mapItemsKeyed } from './nodeKey';

import type {
    BlockNode,
    InlineNode,
    NodeArgs,
    PandocAST,
    ParaBlock,
    PlainBlock,
    HeaderBlock,
    BlockQuoteBlock,
    DivBlock,
    BulletListBlock,
    OrderedListBlock,
    FigureBlock,
    EmphInline,
    StrongInline,
    LinkInline,
    ImageInline,
    SpanInline,
    QuotedInline,
    CustomBlockNode,
    CustomInlineNode,
    Slot,
} from './types';

/**
 * The set of Pandoc tags the framework treats as block-level. Used by both
 * `Node` and `renderNode` to route to the registry's 'Block' or 'Inline'
 * dispatcher.
 *
 * 2B additions:
 *  - `LineBlock`, `DefinitionList`, `Table` — gap-fill leaves rendered by
 *    q2-preview's `blocks/`.
 *  - `CustomBlock` — post-unwrap discriminator. `unwrapCustomNodes`
 *    rewrites custom-block wrapper Divs as `t: 'CustomBlock'`; without
 *    membership here `Node` would route them to `Inline`.
 *  - `BlockMetadata`, `NoteDefinitionPara`, `NoteDefinitionFencedBlock`,
 *    `CaptionBlock` — defensive routing. These tags can appear in the
 *    AST (writer json.rs:1242, :1251, :1257, :1263); without membership
 *    they'd surface as inline placeholders. With it, the `Block`
 *    dispatcher's miss path renders the muted-gray placeholder.
 */
export const blockTypes = [
    'Para', 'Plain', 'Header', 'CodeBlock', 'BulletList', 'OrderedList',
    'BlockQuote', 'Div', 'HorizontalRule', 'RawBlock', 'Figure',
    'LineBlock', 'DefinitionList', 'Table',
    'CustomBlock',
    'BlockMetadata', 'NoteDefinitionPara', 'NoteDefinitionFencedBlock', 'CaptionBlock',
];

/**
 * Per-Pandoc-tag recursive-descent registry. The framework owns this
 * structure (intentionally not re-exported from `framework/index.ts`):
 * each entry knows how to walk the children of one Pandoc base type and
 * emit a `<Node>` per child with a correctly-scoped `setLocalAst`. Format
 * registries (q2-debug, q2-preview) extend behavior at the *leaf* level
 * via `customNodeRegistry`, not here.
 */
const renderChildrenRegistry: Record<string, (args: {
    node: any;
    setLocalAst: (newNode: any) => void;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}) => React.ReactNode> = {
    // Ast type
    Ast: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as PandocAST).blocks, (block, i, key) => (
            <Node key={key} node={block} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newBlock: BlockNode | InlineNode) => {
                    const newBlocks = [...(node as PandocAST).blocks];
                    newBlocks[i] = newBlock as BlockNode;
                    setLocalAst({ ...(node as PandocAST), blocks: newBlocks });
                }}
            />
        )),
    // Inline types
    Emph: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as EmphInline).c, (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as EmphInline).c];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Emph', c: newChildren });
                }}
            />
        )),
    Strong: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as StrongInline).c, (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as StrongInline).c];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Strong', c: newChildren });
                }}
            />
        )),
    // Plan 2B inline gap fills (same {t, c: InlineNode[]} shape as Emph/Strong):
    Underline: makeFlatInlineRenderer('Underline'),
    Strikeout: makeFlatInlineRenderer('Strikeout'),
    Superscript: makeFlatInlineRenderer('Superscript'),
    Subscript: makeFlatInlineRenderer('Subscript'),
    SmallCaps: makeFlatInlineRenderer('SmallCaps'),
    Link: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as LinkInline).c[1], (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as LinkInline).c[1]];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Link', c: [(node as LinkInline).c[0], newChildren, (node as LinkInline).c[2]] });
                }}
            />
        )),
    Image: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as ImageInline).c[1], (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as ImageInline).c[1]];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Image', c: [(node as ImageInline).c[0], newChildren, (node as ImageInline).c[2]] });
                }}
            />
        )),
    Span: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as SpanInline).c[1], (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as SpanInline).c[1]];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Span', c: [(node as SpanInline).c[0], newChildren] });
                }}
            />
        )),
    Quoted: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as QuotedInline).c[1], (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as QuotedInline).c[1]];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Quoted', c: [(node as QuotedInline).c[0], newChildren] });
                }}
            />
        )),
    // Block types
    Para: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as ParaBlock).c, (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as ParaBlock).c];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Para', c: newChildren });
                }}
            />
        )),
    Plain: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as PlainBlock).c, (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as PlainBlock).c];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Plain', c: newChildren });
                }}
            />
        )),
    Header: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as HeaderBlock).c[2], (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as HeaderBlock).c[2]];
                    newChildren[i] = newChild as InlineNode;
                    setLocalAst({ t: 'Header', c: [(node as HeaderBlock).c[0], (node as HeaderBlock).c[1], newChildren] });
                }}
            />
        )),
    BlockQuote: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as BlockQuoteBlock).c, (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as BlockQuoteBlock).c];
                    newChildren[i] = newChild as BlockNode;
                    setLocalAst({ t: 'BlockQuote', c: newChildren });
                }}
            />
        )),
    Div: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as DivBlock).c[1], (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as DivBlock).c[1]];
                    newChildren[i] = newChild as BlockNode;
                    setLocalAst({ t: 'Div', c: [(node as DivBlock).c[0], newChildren] });
                }}
            />
        )),
    // Non-incremental (editable) list path. A per-item block attr
    // (`itemAttr[i]`, bd-aeyss6p5) is applied to the <li> here; the incremental
    // `fragment` class is never present on this path, so pass `false`. NOTE:
    // `setLocalAst` rebuilds the list node without `itemAttr`, so an in-preview
    // text edit drops the item's class — accepted for v1 (see plan).
    BulletList: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapItemsKeyed((node as BulletListBlock).c, (item, i, itemKey) => (
            <li key={itemKey} {...liItemAttrProps((node as BulletListBlock).itemAttr?.[i], false)}>{mapKeyed(item, (block, j, key) => (
                <Node key={key} node={block} onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newBlock: BlockNode | InlineNode) => {
                        const newItems = [...(node as BulletListBlock).c];
                        const newItem = [...newItems[i]];
                        newItem[j] = newBlock as BlockNode;
                        newItems[i] = newItem;
                        setLocalAst({ t: 'BulletList', c: newItems });
                    }}
                />
            ))}</li>
        )),
    OrderedList: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapItemsKeyed((node as OrderedListBlock).c[1], (item, i, itemKey) => (
            <li key={itemKey} {...liItemAttrProps((node as OrderedListBlock).itemAttr?.[i], false)}>{mapKeyed(item, (block, j, key) => (
                <Node key={key} node={block} onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={(newBlock: BlockNode | InlineNode) => {
                        const newItems = [...(node as OrderedListBlock).c[1]];
                        const newItem = [...newItems[i]];
                        newItem[j] = newBlock as BlockNode;
                        newItems[i] = newItem;
                        setLocalAst({ t: 'OrderedList', c: [(node as OrderedListBlock).c[0], newItems] });
                    }}
                />
            ))}</li>
        )),
    Figure: ({ node, setLocalAst, onNavigateToDocument }) =>
        mapKeyed((node as FigureBlock).c[2], (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const newChildren = [...(node as FigureBlock).c[2]];
                    newChildren[i] = newChild as BlockNode;
                    setLocalAst({ t: 'Figure', c: [(node as FigureBlock).c[0], (node as FigureBlock).c[1], newChildren] });
                }}
            />
        )),
    // Custom-node generic walk. Per-type components (Callout, Theorem, ...)
    // drive their own slot rendering via `renderSlot` in q2-preview/utils.ts;
    // these entries are the fallback consumed by the `Fallback` registry
    // entry for unregistered custom-node `type_name`s.
    CustomBlock: renderCustomNodeChildren,
    CustomInline: renderCustomNodeChildren,
};

function makeFlatInlineRenderer(tag: string) {
    return ({ node, setLocalAst, onNavigateToDocument }: {
        node: any;
        setLocalAst: (newNode: any) => void;
        onNavigateToDocument?: (path: string, anchor: string | null) => void;
    }): React.ReactNode => {
        const children = (node as { c: InlineNode[] }).c;
        return mapKeyed(children, (child, i, key) => (
            <Node key={key} node={child} onNavigateToDocument={onNavigateToDocument}
                setLocalAst={(newChild: BlockNode | InlineNode) => {
                    const next = children.slice();
                    next[i] = newChild as InlineNode;
                    setLocalAst({ t: tag, c: next });
                }}
            />
        ));
    };
}

function renderCustomNodeChildren({
    node,
    setLocalAst,
    onNavigateToDocument,
}: {
    node: any;
    setLocalAst: (newNode: any) => void;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}): React.ReactNode {
    const customNode = node as CustomBlockNode | CustomInlineNode;
    const slotEntries = Object.entries(customNode.slots) as Array<[string, Slot]>;
    return slotEntries.flatMap(([name, slot]) => {
        const setSlot = (next: Slot) =>
            setLocalAst({ ...customNode, slots: { ...customNode.slots, [name]: next } });
        switch (slot.kind) {
            case 'block':
                return [
                    <Node key={name} node={slot.value} onNavigateToDocument={onNavigateToDocument}
                        setLocalAst={(n) => setSlot({ kind: 'block', value: n as BlockNode })}
                    />,
                ];
            case 'inline':
                return [
                    <Node key={name} node={slot.value} onNavigateToDocument={onNavigateToDocument}
                        setLocalAst={(n) => setSlot({ kind: 'inline', value: n as InlineNode })}
                    />,
                ];
            case 'blocks':
                return mapKeyed(slot.value, (b, i, key) => (
                    <Node key={`${name}-${key}`} node={b} onNavigateToDocument={onNavigateToDocument}
                        setLocalAst={(n) => {
                            const next = slot.value.slice();
                            next[i] = n as BlockNode;
                            setSlot({ kind: 'blocks', value: next });
                        }}
                    />
                ));
            case 'inlines':
                return mapKeyed(slot.value, (inl, i, key) => (
                    <Node key={`${name}-${key}`} node={inl} onNavigateToDocument={onNavigateToDocument}
                        setLocalAst={(n) => {
                            const next = slot.value.slice();
                            next[i] = n as InlineNode;
                            setSlot({ kind: 'inlines', value: next });
                        }}
                    />
                ));
        }
    });
}

/**
 * Unified function to render children of any node type
 */
export function renderChildren<T extends BlockNode | InlineNode>({
    node,
    setLocalAst,
    onNavigateToDocument,
}: {
    node: T;
    setLocalAst: (newNode: T) => void;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}): React.ReactNode {
    // Check if this is a PandocAST object (has blocks and pandoc-api-version)
    const nodeType = (node as any).t || ((node as any).blocks && (node as any)['pandoc-api-version'] ? 'Ast' : undefined);

    const renderer = renderChildrenRegistry[nodeType];
    if (!renderer) {
        console.warn(`No renderer found for node type: ${nodeType}`);
        return null;
    }

    return renderer({ node, setLocalAst, onNavigateToDocument });
}

/**
 * Direct dispatch by node tag. First tries the format's 'Block' or
 * 'Inline' wrapper (matching `<Node>`'s behavior); falls back to the
 * direct per-tag entry; final miss path renders plain text.
 *
 * `useContext` makes this a hook-equivalent: it must be called inside
 * a React render that has an `<Ast>` ancestor. (All current callers do.)
 */
export const renderNode = (args: NodeArgs<BlockNode | InlineNode>, type: string) => {
    const { registry } = useContext(RegistryContext);

    const isBlock = blockTypes.includes(type);

    // First try to use Block/Inline wrapper (matches Node component behavior)
    if (isBlock) {
        const BlockComponent = registry['Block'];
        if (BlockComponent) {
            return <BlockComponent {...args} />;
        }
    } else {
        const InlineComponent = registry['Inline'];
        if (InlineComponent) {
            return <InlineComponent {...args} />;
        }
    }

    // Fall back to direct component lookup
    const Component = registry[type];
    return Component ? <Component {...args} /> : <span>Not registered: {args.node.t}</span>;
}

const NOOP_SET_LOCAL_AST: (newNode: BlockNode | InlineNode) => void = () => {};

/**
 * Unified Node component that delegates to the format's 'Block' or
 * 'Inline' dispatcher based on the node's Pandoc tag.
 *
 * Atomic-aware gate (Plan 2B): three convergence paths mark a node's
 * subtree as read-only on the iframe side, replacing `setLocalAst`
 * with a NOOP for that subtree. Editing rendered atomic content would
 * corrupt the source AST (e.g. `@fig-1` source vs. "Figure 1" rendered).
 *
 *   1. Derived source_info (Plan 6's shortcode resolutions).
 *   2. Atomic Synthetic source_info (Plan 4's `By::is_atomic_synthesizer()`).
 *   3. Atomic CustomNode types (`CrossrefResolvedRef` today; Plan 8 adds
 *      `IncludeExpansion`).
 *
 * The gate sits at framework's `Node` so it fires once per recursion
 * step, regardless of which format's dispatcher renders the node.
 * Both q2-debug and q2-preview pick it up "for free" — neither
 * dispatcher needs format-specific atomic awareness.
 *
 * Recursion contract: this gate fires only when a node enters via
 * `<Node>`. User-TSX overrides registered via `render-components: [...]`
 * MUST recurse through `<Node>` / `renderChildren` / `renderSlot` —
 * never iterate `node.c` and emit child JSX directly. Doing so bypasses
 * the gate for descendants. See plan §"Recursion contract for the
 * atomic gate" + the negative regression fixture.
 */
export function Node({
    node,
    onNavigateToDocument,
    setLocalAst
}: {
    node: BlockNode | InlineNode,
    onNavigateToDocument?: (path: string, anchor: string | null) => void,
    setLocalAst: (newNode: BlockNode | InlineNode) => void
}) {
    const { registry, sourceInfoPool } = useContext(RegistryContext);

    const isCustom = node.t === 'CustomBlock' || node.t === 'CustomInline';
    const isAtomic =
        isAtomicSourceInfo(node as { s?: number }, sourceInfoPool, ATOMIC_KINDS)
        || (isCustom && isAtomicCustomNode((node as CustomBlockNode | CustomInlineNode).type_name));

    const effectiveSetLocalAst = isAtomic ? NOOP_SET_LOCAL_AST : setLocalAst;

    const isBlock = blockTypes.includes(node.t);

    if (isBlock) {
        const BlockComponent = registry['Block'];
        if (!BlockComponent) {
            return <div>Block wrapper not registered</div>;
        }
        return <BlockComponent
            node={node as BlockNode}
            onNavigateToDocument={onNavigateToDocument}
            setLocalAst={effectiveSetLocalAst as (newBlock: BlockNode) => void}
        />;
    } else {
        const InlineComponent = registry['Inline'];
        if (!InlineComponent) {
            return <span>Inline wrapper not registered</span>;
        }
        return <InlineComponent
            node={node as InlineNode}
            onNavigateToDocument={onNavigateToDocument}
            setLocalAst={effectiveSetLocalAst as (newInline: InlineNode) => void}
        />;
    }
}
