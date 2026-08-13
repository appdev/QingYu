// Lute 配置全部读取全局 window.siyuan.config / window.siyuan.emojis，跨编辑器一致，
// 因此所有 Protyle 编辑器共用同一个 Lute 实例，将内存与初始化开销从 O(编辑器数) 降为 O(1)。
let luteInstance: Lute | undefined;
let luteOptions: ILuteOptions | undefined;

/**
 * 获取（首次调用时创建）共享 Lute 单例。
 *
 * 仅在首次创建时应用 options，后续调用直接返回已缓存的实例 ——
 * Lute 配置本就源于全局 config，跨编辑器一致，无需按编辑器区分。
 */
export const getLute = (options: ILuteOptions): Lute => {
    if (!luteInstance) {
        luteOptions = options;
        luteInstance = setLute(options, true);
    }
    return luteInstance;
};

/**
 * 直接获取已初始化的共享 Lute 单例。
 * 供 emoji 等无需传入 options 的场景使用；尚未创建时返回 undefined。
 */
export const getLuteInstance = (): Lute | undefined => {
    return luteInstance;
};

/**
 * 创建与内核 BlockDOM 输出一致的静态 Protyle Lute。
 *
 * 编辑器共享实例启用了 Spin，用于行内编辑与粘贴；外观探针需要关闭 Spin，才能得到服务端下发给原生编辑器的 DOM 层级。
 */
export const createStaticProtyleLute = (): Lute | undefined => {
    if (!window.siyuan?.config?.editor?.markdown || !window.siyuan.emojis) {
        return undefined;
    }
    return setLute(luteOptions ?? {
        emojiSite: "",
        emojis: {},
        headingAnchor: false,
        listStyle: false,
        paragraphBeginningSpace: false,
        sanitize: true,
    }, false);
};

/**
 * 根据全局配置与传入选项构建一个新的 Lute 实例，供共享单例初始化使用。
 */
const setLute = (options: ILuteOptions, spin: boolean) => {
    const lute: Lute = Lute.New();
    lute.SetSpellcheck(window.siyuan.config.editor.spellcheck);
    lute.SetProtyleMarkNetImg(window.siyuan.config.editor.displayNetImgMark);
    lute.SetFileAnnotationRef(true);
    lute.SetHTMLTag2TextMark(true);
    lute.SetTextMark(true);
    lute.SetHeadingID(false);
    lute.SetYamlFrontMatter(false);
    lute.PutEmojis(options.emojis);
    lute.SetEmojiSite(options.emojiSite);
    lute.SetHeadingAnchor(options.headingAnchor);
    lute.SetInlineMathAllowDigitAfterOpenMarker(true);
    lute.SetToC(false);
    lute.SetIndentCodeBlock(false);
    lute.SetParagraphBeginningSpace(true);
    lute.SetSetext(false);
    lute.SetFootnotes(false);
    lute.SetLinkRef(false);
    lute.SetSanitize(options.sanitize);
    lute.SetChineseParagraphBeginningSpace(options.paragraphBeginningSpace);
    lute.SetRenderListStyle(options.listStyle);
    lute.SetImgPathAllowSpace(true);
    lute.SetKramdownIAL(true);
    lute.SetTag(true);
    lute.SetSuperBlock(true);
    lute.SetInlineAsterisk(window.siyuan.config.editor.markdown.inlineAsterisk);
    lute.SetInlineUnderscore(window.siyuan.config.editor.markdown.inlineUnderscore);
    lute.SetSup(window.siyuan.config.editor.markdown.inlineSup);
    lute.SetSub(window.siyuan.config.editor.markdown.inlineSub);
    lute.SetTag(window.siyuan.config.editor.markdown.inlineTag);
    lute.SetInlineMath(window.siyuan.config.editor.markdown.inlineMath);
    lute.SetGFMStrikethrough1(false);
    lute.SetGFMStrikethrough(window.siyuan.config.editor.markdown.inlineStrikethrough);
    lute.SetMark(window.siyuan.config.editor.markdown.inlineMark);
    if (options.lazyLoadImage) {
        lute.SetImageLazyLoading(options.lazyLoadImage);
    }
    lute.SetBlockRef(true);
    if (window.siyuan.emojis[0].items.length > 0) {
        const emojis: IObject = {};
        window.siyuan.emojis[0].items.forEach(item => {
            emojis[item.keywords] = options.emojiSite + "/" + item.unicode;
        });
        lute.PutEmojis(emojis);
    }
    lute.SetUnorderedListMarker("-");
    lute.SetDataTask(true);
    lute.SetExportNormalizeTaskListMarker(spin);
    lute.SetArbitraryTaskListItemMarker(true);
    lute.SetEnsureListItemParagraph(true); // 空列表项下创建子列表前补一个空段落
    // 模式开关放在所有语法选项之后，确保静态 BlockDOM 与行内 Spin 渲染不会被后续设置改写。
    lute.SetSpin(spin);
    lute.SetProtyleWYSIWYG(true);
    lute.SetCallout(true);
    return lute;
};
