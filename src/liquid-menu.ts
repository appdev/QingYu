// Liquid Glass 菜单的实现，只给 popup-menu.ts 用。其他地方请调用 popup-menu 的 showMenu，尺寸和平台都由那边和这里处理。
import './vendor/liquid-glass/base.css';
import './vendor/liquid-glass/menu.css';
import { clamp, createSpring, springFrom } from './vendor/liquid-glass/spring';
import { haptic } from './vendor/liquid-glass/haptics';
import { attachPress } from './vendor/liquid-glass/press';
import type { MenuItem } from './popup-menu';
import { formatLabel } from './ui-language';

// 和上游 GlassMenu 一致的手感。
const OPEN = springFrom(0.36, 0.74);
/** Underdamped, so the droplet lands in the button with a slight jiggle. */
const CLOSE = springFrom(0.42, 0.66);
const FADE = springFrom(0.24, 1);
const FADE_OUT = springFrom(0.2, 1);
// 桌面档菜单小、用鼠标，换一组更干脆的弹簧：几乎不回弹，大约 0.2 秒落定。
const DESKTOP_OPEN = springFrom(0.24, 0.92);
const DESKTOP_FADE = springFrom(0.16, 1);
// 桌面档收起时直接淡出，不再缩回按钮。
const DESKTOP_CLOSE_MS = 150;
// 只有 Android WebView 需要每帧重算背景模糊，其他平台省下这份开销（虚拟机里尤其明显）。
const android = /Android/i.test(globalThis.navigator?.userAgent ?? '');
// 最近一次按下的位置。按钮菜单就是这一下点开的，鼠标离开这里才算真动了。
let lastPress: { x: number; y: number } | undefined;
globalThis.document?.addEventListener('pointerdown', event => { lastPress = { x: event.clientX, y: event.clientY }; }, true);

const reducedMotion = () => {
  try { return matchMedia('(prefers-reduced-motion: reduce)').matches; } catch { return false; }
};

/**
 * 精简模式：弱设备上展开时不做背景模糊、条目模糊和水滴变形，只动缩放和透明度，交给合成线程，
 * 主线程卡的时候动画照样顺；落定后再换上半透明的玻璃。4 核以下或内存 4GB 以下直接用，
 * 其他设备展开时掉帧多了也会记下来，以后都用。
 */
const LITE_KEY = 'qingyu.menu.lite';
function liteMode() {
  const nav = globalThis.navigator as (Navigator & { deviceMemory?: number }) | undefined;
  if ((nav?.hardwareConcurrency || 8) <= 4 || (nav?.deviceMemory || 8) <= 4) return true;
  try { return localStorage.getItem(LITE_KEY) === '1'; } catch { return false; }
}
function rememberLite() {
  try { localStorage.setItem(LITE_KEY, '1'); } catch { /* 存不下就下次打开再测 */ }
}

/** 把弹簧（从 0 弹到 1，初速为 0）预先算成 CSS 的 linear() 缓动，Web Animations 在合成线程上照着走。 */
function springEasing(response: number, dampingFraction: number) {
  const w = (2 * Math.PI) / response;
  const decay = dampingFraction * w;
  const wd = w * Math.sqrt(1 - dampingFraction * dampingFraction);
  // 振幅衰减到 0.2% 就算落定。
  const seconds = Math.log(500) / decay;
  const points: string[] = [];
  for (let i = 0; i <= 40; i++) {
    const t = (seconds * i) / 40;
    const x = 1 - Math.exp(-decay * t) * (Math.cos(wd * t) + (decay / wd) * Math.sin(wd * t));
    points.push(x.toFixed(4));
  }
  return { duration: seconds * 1000, easing: `linear(${points.join(', ')})` };
}

/** 触感只是点缀：所在环境不支持时（没有 matchMedia 等）安静跳过，不能挡住选择。 */
function buzz() {
  try { haptic(); } catch { /* 没有触感也照常工作 */ }
}

type Metrics = { desktop?: boolean; width: number; maxWidth: number; radius: number; padX: number; padY: number; vars?: Record<string, string> };

