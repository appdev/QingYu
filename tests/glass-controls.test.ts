// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { glassToggle } from '../src/glass-controls';

beforeEach(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: true }));
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    disconnect() {}
  });
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(38);
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(18);
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function pointer(target: EventTarget, type: string, clientX: number) {
  target.dispatchEvent(Object.assign(new MouseEvent(type, { bubbles: true, clientX }), {
    pointerId: 1, pointerType: 'touch', isPrimary: true,
  }));
}

it.each(['Web', 'Apple App', 'Android App'])('keeps label activation and commits a completed switch drag once on %s', platform => {
  if (platform !== 'Web') {
    vi.stubGlobal('__TAURI_INTERNALS__', {});
    vi.spyOn(navigator, 'platform', 'get').mockReturnValue(platform === 'Apple App' ? 'MacIntel' : 'Linux armv8l');
  }
  const input = document.createElement('input');
  input.type = 'checkbox';
  input.setAttribute('role', 'switch');
  const toggle = glassToggle(input);
  document.body.append(toggle.element);
  const change = vi.fn();
  input.addEventListener('change', change);

  try {
    const thumb = toggle.element.querySelector('.glass-toggle-thumb')!;
    expect(thumb.hasAttribute('data-glass-host')).toBe(platform !== 'Apple App');
    expect(thumb.classList.contains('lb')).toBe(platform === 'Apple App');
    toggle.element.click();
    expect(input.checked).toBe(true);

    pointer(toggle.element, 'pointerdown', 28);
    pointer(window, 'pointermove', 4);
    expect(toggle.element.querySelector('[data-dragging]')).not.toBeNull();
    pointer(window, 'pointercancel', 4);
    expect(input.checked).toBe(true);
    expect(change).toHaveBeenCalledTimes(1);

    pointer(toggle.element, 'pointerdown', 28);
    pointer(window, 'pointermove', 4);
    pointer(window, 'pointerup', 4);
    expect(input.checked).toBe(false);
    expect(change).toHaveBeenCalledTimes(2);
  } finally {
    toggle.destroy();
    toggle.element.remove();
  }
});
