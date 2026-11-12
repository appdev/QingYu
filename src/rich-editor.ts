import Vditor from 'vditor';
import DOMPurify from 'dompurify';
import palette from './palette.json';
import { isDark } from './preferences';
import 'vditor/dist/index.css';
import './rich-editor.css';
import type { Format } from './format';
import { emphasisLanguage } from './emphasis';
import { createRichCodeRenderer } from './rich-code';
import { markdownBody } from './document-text';
import { decorateCallouts, installRichMarkdown, referenceHref } from './rich-markdown';
import { originalAssetPath, originalAssetPaths, projectServerAssets, isLocalAsset } from './server-assets';

export function installImagePreviewActions(doc: Document) {
  const close = (preview: Element) => {
    preview.remove();
    if (!doc.querySelector('.vditor-img')) doc.body.style.overflow = '';
  };
  // SDK close/backdrop handlers are inline and blocked by the app's CSP.
  doc.addEventListener('click', event => {
    if (!(event.target instanceof Element)) return;
    const preview = event.target.closest('.vditor-img');
    if (!preview) return;
    const button = event.target.closest('.vditor-img__btn');
    if (event.target.closest('.vditor-img__img') || button && !button.hasAttribute('data-deg')) {
      event.preventDefault(); event.stopImmediatePropagation(); close(preview);
    }
  }, true);
  doc.addEventListener('keydown', event => {
    if (event.key !== 'Escape') return;
    const preview = doc.querySelector('.vditor-img');
    if (preview) { event.preventDefault(); event.stopImmediatePropagation(); close(preview); }
  }, true);
}
installImagePreviewActions(document);

function loadScript(path: string, id: string) {
  if (document.getElementById(id)) return Promise.resolve();
  return new Promise<void>((resolve, reject) => {
    const script = document.createElement('script');
    script.src = path.startsWith('/') ? path : `/vditor/dist/js/${path}`;
    script.id = id;
    script.onload = () => resolve();
    script.onerror = () => { script.remove(); reject(new Error('可视化编辑器资源加载失败')); };
    document.head.appendChild(script);
  });
}

