// @vitest-environment jsdom
import { expect, it, vi } from 'vitest';
import { themePalette } from '../src/themes';

it('sends the active mobile surface color with the system bar appearance', async () => {
  const shell = document.createElement('main');
  shell.id = 'app-shell';
  document.body.append(shell);
  vi.stubGlobal('matchMedia', () => Object.assign(new EventTarget(), { matches: false }));
  const postMessage = vi.fn();
  vi.stubGlobal('nativeAppearance', { postMessage });
  const { preferences } = await import('../src/preferences');

  for (const [page, token] of [['folders', 'mobile-sidebar'], ['notes', 'mobile-list'], ['note', 'paper']] as const) {
    shell.dataset.mobilePage = page;
    await Promise.resolve();
    expect(postMessage).toHaveBeenLastCalledWith(`light:${themePalette('default', false)[token]}`);
  }

  shell.dataset.mobilePage = 'folders';
  preferences.update({ appearance: 'dark', theme: 'forest' });
  const dark = themePalette('forest', true);
  expect(postMessage).toHaveBeenLastCalledWith(`dark:${dark['mobile-sidebar']}`);
  shell.setAttribute('data-mobile-wide', '');
  await Promise.resolve();
  expect(postMessage).toHaveBeenLastCalledWith(`dark:${dark.paper}`);
});
