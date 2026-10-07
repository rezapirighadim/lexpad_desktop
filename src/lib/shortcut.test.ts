import { describe, expect, it } from 'vitest';
import { acceleratorOf, prettyShortcut } from './shortcut.js';

const key = (
  code: string,
  mods: Partial<Record<'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey', boolean>> = {},
) => ({
  code,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

describe('shortcuts', () => {
  it('read the way each platform writes them', () => {
    expect(prettyShortcut('CommandOrControl+Shift+L', true)).toBe('⌘⇧L');
    expect(prettyShortcut('CommandOrControl+Shift+L', false)).toBe('Ctrl+Shift+L');
    expect(prettyShortcut('Control+Alt+K', true)).toBe('⌃⌥K');
  });

  it('are made from a key press with at least one real modifier', () => {
    expect(acceleratorOf(key('KeyL', { metaKey: true, shiftKey: true }), true)).toBe(
      'CommandOrControl+Shift+L',
    );
    expect(acceleratorOf(key('KeyL', { ctrlKey: true, shiftKey: true }), false)).toBe(
      'CommandOrControl+Shift+L',
    );
    expect(acceleratorOf(key('Digit2', { altKey: true }), false)).toBe('Alt+2');
    expect(acceleratorOf(key('KeyL'), true)).toBeUndefined();
    expect(acceleratorOf(key('KeyL', { shiftKey: true }), true)).toBeUndefined();
    expect(acceleratorOf(key('ShiftLeft', { shiftKey: true, metaKey: true }), true)).toBeUndefined();
  });
});
