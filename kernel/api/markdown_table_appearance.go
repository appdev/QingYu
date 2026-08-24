// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package api

import (
	"net/http"

	"github.com/88250/gulu"
	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func getMarkdownTableAppearance(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var documentKey string
	if !util.ParseJsonArgs(arg, ret, util.BindJsonArg("documentKey", &documentKey, true, true)) {
		return
	}
	document, err := model.GetMarkdownTableAppearance(documentKey)
	if err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	ret.Data = document
}

func patchMarkdownTableAppearance(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var documentKey, tableID, origin string
	var patchRaw any
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("documentKey", &documentKey, true, true),
		util.BindJsonArg("tableID", &tableID, true, true),
		util.BindJsonArg("origin", &origin, false, true),
		util.BindJsonArg("patch", &patchRaw, true, false),
	) {
		return
	}
	data, err := gulu.JSON.MarshalJSON(patchRaw)
	if err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	patch := model.MarkdownTableAppearancePatch{}
	if err = gulu.JSON.UnmarshalJSON(data, &patch); err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	result, err := model.PatchMarkdownTableAppearance(documentKey, tableID, patch)
	if err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	ret.Data = result
	event := util.NewCmdResult("markdownTableAppearance", 0, util.PushModeBroadcast)
	event.Data = map[string]any{
		"documentKey": documentKey,
		"origin":      origin,
		"record":      result.Record,
		"revision":    result.DocumentRevision,
	}
	util.PushEvent(event)
}

func migrateMarkdownTableAppearance(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var fromKey, toKey string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("fromKey", &fromKey, true, true),
		util.BindJsonArg("toKey", &toKey, true, true),
	) {
		return
	}
	if err := model.MigrateMarkdownTableAppearanceDocument(fromKey, toKey); err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
	}
}

func removeMarkdownTableAppearance(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var documentKey string
	if !util.ParseJsonArgs(arg, ret, util.BindJsonArg("documentKey", &documentKey, true, true)) {
		return
	}
	if err := model.RemoveMarkdownTableAppearanceDocument(documentKey); err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
	}
}
