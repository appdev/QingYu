// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package api

import (
	"errors"
	"net/http"

	"github.com/88250/gulu"
	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func getNotebookHome(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var notebook string
	if !util.ParseJsonArgs(arg, ret, util.BindJsonArg("notebook", &notebook, true, true)) || util.InvalidIDPattern(notebook, ret) {
		return
	}
	if model.IsReadOnlyRoleContext(c) &&
		!model.CheckNotebookHomeAccessableByPublishAccess(c, model.GetPublishAccess(), notebook) {
		ret.Code = http.StatusForbidden
		ret.Msg = http.StatusText(http.StatusForbidden)
		return
	}
	document, err := model.GetNotebookHome(notebook)
	if err != nil {
		notebookHomeError(ret, err)
		return
	}
	ret.Data = document
}

func saveNotebookHome(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var notebook, content, revision, operationID string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("content", &content, true, false),
		util.BindJsonArg("revision", &revision, true, true),
		util.BindJsonArg("operationID", &operationID, false, true),
	) || util.InvalidIDPattern(notebook, ret) {
		return
	}
	document, err := model.SaveNotebookHome(notebook, content, revision, operationID)
	if err != nil {
		notebookHomeError(ret, err)
		return
	}
	ret.Data = document
}

func notebookHomeError(ret *gulu.Result, err error) {
	ret.Code = -1
	if errors.Is(err, model.ErrNotebookHomeConflict) {
		ret.Code = http.StatusConflict
	}
	ret.Msg = err.Error()
}
