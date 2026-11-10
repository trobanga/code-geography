import type { NodeChange, NodeKind } from './types';

export const KIND_LETTER: Record<NodeKind, string> = {
  module: 'M',
  package: 'P',
  type: 'T',
  function: 'F',
};

/** CSS variable holding the kind's color; defined in app.css for light and dark. */
export const kindColor = (k: NodeKind) => `var(--k-${k})`;

/** Sign of a change mark; a mark differs in shape as well as in color. */
export const CHANGE_SIGN: Record<NodeChange, string> = { added: '+', removed: '−', modified: '~' };
export const changeColor = (c: NodeChange) => `var(--c-${c})`;
