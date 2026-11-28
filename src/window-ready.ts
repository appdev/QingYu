import { invoke } from '@tauri-apps/api/core';

// 桌面上的窗口创建时都是隐藏的，排好版后再显示，避免先露出没套样式或没加载完的页面。
// 隐藏的窗口里 requestAnimationFrame 不触发，所以先读一次布局就通知；macOS 上这时窗口还是透明的，
// 等它画好两帧再通知一次，才真正可见。
export function showWindowWhenPainted() {
  void document.body.offsetHeight;
  void invoke('workspace_window_ready')
    .then(() => requestAnimationFrame(() => requestAnimationFrame(() => void invoke('workspace_window_ready'))))
    .catch(() => {});
}

export function showAndroidWindowWhenReady() {
  const splash = window.nativeSplash;
  if (!splash) return;
  const root = document.documentElement;
  const pendingInsets = 'data-android-inset-pending';
  if (!root.hasAttribute(pendingInsets)) {
    splash.postMessage('ready');
    return;
  }
  const observer = new MutationObserver(() => {
    if (root.hasAttribute(pendingInsets)) return;
    observer.disconnect();
    splash.postMessage('ready');
  });
  observer.observe(root, { attributes: true, attributeFilter: [pendingInsets] });
}
