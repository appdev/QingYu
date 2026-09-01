// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package api

import (
	"net/http"

	"github.com/88250/gulu"
	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func listNotebookRootDocuments(c *gin.Context) {
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
	listing, err := model.ListNotebookRootDocuments(notebook)
	if err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	if model.IsReadOnlyRoleContext(c) {
		publishIgnore := model.GetInvisiblePublishAccess(model.GetPublishAccess())
		visible := make([]*model.NotebookRootDocument, 0, len(listing.Documents))
		for _, document := range listing.Documents {
			if document.Kind == "markdown" {
				continue
			}
			if model.CheckPathAccessableByPublishIgnore(notebook, document.Path, publishIgnore) {
				visible = append(visible, document)
			}
		}
		listing.Documents = visible
	}
	ret.Data = listing
}
