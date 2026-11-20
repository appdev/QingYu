import palette from './palette.json';

// Pinto's Theme.swift, light/dark pairs in page, sidebar, list, selection, ink,
// muted, separator, soft fill, card, code, panel, accent order.
const colors = {
  paper: ['FBF8F1/1A1815','EEE7DA/24201C','F6F1E7/1F1C18','E6DCCB/37312A','2B241C/EDE5D8','6B5D4C/ADA190','E4DACA/37322B','EFE8DB/2E2A24','FFFDF8/26231F','F0E9DC/2A2622','F8F4EC/211E1A','974E00/F2A940'],
  sepia: ['F4ECD8/241E16','E6DABE/1A160F','EDE3CB/1F1A13','DACCA9/3A3124','3A2C1A/E9DCC2','66543A/B5A589','DBCDAE/3A3125','E8DCC0/30281D','FAF4E6/2C251B','EADFC4/30281D','F0E7D1/29221A','8E4411/E0975A'],
  slate: ['F6F8FB/242933','E4E9F0/2E3440','EEF1F6/2A303B','D6DEEA/3C4454','232A36/ECEFF4','4C566A/B8C0CE','D8DEE8/3B4252','E5EAF1/353C4A','FFFFFF/2E3440','E9EDF3/333A47','F1F4F8/2B313C','3D6394/88C0D0'],
  forest: ['FAFBF9/111511','E8ECE5/1A1F1A','F2F4F0/161A16','DAE3D4/2A342A','1E261F/E2E8E0','55605A/A0AC9F','DDE3D9/2A322A','E9EDE6/232A23','FFFFFF/1C221C','EDF0EA/1F251F','F5F7F3/191E19','287045/6CC690'],
  contrast: ['FFFFFF/000000','FFFFFF/000000','FFFFFF/000000','FFE14D/4F4100','000000/FFFFFF','1F1F1F/E6E6E6','000000/FFFFFF','EBEBEB/262626','FFFFFF/000000','F0F0F0/1C1C1C','FFFFFF/000000','0033A0/FFD60A'],
};
export const themeNames = { default: '默认', paper: '纸张', sepia: '暖棕', slate: '岩灰', forest: '森林', contrast: '高对比度' } as const;
export type Theme = keyof typeof themeNames;
export function themePalette(theme: Theme, dark: boolean) {
  const base = palette[dark ? 'dark' : 'light'];
  if (theme === 'default') return base;
  const [page, sidebar, list, selection, ink, muted, line, soft, card, code, panel, accent] = colors[theme].map(pair => '#' + pair.split('/')[Number(dark)]);
  return { ...base, paper: page, sidebar, list, ink, muted, border: line, line, sand: soft, card, code,
    secondary: muted, title: ink, gold: accent, brand: accent, link: accent,
    'editor-ink': ink, 'editor-code': ink, 'render-text': ink, 'render-code': ink, 'quote-text': muted,
    'note-selection': selection, 'folder-selection': selection, 'selection': selection,
    'control-fill': soft, 'control-hover': selection, 'section-line': line, 'row-line': line,
    'settings-ground': panel, 'settings-sidebar': sidebar, 'settings-card': card, 'settings-sheet': panel,
    'settings-disclosure': theme === 'contrast' ? muted : muted + (dark ? 'A6' : '6E'),
    'settings-group-fill': card, 'settings-theme-fill': sidebar,
    'slash-panel': panel, 'accent-soft': dark ? '#3A3A3A' : '#' + [1, 3, 5].map(at => {
      const channel = parseInt(accent.slice(at, at + 2), 16);
      return Math.round(channel + (255 - channel) * .88).toString(16).padStart(2, '0');
    }).join('').toUpperCase(),
    'settings-selection': selection, 'switch-on': accent,
    'mobile-sidebar': sidebar, 'mobile-list': list, 'mobile-row': card,
    'mobile-note-selection': selection, 'mobile-ink': ink, 'mobile-muted': muted, 'mobile-sand': soft,
    'diagram-text': ink, 'diagram-node': card, 'diagram-group': soft, 'diagram-border': line,
  };
}