export async function createRichEditor(parent: HTMLElement, onChange: (source: string) => void, options: { readOnly?: boolean; onTask?: (input: HTMLInputElement) => void; openAsset?: (noteId: string, path: string) => void } = {}) {
  Object.assign(window, { QingYuPalette: palette });
  parent.dataset.readOnly = String(Boolean(options.readOnly));
  await loadScript('/miaoyan/js/frame-fallback.js', 'miaoyanFrameFallbackScript');
  // Preload icons as an external script: Vditor's fallback injects inline JS, blocked by our CSP.
  await Promise.all([
    loadScript('icons/ant.js', 'vditorIconScript'),
    loadScript('i18n/zh_CN.js', 'vditorI18nScriptzh_CN'),
    loadScript('lute/lute.min.js', 'vditorLuteScript'),
    loadScript('/heading-links.js', 'miaoyanHeadingLinksScript'),
    loadScript('/miaoyan/js/highlight.min.js', 'miaoyanHighlightScript'),
    loadScript('/miaoyan/js/mermaid.min.js', 'vditorMermaidScript'),
    loadScript('/miaoyan/js/theme-config.js', 'miaoyanThemeColorsScript'),
    loadScript('/miaoyan/js/theme-manager.js', 'miaoyanThemeConfigScript'),
    loadScript('/mermaid-contrast.js', 'miaoyanMermaidContrastScript'),
    loadScript('/miaoyan/js/plantuml-encoder.min.js', 'vditorPlantumlScript'),
    loadScript('/miaoyan/js/diagram-handler.js', 'miaoyanDiagramHandlerScript'),
    loadScript('/miaoyan/js/d3.min.js', 'miaoyanD3Script'),
  ]);
  await loadScript('/miaoyan/js/markmap-view.min.js', 'miaoyanMarkmapViewScript');
  await loadScript('/markmap-config.js', 'miaoyanMarkmapConfigScript');
  const maps = window as typeof window & { markmap: { autoLoader: { ready?: Promise<void> } } };
  await loadScript('/miaoyan/js/markmap.min.js', 'vditorMarkerScript');
  await maps.markmap.autoLoader.ready;
  let editor: Vditor;
  let active = false;
  let lastSource: string | undefined;
  let lastDocumentId: string | undefined;
  let renderedSource = '';
  let frontmatter = '';
  let assetNoteId: string | undefined;
  let assetBase: string | undefined;
  let insertionRange: Range | undefined;
  const markdownValue = () => originalAssetPaths(editor.vditor.wysiwyg!.element, () => editor.getValue());
  let lastFootnoteReference: HTMLElement | undefined;
  function openLink(href: string) {
    const original = originalAssetPath(editor.vditor.wysiwyg!.element, href);
    if (assetNoteId && isLocalAsset(original) && options.openAsset) { options.openAsset(assetNoteId, original); return; }
    if (href.startsWith('#')) window.MiaoYanHeadingLinks.follow(editor.vditor.wysiwyg!.element, href);
    else if (/^(https?:|mailto:)/i.test(href) || href.startsWith('/api/web/assets/')) window.open(href, '_blank', 'noopener,noreferrer');
  }
  function navigationButton(toolbar: HTMLElement, label: string, action: () => void) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'rich-navigation';
    button.textContent = label;
    button.onpointerdown = event => event.preventDefault();
    button.onclick = () => { toolbar.style.display = 'none'; action(); };
    toolbar.append(button);
  }
  function selectNode(root: HTMLElement, target: HTMLElement) {
    root.focus();
    const range = document.createRange();
    range.selectNodeContents(target.querySelector('p') ?? target);
    range.collapse(true);
    getSelection()?.removeAllRanges();
    getSelection()?.addRange(range);
  }
  function focusNode(root: HTMLElement, target: HTMLElement) {
    selectNode(root, target);
    // Vditor refreshes its contextual toolbar on click/keyup, not on a programmatic selection change.
    target.click();
    target.scrollIntoView({ block: 'center' });
  }
  function alignEmphasis() {
    decorateCallouts(editor.vditor.wysiwyg!.element);
    for (const emphasis of editor.vditor.wysiwyg!.element.querySelectorAll('em')) {
      const language = emphasisLanguage(emphasis.textContent ?? '');
      if (language) emphasis.setAttribute('lang', language);
      else emphasis.removeAttribute('lang');
    }
  }
  function flush(value?: string) {
    if (options.readOnly || !active || editor.vditor.wysiwyg!.composingLock) return;
    const next = assetNoteId ? markdownValue() : value ?? editor.getValue();
    if (next === renderedSource) return;
    const source = frontmatter + (frontmatter && !frontmatter.endsWith('\n') ? '\n' : '') + next;
    lastSource = source;
    renderedSource = next;
    onChange(source);
  }
  await new Promise<void>(resolve => {
    editor = new Vditor(parent, {
      mode: 'wysiwyg', height: '100%', cdn: '/vditor', lang: 'zh_CN',
      cache: { enable: false }, counter: { enable: false }, resize: { enable: false },
      toolbar: ['headings', 'bold', 'italic', 'strike', 'quote', 'check', 'inline-code', 'link', 'table', 'undo', 'redo'],
      link: { click(anchor) {
        const href = anchor.getAttribute('href') ?? '';
        if (href) openLink(href);
      } },
      customWysiwygToolbar(type, toolbar) {
        toolbar.dataset.blockType = type;
        if (['block', 'heading', 'blockquote', 'li'].includes(type)) {
          toolbar.replaceChildren();
          return;
        }
        toolbar.querySelectorAll('button[data-type="up"], button[data-type="down"], button[data-type="remove"]').forEach(button => button.remove());
        if (['link-ref', 'footnotes-ref', 'footnotes-block'].includes(type)) {
          const root = editor.vditor.wysiwyg!.element;
          const range = getSelection()?.rangeCount ? getSelection()!.getRangeAt(0) : undefined;
          const node = range?.startContainer;
          const selected = node?.nodeType === Node.TEXT_NODE ? node.parentElement : node instanceof Element ? node.childNodes[range!.startOffset] ?? node : undefined;
          const element = selected instanceof Element ? selected : selected?.parentElement;
          const reference = element?.closest<HTMLElement>('span[data-type="link-ref"]');
          if (type === 'link-ref' && reference) {
            navigationButton(toolbar, '打开链接', () => {
              if (!root.contains(reference)) return;
              const href = referenceHref(root, reference, editor.vditor.lute);
              if (href) openLink(href);
            });
            navigationButton(toolbar, '编辑定义', () => {
              const definitions = [...root.children].filter(node => node.getAttribute('data-type') === 'link-ref-defs-block') as HTMLElement[];
              const label = `[${reference.dataset.linkLabel ?? ''}]:`.toLowerCase();
              const target = definitions.find(node => (node.textContent ?? '').split('\n').some(line => line.trimStart().toLowerCase().startsWith(label))) ?? definitions[0];
              if (!target) return;
              root.dataset.showReferenceDefinitions = 'true';
              focusNode(root, target);
            });
          }
          const footnote = element?.closest<HTMLElement>('[data-type="footnotes-ref"], [data-type="footnotes-li"]');
          if (footnote && type.startsWith('footnotes-')) {
            const toDefinition = type === 'footnotes-ref';
            const label = footnote.getAttribute(toDefinition ? 'data-footnotes-label' : 'data-marker');
            navigationButton(toolbar, toDefinition ? '跳到脚注' : '返回引用', () => {
              const candidates = [...root.querySelectorAll<HTMLElement>(toDefinition ? '[data-type="footnotes-li"]' : '[data-type="footnotes-ref"]')];
              const previous = !toDefinition && lastFootnoteReference && root.contains(lastFootnoteReference) && lastFootnoteReference.dataset.footnotesLabel === label ? lastFootnoteReference : undefined;
              const target = previous ?? candidates.find(candidate => candidate.getAttribute(toDefinition ? 'data-marker' : 'data-footnotes-label') === label);
              if (!target) return;
              if (toDefinition) lastFootnoteReference = footnote;
              focusNode(root, target);
            });
          }
        }
        if (type === 'table') {
          // Vditor appends below/right after above/left; keep one insertion action per axis.
          const commands = ['left', 'center', 'right', 'insertRow', 'insertColumn', 'deleteRow', 'deleteColumn'];
          toolbar.replaceChildren(...commands.map(command => [...toolbar.querySelectorAll(`button[data-type="${command}"]`)].at(-1)!));
        }
      },
      preview: {
        mode: 'editor', theme: { current: '', path: '' }, hljs: { enable: false },
        markdown: { codeBlockPreview: true, mathBlockPreview: true, sanitize: true, autoSpace: false, fixTermTypo: false, footnotes: true, gfmAutoLink: false },
      },
      input: value => { alignEmphasis(); flush(value); }, after: resolve,
    });
  });
  const instance = editor!;
  installRichMarkdown(instance.vditor.lute);
  if (options.readOnly) {
    // Retain the preview's HTML trust boundary before the SDK inserts its editing DOM.
    const convert = instance.vditor.lute.Md2VditorDOM.bind(instance.vditor.lute);
    instance.vditor.lute.Md2VditorDOM = source => DOMPurify.sanitize(convert(source), {
      FORBID_TAGS: ['style', 'iframe', 'object', 'embed', 'form'], FORBID_ATTR: ['contenteditable', 'autofocus'], ADD_TAGS: ['wbr'],
    });
    instance.disabled();
  }
  const { element, popover } = instance.vditor.wysiwyg!;
  new MutationObserver(() => projectServerAssets(element, assetNoteId, assetBase)).observe(element, { subtree: true, childList: true, attributes: true, attributeFilter: ['src', 'href'] });
  if (options.readOnly) {
    element.tabIndex = 0;
    parent.addEventListener('pointerdown', () => element.focus({ preventScroll: true }), true);
    // disabled() blocks typing, but SDK click/keyboard handlers can still expand or modify blocks.
    for (const type of ['click', 'dblclick', 'keydown', 'keyup', 'beforeinput', 'copy', 'paste', 'cut', 'drop']) {
      parent.addEventListener(type, event => {
        event.stopImmediatePropagation();
        if (['beforeinput', 'paste', 'cut', 'drop'].includes(type)) event.preventDefault();
        if (type === 'keydown' && event instanceof KeyboardEvent && ['Backspace', 'Delete', 'Enter'].includes(event.key)) event.preventDefault();
        if (type !== 'click' || !(event.target instanceof Element)) return;
        const target = event.target;
        if (target instanceof HTMLInputElement && target.type === 'checkbox') {
          event.preventDefault();
          if (target.matches('li.vditor-task > input:not(:disabled)')) options.onTask?.(target);
          return;
        }
        const reference = target.closest<HTMLElement>('span[data-type="link-ref"]');
        const href = reference ? referenceHref(element, reference, instance.vditor.lute) : target.closest('a')?.getAttribute('href');
        if (href) { event.preventDefault(); openLink(href); return; }
        const footnote = target.closest<HTMLElement>('sup[data-type="footnotes-ref"]');
        if (footnote) {
          [...element.querySelectorAll<HTMLElement>('[data-type="footnotes-li"]')].find(node => node.dataset.marker === footnote.dataset.footnotesLabel)?.scrollIntoView({ block: 'center' });
        }
      }, true);
    }
  }
  element.addEventListener('click', event => {
    const anchor = event.target instanceof Element ? event.target.closest('a') : null;
    const anchorHref = anchor?.getAttribute('href');
    if (anchorHref && assetNoteId && options.openAsset && isLocalAsset(originalAssetPath(element, anchorHref))) { event.preventDefault(); event.stopImmediatePropagation(); openLink(anchorHref); return; }
    const footnote = event.target instanceof Element ? event.target.closest<HTMLElement>('sup[data-type="footnotes-ref"]') : null;
    if (footnote && !footnote.contains(getSelection()?.anchorNode ?? null)) selectNode(element, footnote);
    if (!event.metaKey && !event.ctrlKey) return;
    const reference = event.target instanceof Element ? event.target.closest<HTMLElement>('span[data-type="link-ref"]') : null;
    if (!reference) return;
    const href = referenceHref(element, reference, instance.vditor.lute);
    event.preventDefault();
    event.stopImmediatePropagation();
    if (href) openLink(href);
  }, true);
  const renderCode = await createRichCodeRenderer(element);
  popover.addEventListener('pointerdown', event => {
    if (event.target instanceof Element && event.target.closest('button')) event.preventDefault();
  });
  // WebKit blurs the editor before toolbar clicks; hide only for clicks outside the table and toolbar.
  document.addEventListener('pointerdown', event => {
    const target = event.target;
    if (target instanceof Element && !popover.contains(target) && !target.closest('[data-type="link-ref-defs-block"]')) delete element.dataset.showReferenceDefinitions;
    if (popover.dataset.blockType === 'table' && target instanceof Element &&
        !popover.contains(target) && !element.contains(target.closest('table'))) {
      popover.style.display = 'none';
    }
  });
  const updateTheme = () => instance.setTheme(isDark() ? 'dark' : 'classic');
  window.addEventListener('preferences-applied', updateTheme);
  updateTheme();
  instance.vditor.wysiwyg!.element.setAttribute('aria-label', options.readOnly ? 'Markdown 预览' : '所见即所得编辑区');
  instance.vditor.wysiwyg!.element.setAttribute('spellcheck', 'false');
  return {
    element,
    lute: instance.vditor.lute,
    get composing() { return instance.vditor.wysiwyg!.composingLock; },
    activate(source: string, documentId: string, attachmentNoteId?: string, attachmentBase?: string) {
      assetNoteId = attachmentNoteId;
      assetBase = attachmentBase;
      // Identical text can belong to different notes with different undo histories.
      if (source !== lastSource || documentId !== lastDocumentId) {
        insertionRange = undefined;
        lastFootnoteReference = undefined;
        delete element.dataset.showReferenceDefinitions;
        const { body } = markdownBody(source);
        frontmatter = source.slice(0, source.length - body.length);
        // Reuse display panels so unrelated source edits do not redraw unchanged diagrams.
        const panels = new Map<string, Element[]>();
        const codeKey = (block: Element) => {
          const code = block.querySelector('.vditor-wysiwyg__pre > code');
          return `${code?.className}:${code?.textContent}`;
        };
        if (options.readOnly) for (const block of element.querySelectorAll('[data-type="code-block"]')) {
          const panel = block.querySelector('.vditor-wysiwyg__preview');
          if (panel) { const key = codeKey(block), list = panels.get(key) ?? []; list.push(panel); panels.set(key, list); }
        }
        instance.setValue(body, true);
        if (options.readOnly) for (const block of element.querySelectorAll('[data-type="code-block"]')) {
          const panel = panels.get(codeKey(block))?.shift();
          if (panel) block.querySelector('.vditor-wysiwyg__preview')?.replaceWith(panel);
        }
        if (options.readOnly) element.querySelectorAll('[contenteditable]').forEach(node => node.setAttribute('contenteditable', 'false'));
        lastSource = source;
        lastDocumentId = documentId;
        // Opening a document must not save Vditor's normalized Markdown until the user edits.
        renderedSource = options.readOnly ? '' : markdownValue();
      }
      alignEmphasis();
      projectServerAssets(element, assetNoteId, assetBase);
      renderCode();
      active = true;
    },
    deactivate() { flush(); active = false; },
    flush,
    focus() { instance.focus(); },
    captureSelection() {
      const selection = getSelection();
      if (selection?.rangeCount && element.contains(selection.getRangeAt(0).commonAncestorContainer)) insertionRange = selection.getRangeAt(0).cloneRange();
    },
    insert(markdown: string) {
      instance.focus();
      if (insertionRange && element.contains(insertionRange.commonAncestorContainer)) { const selection = getSelection(); selection?.removeAllRanges(); selection?.addRange(insertionRange); }
      instance.insertValue(markdown);
      const selection = getSelection();
      insertionRange = selection?.rangeCount ? selection.getRangeAt(0).cloneRange() : undefined;
      flush();
    },
    format(action: Format) {
      const command = { heading: 'headings', task: 'check', code: 'inline-code' }[action as 'heading' | 'task' | 'code'] ?? action;
      const item = instance.vditor.toolbar!.elements![command];
      const button = action === 'heading' ? item.querySelector<HTMLElement>('[data-tag="h2"]') : item.querySelector<HTMLElement>('button');
      // Vditor 4.0.1 binds toolbar commands to touchstart on iPhone, click elsewhere.
      button?.dispatchEvent(new Event(navigator.userAgent.includes('iPhone') ? 'touchstart' : 'click', { bubbles: true, cancelable: true }));
      flush();
    },
  };
}

export type RichEditor = Awaited<ReturnType<typeof createRichEditor>>;
