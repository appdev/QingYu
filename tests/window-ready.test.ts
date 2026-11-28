// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { showAndroidWindowWhenReady } from '../src/window-ready';

const root = document.documentElement;
const pendingInsets = 'data-android-inset-pending';

afterEach(() => {
  root.removeAttribute(pendingInsets);
  vi.unstubAllGlobals();
});

it('notifies native Android as soon as the initial layout is visible', () => {
  const postMessage = vi.fn();
  vi.stubGlobal('nativeSplash', { postMessage });

  showAndroidWindowWhenReady();

  expect(postMessage).toHaveBeenCalledExactlyOnceWith('ready');
});

it('waits for Android insets before notifying native Android', async () => {
  const postMessage = vi.fn();
  vi.stubGlobal('nativeSplash', { postMessage });
  root.setAttribute(pendingInsets, '');

  showAndroidWindowWhenReady();
  expect(postMessage).not.toHaveBeenCalled();

  root.removeAttribute(pendingInsets);
  await Promise.resolve();
  root.setAttribute(pendingInsets, '');
  root.removeAttribute(pendingInsets);
  await Promise.resolve();

  expect(postMessage).toHaveBeenCalledExactlyOnceWith('ready');
});
