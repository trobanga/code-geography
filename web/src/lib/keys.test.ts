import { describe, expect, it } from 'vitest';
import { depthKey } from './keys';

const key = (k: string, more: Partial<Parameters<typeof depthKey>[0]> = {}) =>
  depthKey({ key: k, ctrlKey: false, metaKey: false, altKey: false, target: null, ...more });

describe('depthKey', () => {
  it('maps + and - to one layer more and less', () => {
    expect(key('+')).toBe('deeper');
    expect(key('-')).toBe('shallower');
    expect(key('a')).toBe(null);
  });
  it('ignores the keys with a modifier, which the browser uses for its zoom', () => {
    expect(key('-', { ctrlKey: true })).toBe(null);
    expect(key('+', { metaKey: true })).toBe(null);
    expect(key('+', { altKey: true })).toBe(null);
  });
  it('ignores the keys while a text field has the focus', () => {
    const at = (t: object) => key('+', { target: t as EventTarget });
    expect(at({ tagName: 'INPUT' })).toBe(null);
    expect(at({ tagName: 'TEXTAREA' })).toBe(null);
    expect(at({ tagName: 'SELECT' })).toBe(null);
    expect(at({ tagName: 'DIV', isContentEditable: true })).toBe(null);
    expect(at({ tagName: 'BUTTON' })).toBe('deeper');
  });
  it('maps Escape to cancel a step that waits for a second click', () => {
    expect(key('Escape')).toBe('cancel');
  });
});
