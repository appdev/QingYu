// @vitest-environment jsdom
import { expect, it, vi } from 'vitest';
import { installImagePreviewActions } from '../src/rich-editor';

vi.hoisted(() => {
  Object.assign(globalThis, { matchMedia: () => ({ matches: false, addEventListener() {} }) });
});

function fixture(install = true) {
  const doc = document.implementation.createHTMLDocument();
  doc.body.innerHTML = '<div class="vditor-img"><div><span class="vditor-img__btn" data-deg="0">旋转</span><span class="vditor-img__btn" onclick="this.parentElement.parentElement.remove()">关闭</span></div><div class="vditor-img__img" onclick="this.parentElement.remove()"><img></div></div>';
  doc.body.style.overflow = 'hidden';
  if (install) installImagePreviewActions(doc);
  return doc;
}

it('closes the SDK image viewer even when inline handlers cannot execute', () => {
  const blocked = fixture(false);
  blocked.querySelector<HTMLElement>('.vditor-img__btn:not([data-deg])')!.click();
  expect(blocked.querySelector('.vditor-img')).not.toBeNull();
  const doc = fixture();
  doc.querySelector<HTMLElement>('.vditor-img__btn:not([data-deg])')!.click();
  expect(doc.querySelector('.vditor-img')).toBeNull(); expect(doc.body.style.overflow).toBe('');
});

it('retains rotation and closes from the backdrop or Escape', () => {
  const doc = fixture();
  doc.querySelector<HTMLElement>('[data-deg]')!.click();
  expect(doc.querySelector('.vditor-img')).not.toBeNull();
  doc.querySelector<HTMLElement>('.vditor-img__img')!.click();
  expect(doc.querySelector('.vditor-img')).toBeNull();
  const keyboard = fixture();
  keyboard.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  expect(keyboard.querySelector('.vditor-img')).toBeNull(); expect(keyboard.body.style.overflow).toBe('');
});
