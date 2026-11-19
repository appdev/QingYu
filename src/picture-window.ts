import { invoke } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { PictureRequest } from './note-picture';

type Snapshot = Pick<PictureRequest, 'title' | 'source' | 'selection'>;
type Request = { window: string; id: string; path?: string };
type Response = { id: string; snapshot?: Snapshot; image?: string; failed?: boolean };

export async function openPictureWindow(picture: PictureRequest, restoreFocus: () => void) {
  const owner = getCurrentWindow().label;
  const label = `feature-picture-${crypto.randomUUID()}`;
  const stopRequest = await listen<Request>('feature-picture-request', async ({ payload }) => {
    if (payload?.window !== label || typeof payload.id !== 'string') return;
    const response: Response = { id: payload.id };
    if (typeof payload.path === 'string') {
      try {
        await picture.prepareImage?.(payload.path);
        response.image = await picture.image?.(payload.path);
      } catch { response.failed = true; }
    } else {
      const { title, source, selection } = picture;
      response.snapshot = { title, source, selection };
    }
    await emitTo(label, 'feature-picture-response', response).catch(() => {});
  }, { target: owner });
  let stopClose: (() => void) | undefined;
  const dispose = () => { stopRequest(); stopClose?.(); };
  try {
    stopClose = await listen<string>('feature-window-closed', ({ payload }) => {
      if (payload !== label) return;
      dispose();
      restoreFocus();
    }, { target: owner });
    await invoke('open_feature_window', { label });
  } catch (error) {
    dispose();
    throw error;
  }
}

export async function loadPictureWindow(owner: string): Promise<PictureRequest> {
  const label = getCurrentWindow().label;
  async function read(path?: string): Promise<Response> {
    const id = crypto.randomUUID();
    let stop: (() => void) | undefined;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      return await new Promise<Response>((resolve, reject) => {
        timeout = setTimeout(() => reject(new Error('编辑器未响应')), path === undefined ? 10000 : 300000);
        void listen<Response>('feature-picture-response', ({ payload }) => {
          if (payload.id !== id) return;
          if (payload.failed) reject(new Error('无法加载笔记图片'));
          else resolve(payload);
        }, { target: label }).then(unlisten => {
          stop = unlisten;
          return emitTo(owner, 'feature-picture-request', { window: label, id, path });
        }).catch(reject);
      });
    } finally {
      clearTimeout(timeout);
      stop?.();
    }
  }
  const { snapshot } = await read();
  if (!snapshot) throw new Error('无法读取笔记');
  // Keep attachment resolution in the editor that owns its library and sync session.
  const pending = new Map<string, Promise<Response>>();
  return { ...snapshot, prepareImage(path) {
    const result = read(path);
    pending.set(path, result);
    return result;
  }, async image(path) {
    return (await pending.get(path))?.image;
  } };
}
