import { CloudDownload, LoaderCircle } from 'lucide';
import { createLucideIcon } from '@markra/shared';
import type { attachmentPlaceholder } from './attachments';

type PendingAsset = NonNullable<Awaited<ReturnType<typeof attachmentPlaceholder>>>;
export function formatAssetSize(bytes: number) {
  return bytes < 1_000 ? `${bytes} B` : bytes < 1_000_000 ? `${Number((bytes / 1_000).toFixed(1))} KB` : `${Number((bytes / 1_000_000).toFixed(1))} MB`;
}

export function mountPendingImage(
  image: HTMLImageElement, frame: HTMLElement, url: string,
  lookup: Promise<PendingAsset | undefined>, download: (entry: PendingAsset) => Promise<string | undefined>, thumbnail = false,
) {
  let disposed = false, loading = false;
  let button: HTMLElement | undefined;
  let observer: IntersectionObserver | undefined;
  const showImage = (hash?: string) => {
    button?.remove();
    if (hash) {
      const refreshed = new URL(url, image.ownerDocument.baseURI);
      refreshed.searchParams.set('v', hash);
      image.src = refreshed.href;
    } else image.src = url;
    image.hidden = false;
  };
  void lookup.catch(() => undefined).then(pending => {
    if (disposed) return;
    if (!pending) { showImage(); return; }
    const entry = pending;
    const document = image.ownerDocument;
    button = document.createElement(thumbnail ? 'span' : 'button');
    if (button instanceof HTMLButtonElement) button.type = 'button';
    else button.role = 'img';
    button.className = thumbnail ? 'asset-image-placeholder note-row-thumbnail' : 'asset-image-placeholder';
    const width = image.naturalWidth || Number(image.getAttribute('width'));
    const height = image.naturalHeight || Number(image.getAttribute('height'));
    if (!thumbnail && width > 0 && height > 0) button.style.aspectRatio = `${width} / ${height}`;
    const size = formatAssetSize(entry.size);
    const render = (label: string, textValue = size, busy = false) => {
      if (!button) return;
      button.setAttribute('aria-label', label);
      button.setAttribute('aria-busy', String(busy));
      button.title = label;
      const text = document.createElement('span');
      text.textContent = textValue;
      button.replaceChildren(createLucideIcon(document, busy ? LoaderCircle : CloudDownload, ''), text);
    };
    async function start() {
      if (disposed || loading || !button?.isConnected) return;
      loading = true;
      observer?.disconnect();
      render('正在下载图片', size, true);
      try {
        const hash = await download(entry);
        if (!disposed) showImage(hash ?? entry.hash);
      } catch {
        if (!disposed) render('点按重试', '点按重试');
      } finally { loading = false; }
    }
    render(thumbnail ? `图片尚未下载 · ${size}` : `下载图片 · ${size}`);
    image.hidden = true;
    image.removeAttribute('src');
    frame.append(button);
    // 列表缩略图只占几十像素，不值得为它拉原图；打开笔记后正文里的图片再下载。
    if (thumbnail) return;
    button.onpointerdown = button.onmousedown = event => event.stopPropagation();
    button.onclick = event => { event.preventDefault(); event.stopPropagation(); void start(); };
    if (typeof IntersectionObserver === 'function') {
      observer = new IntersectionObserver(entries => {
        if (entries.some(item => item.isIntersecting)) void start();
      });
      observer.observe(button);
    } else void start();
  });
  return () => {
    disposed = true;
    observer?.disconnect();
    button?.remove();
    image.hidden = false;
  };
}
