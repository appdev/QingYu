import { formatLabel, translatedLabel } from './ui-language';
import { syntaxTree } from '@codemirror/language';
import { StateEffect, StateField, type EditorState, type Range } from '@codemirror/state';
import { Decoration, EditorView, WidgetType, type DecorationSet } from '@codemirror/view';
import { createLucideIcon } from '@markra/shared';
import { FileText, Folder, Globe, NotebookText, Paperclip } from 'lucide';
import { readWikiLink } from './vendor/markra/editor/codemirror/wiki-link';
import type { webLinkPreview, webLinkThumbnail } from './web-link-preview';
import { formatAssetSize } from './asset-download-view';

type LinkStyle = 'link' | 'compact' | 'card';
type LinkForm = { kind: 'web' | 'note' | 'file'; style: LinkStyle; href: string; label: string; target: string; markup: string; indent: string };
export type NoteCardPreview = { detail?: string; folder: string };
export type LinkPreviewLoader = { page: typeof webLinkPreview; thumbnail: typeof webLinkThumbnail };

function parseLine(text: string): LinkForm | null {
  const markup = text.trim(), indent = text.slice(0, text.length - text.trimStart().length);
  if (markup.startsWith('[[') || markup.startsWith('![[')) {
    const card = markup.startsWith('!');
    const link = readWikiLink(card ? markup.slice(1) : markup);
    return link ? { kind: 'note', style: card ? 'card' : 'link', href: link.href, label: link.text, target: link.target, markup, indent } : null;
  }
  const named = /^(!?)\[((?:\\.|[^\\\]])*)\]\((?:<([^\s<>]+)>|([^\s<>]+))\)$/u.exec(markup);
  const destination = named?.[3] ?? named?.[4];
  if (named && destination && /^\/?(?:\.attachments|files|i)\//u.test(destination)) {
    const path = destination.split(/[?#]/u)[0];
    let name: string;
    try { name = decodeURIComponent(path).split('/').at(-1) ?? ''; } catch { return null; }
    if (!name || /\.(?:png|jpe?g|gif|avif|heic|heif|webp|tiff?|bmp|svg|ico)$/iu.test(name)) return null;
    return { kind: 'file', style: named[1] ? 'card' : 'compact', href: destination, target: destination, label: named[2].replace(/\\([\\[\]])/gu, '$1') || name, markup, indent };
  }
  if (named?.[1]) return null;
  const angle = /^<(https?:\/\/[^\s<>]+)>$/iu.exec(markup);
  const target = destination ?? angle?.[1] ?? markup;
  if (!/^https?:\/\/[^\s<>]+$/iu.test(target)) return null;
  try {
    const { hostname } = new URL(target);
    const host = hostname.replace(/^www\./iu, '');
    return { kind: 'web', style: named ? 'link' : angle ? 'card' : 'compact', href: target, label: host, target, markup, indent };
  } catch { return null; }
}

function atRootParagraph(state: EditorState, from: number) {
  for (let node = syntaxTree(state).resolveInner(from, 1); node.parent; node = node.parent) {
    if (node.name === 'Paragraph') return node.parent.type.isTop;
  }
  return false;
}

export function linkFormAt(state: EditorState, at: number) {
  const line = state.doc.lineAt(at);
  const form = parseLine(line.text);
  return form && atRootParagraph(state, line.from + form.indent.length) ? { line, form } : null;
}

export function otherLinkStyles(form: LinkForm) {
  const styles: LinkStyle[] = form.kind === 'web' ? ['link', 'compact', 'card'] : form.kind === 'note' ? ['link', 'card'] : ['compact', 'card'];
  return styles.filter(style => style !== form.style);
}

export function linkStyleSource(form: LinkForm, style: LinkStyle, webTitle?: string) {
  if (form.kind !== 'web') return form.indent + (style === 'card' ? '!' : '') + form.markup.replace(/^!/u, '');
  if (style === 'compact') return form.indent + form.target;
  if (style === 'card') return `${form.indent}<${form.target}>`;
  const label = (webTitle || form.label).replace(/[\\[\]<>&*_~!=\u0060$]/gu, '\\$&');
  const destination = /[()\\]/u.test(form.target) ? `<${form.target}>` : form.target;
  return `${form.indent}[${label}](${destination})`;
}

export const refreshLinkCards = StateEffect.define<null>();

export function linkCards(open: (href: string) => void | Promise<void>, describeNote?: (target: string) => NoteCardPreview | undefined,
  web?: LinkPreviewLoader, pendingAssetSize?: (path: string) => Promise<number | undefined>) {
  class LinkCardWidget extends WidgetType {
    constructor(readonly form: LinkForm, readonly about: NoteCardPreview | undefined, readonly selected: boolean) { super(); }

    eq(other: LinkCardWidget) {
      return this.form.markup === other.form.markup && this.about?.detail === other.about?.detail
        && this.about?.folder === other.about?.folder && this.selected === other.selected;
    }

    get estimatedHeight() { return this.form.style === 'card' ? 72 : 34; }
    ignoreEvent(event: Event) { return event.type !== 'contextmenu'; }

    toDOM() {
      const { style, kind, href, label } = this.form;
      const link = document.createElement('a');
      link.className = 'qingyu-link-card';
      link.dataset.style = style;
      link.dataset.kind = kind;
      link.classList.toggle('qingyu-link-card-selected', this.selected);
      link.setAttribute('href', href);
      link.contentEditable = 'false';
      let download: HTMLSpanElement | undefined;
      link.onclick = async event => {
        event.preventDefault();
        if (link.ariaBusy === 'true') return;
        if (download) { link.ariaBusy = 'true'; download.textContent = translatedLabel('正在下载…'); }
        try { await open(href); download?.remove(); download = undefined; }
        catch { if (download) download.textContent = translatedLabel('点按重试'); }
        finally { link.removeAttribute('aria-busy'); }
      };
      const thumbnail = document.createElement('span');
      thumbnail.className = 'link-card-thumbnail';
      thumbnail.append(createLucideIcon(document, kind === 'web' ? Globe : kind === 'note' ? NotebookText : FileText, 'link-card-mark'));
      const text = document.createElement('span'); text.className = 'link-card-text';
      const title = document.createElement('span'); title.className = 'link-card-title'; title.textContent = label;
      text.append(title);
      const summary = document.createElement('span'); summary.className = 'link-card-detail';
      const source = document.createElement('span'); source.className = 'link-card-source';
      if (style === 'card') {
        const detail = kind === 'note' ? (this.about ? this.about.detail : '还没有这个名称的笔记') : undefined;
        if (detail) {
          summary.textContent = detail;
          text.append(summary);
        }
        source.append(createLucideIcon(document, kind === 'note' ? Folder : kind === 'file' ? Paperclip : Globe, 'link-card-mark'),
          kind === 'note' ? this.about?.folder ?? '备忘录' : kind === 'file' ? '文件' : label);
        text.append(source);
      }
      link.append(thumbnail, text);
      if (kind === 'file' && pendingAssetSize) void pendingAssetSize(href).then(size => {
        if (size === undefined || !link.isConnected || link.ariaBusy === 'true') return;
        download = document.createElement('span');
        download.className = 'link-card-download';
        download.textContent = formatLabel('下载 · {size}', { size: formatAssetSize(size) });
        link.append(download);
      }).catch(() => undefined);
      if (kind === 'web' && web) void web.page(href).then(async info => {
        if (!info || !link.isConnected) return;
        if (info.title) {
          title.textContent = info.title;
          link.dataset.webTitle = info.title;
          if (style === 'compact') { source.textContent = label; text.append(source); }
        }
        if (style === 'card' && info.summary) {
          summary.textContent = info.summary;
          text.insertBefore(summary, source);
        }
        async function showThumbnail(url: string | undefined, cover: boolean) {
          if (!url || !link.isConnected) return false;
          const picture = await web!.thumbnail(url);
          if (!picture || !link.isConnected) return false;
          const image = document.createElement('img');
          image.alt = '';
          image.className = cover ? 'link-card-cover' : 'link-card-icon';
          image.src = picture;
          try { await image.decode(); } catch { return false; }
          if (link.isConnected) thumbnail.replaceChildren(image);
          return true;
        }
        if (style !== 'card' || !await showThumbnail(info.cover, true)) await showThumbnail(info.icon, false);
      });
      return link;
    }
  }

  function decorations(state: EditorState) {
    const cards: Range<Decoration>[] = [];
    syntaxTree(state).iterate({ enter(node) {
      if (node.type.isTop) return;
      if (node.name === 'Paragraph') {
        const last = state.doc.lineAt(Math.max(node.from, node.to - 1)).number;
        for (let number = state.doc.lineAt(node.from).number; number <= last; number++) {
          const line = state.doc.line(number);
          if (!/^[\t ]*(?:!|\[|<|https?:\/\/)/iu.test(line.text)) continue;
          const form = parseLine(line.text);
          if (!form || form.style === 'link') continue;
          const selected = state.selection.ranges.some(range => !range.empty && range.from <= line.from && range.to >= line.to);
          const about = form.kind === 'note' ? describeNote?.(form.target) : undefined;
          cards.push(Decoration.replace({ block: true, widget: new LinkCardWidget(form, about, selected) }).range(line.from, line.to));
        }
      }
      return false;
    } });
    return Decoration.set(cards, true);
  }

  const field = StateField.define<DecorationSet>({
    create: decorations,
    update(cards, tr) {
      const selectionChanged = tr.selection && (tr.state.selection.ranges.some(range => !range.empty) || tr.startState.selection.ranges.some(range => !range.empty));
      return tr.docChanged || selectionChanged
        || syntaxTree(tr.state) !== syntaxTree(tr.startState) || tr.effects.some(effect => effect.is(refreshLinkCards)) ? decorations(tr.state) : cards;
    },
    provide: field => [EditorView.decorations.from(field), EditorView.atomicRanges.of(view => view.state.field(field))],
  });
  return field;
}
