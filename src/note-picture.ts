import { GFM, parser } from '@lezer/markdown';
import DOMPurify from 'dompurify';
import { invoke } from '@tauri-apps/api/core';
import { markdownBody, renderMarkdown } from './markdown';
import { isDark, native, previewPreferences } from './preferences';
import { themePalette } from './themes';
import { readWikiLink } from './vendor/markra/editor/codemirror/wiki-link';
import './note-picture.css';

export type PictureRange = { from: number; to: number };
export type PictureRequest = { title: string; source: string; selection?: PictureRange; image?: (path: string) => Promise<string | undefined> };
const scenes = { hills: '山丘', sea: '海边', dusk: '暮色', orchard: '果园', paper: '纸白', ink: '墨色' };
type Scene = keyof typeof scenes;
type Look = { scene: Scene; dark: boolean; mark: boolean };
const sceneURL = (scene: Scene) => `./note-picture/${scene}.webp`;
const plain = { paper: '#f5eedf', ink: '#20140c' };
const markdown = parser.configure(GFM);
const configKey = 'qingyu.notePicture';

export function picturePassage(source: string, selection?: PictureRange) {
  const { body } = markdownBody(source);
  const offset = source.length - body.length;
  const whole = { text: body.trim(), startsAtTitle: true, isSelection: false };
  if (!selection || selection.from < 0 || selection.to > source.length || selection.from >= selection.to
    || !body.slice(Math.max(0, selection.from - offset), Math.max(0, selection.to - offset)).trim()) return whole;
  const lineStart = (at: number) => at === 0 ? 0 : body.lastIndexOf('\n', at - 1) + 1;
  const lineEnd = (at: number) => { const next = body.indexOf('\n', at); return next < 0 ? body.length : next + 1; };
  let from = lineStart(Math.max(0, selection.from - offset));
  let to = lineEnd(Math.max(0, selection.to - offset - 1));
  markdown.parse(body).iterate({ enter(node) {
    if (!['Table', 'FencedCode', 'CodeBlock'].includes(node.name)) return;
    if (node.from < to && node.to > from) {
      from = Math.min(from, lineStart(node.from));
      to = Math.max(to, lineEnd(node.to - 1));
    }
    return false;
  } });
  return { text: body.slice(from, to).trim(), startsAtTitle: !body.slice(0, from).trim(), isSelection: from > 0 || to < body.length };
}

