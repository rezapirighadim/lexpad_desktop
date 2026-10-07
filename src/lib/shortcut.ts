/**
 * Shortcuts, as the global-shortcut plugin writes them
 * ("CommandOrControl+Shift+L") and as people read them (⌘⇧L, Ctrl+Shift+L).
 */

export function isMac(platform: string = navigator.platform): boolean {
  return /Mac/i.test(platform);
}

/** How the platform writes a shortcut. */
export function prettyShortcut(accelerator: string, mac: boolean = isMac()): string {
  const parts = accelerator.split('+').map((part) => {
    switch (part) {
      case 'CommandOrControl':
      case 'CmdOrCtrl':
        return mac ? '⌘' : 'Ctrl';
      case 'Command':
      case 'Cmd':
      case 'Super':
        return mac ? '⌘' : 'Win';
      case 'Control':
      case 'Ctrl':
        return mac ? '⌃' : 'Ctrl';
      case 'Shift':
        return mac ? '⇧' : 'Shift';
      case 'Alt':
      case 'Option':
        return mac ? '⌥' : 'Alt';
      default:
        return part;
    }
  });
  return parts.join(mac ? '' : '+');
}

/** The key part of a shortcut, from a key press's physical code. */
function keyOf(code: string): string | undefined {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (code === 'Space') return 'Space';
  return undefined;
}

/**
 * The shortcut a key press makes, or undefined while it is only modifiers or
 * has none: a global shortcut without a modifier would steal a letter from
 * every app. The main modifier is saved as CommandOrControl, so a settings
 * file means the same thing on either platform.
 */
export function acceleratorOf(
  event: Pick<KeyboardEvent, 'code' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>,
  mac: boolean = isMac(),
): string | undefined {
  const key = keyOf(event.code);
  if (key === undefined) return undefined;
  const parts: string[] = [];
  const primary = mac ? event.metaKey : event.ctrlKey;
  if (primary) parts.push('CommandOrControl');
  if (mac && event.ctrlKey) parts.push('Control');
  if (!mac && event.metaKey) parts.push('Super');
  if (event.altKey) parts.push('Alt');
  if (event.shiftKey) parts.push('Shift');
  // Shift alone is typing a capital letter, not a shortcut.
  if (parts.length === 0 || (parts.length === 1 && parts[0] === 'Shift')) return undefined;
  return [...parts, key].join('+');
}
