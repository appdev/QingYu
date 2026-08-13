export interface SiyuanCodeBlockConfig {
    ligatures: boolean;
    lineWrap: boolean;
    showLineNumbers: boolean;
}

export const readSiyuanCodeBlockConfig = (config: Pick<Config.IEditor,
"codeLigatures" | "codeLineWrap" | "codeSyntaxHighlightLineNum"> | undefined): SiyuanCodeBlockConfig => ({
    ligatures: config?.codeLigatures ?? false,
    lineWrap: config?.codeLineWrap ?? true,
    showLineNumbers: config?.codeSyntaxHighlightLineNum ?? false,
});