/**
 * 菜单尺寸，按视口分三档，和页面其余控件同一个比例：
 * - 手机（小于 780px）：按 iOS 实测的菜单，以 402px 宽为基准、缩放比限制在 0.8–1：
 *   宽 250、圆角 25、文字 17、图标 22、内容内边距 8 / 12、单行高 42（文字行高 20）、分组间隔 21。
 * - 平板（780–900px）：宽 236、圆角 25、文字 16、图标 18、内边距 8 / 12、单行高 38、分组间隔 17。
 * - 桌面（901px 起）：宽 218、圆角 12、文字 13、图标 14、内边距 6 / 8、单行高 24、分组间隔 11，
 *   跟 28px 的紧凑按钮和 13px 的列表文字放在一起不显得大一号。
 */
function menuMetrics(): Metrics {
  // 分组间隔从上一行底边量到下一行顶边，包含 0.5px 的分隔线。
  const sep = (gap: number, side: number) => `${(gap - 0.5) / 2}px ${side}px`;
  if (innerWidth >= 901) {
    return {
      desktop: !document.documentElement.hasAttribute('data-mobile'),
      width: 218, maxWidth: innerWidth - 24, radius: 12, padX: 8, padY: 6,
      vars: {
        '--qlg-menu-pad': '6px 8px',
        '--qlg-menu-row': '24px', '--qlg-menu-line': '20px', '--qlg-menu-font': '13px', '--qlg-menu-icon': '14px',
        '--qlg-menu-row-pad': '8px', '--qlg-menu-gap': '8px', '--qlg-menu-row-radius': '6px',
        '--qlg-menu-sep': sep(11, 8), '--qlg-menu-shortcut': '12px', '--qlg-menu-mark': '12px',
      },
    };
  }
  if (innerWidth >= 780) {
    return {
      width: 236, maxWidth: 236, radius: 25, padX: 12, padY: 8,
      vars: {
        '--qlg-menu-pad': '8px 12px',
        '--qlg-menu-row': '38px', '--qlg-menu-line': '20px', '--qlg-menu-font': '16px', '--qlg-menu-icon': '18px',
        '--qlg-menu-row-pad': '12px', '--qlg-menu-gap': '12px', '--qlg-menu-row-radius': '12px',
        '--qlg-menu-sep': sep(17, 10),
      },
    };
  }
  const k = Math.min(1, Math.max(0.8, innerWidth / 402));
  const px = (value: number) => `${value * k}px`;
  return {
    width: 250 * k, maxWidth: Math.min(250 * k, innerWidth - 16), radius: 25 * k, padX: 12 * k, padY: 8 * k,
    vars: {
      '--qlg-menu-pad': `${px(8)} ${px(12)}`,
      '--qlg-menu-row': px(42), '--qlg-menu-line': px(20), '--qlg-menu-font': px(17), '--qlg-menu-icon': px(22),
      '--qlg-menu-row-pad': px(14), '--qlg-menu-gap': px(14), '--qlg-menu-row-radius': px(14),
      '--qlg-menu-sep': sep(21 * k, 10 * k),
    },
  };
}

/** 调用方统一写 macOS 的符号（⇧⌘P），Windows 和 Linux 上换成 Ctrl+Shift+P 这种写法。 */
export function shortcutText(shortcut: string) {
  if (/Mac|iPhone|iPad/u.test(globalThis.navigator?.platform ?? '')) return shortcut;
  const names: Record<string, string> = { '⌃': 'Ctrl', '⌘': 'Ctrl', '⇧': 'Shift', '⌥': 'Alt' };
  const order = ['Ctrl', 'Shift', 'Alt'];
  const mods: string[] = [];
  let rest = shortcut;
  while (rest && names[rest[0]]) { if (!mods.includes(names[rest[0]])) mods.push(names[rest[0]]); rest = rest.slice(1); }
  const key = ({ '⌫': 'Backspace', '⌦': 'Delete', '↩': 'Enter', '⎋': 'Esc' } as Record<string, string>)[rest] ?? rest;
  return [...mods.sort((a, b) => order.indexOf(a) - order.indexOf(b)), key].join('+');
}

