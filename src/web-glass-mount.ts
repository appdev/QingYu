import type { WebGlassProps } from './web-glass';

export function usesGlisse(element: HTMLElement) {
  const view = element.ownerDocument.defaultView;
  if (!view || !('__TAURI_INTERNALS__' in view) || element.ownerDocument.documentElement.hasAttribute('data-web')) return true;
  const { platform, userAgent } = view.navigator;
  return !(element.ownerDocument.documentElement.dataset.platform === 'macos'
    || /^(Mac|iPhone|iPad|iPod)/.test(platform) || /iPhone|iPad|iPod/.test(userAgent));
}

export function mountWebGlass(host: HTMLElement, props: WebGlassProps) {
  host.dataset.glassHost = '';
  let disposed = false;
  let unmount: (() => void) | undefined;
  const view = host.ownerDocument.defaultView;
  if (view?.CanvasRenderingContext2D && view.ResizeObserver) {
    void import('./web-glass').then(({ renderWebGlass }) => {
      if (!disposed) unmount = renderWebGlass(host, props);
    });
  }
  return () => {
    disposed = true;
    unmount?.();
    delete host.dataset.glassHost;
  };
}
