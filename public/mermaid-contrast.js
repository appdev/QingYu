(() => {
  function luminance(color) {
    const channels = color.match(/^rgba?\(([^)]+)\)$/)?.[1].split(/[,\s/]+/).map(Number);
    // shortcut: Opaque RGB fills cover these diagrams; composite translucent fills if they are introduced.
    if (!channels || channels.length < 3 || (channels.length > 3 && channels[3] !== 1)) return null;
    return channels.slice(0, 3).map(value => {
      const srgb = value / 255;
      return srgb <= .04045 ? srgb / 12.92 : ((srgb + .055) / 1.055) ** 2.4;
    }).reduce((sum, value, index) => sum + value * [.2126, .7152, .0722][index], 0);
  }
  const contrast = (a, b) => (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
  window.adjustMermaidContrast = (root, dark) => {
    if (!dark) return;
    root.querySelectorAll('.language-mermaid svg :is(.node, .cluster)').forEach(node => {
      const shape = node.querySelector('rect, polygon, circle, ellipse, path');
      const label = node.querySelector('.nodeLabel, .cluster-label');
      if (!shape || !label) return;
      const style = getComputedStyle(shape);
      const background = luminance(style.fill);
      if (background === null || Number(style.fillOpacity) < 1 || Number(style.opacity) < 1) return;
      const color = contrast(background, 0) >= contrast(background, 1) ? '#000000' : '#ffffff';
      const text = luminance(getComputedStyle(label.querySelector('p, text') || label).color);
      if (text !== null && contrast(background, text) < 4.5) {
        [label, ...label.querySelectorAll('*')].forEach(part => {
          part.style.setProperty('color', color, 'important');
          part.style.setProperty('fill', color, 'important');
        });
      }
      const border = luminance(style.stroke);
      if (border !== null && contrast(background, border) < 3) shape.style.setProperty('stroke', color, 'important');
    });
  };
})();