export type LiquidMenuRequest = {
  items: MenuItem[];
  label: string;
  /** 从这个按钮长出来；不传时从 at 指定的点长出来（右键菜单）。 */
  anchor?: HTMLElement;
  at?: { x: number; y: number };
  /** 键盘打开时焦点落在首个可用项，鼠标或触摸打开时落在菜单本身。 */
  keyboard: boolean;
  iconFor: (item: MenuItem) => Node | undefined;
  /** 选中一项：由调用方关闭菜单、恢复焦点并执行。 */
  select: (item: MenuItem) => void;
  /** 没选就关掉了（点外面、Esc）。 */
  dismiss: () => void;
};

const svg = (path: string, size = 16, stroke = 1.8) => {
  const node = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  node.setAttribute('viewBox', `0 0 ${size} ${size}`);
  // 和上游的 Tick 一样写明尺寸，否则在弹性布局里会被压成 0。
  node.setAttribute('width', String(size));
  node.setAttribute('height', String(size));
  node.setAttribute('fill', 'none');
  node.setAttribute('aria-hidden', 'true');
  const line = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  line.setAttribute('d', path);
  line.setAttribute('stroke', 'currentColor');
  line.setAttribute('stroke-width', String(stroke));
  line.setAttribute('stroke-linecap', 'round');
  line.setAttribute('stroke-linejoin', 'round');
  node.append(line);
  return node;
};
const tick = () => svg('M3.5 8.5l3 3 6-7');
const chevron = (down = false) => svg(down ? 'M4 6l4 4 4-4' : 'M6 4l4 4-4 4', 16, 2);

type Rect = { left: number; top: number; width: number; height: number };

type Level = {
  surface: HTMLElement;
  content: HTMLElement;
  buttons: HTMLButtonElement[];
  /** 子菜单从哪一行长出来；根菜单为空。 */
  row?: HTMLButtonElement;
  child?: Level;
  parent?: Level;
  closed: boolean;
  set(target: number): void;
};

