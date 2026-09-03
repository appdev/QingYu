// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package api

import (
	"net/http"
	"path/filepath"
	"strings"

	"github.com/88250/gulu"
	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/logging"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func exportMarkdownDocumentZip(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	boxID, p, ok := markdownExportArgs(c, ret)
	if !ok {
		return
	}
	artifact, err := model.ExportMarkdownDocumentZip(boxID, p)
	if err != nil {
		ret.Code = -1
		ret.Msg = util.EscapeHTML(err.Error())
		return
	}
	ret.Data = artifact
}

func saveMarkdownDocumentAsTemplate(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var boxID, p, name string
	var overwrite bool
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &boxID, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("name", &name, true, true),
		util.BindJsonArg("overwrite", &overwrite, false, false),
	) {
		return
	}
	code, err := model.SaveMarkdownDocumentAsTemplate(boxID, p, name, overwrite)
	if err != nil {
		ret.Code = -1
		ret.Msg = util.EscapeHTML(err.Error())
		return
	}
	ret.Code = code
}

func exportMarkdownDocumentPreview(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var boxID, p string
	var cardPreview bool
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &boxID, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("cardPreview", &cardPreview, false, false),
	) {
		return
	}
	var name, content string
	var missing []string
	var err error
	if cardPreview {
		name, content, missing, err = model.ExportMarkdownDocumentCardPreview(boxID, p)
	} else {
		name, content, missing, err = model.ExportMarkdownDocumentPreview(boxID, p)
	}
	if err != nil {
		previewType := "document"
		if cardPreview {
			previewType = "card"
		}
		logging.LogErrorf("export Markdown %s preview failed [notebook=%q, path=%q]: %s",
			previewType, boxID, p, err)
		ret.Code = -1
		ret.Msg = util.EscapeHTML(err.Error())
		return
	}
	ret.Data = map[string]any{
		"name":    name,
		"content": content,
		"missing": missing,
		"attrs":   map[string]string{},
		"type":    "NodeDocument",
	}
}

func exportMarkdownDocumentPandoc(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var boxID, p, format string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &boxID, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("format", &format, true, true),
	) {
		return
	}
	artifact, err := model.ExportMarkdownDocumentPandoc(boxID, p, format)
	if err != nil {
		ret.Code = -1
		ret.Msg = util.EscapeHTML(err.Error())
		return
	}
	ret.Data = artifact
}

func exportMarkdownDocumentHTML(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var boxID, p, savePath string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &boxID, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("savePath", &savePath, false, false),
	) {
		return
	}
	folder := ""
	if savePath = strings.TrimSpace(savePath); savePath == "" {
		folder = "markdown-html-" + util.CurrentTimeSecondsStr()
		savePath = filepath.Join(util.TempDir, "export", folder)
	} else if rejectEncryptedBoxPath(savePath) {
		ret.Code = -1
		ret.Msg = model.Conf.Language(313)
		return
	}
	name, content, missing, err := model.ExportMarkdownDocumentHTML(boxID, p, savePath)
	if err != nil {
		ret.Code = -1
		ret.Msg = util.EscapeHTML(err.Error())
		return
	}
	ret.Data = map[string]any{"name": name, "content": content, "folder": folder, "missing": missing}
}

func exportMarkdownDocumentDocx(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	boxID, p, ok := markdownExportArgs(c, ret)
	if !ok {
		return
	}
	artifact, err := model.ExportMarkdownDocumentDocx(boxID, p)
	if err != nil {
		ret.Code = -1
		ret.Msg = util.EscapeHTML(err.Error())
		return
	}
	ret.Data = artifact
}

func processMarkdownDocumentPDF(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var pdfPath string
	var watermark bool
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("path", &pdfPath, true, true),
		util.BindJsonArg("watermark", &watermark, false, false),
	) {
		return
	}
	if err := model.ProcessMarkdownPDF(pdfPath, watermark); err != nil {
		ret.Code = -1
		ret.Msg = util.EscapeHTML(err.Error())
	}
}

func markdownExportArgs(c *gin.Context, ret *gulu.Result) (boxID, p string, ok bool) {
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return "", "", false
	}
	ok = util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &boxID, true, true),
		util.BindJsonArg("path", &p, true, true),
	)
	return
}
