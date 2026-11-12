// Palette values are shared with CSS and native UI through src/palette.json.
const THEME_COLORS = Object.fromEntries(Object.entries(window.QingYuPalette).map(([mode, colors]) => [mode, {
  background: colors.paper,
  textColor: colors['diagram-text'],
  textSecondary: colors['quote-text'],
  linkColor: colors.brand,
  borderColor: colors.border,
  codeBg: colors.code,
  diagramBg: colors.code,
  blockquoteBorderColor: colors['quote-bar'],
  primaryColor: colors['diagram-node'],
  primaryBorderColor: colors['diagram-border'],
  primaryTextColor: colors['diagram-text'],
  secondaryColor: colors['diagram-group'],
  tertiaryColor: colors['diagram-node'],
  lineColor: colors.brand,
  mainBkg: colors['diagram-node'],
  secondBkg: colors['diagram-group'],
  tertiaryTextColor: colors['diagram-text'],
  nodeBorder: colors['diagram-border'],
  clusterBkg: colors['diagram-group'],
  clusterBorder: colors['diagram-border'],
  edgeLabelBackground: mode === 'dark' ? colors['diagram-node'] : colors.code
}]));

/**
 * Get theme colors based on dark mode flag
 * @param {boolean} isDark - Whether dark mode is enabled
 * @returns {object} Color palette for the theme
 */
function getThemeColors(isDark = false) {
  return { ...THEME_COLORS[isDark ? 'dark' : 'light'] };
}

/**
 * Get Mermaid diagram configuration
 * @param {boolean} isDark - Whether dark mode is enabled
 * @returns {object} Mermaid configuration object
 */
function getMermaidConfig(isDark = false) {
  const themeColors = getThemeColors(isDark);

  const diagramBg = themeColors.diagramBg;
  const nodeBackground = themeColors.primaryColor;
  const clusterBackground = themeColors.secondaryColor;
  const borderColor = themeColors.textColor;
  const textColor = themeColors.textColor;
  const lineColor = themeColors.lineColor;
  const accentColor = lineColor;

  const extendedColors = {
    ...themeColors,
    background: diagramBg,
    primaryColor: nodeBackground,
    secondaryColor: clusterBackground,
    tertiaryColor: nodeBackground,
    primaryTextColor: textColor,
    textColor,
    nodeBorder: borderColor,
    nodeBkg: nodeBackground,
    clusterBkg: clusterBackground,
    clusterBorder: borderColor,
    edgeLabelBackground: 'transparent',
    edgeLabelTextColor: textColor,
    actorBkg: nodeBackground,
    actorBorder: borderColor,
    actorTextColor: textColor,
    signalColor: accentColor,
    signalTextColor: textColor,
    noteBkgColor: clusterBackground,
    noteBorderColor: borderColor,
    noteTextColor: textColor,
    arrowheadColor: lineColor,
    relationColor: lineColor,
    lineColor
  };
  return {
    startOnLoad: false,
    suppressErrorRendering: true,
    theme: isDark ? 'dark' : 'neutral',
    themeVariables: extendedColors,
    darkMode: isDark,
    fontSize: 15,
    fontFamily: "-apple-system, BlinkMacSystemFont, 'Helvetica Neue', 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei UI', 'Microsoft YaHei', Arial, sans-serif",
    flowchart: {
      useMaxWidth: true,
      htmlLabels: true,
      curve: 'basis',
      nodeSpacing: 80,
      rankSpacing: 80,
      padding: 30,
    },
    elk: {
      mergeEdges: true,
      padding: 40,
      nodeSpacing: 80,
      rankSpacing: 80,
    },
    sequence: {
      useMaxWidth: true,
    },
    gantt: {
      useMaxWidth: true,
    },
    journey: {
      useMaxWidth: true,
    },
    themeVariables: extendedColors,
    darkMode: isDark
  };
}

/**
 * Get PlantUML skinparams configuration
 * @param {boolean} isDark - Whether dark mode is enabled
 * @returns {string} PlantUML skinparams string
 */
function getPlantUMLSkinparams(isDark = false) {
  const colors = getThemeColors(isDark);
  const backgroundColor = colors.diagramBg || colors.background;

  const baseParams = [
    `skinparam backgroundColor ${backgroundColor}`,
    `skinparam defaultTextColor ${colors.textColor}`,
    `skinparam defaultFontColor ${colors.textColor}`,
    'skinparam defaultFontName "Helvetica"',
    'skinparam defaultFontSize 12',
    `skinparam actorBackgroundColor ${colors.primaryColor}`,
    `skinparam actorFontColor ${colors.textColor}`,
    `skinparam participantBackgroundColor ${colors.primaryColor}`,
    `skinparam participantFontColor ${colors.textColor}`,
    `skinparam classBackgroundColor ${colors.primaryColor}`,
    `skinparam classFontColor ${colors.textColor}`,
    `skinparam classAttributeFontColor ${colors.textColor}`,
    `skinparam sequenceActorBackgroundColor ${colors.primaryColor}`,
    `skinparam sequenceActorFontColor ${colors.textColor}`,
    `skinparam sequenceGroupBackgroundColor ${colors.primaryColor}`,
    `skinparam sequenceGroupHeaderFontColor ${colors.textColor}`,
    'skinparam sequenceMessageTextAlignment center'
  ].join('\n');

  const arrowParams = `
skinparam arrowColor ${colors.lineColor}
skinparam sequenceArrowColor ${colors.lineColor}
skinparam usecaseArrowColor ${colors.lineColor}
skinparam classArrowColor ${colors.lineColor}
skinparam componentArrowColor ${colors.lineColor}
skinparam stateArrowColor ${colors.lineColor}
skinparam activityArrowColor ${colors.lineColor}`;

  const componentParams = `
skinparam note {
  BackgroundColor ${colors.primaryColor}
  FontColor ${colors.textColor}
}
skinparam activity {
  BackgroundColor ${colors.primaryColor}
  FontColor ${colors.textColor}
  ArrowColor ${colors.lineColor}
}
skinparam state {
  BackgroundColor ${colors.primaryColor}
  FontColor ${colors.textColor}
  ArrowColor ${colors.lineColor}
}`;

  const additionalParams = `
skinparam usecase {
  BackgroundColor ${colors.primaryColor}
  FontColor ${colors.textColor}
  ArrowColor ${colors.lineColor}
}
skinparam component {
  BackgroundColor ${colors.primaryColor}
  FontColor ${colors.textColor}
  ArrowColor ${colors.lineColor}
}`;

  return baseParams + arrowParams + componentParams + additionalParams;
}

/**
 * Markmap branch colours by depth: the brand green at the root, then the
 * editor's highlight colours (title purple, link teal, bold gold, list grey),
 * the same values as the title, link, html and list colour assets.
 * @param {boolean} isDark - Whether dark mode is enabled
 * @returns {string[]} Colours, index = node depth modulo length
 */
function getMarkmapColors(isDark = false) {
  const colors = window.QingYuPalette[isDark ? 'dark' : 'light'];
  return [colors.brand, colors.title, colors.link, colors.gold, colors.markup];
}

// Export for use in other modules
window.ThemeColors = {
  THEME_COLORS,
  getThemeColors,
  getMermaidConfig,
  getPlantUMLSkinparams,
  getMarkmapColors
};