export function showLiquidMenu(request: LiquidMenuRequest) {
  const levels: Level[] = [];
  let finished = false;
  // 桌面档打开菜单时鼠标停的位置；从这里挪开几像素才算鼠标真动了。
  let pointerAt = lastPress;
  // close() 之后各层还在播收起动画，这段时间里的指针、键盘和点击都不再处理。
  let dismissed = false;
  const cleanups: (() => void)[] = [];
  const desktop = innerWidth >= 901;
  const metrics = menuMetrics();
  const lite = liteMode();

  function finish() {
    if (finished || levels.some(level => !level.closed)) return;
    finished = true;
    for (const cleanup of cleanups) cleanup();
  }

  function buildItems(entries: MenuItem[], content: HTMLElement, header?: MenuItem) {
    const buttons: HTMLButtonElement[] = [];
    const add = (entry: MenuItem, index: number, headerRow = false) => {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'qlg-menu__item';
      button.role = entry.checked === undefined || headerRow ? 'menuitem' : 'menuitemradio';
      if (entry.checked !== undefined && !headerRow) button.setAttribute('aria-checked', String(entry.checked));
      if (entry.destructive) button.dataset.destructive = '';
      button.dataset.index = String(index);
      // 和原来的网页菜单一样，用菜单文字作可访问名称；子菜单的标题行用来收起，叫「收起××」。
      button.ariaLabel = headerRow ? formatLabel('收起{action}', { action: entry.text }) : entry.text;
      button.disabled = entry.enabled === false;
      const icon = headerRow ? undefined : request.iconFor(entry);
      if (icon) {
        const slot = document.createElement('span');
        slot.className = 'qlg-menu__icon';
        slot.append(icon.cloneNode(true));
        button.append(slot);
      }
      const label = document.createElement('span');
      label.className = 'qlg-menu__label';
      label.textContent = entry.text;
      button.append(label);
      if (headerRow) {
        button.dataset.header = '';
        const mark = document.createElement('span');
        mark.className = 'qlg-menu__chevron';
        mark.append(chevron(true));
        button.append(mark);
      } else if (entry.items) {
        button.setAttribute('aria-haspopup', 'menu');
        button.setAttribute('aria-expanded', 'false');
        const mark = document.createElement('span');
        mark.className = 'qlg-menu__chevron';
        mark.append(chevron());
        button.append(mark);
      } else {
        if (desktop && entry.shortcut) {
          const key = document.createElement('kbd');
          key.className = 'qlg-menu__shortcut';
          key.textContent = shortcutText(entry.shortcut);
          button.append(key);
        }
        if (entry.checked) {
          const mark = document.createElement('span');
          mark.className = 'qlg-menu__check';
          mark.append(tick());
          button.append(mark);
        }
      }
      content.append(button);
      buttons.push(button);
      return button;
    };
    if (header) {
      add(header, -1, true);
      const line = document.createElement('hr');
      line.role = 'separator';
      line.className = 'qlg-menu__separator';
      content.append(line);
    }
    entries.forEach((entry, index) => {
      if (entry.text) { add(entry, index); return; }
      // 分隔线：首尾和连续的不画。
      if (!content.lastElementChild || content.lastElementChild.role === 'separator' || index === entries.length - 1) return;
      const line = document.createElement('hr');
      line.role = 'separator';
      line.className = 'qlg-menu__separator';
      content.append(line);
    });
    return buttons;
  }

  /**
   * 上游 GlassMenu 的开合：玻璃保持最终尺寸，用 transform 从触发点的角落长出来；
   * 飞行时是圆水滴，落地才变成菜单圆角；菜单项从模糊里浮现。
   */
  function openLevel(entries: MenuItem[], from: Rect, trigger: HTMLElement | null, viaKeyboard: boolean, parent?: Level, row?: HTMLButtonElement, header?: MenuItem): Level {
    const surface = document.createElement('div');
    surface.className = 'qlg-menu-surface';
    const panel = document.createElement('div');
    panel.className = 'qlg-glass qlg-menu';
    panel.ariaHidden = 'true';
    const content = document.createElement('div');
    content.className = 'qlg-menu__content';
    content.role = 'menu';
    content.ariaLabel = header?.text ?? request.label;
    content.tabIndex = -1;
    surface.append(panel, content);
    for (const [name, value] of Object.entries(metrics.vars ?? {})) surface.style.setProperty(name, value);
    // 桌面档按 macOS 菜单的样子，指到的行铺满主题高亮色。
    // 菜单盖住按钮时鼠标正停在某一行上，等鼠标真动了再给悬停高亮。
    if (metrics.desktop) { surface.dataset.desktop = ''; surface.dataset.idle = ''; }
    if (lite) surface.dataset.lite = '';
    const buttons = buildItems(entries, content, header);
    document.body.append(surface);

    // 宽度取档位宽度；Windows 的快捷键文字更长，放不下时按内容撑开，但不超出视口。
    // 宽高一次量完，只触发一次布局：每行文字都不换行，宽度定下来高度也不会变。
    const natural = content.offsetWidth;
    const naturalHeight = content.offsetHeight;
    const W = Math.max(1, Math.min(Math.max(metrics.width, natural), metrics.maxWidth));
    const w = Math.min(W, Math.max(0, from.width));
    content.style.width = `${W}px`;
    const room = Math.max(1, innerHeight - 16);
    const scrollable = naturalHeight > room;
    if (scrollable) {
      content.style.maxHeight = `${room}px`;
      surface.dataset.scrollable = '';
    }
    const H = Math.max(1, scrollable ? room : naturalHeight);
    // 趁刚量完尺寸、样式还没被改动时放焦点，浏览器不用为它再算一遍样式。
    // 键盘打开时落在首个可用项，鼠标或触摸打开时落在菜单本身。
    const first = buttons.find(button => !button.disabled && button.dataset.header === undefined);
    (viaKeyboard && first ? first : content).focus({ preventScroll: true });
    const h = Math.min(H, Math.max(0, from.height));
    surface.style.width = `${W}px`;
    surface.style.height = `${H}px`;
    // 按钮在右半边时从它的右上角往左下长，在左半边时从左上角往右下长。
    const end = parent ? parent.surface.dataset.align !== 'start' : from.left + from.width / 2 > innerWidth / 2;
    surface.dataset.align = end ? 'end' : 'start';
    const left = end ? from.left + from.width - W : from.left;
    surface.style.left = `${clamp(left, 8, Math.max(8, innerWidth - W - 8))}px`;
    surface.style.top = `${clamp(from.top, 8, Math.max(8, innerHeight - H - 8))}px`;

    // The live droplet outline, shared by the glass and the items above it.
    const box = { cw: w, ch: h, r: h / 2 };
    let scaleNow = 1;
    let closing = false;
    const clipContent = () => {
      // Items are scaled about the trigger's corner; clip in their own units.
      const k = scaleNow;
      const right = Math.max(0, W - box.cw / k);
      const bottom = Math.max(0, H - box.ch / k);
      content.style.clipPath = box.cw >= W - 0.5 && box.ch >= H - 0.5 && k === 1
        ? ''
        : `inset(0 ${end ? 0 : right}px ${bottom}px ${end ? right : 0}px round ${box.r / k}px)`;
    };
    const shape = (p: number) => {
      const cw = Math.max(1, w + (W - w) * p);
      const ch = Math.max(1, h + (H - h) * p);
      const sx = cw / W;
      const sy = ch / H;
      panel.style.transform = `scale(${sx}, ${sy})`;
      // In flight it's a round droplet (fully rounded on its short side) and
      // only squares off into the menu's corners as it lands.
      const land = clamp((p - 0.5) / 0.5, 0, 1);
      const r = (Math.min(cw, ch) / 2) * (1 - land) + metrics.radius * land;
      panel.style.borderRadius = `${r / sx}px / ${r / sy}px`;
      // 部分 Android WebView（Chrome 138 实测）改圆角时不更新背景模糊的裁剪，模糊会停在前面某一帧的水滴形状。
      // 每帧把模糊半径在 12px 和 12.001px 之间换一下，让它跟着当前形状重新生成，肉眼看不出差别。
      if (android) panel.style.setProperty('--qlg-glass-blur', panel.style.getPropertyValue('--qlg-glass-blur') ? '' : '12.001px');
      box.cw = cw;
      box.ch = ch;
      box.r = r;
      clipContent();
      // Near the button's size the two shapes coincide, so the button and the
      // menu cross-fade there. Closing, the button starts showing through early.
      const k = clamp(p / (closing ? 0.7 : 0.35), 0, 1);
      panel.style.opacity = String(closing ? Math.min(1, k * 1.4) : k);
      if (trigger) {
        trigger.style.opacity = String(1 - k);
        // Past the button's size the close overshoots: the button squashes and springs back.
        trigger.style.scale = closing && p < -0.0005 ? `${1 - p * 0.6} ${1 + p * 1.2}` : '';
      }
    };
    // The items grow out of a blur as the droplet lands, on a critically damped curve.
    const reveal = (q: number) => {
      const t = clamp(q, 0, 1);
      // 13px 的小字从模糊里浮现会糊成一片，桌面档只淡入。
      const blur = metrics.desktop ? 0 : 9 * (1 - t) ** 2.5;
      content.style.opacity = String(t);
      scaleNow = t < 0.999 ? (metrics.desktop ? 0.94 + 0.06 * t : 0.82 + 0.18 * t) : 1;
      content.style.scale = scaleNow === 1 ? '' : String(scaleNow);
      clipContent();
      content.style.filter = blur > 0.05 ? `blur(${blur.toFixed(2)}px)` : 'none';
    };
    shape(0);
    reveal(0);

    // 按住时整块玻璃微微鼓起、跟着手指倾斜，是触屏的手感；桌面档用鼠标点，不需要。
    const detachPress = attachPress(surface, { grow: 0.035, lean: 6, stretch: 0.1, disabled: scrollable || metrics.desktop });
    const level: Level = { surface, content, buttons, row, parent, closed: false, set: () => {} };
    // 精简模式里按钮跟着菜单一起淡出的动画；收起时先取消，按钮才能用回自己的透明度。
    let triggerFade: Animation | undefined;
    const teardown = () => {
      if (level.closed) return;
      level.closed = true;
      s.stop();
      fade.stop();
      detachPress();
      triggerFade?.cancel();
      if (trigger) { trigger.style.opacity = ''; trigger.style.scale = ''; }
      surface.remove();
      finish();
    };
    const s = createSpring(0, metrics.desktop ? DESKTOP_OPEN : OPEN, (p, v) => {
      shape(p);
      // Unmount only once the landing jiggle has fully settled.
      if (s.target === 0 && p === 0 && v === 0) teardown();
    });
    const fade = createSpring(0, metrics.desktop ? DESKTOP_FADE : FADE, reveal);
    level.set = target => {
      closing = target === 0;
      // A closing menu lets clicks through to the button beneath it.
      surface.style.pointerEvents = closing ? 'none' : '';
      // 收起中的菜单只剩折回的画面：不再可用，读屏和页面里的查找也不再把它和它的条目当成菜单。
      surface.inert = closing;
      if (closing) {
        content.removeAttribute('role');
        for (const button of buttons) { button.removeAttribute('role'); button.removeAttribute('aria-label'); }
      }
      if (!closing && lite) {
        // 精简模式：一开始就是最终形状，整块从按钮的角落放大淡入，按钮同时淡出；落定后换上半透明玻璃。
        shape(1);
        reveal(1);
        const land = () => {
          if (closing || level.closed) return;
          surface.dataset.landed = '';
        };
        if (reducedMotion() || typeof surface.animate !== 'function') { land(); return; }
        surface.style.transformOrigin = end ? 'top right' : 'top left';
        const { duration, easing } = metrics.desktop ? springEasing(0.24, 0.92) : springEasing(0.36, 0.74);
        surface.animate([{ transform: 'scale(0.5)' }, { transform: 'none' }], { duration, easing });
        surface.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 120, easing: 'ease-out' });
        if (trigger) {
          // shape(1) 已经把按钮设成透明，这里从不透明淡到透明，和菜单的淡入对上。
          triggerFade = trigger.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 120, easing: 'ease-out' });
        }
        setTimeout(land, duration);
        return;
      }
      if (closing && (metrics.desktop || lite)) {
        // 桌面档和精简模式：停在当前样子，整块淡出并稍微缩小，按钮同时露出来。
        s.stop();
        fade.stop();
        triggerFade?.cancel();
        if (trigger) { trigger.style.opacity = ''; trigger.style.scale = ''; }
        if (reducedMotion()) { teardown(); return; }
        surface.style.transformOrigin = end ? 'top right' : 'top left';
        surface.style.transition = `opacity ${DESKTOP_CLOSE_MS}ms ease-out, scale ${DESKTOP_CLOSE_MS}ms ease-out`;
        surface.style.opacity = '0';
        surface.style.scale = '0.97';
        setTimeout(teardown, DESKTOP_CLOSE_MS + 20);
        return;
      }
      s.tune(closing ? CLOSE : metrics.desktop ? DESKTOP_OPEN : OPEN);
      fade.tune(target ? (metrics.desktop ? DESKTOP_FADE : FADE) : FADE_OUT);
      fade.set(target);
      s.set(target);
    };
    levels.push(level);
    if (parent) {
      parent.child = level;
      parent.surface.dataset.covered = '';
      row?.setAttribute('aria-expanded', 'true');
    }
    level.set(1);
    return level;
  }

  const top = () => levels.filter(level => !level.closed && level.surface.style.pointerEvents !== 'none').at(-1);

  function back() {
    const level = top();
    if (level?.parent) closeChild(level.parent);
    else request.dismiss();
  }

  function closeChild(level: Level, focusRow = true) {
    const child = level.child;
    if (!child) return;
    closeChild(child, false);
    level.child = undefined;
    delete level.surface.dataset.covered;
    child.row?.setAttribute('aria-expanded', 'false');
    child.set(0);
    if (focusRow) child.row?.focus({ preventScroll: true });
  }

  function openChild(level: Level, button: HTMLButtonElement, entry: MenuItem, viaKeyboard: boolean) {
    if (!entry.items || button.disabled) return;
    closeChild(level, false);
    const box = button.getBoundingClientRect();
    // 子菜单的标题行落在被点的那一行上，玻璃从这一行长出来盖住父菜单。
    const child = openLevel(entry.items, {
      left: box.left - metrics.padX, top: box.top - metrics.padY,
      width: box.width + metrics.padX * 2, height: box.height + metrics.padY * 2,
    }, button, viaKeyboard, level, button, entry);
  }

  const entryOf = (level: Level, button: HTMLButtonElement) => {
    if (button.dataset.header !== undefined) return undefined;
    const list = level.parent ? findItems(level) : request.items;
    return list[Number(button.dataset.index)];
  };
  // 子菜单的条目就是父行对应条目的 items。
  function findItems(level: Level): MenuItem[] {
    if (!level.parent || !level.row) return request.items;
    const parentItems = findItems(level.parent);
    return parentItems[Number(level.row.dataset.index)]?.items ?? [];
  }

  function activate(level: Level, button: HTMLButtonElement, viaKeyboard: boolean) {
    if (button.disabled) return;
    if (button.dataset.header !== undefined) { if (level.parent) closeChild(level.parent); return; }
    const entry = entryOf(level, button);
    if (!entry) return;
    if (entry.items) { openChild(level, button, entry, viaKeyboard); return; }
    buzz();
    request.select(entry);
  }

  // ---- 根菜单 ----
  const anchor = request.anchor;
  const from: Rect = anchor
    ? (() => { const box = anchor.getBoundingClientRect(); return { left: box.left, top: box.top, width: box.width, height: box.height }; })()
    : { left: request.at?.x ?? 0, top: request.at?.y ?? 0, width: 0, height: 0 };
  openLevel(request.items, from, anchor ?? null, request.keyboard);
  if (!lite && !reducedMotion()) watchFrames();

  /** 展开的前 400ms 里，去掉建菜单的头两帧，还有 3 帧以上超过 34ms（60Hz 下丢了一帧多），以后就用精简模式。 */
  function watchFrames() {
    if (typeof requestAnimationFrame !== 'function' || document.visibilityState === 'hidden') return;
    const gaps: number[] = [];
    let start = 0;
    let last = 0;
    const tick = (time: number) => {
      if (last) gaps.push(time - last);
      else start = time;
      last = time;
      if (time - start < 400 && !dismissed) { requestAnimationFrame(tick); return; }
      if (gaps.slice(2).filter(gap => gap > 34).length >= 3) rememberLite();
    };
    requestAnimationFrame(tick);
  }

  // Slide a finger across the items: the one beneath it lights up, and letting go there picks it, as on iOS.
  let held: number | null = null;
  let hot: HTMLButtonElement | null = null;
  const itemAt = (x: number, y: number) =>
    (document.elementFromPoint(x, y) as HTMLElement | null)?.closest<HTMLButtonElement>('.qlg-menu__item:not(:disabled)') ?? null;
  const light = (next: HTMLButtonElement | null) => {
    if (next === hot) return;
    hot?.removeAttribute('data-active');
    hot = next;
    if (hot) {
      hot.setAttribute('data-active', '');
      buzz();
    }
  };
  const levelOf = (node: Node | null) => levels.find(level => !level.closed && level.surface.contains(node));
  const down = (event: PointerEvent) => {
    if (dismissed) return;
    const level = levelOf(event.target as Node);
    if (!level) {
      // 点在按钮上交给按钮自己切换开合，点在别处关掉菜单。
      if (anchor?.contains(event.target as Node)) return;
      request.dismiss();
      return;
    }
    // 子菜单盖着父菜单时，点父菜单只是收回子菜单。
    if (level.child) {
      event.preventDefault();
      event.stopPropagation();
      closeChild(level);
      return;
    }
    if (!event.isPrimary || event.button !== 0) return;
    held = event.pointerId;
    hot = null;
    light(itemAt(event.clientX, event.clientY));
  };
  const move = (event: PointerEvent) => {
    if (event.pointerType !== 'touch' && levels.some(level => level.surface.hasAttribute('data-idle'))) {
      // 菜单在静止的鼠标下面长出来时浏览器也可能补发 pointermove，所以按挪动的距离判断。
      if (!pointerAt) pointerAt = { x: event.clientX, y: event.clientY };
      else if (Math.hypot(event.clientX - pointerAt.x, event.clientY - pointerAt.y) > 2) {
        for (const level of levels) delete level.surface.dataset.idle;
      }
    }
    if (event.pointerId === held) light(itemAt(event.clientX, event.clientY));
  };
  const up = (event: PointerEvent) => {
    if (dismissed || event.pointerId !== held) return;
    held = null;
    const target = hot;
    light(null);
    const level = target && levelOf(target);
    if (target && level && level === top()) {
      event.preventDefault();
      activate(level, target, false);
    }
  };
  const cancel = (event: PointerEvent) => {
    if (event.pointerId !== held) return;
    held = null;
    light(null);
  };
  // Touch drags must not scroll the page while scrubbing the menu.
  const hold = (event: TouchEvent) => {
    if (held !== null && event.cancelable && !levelOf(event.target as Node)?.surface.hasAttribute('data-scrollable')) event.preventDefault();
  };

  let typed = '';
  let typedAt = 0;
  const key = (event: KeyboardEvent) => {
    const level = top();
    if (dismissed || !level || event.isComposing) return;
    const enabled = level.buttons.filter(button => !button.disabled);
    const index = enabled.indexOf(document.activeElement as HTMLButtonElement);
    const focus = (button?: HTMLButtonElement) => {
      if (!button) return;
      event.preventDefault();
      button.focus({ preventScroll: true });
      if (level.surface.hasAttribute('data-scrollable')) button.scrollIntoView({ block: 'nearest' });
    };
    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        event.stopPropagation();
        back();
        return;
      case 'ArrowDown': focus(enabled[index < 0 ? 0 : (index + 1) % enabled.length]); return;
      case 'ArrowUp': focus(enabled[index < 0 ? enabled.length - 1 : (index - 1 + enabled.length) % enabled.length]); return;
      case 'Home': focus(enabled[0]); return;
      case 'End': focus(enabled.at(-1)); return;
      case 'Tab': event.preventDefault(); return;
      case 'ArrowLeft':
        if (level.parent) { event.preventDefault(); closeChild(level.parent); }
        return;
      case 'ArrowRight': {
        const button = enabled[index];
        const entry = button && entryOf(level, button);
        if (button && entry?.items) { event.preventDefault(); openChild(level, button, entry, true); }
        return;
      }
      case 'Enter':
      case ' ': {
        const button = enabled[index];
        if (button) { event.preventDefault(); activate(level, button, true); }
        return;
      }
    }
    // 首字母跳转：连续输入时按整串匹配。
    if (event.key.length === 1 && !event.metaKey && !event.ctrlKey && !event.altKey) {
      const now = performance.now();
      typed = now - typedAt > 700 ? event.key : typed + event.key;
      typedAt = now;
      const find = (text: string) => {
        const start = index < 0 ? 0 : index + (text.length === 1 ? 1 : 0);
        return [...enabled.slice(start), ...enabled.slice(0, start)]
          .find(button => button.textContent?.trim().toLowerCase().startsWith(text.toLowerCase()));
      };
      // 连着输入的串匹配不上时，改用刚输入的这一个字重新找。
      let match = find(typed);
      if (!match && typed.length > 1) { typed = event.key; match = find(typed); }
      focus(match);
    }
  };
  // 键盘或读屏的点击（detail 为 0）走这里，指针选择在抬起时处理。
  const click = (event: MouseEvent) => {
    if (dismissed || event.detail !== 0) return;
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('.qlg-menu__item');
    const level = button && levelOf(button);
    if (button && level) activate(level, button, true);
  };

  document.addEventListener('pointerdown', down, true);
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', up);
  window.addEventListener('pointercancel', cancel);
  window.addEventListener('keydown', key, true);
  document.addEventListener('click', click, true);
  document.addEventListener('touchmove', hold, { passive: false });
  cleanups.push(() => {
    document.removeEventListener('pointerdown', down, true);
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    window.removeEventListener('pointercancel', cancel);
    window.removeEventListener('keydown', key, true);
    document.removeEventListener('click', click, true);
    document.removeEventListener('touchmove', hold);
  });

  return {
    back,
    /** 所有层一起折回各自的触发点，落定后移除。 */
    close() {
      dismissed = true;
      for (const level of [...levels].reverse()) if (!level.closed) level.set(0);
    },
  };
}
