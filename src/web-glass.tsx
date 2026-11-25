import { useEffect, useState, type CSSProperties } from 'react';
import { createPortal } from 'react-dom';
import { createRoot } from 'react-dom/client';
import { Glass, Slider } from 'glisse';

export type WebGlassProps = { tint: number; blur: number };

export function WebGlass({ tint, blur }: WebGlassProps) {
  const [disabled, setDisabled] = useState(true);
  useEffect(() => {
    if (!window.CanvasRenderingContext2D || !window.ResizeObserver || !window.matchMedia) return;
    const preference = window.matchMedia('(prefers-reduced-transparency: reduce)');
    const update = () => setDisabled(preference.matches);
    update();
    preference.addEventListener('change', update);
    return () => preference.removeEventListener('change', update);
  }, []);
  if (disabled) return null;
  return <Glass aria-hidden="true" className="web-glisse" style={{
    '--lg-glass': `color-mix(in srgb, var(--web-glass-fill) ${tint}%, transparent)`,
    '--lg-blur': `${blur}px`,
  } as CSSProperties} />;
}

export function renderWebGlass(host: HTMLElement, props: WebGlassProps) {
  const root = createRoot(host.ownerDocument.createElement('div'));
  root.render(createPortal(<WebGlass {...props} />, host));
  return () => root.unmount();
}

export function renderGlisseSlider(host: HTMLElement, input: HTMLInputElement) {
  const root = createRoot(host);
  const id = input.id;
  input.removeAttribute('id');
  input.hidden = true;
  function update() {
    root.render(<Slider id={id} min={Number(input.min)} max={Number(input.max)} step={Number(input.step)}
      value={input.valueAsNumber} aria-describedby={input.getAttribute('aria-describedby') ?? undefined}
      aria-valuetext={input.getAttribute('aria-valuetext') ?? undefined}
      onValueChange={value => {
        input.valueAsNumber = value;
        input.dispatchEvent(new Event('input', { bubbles: true }));
      }} />);
  }
  update();
  return { update, destroy() { root.unmount(); input.id = id; input.hidden = false; } };
}
