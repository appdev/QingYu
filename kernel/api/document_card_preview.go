// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package api

import (
	"net/http"
	"strings"

	"github.com/88250/gulu"
	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/logging"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func logDocumentCardPreviewError(stage string, ref model.DocumentCardReference, err error) {
	logging.LogErrorf("document card preview %s failed [kind=%q, notebook=%q, path=%q, id=%q]: %s",
		stage, ref.Kind, ref.Notebook, ref.Path, ref.ID, err)
}

func prepareDocumentCardPreview(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var refArg map[string]any
	var theme, appearanceKey, size string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("reference", &refArg, true, true),
		util.BindJsonArg("theme", &theme, true, true),
		util.BindJsonArg("appearanceKey", &appearanceKey, true, true),
		util.BindJsonArg("size", &size, true, true),
	) {
		return
	}
	data, err := gulu.JSON.MarshalJSON(refArg)
	if err != nil {
		logging.LogErrorf("document card preview reference marshal failed: %s", err)
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	var ref model.DocumentCardReference
	if err = gulu.JSON.UnmarshalJSON(data, &ref); err != nil {
		logging.LogErrorf("document card preview reference unmarshal failed: %s", err)
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	descriptor, err := model.PrepareDocumentCardPreview(ref, theme, appearanceKey, size)
	if err != nil {
		logDocumentCardPreviewError("prepare", ref, err)
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	ret.Data = descriptor
}

func storeDocumentCardPreview(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	var ref model.DocumentCardReference
	var descriptor model.DocumentCardPreviewDescriptor
	if err := gulu.JSON.UnmarshalJSON([]byte(c.PostForm("reference")), &ref); err != nil {
		logging.LogErrorf("document card preview store reference unmarshal failed: %s", err)
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	if err := gulu.JSON.UnmarshalJSON([]byte(c.PostForm("descriptor")), &descriptor); err != nil {
		logDocumentCardPreviewError("store descriptor unmarshal", ref, err)
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	file, err := c.FormFile("file")
	if err != nil {
		logDocumentCardPreviewError("store form file", ref, err)
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	opened, err := file.Open()
	if err != nil {
		logDocumentCardPreviewError("store file open", ref, err)
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	defer opened.Close()
	if err = model.StoreDocumentCardPreview(ref, descriptor, opened); err != nil {
		logDocumentCardPreviewError("store", ref, err)
		ret.Code = -1
		ret.Msg = err.Error()
	}
}

func getDocumentCardPreview(c *gin.Context) {
	cacheKey := c.Param("cacheKey")
	cacheKey = strings.TrimSuffix(cacheKey, ".webp")
	filePath, err := model.DocumentCardPreviewFile(c.Param("notebook"), cacheKey)
	if err != nil {
		logging.LogErrorf("document card preview read failed [notebook=%q, cacheKey=%q]: %s",
			c.Param("notebook"), cacheKey, err)
		c.Status(http.StatusForbidden)
		return
	}
	c.Header("Cache-Control", "private, max-age=31536000, immutable")
	c.File(filePath)
}
