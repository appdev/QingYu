import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import type { Notebook } from './notebook';
import { native } from './preferences';
import { isLocalAsset } from './server-assets';
import { assetPath, assetRelative, type AssetPlaceholders } from './sync-files';

const placeholderScans = new WeakMap<Notebook, { root: string; revision: number; entries: Promise<AssetPlaceholders> }>();

export async function attachmentPlaceholder(book: Notebook, noteId: string, source: string) {
  const root = book.root, note = book.data.notes.find(item => item.id === noteId);
  if (!root || book.external || book.server || book.data.sync?.version !== 2 || !note?.path || !isLocalAsset(source)) return;
  let path: string;
  try {
    const value = decodeURIComponent(source.split(/[?#]/)[0]!);
    if (value.startsWith('/') && !/^\/(?:i|files)\//.test(value)) return;
    const parts = note.path.split('/').slice(0, -1);
    for (const part of value.split('/')) {
      if (!part || part === '.') continue;
      if (part === '..') { if (!parts.length) return; parts.pop(); }
      else parts.push(part);
    }
    if (parts.some(part => part.startsWith('.') && part !== '.attachments')) return;
    path = assetRelative(assetPath(parts.join('/')));
  } catch { return; }
  const revision = book.assetRevision;
  let scan = placeholderScans.get(book);
  if (scan?.root !== root || scan.revision !== revision) {
    const entries = book.readAssetPlaceholders();
    scan = { root, revision, entries };
    placeholderScans.set(book, scan);
    void entries.catch(() => { if (placeholderScans.get(book) === scan) placeholderScans.delete(book); });
  }
  const entries = await scan.entries;
  if (book.root === root && book.assetRevision === revision && Object.hasOwn(entries, path)) {
    return { root, path, ...entries[path]! };
  }
}

export function assetBase(book: Notebook, noteId = book.current?.id) {
  if (!native || !noteId) return undefined;
  return `${convertFileSrc(noteId, 'qingyu-asset')}?v=${book.assetRevision}`;
}

/** 某篇笔记里相对路径附件的地址：桌面走资源协议，服务器笔记库走 Web 接口。 */
export function attachmentUrl(book: Notebook, noteId: string, source: string) {
  let path: string;
  try { path = decodeURIComponent(source.split(/[?#]/)[0]); } catch { return; }
  const query = new URLSearchParams({ path });
  if (native) return `${convertFileSrc(noteId, 'qingyu-asset')}?v=${book.assetRevision}&${query}`;
  if (book.server) return `/api/web/assets/${encodeURIComponent(noteId)}?${query}`;
}

export function attachmentMarkdown(file: { name: string; path: string; image: boolean }) {
  const label = file.name.replace(/[\\[\]]/g, '\\$&');
  const path = file.path.split('/').map(part => encodeURIComponent(part).replace(/[()']/g, char => `%${char.charCodeAt(0).toString(16).toUpperCase()}`)).join('/');
  return `${file.image ? '!' : ''}[${label}](${path})`;
}

export async function pickMemoAttachments(images: boolean, remaining: number) {
  const bytes = await invoke<ArrayBuffer>('memo_pick_attachments', { images, remaining });
  const index = new DataView(bytes).getUint32(0, true);
  const files: { name: string; type: string; size: number }[] = JSON.parse(new TextDecoder().decode(new Uint8Array(bytes, index)));
  let offset = 4;
  return files.map(({ name, type, size }) => {
    const file = new File([new Uint8Array(bytes, offset, size)], name, { type });
    offset += size;
    return file;
  });
}

export async function openAttachment(book: Notebook, id: string, value: string) {
  const path = decodeURIComponent(value.split(/[?#]/)[0]);
  if (native) await invoke('attachment_open', { id, path });
  else if (book.server) window.open(`/api/web/assets/${encodeURIComponent(id)}?${new URLSearchParams({ path })}`, '_blank', 'noopener,noreferrer');
}
