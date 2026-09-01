// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

package api

import (
	"errors"
	"net/http"
	"os"

	"github.com/88250/gulu"
	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func createMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var notebook, parentPath, name, operationID string
	var autoName bool
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("parentPath", &parentPath, true, false),
		util.BindJsonArg("name", &name, true, true),
		util.BindJsonArg("autoName", &autoName, false, false),
		util.BindJsonArg("operationID", &operationID, false, true),
	) {
		return
	}
	if util.InvalidIDPattern(notebook, ret) {
		return
	}

	document, err := model.CreateMarkdownWithOperationID(notebook, parentPath, name, autoName, operationID)
	ret.Data, _ = markdownResult(ret, document, err)
}

func getMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	notebook, p, ok := markdownPathArgs(c, ret)
	if !ok {
		return
	}
	document, err := model.GetMarkdown(notebook, p)
	ret.Data, _ = markdownResult(ret, document, err)
}

func saveMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var notebook, p, content, revision, operationID string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("content", &content, true, false),
		util.BindJsonArg("revision", &revision, true, true),
		util.BindJsonArg("operationID", &operationID, false, true),
	) {
		return
	}
	if util.InvalidIDPattern(notebook, ret) {
		return
	}
	document, err := model.SaveMarkdownWithOperationID(notebook, p, content, revision, operationID)
	ret.Data, _ = markdownResult(ret, document, err)
}

func ensureMarkdownDocumentIdentity(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var notebook, p, revision, operationID string
	var forceNew bool
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("revision", &revision, true, true),
		util.BindJsonArg("operationID", &operationID, true, true),
		util.BindJsonArg("forceNew", &forceNew, false, false),
	) || util.InvalidIDPattern(notebook, ret) {
		return
	}
	document, err := model.EnsureMarkdownDocumentIdentity(notebook, p, revision, operationID, forceNew)
	ret.Data, _ = markdownResult(ret, document, err)
}

func renameMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var notebook, p, name, revision, operationID string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("name", &name, true, true),
		util.BindJsonArg("revision", &revision, true, true),
		util.BindJsonArg("operationID", &operationID, false, true),
	) {
		return
	}
	if util.InvalidIDPattern(notebook, ret) {
		return
	}
	document, err := model.RenameMarkdownWithRevision(notebook, p, name, revision, operationID)
	ret.Data, _ = markdownResult(ret, document, err)
}

func duplicateMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var notebook, p, revision, operationID string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("revision", &revision, true, true),
		util.BindJsonArg("operationID", &operationID, false, true),
	) || util.InvalidIDPattern(notebook, ret) {
		return
	}
	document, err := model.DuplicateMarkdownWithOperationID(notebook, p, revision, operationID)
	ret.Data, _ = markdownResult(ret, document, err)
}

func moveMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var notebook, p, revision, toNotebook, toParentPath, operationID string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("revision", &revision, true, true),
		util.BindJsonArg("toNotebook", &toNotebook, true, true),
		util.BindJsonArg("toParentPath", &toParentPath, true, false),
		util.BindJsonArg("operationID", &operationID, false, true),
	) || util.InvalidIDPattern(notebook, ret) || util.InvalidIDPattern(toNotebook, ret) {
		return
	}
	document, err := model.MoveMarkdown(notebook, p, revision, toNotebook, toParentPath, operationID)
	ret.Data, _ = markdownResult(ret, document, err)
}

func removeMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var notebook, p, revision, operationID string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("revision", &revision, true, true),
		util.BindJsonArg("operationID", &operationID, false, true),
	) || util.InvalidIDPattern(notebook, ret) {
		return
	}
	entry, err := model.RecycleMarkdown(model.MarkdownDocumentRef{Notebook: notebook, Path: p}, revision, operationID)
	if err != nil {
		markdownError(ret, err)
		return
	}
	ret.Data = entry
}

func listDeletedMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	entries, err := model.ListDeletedMarkdown()
	if err != nil {
		markdownError(ret, err)
		return
	}
	ret.Data = entries
}

func getDeletedMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var id string
	if !util.ParseJsonArgs(arg, ret, util.BindJsonArg("id", &id, true, true)) {
		return
	}
	entry, data, err := model.GetDeletedMarkdown(id)
	if err != nil {
		markdownError(ret, err)
		return
	}
	ret.Data = map[string]any{"entry": entry, "content": string(data)}
}

func restoreDeletedMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var id, toNotebook, toParentPath, name, operationID string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("id", &id, true, true),
		util.BindJsonArg("toNotebook", &toNotebook, true, true),
		util.BindJsonArg("toParentPath", &toParentPath, true, false),
		util.BindJsonArg("name", &name, true, true),
		util.BindJsonArg("operationID", &operationID, false, true),
	) || util.InvalidIDPattern(toNotebook, ret) {
		return
	}
	document, err := model.RestoreDeletedMarkdown(id, toNotebook, toParentPath, name, operationID)
	ret.Data, _ = markdownResult(ret, document, err)
}

func purgeDeletedMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var id, requestedOperationID string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("id", &id, true, true),
		util.BindJsonArg("operationID", &requestedOperationID, false, true),
	) {
		return
	}
	operationID, err := model.ResolveMarkdownOperationID(requestedOperationID)
	if err != nil {
		markdownError(ret, err)
		return
	}
	if err = model.PurgeDeletedMarkdown(id, operationID); err != nil {
		markdownError(ret, err)
		return
	}
	ret.Data = map[string]any{"operationID": operationID}
}

func markdownPathArgs(c *gin.Context, ret *gulu.Result) (notebook, p string, ok bool) {
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return "", "", false
	}
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("path", &p, true, true),
	) || util.InvalidIDPattern(notebook, ret) {
		return "", "", false
	}
	return notebook, p, true
}

func markdownResult(ret *gulu.Result, data *model.MarkdownDocument, err error) (*model.MarkdownDocument, bool) {
	if err != nil {
		markdownError(ret, err)
		return nil, false
	}
	return data, true
}

func markdownError(ret *gulu.Result, err error) {
	ret.Code = -1
	if errors.Is(err, model.ErrMarkdownConflict) || errors.Is(err, os.ErrExist) {
		ret.Code = http.StatusConflict
	}
	ret.Msg = err.Error()
}
