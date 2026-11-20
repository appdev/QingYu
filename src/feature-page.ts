import { bindLabels, translatedLabel } from './ui-language';
import './style.css';
import { mountFeaturePanel, mountFeatureWindow } from './feature-panel';
import { mountNotePicture } from './note-picture';
import { loadPictureWindow } from './picture-window';
import { native } from './preferences';

const root = document.querySelector<HTMLElement>('#feature-root')!;
const localizeLabels = bindLabels(document);
localizeLabels();
window.addEventListener('ui-language-change', localizeLabels);
const owner = new URLSearchParams(location.search).get('owner');
if (native && owner) {
  document.documentElement.dataset.platform = /Mac/.test(navigator.platform) ? 'macos' : 'other';
  await mountFeatureWindow(root, close => mountFeaturePanel(root, translatedLabel('分享为图片'), close, content => {
    let dispose: (() => void) | undefined;
    void loadPictureWindow(owner).then(picture => {
      if (content.isConnected) dispose = mountNotePicture(content, picture, true);
    }).catch(() => { content.textContent = translatedLabel('无法打开图片预览，请返回编辑器重试。'); });
    return () => dispose?.();
  }));
}
