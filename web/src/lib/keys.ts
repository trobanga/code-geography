/** The fields of a KeyboardEvent that `depthKey` reads. */
export interface KeyInput {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  target: EventTarget | null;
}

const TEXT_FIELDS = ['INPUT', 'TEXTAREA', 'SELECT'];

function inField(target: EventTarget | null): boolean {
  const t = target as { tagName?: string; isContentEditable?: boolean } | null;
  return !!t && (TEXT_FIELDS.includes(t.tagName ?? '') || t.isContentEditable === true);
}

/** "+" and "-" step the depth of the view by one layer; Escape cancels a step that waits for a second click. */
export function depthKey(e: KeyInput): 'deeper' | 'shallower' | 'cancel' | null {
  if (e.ctrlKey || e.metaKey || e.altKey || inField(e.target)) return null;
  if (e.key === '+') return 'deeper';
  if (e.key === '-') return 'shallower';
  if (e.key === 'Escape') return 'cancel';
  return null;
}