export function pictureFilename(title: string) {
  const name = [...title.replace(/[\\/:*?"<>|\u0000-\u001f\u007f]/gu, '-').trim()].slice(0, 72).join('');
  return `${name || '轻羽笔记'}.png`;
}

function lookFromDevice(): Look {
  const dark = isDark();
  const defaults = { scene: dark ? 'dusk' : 'hills', dark, mark: true } as const;
  try {
    const saved = JSON.parse(localStorage.getItem(configKey) ?? '{}');
    return { ...defaults, scene: typeof saved.scene === 'string' && Object.hasOwn(scenes, saved.scene) ? saved.scene : defaults.scene, mark: saved.mark !== false };
  } catch { return defaults; }
}

function replaceUnavailableImage(image: HTMLImageElement) {
  const label = image.alt || '图片';
  if (!image.hasAttribute('data-standalone-image')) {
    image.replaceWith(document.createTextNode(`〔${label}〕`));
    return;
  }
  const placeholder = document.createElement('span');
  placeholder.className = 'note-picture-image-placeholder';
  placeholder.textContent = label;
  image.replaceWith(placeholder);
}

async function pictureContent(text: string, startsAtTitle: boolean, image: PictureRequest['image']) {
  const first = text.split('\n', 1)[0];
  const title = startsAtTitle && first && /^[\p{L}\p{N}]/u.test(first) && !/^\d+[.)]\s/u.test(first) && !first.includes('|') && !/^https?:\/\//iu.test(first);
  const fragment = DOMPurify.sanitize(renderMarkdown(title ? `# ${text}` : text).html, {
    RETURN_DOM_FRAGMENT: true, USE_PROFILES: { html: true }, FORBID_TAGS: ['style', 'iframe', 'audio', 'video', 'source', 'picture', 'form', 'link', 'object', 'embed'],
    FORBID_ATTR: ['style', 'srcset', 'background', 'ping', 'poster'],
  });
  for (const element of fragment.querySelectorAll('input')) {
    if (element.type !== 'checkbox') element.remove();
    else {
      const tick = document.createElement('span'); tick.className = 'note-picture-tick';
      if (element.checked) { tick.classList.add('checked'); tick.textContent = '✓'; }
      element.replaceWith(tick);
    }
  }
  // Attachments are loaded from the note's own library. The detached document never loads remote images.
  let imageCount = 0;
  await Promise.all([...fragment.querySelectorAll('img')].map(async element => {
    const src = element.getAttribute('src') ?? '';
    const data = image && src && imageCount++ < 12 ? await image(src).catch(() => undefined) : undefined;
    if (data && /^data:image\/(?:png|jpeg|webp|gif|bmp);base64,/u.test(data)) element.src = data;
    else replaceUnavailableImage(element);
  }));
  for (const anchor of fragment.querySelectorAll('a')) anchor.removeAttribute('href');
  const walker = document.createTreeWalker(fragment, NodeFilter.SHOW_TEXT);
  const wiki: Text[] = [];
  while (walker.nextNode()) {
    const node = walker.currentNode as Text;
    if (node.data.includes('[[') && !node.parentElement?.closest('code, pre, a')) wiki.push(node);
  }
  for (const node of wiki) {
    const pieces = node.data.split(/(!?\[\[[^\[\]\r\n]+\]\])/gu);
    node.replaceWith(...pieces.map(piece => {
      const link = readWikiLink(piece.replace(/^!/u, ''));
      if (!link) return document.createTextNode(piece);
      const label = document.createElement('a'); label.textContent = link.text; return label;
    }));
  }
  return fragment;
}

async function renderPicture(request: PictureRequest, look: Look) {
  const passage = picturePassage(request.source, request.selection);
  if (!passage.text) return undefined;
  const { default: html2canvas } = await import('html2canvas');
  const prefs = previewPreferences(), colors = themePalette(prefs.theme, look.dark);
  const scene = document.createElement('div'); scene.className = 'note-picture-scene'; scene.setAttribute('aria-hidden', 'true');
  scene.style.colorScheme = look.dark ? 'dark' : 'light';
  scene.style.backgroundColor = look.scene === 'ink' ? plain.ink : plain.paper;
  if (look.scene !== 'paper' && look.scene !== 'ink') scene.style.backgroundImage = `url("${new URL(sceneURL(look.scene), location.href).href}")`;
  const card = document.createElement('div'); card.className = 'note-picture-card';
  card.style.cssText = `background:${colors.paper};color:${colors.ink};--picture-page:${colors.paper};--picture-link:${colors.link};--picture-soft:${colors.sand};--picture-rule:${colors.line};--picture-code:${colors.code};--picture-mark:${colors.brand};--picture-muted:${colors.muted}`;
  card.style.fontFamily = prefs.font;
  const body = document.createElement('article'); body.className = 'note-picture-content';
  const prefix = passage.text.slice(0, 24_000);
  body.append(await pictureContent(prefix, passage.startsAtTitle, request.image)); card.append(body);
  if (look.mark) {
    const mark = document.createElement('div'); mark.className = 'note-picture-mark'; mark.textContent = '轻羽 QingYu'; card.append(mark);
  }
  scene.append(card); document.body.append(scene);
  try {
    const column = body.clientWidth;
    await Promise.all([...body.querySelectorAll('img')].map(async img => {
      try { await img.decode(); } catch { replaceUnavailableImage(img); return; }
      if (img.hasAttribute('data-standalone-image') && img.naturalWidth && img.naturalHeight) {
        const aspect = img.naturalWidth / img.naturalHeight;
        const width = Math.min(column, column * .9 * aspect);
        img.style.width = `${Math.round(width)}px`;
        img.style.height = `${Math.max(40, Math.round(width / aspect))}px`;
      }
    }));
    const maxBody = look.mark ? 440 : 472;
    const fades = body.scrollHeight > maxBody || prefix.length < passage.text.length;
    if (fades) { body.style.maxHeight = `${maxBody}px`; body.classList.add('note-picture-fades'); }
    scene.style.height = `${Math.max(360, Math.min(600, card.offsetHeight + 80))}px`;
    const canvas = await html2canvas(scene, { scale: 5, backgroundColor: null, imageTimeout: 3000, logging: false });
    const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob(result => result ? resolve(result) : reject(new Error('无法生成 PNG'))));
    return { blob, fades, selected: passage.isSelection };
  } finally { scene.remove(); }
}

export function mountNotePicture(root: HTMLElement, request: PictureRequest, detailed = false) {
  const look = lookFromDevice();
  const area = document.createElement('div'); area.className = 'note-picture';
  const preview = document.createElement('img'); preview.className = 'note-picture-preview'; preview.alt = '导出的笔记图片预览'; preview.hidden = true;
  const status = document.createElement('p'); status.className = 'note-picture-status'; status.role = 'status';
  const backgrounds = document.createElement('div'); backgrounds.className = 'note-picture-backgrounds'; backgrounds.role = 'group'; backgrounds.ariaLabel = '背景';
  const label = document.createElement('span'); label.textContent = '背景'; if (!detailed) backgrounds.append(label);
  const swatches = Object.entries(scenes).map(([key, name]) => {
    const scene = key as Scene;
    const button = document.createElement('button'); button.type = 'button'; button.ariaLabel = name; button.title = name;
    button.className = 'picture-swatch';
    if (scene === 'paper' || scene === 'ink') button.style.backgroundColor = plain[scene];
    else button.style.backgroundImage = `url("${sceneURL(scene)}")`;
    button.onclick = () => { look.scene = scene; saveLook(); void draw(); };
    backgrounds.append(button); return { scene, button };
  });
  area.append(preview, status, backgrounds);
  function saveLook() { try { localStorage.setItem(configKey, JSON.stringify({ scene: look.scene, mark: look.mark })); } catch { /* This render remains usable. */ } }
  if (detailed) {
    const options = document.createElement('div'); options.className = 'note-picture-options';
    const mode = document.createElement('div'); mode.role = 'group'; mode.ariaLabel = '图片外观'; mode.className = 'note-picture-mode';
    const light = document.createElement('button'), dark = document.createElement('button');
    light.type = dark.type = 'button'; light.textContent = '浅色'; dark.textContent = '深色';
    const updateMode = () => { light.ariaPressed = String(!look.dark); dark.ariaPressed = String(look.dark); };
    light.onclick = () => { look.dark = false; updateMode(); void draw(); }; dark.onclick = () => { look.dark = true; updateMode(); void draw(); };
    updateMode(); mode.append(light, dark);
    const mark = document.createElement('label'), toggle = document.createElement('input'); toggle.type = 'checkbox'; toggle.checked = look.mark;
    toggle.onchange = () => { look.mark = toggle.checked; saveLook(); void draw(); }; mark.append(toggle, ' 轻羽标记');
    options.append(mode, mark); area.append(options);
  }
  const buttons = document.createElement('div'); buttons.className = 'note-picture-actions';
  const isMac = native && document.documentElement.dataset.platform === 'macos';
  const filename = pictureFilename(request.title);
  let result: Awaited<ReturnType<typeof renderPicture>>, imageURL = '', generation = 0;
  const actions = (['copy', 'save', 'share'] as const).map(action => {
    const button = document.createElement('button'); button.type = 'button'; button.disabled = true;
    const title = { copy: '复制图片', save: '保存…', share: '分享…' }[action];
    button.textContent = title; button.dataset.pictureAction = action;
    if (action === (detailed ? 'share' : 'copy')) button.className = 'primary-button';
    if (action === 'share' && !isMac && !navigator.canShare?.({ files: [new File([], filename, { type: 'image/png' })] })) button.hidden = true;
    button.onclick = async () => {
      if (!result) return;
      const blob = result.blob;
      try {
        if (isMac || native && action === 'save' && !document.documentElement.hasAttribute('data-mobile')) {
          const { x, y, width, height } = button.getBoundingClientRect();
          const completed = await invoke<boolean>('note_picture_action', { action, filename, png: Array.from(new Uint8Array(await blob.arrayBuffer())), anchor: { x, y, width, height } });
          if (!completed) return;
        } else if (action === 'copy') await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
        else if (action === 'share') await navigator.share({ files: [new File([blob], filename, { type: 'image/png' })] });
        else {
          const link = document.createElement('a'); link.href = imageURL; link.download = filename; link.click();
        }
        if (action !== 'share') { button.textContent = action === 'copy' ? '已复制' : '已保存'; setTimeout(() => { button.textContent = title; }, 1500); }
      } catch (err) { if (!(err instanceof DOMException && err.name === 'AbortError')) status.textContent = '操作未完成，请重试。'; }
    };
    buttons.append(button); return button;
  });
  area.append(buttons); root.append(area);
  async function draw() {
    const version = ++generation;
    actions.forEach(button => { button.disabled = true; });
    for (const { scene, button } of swatches) button.ariaPressed = String(scene === look.scene);
    status.textContent = '正在生成图片…';
    const next = await renderPicture(request, { ...look }).catch(() => null);
    if (version !== generation || !area.isConnected) return;
    if (!next) { status.textContent = next === null ? '无法生成图片。' : '没有可分享的内容。'; return; }
    result = next;
    if (imageURL) URL.revokeObjectURL(imageURL);
    imageURL = URL.createObjectURL(next.blob);
    preview.src = imageURL; preview.hidden = false;
    status.textContent = next.fades ? (next.selected ? '选区过长，图片末尾会淡出。可缩小选区以显示全部。' : '笔记过长，图片末尾会淡出。可选择一段内容再分享。') : '';
    actions.forEach(button => { button.disabled = false; });
  }
  void draw();
  return () => { generation++; if (imageURL) URL.revokeObjectURL(imageURL); area.remove(); };
}

export function showNotePicture(request: PictureRequest, restoreFocus: () => void) {
  const dialog = document.createElement('dialog'); dialog.className = 'note-picture-dialog'; dialog.ariaLabel = '分享为图片';
  const close = document.createElement('button'); close.type = 'button'; close.className = 'note-picture-done'; close.textContent = '完成'; close.onclick = () => dialog.close();
  dialog.addEventListener('keydown', event => event.stopPropagation());
  dialog.append(close); document.body.append(dialog); dialog.showModal();
  const dispose = mountNotePicture(dialog, request, true);
  dialog.addEventListener('close', () => { dispose(); dialog.remove(); restoreFocus(); }, { once: true });
}
