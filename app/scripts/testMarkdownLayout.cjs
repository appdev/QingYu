const path = require("path");
const {app, BrowserWindow} = require("electron");
const sass = require("sass");
const {markdownToBlockDOM} = require("./markdownAppearanceFixture.cjs");

app.commandLine.appendSwitch("disable-gpu");

app.whenReady().then(async () => {
    const css = sass.compile(path.join(__dirname, "../src/assets/scss/base.scss")).css;
    const nativeCodeBlockDOM = markdownToBlockDOM("```javascript\nconst nativeValue = 1;\n```")
        .replace("const nativeValue = 1;", "<span class=\"hljs-keyword\">const</span>&nbsp;nativeValue = 1;");
    const window = new BrowserWindow({
        width: 500,
        show: false,
        webPreferences: {
            sandbox: true,
        },
    });
    const columns = Array.from({length: 8}, (_, index) => `<th>Column ${index + 1} with a long heading</th>`).join("");
    const cells = Array.from({length: 8}, (_, index) => `<td>Value ${index + 1} with content that keeps the table wide</td>`).join("");
    const html = `<!doctype html><style>.syntax-token{color:rgb(224, 108, 117)}.hljs-keyword{color:rgb(197, 80, 90)}.toolbar-fixture .markra-table-control{opacity:1;pointer-events:auto}.parity-fixture .protyle-wysiwyg .code-block{background-color:rgb(24, 25, 26);border-radius:8px}${css}</style>
<div class="protyle markdown-editor" style="--b3-theme-primary:rgb(66, 133, 244);height:600px;width:480px">
    <div class="protyle-content markdown-editor__content">
        <div class="markdown-editor__body">
            <div class="markdown-editor__surface b3-typography">
                <div class="cm-markra-table-wrap markra-table-controls-wrapper">
                    <div class="markra-table-scroll"><table class="cm-markra-table"><thead><tr>${columns}</tr></thead><tbody><tr>${cells}</tr></tbody></table></div>
                </div>
                <span class="img markra-image-node markra-image-node-selected"><span class="markra-image-frame"><img alt="test" style="height:80px;width:160px"><span class="protyle-action__drag" style="display:block"></span></span></span>
                <br><span class="img markra-image-node"><span class="markra-image-frame" id="default-image-frame" style="width:506px"><img alt="default-size" src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='506' height='538'%3E%3Crect width='506' height='538' fill='blue'/%3E%3C/svg%3E"><span class="protyle-action__drag" style="display:block"></span></span></span>
            </div>
        </div>
    </div>
</div>
<div class="protyle markdown-editor" id="visual-editor" style="--b3-editor-appearance-block-heading-1-color:rgb(120, 40, 160);--b3-editor-appearance-block-heading-1-font-size:36px;--b3-editor-appearance-inline-link-color:rgb(20, 110, 180);--b3-editor-appearance-shell-document-color:rgb(33, 33, 33);--b3-theme-on-background:rgb(33, 33, 33);--b3-theme-primary:rgb(66, 133, 244);height:200px;width:480px">
    <div class="cm-editor" data-markdown-mode="visual">
        <div class="cm-content">
            <div class="cm-line cm-markra-h1"><span class="syntax-token">Visual heading</span></div>
            <div class="cm-line"><span class="syntax-token cm-markra-link">Visual link</span></div>
        </div>
    </div>
</div>
<div class="protyle markdown-editor" id="large-image-editor" style="width:1000px">
    <div class="markdown-editor__surface b3-typography">
        <span class="img markra-image-node"><span class="markra-image-frame markra-image-frame-sized" style="width:800px"><img class="cm-markra-image" alt="enlarged" src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='506' height='538'%3E%3Crect width='506' height='538' fill='blue'/%3E%3C/svg%3E"><span class="protyle-action__drag" style="display:block"></span></span></span>
    </div>
</div>
<div class="protyle markdown-editor" id="empty-editor" style="height:600px;width:480px">
    <div class="protyle-content markdown-editor__content">
        <div class="protyle-top markdown-editor__top">
            <div class="protyle-title markdown-editor__title">Untitled.md</div>
        </div>
        <div class="markdown-editor__body">
            <div class="markdown-editor__surface b3-typography"></div>
        </div>
    </div>
</div>
<div class="protyle markdown-editor" id="header-editor" style="--b3-font-family-protyle:serif;--b3-font-size-editor:18px;--b3-theme-background:rgb(30, 30, 30);--b3-theme-on-background:rgb(235, 235, 235);--b3-theme-on-surface-light:rgb(150, 150, 150);width:480px">
    <div class="protyle-top markdown-editor__top">
        <div class="protyle-background markdown-editor__metadata protyle-background--enable">
            <div class="protyle-background__img markdown-editor__cover"><img alt="cover"></div>
            <div class="protyle-background__ia"><div class="protyle-background__icon fn__none"></div><div class="b3-chips b3-chips__doctag markdown-editor__tags"><span class="b3-chip b3-chip--middle">tag</span></div><div class="protyle-background__action markdown-editor__actions"><button class="b3-button b3-button--cancel" data-type="tag"><svg></svg>Add Tag</button><button class="b3-button b3-button--cancel" data-type="icon"><svg></svg>Add icon</button><button class="b3-button b3-button--cancel" data-type="cover"><svg></svg>Add cover</button></div></div>
        </div>
        <div class="protyle-title markdown-editor__title"><div class="protyle-title__input">Theme title</div></div>
    </div>
</div>
<div class="protyle markdown-editor" data-markdown-platform="mobile" id="mobile-editor" style="height:640px;width:375px">
    <div class="protyle-content markdown-editor__content">
        <div class="protyle-top markdown-editor__top">
            <div class="protyle-title markdown-editor__title">Mobile.md</div>
        </div>
        <div class="markdown-editor__body">
            <div class="markdown-editor__surface b3-typography">
                <div class="cm-markra-table-wrap markra-table-controls-wrapper">
                    <div class="markra-table-scroll"><table class="cm-markra-table"><thead><tr>${columns}</tr></thead><tbody><tr>${cells}</tr></tbody></table></div>
                </div>
            </div>
        </div>
    </div>
</div>
<div class="protyle markdown-editor toolbar-fixture" id="toolbar-editor" style="--b3-border-color:rgb(80, 80, 80);--b3-list-hover:rgb(55, 55, 55);--b3-theme-background:rgb(30, 30, 30);--b3-theme-error:rgb(240, 80, 80);--b3-theme-on-surface:rgb(180, 180, 180);--b3-theme-primary:rgb(66, 133, 244);width:320px">
    <div class="markdown-editor__surface b3-typography">
        <div class="cm-markra-table-wrap markra-table-controls-wrapper">
            <span class="markra-table-align-controls">
                <span class="markra-table-control-group markra-table-size-controls" data-table-control-group="size"><button class="markra-table-control markra-table-size-button"><svg class="markra-table-control-icon"></svg></button></span>
                <span class="markra-table-control-group markra-table-alignment-controls" data-table-control-group="alignment"><button class="markra-table-control markra-table-align-button" aria-pressed="true"><svg class="markra-table-control-icon"></svg></button><button class="markra-table-control markra-table-align-button"><svg class="markra-table-control-icon"></svg></button><button class="markra-table-control markra-table-align-button"><svg class="markra-table-control-icon"></svg></button></span>
                <span class="markra-table-control-group markra-table-width-controls" data-table-control-group="width"><button class="markra-table-control markra-table-width-button"><svg class="markra-table-control-icon"></svg></button></span>
                <span class="markra-table-control-group markra-table-delete-controls" data-table-control-group="delete"><button class="markra-table-control markra-table-delete-table"><svg class="markra-table-control-icon"></svg></button></span>
            </span>
            <div class="markra-table-scroll"><table class="cm-markra-table"><tbody><tr><td>内容</td></tr></tbody></table></div>
    </div>
</div>
</div>
<div class="protyle markdown-editor parity-fixture" id="code-parity-editor" style="--b3-font-family-code:'Theme Code';--b3-editor-appearance-block-code-background-color:rgb(24, 25, 26);--b3-editor-appearance-block-code-border-radius:8px;--b3-editor-appearance-control-code-language-color:rgb(91, 92, 93);--b3-theme-on-surface:rgb(91, 92, 93);width:640px">
    <div class="protyle-wysiwyg" data-appearance-fixture="native">${nativeCodeBlockDOM}</div>
    <div class="cm-editor" data-markdown-mode="visual"><div class="cm-content"><div class="cm-line cm-markra-code-line cm-markra-code-opening-line"><span class="protyle-action cm-markra-code-actions"><button class="protyle-action--first protyle-action__language markra-code-language-control cm-markra-code-header markra-code-language-label">javascript</button><span class="fn__flex-1"></span><button class="protyle-icon protyle-action__copy markra-code-copy-button" data-copied="false"><svg class="markra-code-copy-icon"></svg><svg class="markra-code-copy-check-icon"></svg></button><button class="protyle-icon protyle-action__menu markra-code-more-button"><svg></svg></button></span></div><div class="cm-line cm-markra-code-line cm-markra-code-content-line cm-markra-code-content-first cm-markra-code-content-last"><span class="cm-markra-code-token hljs-keyword">const</span>&nbsp;markdownValue = 1;</div><div class="cm-line cm-markra-code-line cm-markra-code-closing-line"></div></div></div>
</div>
`;
    await window.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(html)}`);
    const metrics = await window.webContents.executeJavaScript(`(() => {
        const editor = document.querySelector(".markdown-editor");
        const surface = document.querySelector(".markdown-editor__surface");
        const tableScroll = surface.querySelector(".markra-table-scroll");
        const table = tableScroll.querySelector("table");
        const imageFrame = surface.querySelector(".markra-image-frame");
        const selectedImageFrameStyle = getComputedStyle(imageFrame);
        const defaultImageFrame = surface.querySelector("#default-image-frame");
        const defaultImageRoot = defaultImageFrame.closest(".markra-image-node");
        const defaultImageHandle = defaultImageFrame.querySelector(".protyle-action__drag");
        const imageHandle = surface.querySelector(".markra-image-node .protyle-action__drag");
        const visualEditor = document.querySelector("#visual-editor");
        const largeImageEditor = document.querySelector("#large-image-editor");
        const enlargedImageFrame = largeImageEditor.querySelector(".markra-image-frame");
        const enlargedImage = largeImageEditor.querySelector("img");
        const visualHeading = visualEditor.querySelector(".cm-markra-h1 span");
        const visualLink = visualEditor.querySelector(".cm-markra-link");
        const emptyEditor = document.querySelector("#empty-editor");
        const emptyBody = emptyEditor.querySelector(".markdown-editor__body");
        const emptySurface = emptyEditor.querySelector(".markdown-editor__surface");
        const emptyBodyRect = emptyBody.getBoundingClientRect();
        const emptySurfaceRect = emptySurface.getBoundingClientRect();
        const mobileEditor = document.querySelector("#mobile-editor");
        const mobileBody = mobileEditor.querySelector(".markdown-editor__body");
        const mobileTableViewport = mobileEditor.querySelector(".markra-table-scroll");
        const headerEditor = document.querySelector("#header-editor");
        const headerCover = headerEditor.querySelector(".markdown-editor__cover");
        const headerTitle = headerEditor.querySelector(".protyle-title__input");
        const headerActionButtons = Array.from(headerEditor.querySelectorAll(".b3-button"));
        const toolbarEditor = document.querySelector("#toolbar-editor");
        const lightToolbarEditor = toolbarEditor.cloneNode(true);
        lightToolbarEditor.id = "light-toolbar-editor";
        lightToolbarEditor.style.setProperty("--b3-border-color", "rgb(210, 210, 210)");
        lightToolbarEditor.style.setProperty("--b3-list-hover", "rgb(235, 240, 250)");
        lightToolbarEditor.style.setProperty("--b3-theme-background", "rgb(255, 255, 255)");
        lightToolbarEditor.style.setProperty("--b3-theme-on-surface", "rgb(90, 90, 90)");
        lightToolbarEditor.style.setProperty("--b3-theme-primary", "rgb(30, 100, 220)");
        document.body.append(lightToolbarEditor);
        const toolbarSurface = toolbarEditor.querySelector(".markdown-editor__surface");
        const toolbar = toolbarEditor.querySelector(".markra-table-align-controls");
        const toolbarGroups = Array.from(toolbar.querySelectorAll(":scope > .markra-table-control-group"));
        const toolbarButtons = Array.from(toolbar.querySelectorAll(".markra-table-control"));
        const toolbarIcons = Array.from(toolbar.querySelectorAll(".markra-table-control-icon"));
        const selectedToolbarButton = toolbar.querySelector('[aria-pressed="true"]');
        const deleteToolbarButton = toolbar.querySelector(".markra-table-delete-table");
        const lightToolbar = lightToolbarEditor.querySelector(".markra-table-align-controls");
        const lightToolbarGroup = lightToolbar.querySelector(".markra-table-control-group");
        const lightSelectedToolbarButton = lightToolbar.querySelector('[aria-pressed="true"]');
        const lightDeleteToolbarButton = lightToolbar.querySelector(".markra-table-delete-table");
        const toolbarRect = toolbar.getBoundingClientRect();
        const toolbarTableRect = toolbarEditor.querySelector(".cm-markra-table").getBoundingClientRect();
        const codeParityEditor = document.querySelector("#code-parity-editor");
        const nativeCode = codeParityEditor.querySelector(".code-block");
        const nativeCodeContent = codeParityEditor.querySelector(".hljs");
        const nativeCodeAction = codeParityEditor.querySelector(".protyle-action__language");
        const nativeCodeActions = codeParityEditor.querySelector(".code-block > .protyle-action");
        const nativeCopyButton = codeParityEditor.querySelector(".protyle-wysiwyg .protyle-action__copy");
        const markdownCode = codeParityEditor.querySelector(".cm-markra-code-content-first");
        const markdownCodeContent = codeParityEditor.querySelector(".cm-markra-code-content-line");
        const markdownCodeActions = codeParityEditor.querySelector(".cm-markra-code-actions");
        const nativeCodeToken = codeParityEditor.querySelector(".protyle-wysiwyg .hljs-keyword");
        const markdownCodeToken = codeParityEditor.querySelector(".cm-markra-code-token.hljs-keyword");
        const markdownCodeAction = codeParityEditor.querySelector(".markra-code-language-label");
        const markdownCopyButton = codeParityEditor.querySelector(".markra-code-copy-button");
        const markdownCopyIcon = codeParityEditor.querySelector(".markra-code-copy-icon");
        const markdownCopyCheckIcon = codeParityEditor.querySelector(".markra-code-copy-check-icon");
        const imageFrameRect = imageFrame.getBoundingClientRect();
        const imageHandleRect = imageHandle.getBoundingClientRect();
        return {
            editorClientWidth: editor.clientWidth,
            editorScrollWidth: editor.scrollWidth,
            previewClientWidth: surface.clientWidth,
            previewScrollWidth: surface.scrollWidth,
            tableClientWidth: table.clientWidth,
            tableScrollWidth: table.scrollWidth,
            tableViewportClientWidth: tableScroll.clientWidth,
            tableViewportScrollWidth: tableScroll.scrollWidth,
            tableDisplay: getComputedStyle(table).display,
            tableHeight: table.getBoundingClientRect().height,
            imageFrameRight: imageFrameRect.right,
            imageHandleCenter: imageHandleRect.left + imageHandleRect.width / 2,
            selectedImageOutlineColor: selectedImageFrameStyle.outlineColor,
            selectedImageOutlineWidth: selectedImageFrameStyle.outlineWidth,
            defaultImageWidth: defaultImageFrame.getBoundingClientRect().width,
            defaultImageLeft: defaultImageFrame.getBoundingClientRect().left,
            defaultImageRight: defaultImageFrame.getBoundingClientRect().right,
            defaultImageRootWidth: defaultImageRoot.getBoundingClientRect().width,
            defaultImageHandleRight: defaultImageHandle.getBoundingClientRect().right,
            surfaceLeft: surface.getBoundingClientRect().left,
            surfaceRight: surface.getBoundingClientRect().right,
            visualEditorColor: getComputedStyle(visualEditor.querySelector(".cm-editor")).color,
            visualHeadingColor: getComputedStyle(visualHeading).color,
            visualHeadingFontSize: getComputedStyle(visualHeading.closest(".cm-markra-h1")).fontSize,
            visualLinkColor: getComputedStyle(visualLink).color,
            visualThemeHeadingColor: getComputedStyle(visualEditor).getPropertyValue("--b3-editor-appearance-block-heading-1-color").trim(),
            visualThemeLinkColor: getComputedStyle(visualEditor).getPropertyValue("--b3-editor-appearance-inline-link-color").trim(),
            enlargedImageFrameWidth: enlargedImageFrame.getBoundingClientRect().width,
            enlargedImageWidth: enlargedImage.getBoundingClientRect().width,
            enlargedImageHeight: enlargedImage.getBoundingClientRect().height,
            emptyBodyBottom: emptyBodyRect.bottom,
            emptyPreviewBottom: emptySurfaceRect.bottom,
            emptyPreviewHeight: emptySurfaceRect.height,
            mobileBodyPaddingLeft: getComputedStyle(mobileBody).paddingLeft,
            mobileClientWidth: mobileEditor.clientWidth,
            mobileScrollWidth: mobileEditor.scrollWidth,
            mobileTableClientWidth: mobileTableViewport.clientWidth,
            mobileTableScrollWidth: mobileTableViewport.scrollWidth,
            headerCoverWidth: headerCover.getBoundingClientRect().width,
            headerEditorWidth: headerEditor.getBoundingClientRect().width,
            headerTitleColor: getComputedStyle(headerTitle).color,
            headerTitleFontFamily: getComputedStyle(headerTitle).fontFamily,
            headerTitleFontSize: getComputedStyle(headerTitle).fontSize,
            headerActionButtonTops: headerActionButtons.map((button) => button.getBoundingClientRect().top),
            lightToolbarBackground: getComputedStyle(lightToolbarGroup).backgroundColor,
            lightToolbarBorderColor: getComputedStyle(lightToolbarGroup).borderColor,
            lightToolbarDeleteColor: getComputedStyle(lightDeleteToolbarButton).color,
            lightToolbarSelectedColor: getComputedStyle(lightSelectedToolbarButton).color,
            lightToolbarThemeBackground: getComputedStyle(lightToolbarEditor).getPropertyValue("--b3-theme-background").trim(),
            lightToolbarThemeBorder: getComputedStyle(lightToolbarEditor).getPropertyValue("--b3-border-color").trim(),
            lightToolbarThemeOnSurface: getComputedStyle(lightToolbarEditor).getPropertyValue("--b3-theme-on-surface").trim(),
            lightToolbarThemePrimary: getComputedStyle(lightToolbarEditor).getPropertyValue("--b3-theme-primary").trim(),
            toolbarButtonHeights: toolbarButtons.map((button) => button.getBoundingClientRect().height),
            toolbarButtonWidths: toolbarButtons.map((button) => button.getBoundingClientRect().width),
            toolbarDeleteColor: getComputedStyle(deleteToolbarButton).color,
            toolbarEditorClientWidth: toolbarEditor.clientWidth,
            toolbarEditorScrollWidth: toolbarEditor.scrollWidth,
            toolbarGap: getComputedStyle(toolbar).gap,
            toolbarGroupCount: toolbarGroups.length,
            toolbarGroupTops: toolbarGroups.map((group) => group.getBoundingClientRect().top),
            toolbarHeight: toolbarRect.height,
            toolbarTop: toolbarRect.top,
            toolbarIconHeights: toolbarIcons.map((icon) => icon.getBoundingClientRect().height),
            toolbarIconWidths: toolbarIcons.map((icon) => icon.getBoundingClientRect().width),
            toolbarRight: toolbarRect.right,
            toolbarScrollWidth: toolbar.scrollWidth,
            toolbarSelectedColor: getComputedStyle(selectedToolbarButton).color,
            toolbarSurfaceRight: toolbarSurface.getBoundingClientRect().right,
            toolbarTableTop: toolbarTableRect.top,
            toolbarTableBottom: toolbarTableRect.bottom,
            toolbarBottom: toolbarRect.bottom,
            toolbarThemeOnSurface: getComputedStyle(toolbarEditor).getPropertyValue("--b3-theme-on-surface").trim(),
            toolbarThemePrimary: getComputedStyle(toolbarEditor).getPropertyValue("--b3-theme-primary").trim(),
            nativeCodeBackground: getComputedStyle(nativeCode).backgroundColor,
            nativeCodeRadius: getComputedStyle(nativeCode).borderRadius,
            nativeCodeFontFamily: getComputedStyle(nativeCodeContent).fontFamily,
            nativeCodeActionColor: getComputedStyle(nativeCodeAction).color,
            markdownCodeBackground: getComputedStyle(markdownCode).backgroundColor,
            markdownCodeRadius: getComputedStyle(markdownCode).borderTopLeftRadius,
            markdownCodeFontFamily: getComputedStyle(markdownCodeContent).fontFamily,
            markdownCodeActionColor: getComputedStyle(markdownCodeAction).color,
            nativeCodeTextOffset: nativeCodeToken.getBoundingClientRect().top - nativeCode.getBoundingClientRect().top,
            markdownCodeTextOffset: markdownCodeToken.getBoundingClientRect().top - markdownCode.getBoundingClientRect().top,
            nativeCodeHeight: nativeCode.getBoundingClientRect().height,
            markdownCodeHeight: markdownCode.getBoundingClientRect().height,
            nativeCodeLanguageLeft: nativeCodeAction.getBoundingClientRect().left - nativeCode.getBoundingClientRect().left,
            markdownCodeLanguageLeft: markdownCodeAction.getBoundingClientRect().left - markdownCode.getBoundingClientRect().left,
            nativeCodeLanguageTop: nativeCodeAction.getBoundingClientRect().top - nativeCode.getBoundingClientRect().top,
            markdownCodeLanguageTop: markdownCodeAction.getBoundingClientRect().top - markdownCode.getBoundingClientRect().top,
            nativeCodeActionRight: nativeCode.getBoundingClientRect().right - nativeCodeActions.getBoundingClientRect().right,
            markdownCodeActionRight: markdownCode.getBoundingClientRect().right - markdownCodeActions.getBoundingClientRect().right,
            nativeCodeActionTop: nativeCodeActions.getBoundingClientRect().top - nativeCode.getBoundingClientRect().top,
            markdownCodeActionTop: markdownCodeActions.getBoundingClientRect().top - markdownCode.getBoundingClientRect().top,
            nativeCodeTokenColor: getComputedStyle(nativeCodeToken).color,
            markdownCodeTokenColor: getComputedStyle(markdownCodeToken).color,
            markdownCopyIconDisplay: getComputedStyle(markdownCopyIcon).display,
            markdownCopyCheckIconDisplay: getComputedStyle(markdownCopyCheckIcon).display,
            markdownCopyButtonWidth: markdownCopyButton.getBoundingClientRect().width,
            nativeCopyButtonWidth: nativeCopyButton.getBoundingClientRect().width,
        };
    })()`);
    const keepsTabWidth = metrics.editorScrollWidth === metrics.editorClientWidth &&
        metrics.previewScrollWidth === metrics.previewClientWidth &&
        metrics.tableViewportClientWidth <= metrics.previewClientWidth &&
        metrics.tableViewportScrollWidth >= metrics.tableViewportClientWidth &&
        metrics.tableDisplay === "table" &&
        metrics.tableHeight > 0 &&
        Math.abs(metrics.imageFrameRight - metrics.imageHandleCenter) <= 3;
    if (!keepsTabWidth) {
        console.error("Markdown wide table escapes the tab width", metrics);
        app.exit(1);
        return;
    }
    if (metrics.selectedImageOutlineWidth !== "1px" || metrics.selectedImageOutlineColor !== "rgb(66, 133, 244)") {
        console.error("Markdown selected image does not use the theme outline", metrics);
        app.exit(1);
        return;
    }
    if (metrics.defaultImageWidth < 400 || metrics.defaultImageWidth > metrics.previewClientWidth) {
        console.error("Markdown image does not use its intrinsic default size", metrics);
        app.exit(1);
        return;
    }
    const visualColorsMatch = metrics.visualEditorColor === "rgb(33, 33, 33)" &&
        metrics.visualHeadingColor === metrics.visualThemeHeadingColor &&
        metrics.visualHeadingFontSize === "36px" &&
        metrics.visualLinkColor === metrics.visualThemeLinkColor;
    if (!visualColorsMatch) {
        console.error("Markdown visual mode leaks source syntax colors", metrics);
        app.exit(1);
        return;
    }
    const enlargedRatio = metrics.enlargedImageWidth / metrics.enlargedImageHeight;
    if (Math.abs(metrics.enlargedImageWidth - metrics.enlargedImageFrameWidth) > 1 ||
        Math.abs(enlargedRatio - 506 / 538) > 0.01) {
        console.error("Markdown enlarged image is clipped or does not fill its frame", metrics);
        app.exit(1);
        return;
    }
    const fillsEditableBody = metrics.emptyPreviewBottom >= metrics.emptyBodyBottom - 1 &&
        metrics.emptyPreviewHeight > 300;
    if (!fillsEditableBody) {
        console.error("Markdown empty preview does not fill the editable body", metrics);
        app.exit(1);
        return;
    }
    const keepsMobileWidth = metrics.mobileClientWidth === 375 &&
        metrics.mobileScrollWidth === metrics.mobileClientWidth &&
        metrics.mobileTableClientWidth < metrics.mobileTableScrollWidth &&
        metrics.mobileBodyPaddingLeft === "16px";
    if (!keepsMobileWidth) {
        console.error("Markdown mobile layout escapes the viewport", metrics);
        app.exit(1);
        return;
    }
    const headerUsesNativeTheme = Math.abs(metrics.headerCoverWidth - metrics.headerEditorWidth) <= 1 &&
        metrics.headerTitleColor === "rgb(235, 235, 235)" &&
        metrics.headerTitleFontFamily === "serif" &&
        metrics.headerTitleFontSize === "36px";
    if (!headerUsesNativeTheme) {
        console.error("Markdown metadata header does not follow the native theme and layout", metrics);
        app.exit(1);
        return;
    }
    const headerActionsStayTogether = metrics.headerActionButtonTops.length === 3 &&
        metrics.headerActionButtonTops.every((top) => Math.abs(top - metrics.headerActionButtonTops[0]) <= 0.5);
    if (!headerActionsStayTogether) {
        console.error("Markdown metadata actions split across rows after adding a tag", metrics);
        app.exit(1);
        return;
    }
    const toolbarFitsNarrowEditor = metrics.toolbarEditorScrollWidth === metrics.toolbarEditorClientWidth &&
        metrics.toolbarScrollWidth <= metrics.toolbarEditorClientWidth &&
        metrics.toolbarRight <= metrics.toolbarSurfaceRight + 1 &&
        metrics.toolbarTableTop <= metrics.toolbarTop + 1 &&
        metrics.toolbarTableBottom >= metrics.toolbarBottom - 1 &&
        metrics.toolbarGroupCount === 4 &&
        metrics.toolbarGroupTops.every((top) => Math.abs(top - metrics.toolbarGroupTops[0]) <= 0.5) &&
        metrics.toolbarGap === "8px" &&
        metrics.toolbarButtonWidths.every((width) => Math.abs(width - 28) <= 0.5) &&
        metrics.toolbarButtonHeights.every((height) => Math.abs(height - 28) <= 0.5) &&
        metrics.toolbarIconWidths.every((width) => Math.abs(width - 16) <= 0.5) &&
        metrics.toolbarIconHeights.every((height) => Math.abs(height - 16) <= 0.5) &&
        metrics.toolbarSelectedColor === metrics.toolbarThemePrimary &&
        metrics.toolbarDeleteColor === metrics.toolbarThemeOnSurface &&
        metrics.lightToolbarBackground === metrics.lightToolbarThemeBackground &&
        metrics.lightToolbarBorderColor === metrics.lightToolbarThemeBorder &&
        metrics.lightToolbarSelectedColor === metrics.lightToolbarThemePrimary &&
        metrics.lightToolbarDeleteColor === metrics.lightToolbarThemeOnSurface;
    if (!toolbarFitsNarrowEditor) {
        console.error("Markdown table toolbar does not fit the narrow editor", metrics);
        app.exit(1);
        return;
    }
    const codeBlockUsesNativeTheme = metrics.markdownCodeBackground === metrics.nativeCodeBackground &&
        metrics.markdownCodeRadius === metrics.nativeCodeRadius &&
        metrics.markdownCodeFontFamily === metrics.nativeCodeFontFamily &&
        metrics.markdownCodeActionColor === metrics.nativeCodeActionColor &&
        Math.abs(metrics.markdownCodeTextOffset - metrics.nativeCodeTextOffset) <= 1 &&
        Math.abs(metrics.markdownCodeHeight - metrics.nativeCodeHeight) <= 1 &&
        Math.abs(metrics.markdownCodeLanguageLeft - metrics.nativeCodeLanguageLeft) <= 1 &&
        Math.abs(metrics.markdownCodeLanguageTop - metrics.nativeCodeLanguageTop) <= 1 &&
        Math.abs(metrics.markdownCodeActionRight - metrics.nativeCodeActionRight) <= 1 &&
        Math.abs(metrics.markdownCodeActionTop - metrics.nativeCodeActionTop) <= 1 &&
        metrics.markdownCodeTokenColor === metrics.nativeCodeTokenColor &&
        metrics.markdownCopyIconDisplay !== "none" &&
        metrics.markdownCopyCheckIconDisplay === "none" &&
        Math.abs(metrics.markdownCopyButtonWidth - metrics.nativeCopyButtonWidth) <= 1;
    if (!codeBlockUsesNativeTheme) {
        console.error("Markdown code block does not use the native theme", metrics);
        app.exit(1);
        return;
    }
    console.log("Markdown wide table stays within the tab width", metrics);
    app.exit(0);
}).catch((error) => {
    console.error(error);
    app.exit(1);
});
