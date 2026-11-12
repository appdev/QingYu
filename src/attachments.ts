import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import type { Notebook } from './notebook';
import { native } from './preferences';

export function assetBase(book: Notebook) {
  if (!native || !book.current) return undefined;
  return `${convertFileSrc(book.current.id, 'qingyu-asset')}?v=${book.assetRevision}`;
}

export function attachmentMarkdown(file: { name: string; path: string; image: boolean }) {
  const label = file.name.replace(/[\\[\]]/g, '\\$&');
  const path = file.path.split('/').map(part => encodeURIComponent(part).replace(/[()']/g, char => `%${char.charCodeAt(0).toString(16).toUpperCase()}`)).join('/');
  return `${file.image ? '!' : ''}[${label}](${path})`;
}

export async function openAttachment(book: Notebook, id: string, value: string) {
  const path = decodeURIComponent(value.split(/[?#]/)[0]);
  if (native) await invoke('attachment_open', { id, path });
  else if (book.server) window.open(`/api/web/assets/${encodeURIComponent(id)}?${new URLSearchParams({ path })}`, '_blank', 'noopener,noreferrer');
}
