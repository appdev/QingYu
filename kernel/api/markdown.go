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
	var notebook, parentPath, name string
	var autoName bool
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("parentPath", &parentPath, true, false),
		util.BindJsonArg("name", &name, true, true),
		util.BindJsonArg("autoName", &autoName, false, false),
	) {
		return
	}
	if util.InvalidIDPattern(notebook, ret) {
		return
	}

	document, err := model.CreateMarkdown(notebook, parentPath, name, autoName)
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
	var notebook, p, content, revision string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("content", &content, true, false),
		util.BindJsonArg("revision", &revision, true, true),
	) {
		return
	}
	if util.InvalidIDPattern(notebook, ret) {
		return
	}
	document, err := model.SaveMarkdown(notebook, p, content, revision)
	ret.Data, _ = markdownResult(ret, document, err)
}

func renameMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var notebook, p, name string
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("notebook", &notebook, true, true),
		util.BindJsonArg("path", &p, true, true),
		util.BindJsonArg("name", &name, true, true),
	) {
		return
	}
	if util.InvalidIDPattern(notebook, ret) {
		return
	}
	document, err := model.RenameMarkdown(notebook, p, name)
	ret.Data, _ = markdownResult(ret, document, err)
}

func removeMarkdown(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	notebook, p, ok := markdownPathArgs(c, ret)
	if !ok {
		return
	}
	if err := model.RemoveMarkdown(notebook, p); err != nil {
		markdownError(ret, err)
	}
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
	if errors.Is(err, model.ErrMarkdownConflict) {
		ret.Code = http.StatusConflict
	}
	ret.Msg = err.Error()
}
