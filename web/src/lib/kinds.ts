import type { NodeKind } from './types';

export const KIND_LETTER: Record<NodeKind, string> = {
  module: 'M',
  package: 'P',
  type: 'T',
  function: 'F',
};

/** CSS variable holding the kind's color; defined in app.css for light and dark. */
export const kindColor = (k: NodeKind) => `var(--k-${k})`;
