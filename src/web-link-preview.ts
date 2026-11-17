import { invoke } from '@tauri-apps/api/core';
import { native } from './preferences';
import { isServerLibrary } from './server-library';

type Resource = { url: string; mime: string; content: string };
export type WebLinkPreview = { title?: string; summary?: string; cover?: string; icon?: string };
const imageType = /^image\/(?:avif|gif|jpeg|png|webp|x-icon|vnd\.microsoft\.icon)$/u;
const pages = new Map<string, Promise<WebLinkPreview | null>>();
const pictures = new Map<string, Promise<string | null>>();

async function browserResource(target: string, image: boolean): Promise<Resource | null> {
  const url = new URL(target);
  if (url.protocol !== 'https:' || !url.hostname.includes('.') || url.port ||
    url.username || url.password || /^[\d.]+$/u.test(url.hostname) || /\.(?:local|localhost)\.?$/iu.test(url.hostname)) return null;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 8000);
  try {
    const response = await fetch(url, { credentials: 'omit', referrerPolicy: 'no-referrer', redirect: 'error', signal: controller.signal });
    const mime = response.headers.get('Content-Type')?.split(';')[0].toLowerCase().trim() ?? '';
    if (!response.ok || (image ? !imageType.test(mime) : !['text/html', 'application/xhtml+xml'].includes(mime))) return null;
    const reader = response.body?.getReader();
    if (!reader) return null;
    const bytes = new Uint8Array((image ? 768 : 300) * 1024);
    let size = 0;
    try {
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        if (image && size + value.length > bytes.length) return null;
        const part = value.subarray(0, bytes.length - size);
        bytes.set(part, size);
        size += part.length;
        if (!image && size === bytes.length) break;
      }
    } finally { void reader.cancel().catch(() => {}); }
    if (!size) return null;
    if (!image) return { url: response.url, mime, content: new TextDecoder().decode(bytes.subarray(0, size)) };
    const content = await new Promise<string>((resolve, reject) => {
      const file = new FileReader();
      file.onload = () => resolve(String(file.result).split(',', 2)[1]);
      file.onerror = () => reject(file.error);
      file.readAsDataURL(new Blob([bytes.subarray(0, size)], { type: mime }));
    });
    return { url: response.url, mime, content };
  } finally { clearTimeout(timeout); }
}

async function resource(target: string, image: boolean): Promise<Resource | null> {
  const request = { target, image };
  if (native) return invoke('fetch_link_preview', { request });
  if (!isServerLibrary()) return browserResource(target, image);
  const result = await fetch('/api/web/link-preview', {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(request),
  });
  return result.ok ? result.json() : null;
}

export function readWebLinkPreview(html: string, url: string): WebLinkPreview {
  // Template contents stay inert. Only text and candidate URLs leave this fragment.
  const template = document.createElement('template');
  template.innerHTML = html;
  const meta = (key: string) => template.content.querySelector('meta[property="' + key + '" i],meta[name="' + key + '" i]')?.getAttribute('content');
  const text = (value: string | null | undefined, limit: number) => value?.replace(/\s+/gu, ' ').trim().slice(0, limit) || undefined;
  function image(value: string | null | undefined) {
    if (!value?.trim()) return undefined;
    try {
      const target = new URL(value.trim(), url);
      return ['https:', 'http:'].includes(target.protocol) && !target.username && !target.password ? target.href : undefined;
    } catch { return undefined; }
  }
  return {
    title: text(meta('og:title'), 160) || text(meta('twitter:title'), 160) || text(template.content.querySelector('title')?.textContent, 160),
    summary: text(meta('og:description'), 250) || text(meta('twitter:description'), 250) || text(meta('description'), 250),
    cover: image(meta('og:image:secure_url')) ?? image(meta('og:image')) ?? image(meta('twitter:image')),
    icon: image(template.content.querySelector('link[rel~="icon" i]')?.getAttribute('href')),
  };
}

function cached<T>(cache: Map<string, Promise<T | null>>, key: string, load: () => Promise<T | null>, max: number) {
  const hit = cache.get(key);
  if (hit) return hit;
  const value = load().catch(() => null);
  cache.set(key, value);
  if (cache.size > max) cache.delete(cache.keys().next().value!);
  void value.then(result => {
    if (result === null) setTimeout(() => { if (cache.get(key) === value) cache.delete(key); }, 30000);
  });
  return value;
}

export function webLinkPreview(target: string) {
  const url = new URL(target);
  url.hash = '';
  target = url.href;
  return cached(pages, target, async () => {
    const page = await resource(target, false);
    return page ? readWebLinkPreview(page.content, page.url) : null;
  }, 128);
}

export function webLinkThumbnail(target: string) {
  return cached(pictures, target, async () => {
    const preview = await resource(target, true);
    return preview && imageType.test(preview.mime) && /^[A-Za-z\d+/]+={0,2}$/u.test(preview.content)
      ? 'data:' + preview.mime + ';base64,' + preview.content : null;
  }, 24);
}
