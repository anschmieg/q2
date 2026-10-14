import React, { useContext } from 'react';
import { Node, dataLocProps, mapKeyed, nodeIdentity } from '../../framework';
import type {
    BlockNode,
    DefinitionListBlock,
    InlineNode,
    NodeArgs,
} from '../../framework';
import { PreviewContext } from '../PreviewContext';
import { isLeadingBlockBorrowable } from './listBorrow';
import { PlainHost } from '../commentAnchor';

/**
 * DefinitionList → `<dl><dt>term</dt><dd>def</PlainHost>...</dl>`. Pandoc
 * writes one `<dt>` per term and one `<dd>` per definition; multiple
 * definitions for the same term yield multiple sibling `<dd>`s
 * (no wrapping element per item, matching Pandoc's HTML writer).
 *
 * AST shape: `c: [InlineNode[], BlockNode[][]][]` — each item is a
 * (term, definitions) pair where definitions is an array of block
 * arrays (each block array is one definition).
 */
export const DefinitionList = (args: NodeArgs<DefinitionListBlock>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined && !ctx?.editingDisabled;
    const affordanceAttr = isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {};
    const { node, setLocalAst, onNavigateToDocument } = args;

    return (
        <dl {...affordanceAttr} {...dataLocProps(node)}>
            {node.c.flatMap(([term, defs], i) => {
                // Item identity: the term's inlines (bounded-prefix hash) plus the
                // position as a tiebreak for repeated terms.
                const itemKey = `${term.map(nodeIdentity).join('+')}@${i}`;
                const dt = (
                    <dt key={`dt-${itemKey}`}>
                        {mapKeyed(term, (inl, j, key) => (
                            <Node
                                key={key}
                                node={inl}
                                onNavigateToDocument={onNavigateToDocument}
                                setLocalAst={(newInl: BlockNode | InlineNode) => {
                                    const newTerm = term.slice();
                                    newTerm[j] = newInl as InlineNode;
                                    const newItems = node.c.slice();
                                    newItems[i] = [newTerm, defs];
                                    setLocalAst({ t: 'DefinitionList', c: newItems });
                                }}
                            />
                        ))}
                    </dt>
                );
                const dds = defs.map((blocks, k) => {
                    // §0 Pool-id borrow: if the first block is a Plain and editable,
                    // borrow its pool-id onto the <dd> (empty-guard: skip borrow if blocks is empty).
                    const leadingBlock = blocks.length > 0 ? (blocks[0] as any) : null;
                    const borrowPoolId = leadingBlock && isLeadingBlockBorrowable(leadingBlock, ctx)
                        ? (leadingBlock.s as string | number)
                        : undefined;
                    const ddProps = borrowPoolId !== undefined
                        ? { 'data-block-pool-id': borrowPoolId, tabIndex: -1 as const }
                        : {};
                    return (
                        <PlainHost as="dd" key={`dd-${itemKey}-${k}`} {...ddProps}>
                            {mapKeyed(blocks, (b, m, key) => (
                                <Node
                                    key={key}
                                    node={b}
                                    onNavigateToDocument={onNavigateToDocument}
                                    setLocalAst={(newBlock: BlockNode | InlineNode) => {
                                        const newBlocks = blocks.slice();
                                        newBlocks[m] = newBlock as BlockNode;
                                        const newDefs = defs.slice();
                                        newDefs[k] = newBlocks;
                                        const newItems = node.c.slice();
                                        newItems[i] = [term, newDefs];
                                        setLocalAst({ t: 'DefinitionList', c: newItems });
                                    }}
                                />
                            ))}
                        </PlainHost>
                    );
                });
                return [dt, ...dds] as React.ReactNode[];
            })}
        </dl>
    );
};
