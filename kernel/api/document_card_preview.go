// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package api

import (
	"net/http"
	"strings"

	"github.com/88250/gulu"
	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/util"
)

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
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	var ref model.DocumentCardReference
	if err = gulu.JSON.UnmarshalJSON(data, &ref); err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	descriptor, err := model.PrepareDocumentCardPreview(ref, theme, appearanceKey, size)
	if err != nil {
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
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	if err := gulu.JSON.UnmarshalJSON([]byte(c.PostForm("descriptor")), &descriptor); err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	file, err := c.FormFile("file")
	if err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	opened, err := file.Open()
	if err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	defer opened.Close()
	if err = model.StoreDocumentCardPreview(ref, descriptor, opened); err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
	}
}

func getDocumentCardPreview(c *gin.Context) {
	cacheKey := c.Param("cacheKey")
	cacheKey = strings.TrimSuffix(cacheKey, ".webp")
	filePath, err := model.DocumentCardPreviewFile(c.Param("notebook"), cacheKey)
	if err != nil {
		c.Status(http.StatusForbidden)
		return
	}
	c.Header("Cache-Control", "private, max-age=31536000, immutable")
	c.File(filePath)
}
