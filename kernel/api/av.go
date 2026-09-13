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
	"fmt"
	"net/http"

	"github.com/88250/gulu"
	"github.com/gin-gonic/gin"
	goccyJSON "github.com/goccy/go-json"
	"github.com/siyuan-note/siyuan/kernel/av"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/treenode"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func getUnusedAttributeViews(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	unusedAttributeViews := model.UnusedAttributeViews(true)
	total := len(unusedAttributeViews)

	const maxUnusedAttributeViews = 512
	if total > maxUnusedAttributeViews {
		unusedAttributeViews = unusedAttributeViews[:maxUnusedAttributeViews]
		util.PushMsg(fmt.Sprintf(model.Conf.Language(279), total, maxUnusedAttributeViews), 5000)
	}

	ret.Data = unusedAttributeViews
}

func getAttributeViewItemIDsByBoundIDs(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}

	avID := arg["avID"].(string)
	blockIDsArg := arg["blockIDs"].([]any)
	var blockIDs []string
	for _, v := range blockIDsArg {
		blockIDs = append(blockIDs, v.(string))
	}

	ret.Data = model.GetAttributeViewItemIDs(avID, blockIDs)
}

func getAttributeViewBoundBlockIDsByItemIDs(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}

	avID := arg["avID"].(string)
	itemIDsArg := arg["itemIDs"].([]any)
	var itemIDs []string
	for _, v := range itemIDsArg {
		itemIDs = append(itemIDs, v.(string))
	}

	ret.Data = model.GetAttributeViewBoundBlockIDs(avID, itemIDs)
}

// getAttributeViewAddingBlockDefaultValues 用于获取添加块时的默认值。
// 存在过滤或分组条件时，添加块时需要填充默认值到过滤字段或分组字段中，前端需要调用该接口来获取这些默认值以便填充。
func getAttributeViewAddingBlockDefaultValues(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}

	avID := arg["avID"].(string)
	var viewID string
	if viewIDArg := arg["viewID"]; nil != viewIDArg {
		viewID = viewIDArg.(string)
	}
	var groupID string
	if groupIDArg := arg["groupID"]; nil != groupIDArg {
		groupID = groupIDArg.(string)
	}
	var previousID string
	if nil != arg["previousID"] {
		previousID = arg["previousID"].(string)
	}
	var addingBlockID string
	if nil != arg["addingBlockID"] {
		addingBlockID = arg["addingBlockID"].(string)
	}

	values := model.GetAttrViewAddingBlockDefaultValues(avID, viewID, groupID, previousID, addingBlockID)
	if 1 > len(values) {
		values = nil
	}
	ret.Data = map[string]any{
		"values": values,
	}
}

func getAttributeViewKeysByAvID(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	avID := arg["avID"].(string)
	ret.Data = model.GetAttributeViewKeysByID(avID)
}

func getAttributeViewKeysByID(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	avID := arg["avID"].(string)
	keyIDsArg := arg["keyIDs"].([]any)
	var keyIDs []string
	for _, v := range keyIDsArg {
		keyIDs = append(keyIDs, v.(string))
	}
	ret.Data = model.GetAttributeViewKeysByID(avID, keyIDs...)
}

func getMirrorDatabaseBlocks(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}

	avID := arg["avID"].(string)
	blockIDs := treenode.GetMirrorAttrViewBlockIDs(avID)
	var retRefDefs []model.RefDefs
	for _, blockID := range blockIDs {
		retRefDefs = append(retRefDefs, model.RefDefs{RefID: blockID, DefIDs: []string{}})
	}
	if 1 > len(retRefDefs) {
		retRefDefs = []model.RefDefs{}
	}

	ret.Data = map[string]any{
		"refDefs": retRefDefs,
	}
}

func getAttributeViewPrimaryKeyValues(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}

	id := arg["id"].(string)
	page := 1
	pageArg := arg["page"]
	if nil != pageArg {
		page = int(pageArg.(float64))
	}

	pageSize := -1
	pageSizeArg := arg["pageSize"]
	if nil != pageSizeArg {
		pageSize = int(pageSizeArg.(float64))
	}

	keyword := ""
	if keywordArg := arg["keyword"]; nil != keywordArg {
		keyword = keywordArg.(string)
	}
	attributeViewName, databaseBlockIDs, rows, err := model.GetAttributeViewPrimaryKeyValues(id, keyword, page, pageSize)
	if err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}

	ret.Data = map[string]any{
		"name":     attributeViewName,
		"blockIDs": databaseBlockIDs,
		"rows":     rows,
	}
}

func getAttributeViewFilterSort(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, _ := util.JsonArg(c, ret)
	if nil == arg {
		return
	}

	avID := arg["id"].(string)
	blockID := arg["blockID"].(string)

	filters, sorts := model.GetAttributeViewFilterSort(avID, blockID)
	ret.Data = map[string]any{
		"filters": filters,
		"sorts":   sorts,
	}
}

func searchAttributeViewRollupDestKeys(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, _ := util.JsonArg(c, ret)
	if nil == arg {
		return
	}

	avID := arg["avID"].(string)
	keyword := arg["keyword"].(string)

	rollupDestKeys := model.SearchAttributeViewRollupDestKeys(avID, keyword)
	ret.Data = map[string]any{
		"keys": rollupDestKeys,
	}
}

func searchAttributeViewRelationKey(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, _ := util.JsonArg(c, ret)
	if nil == arg {
		return
	}

	avID := arg["avID"].(string)
	keyword := arg["keyword"].(string)

	relationKeys := model.SearchAttributeViewRelationKey(avID, keyword)
	ret.Data = map[string]any{
		"keys": relationKeys,
	}
}

func getAttributeView(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, _ := util.JsonArg(c, ret)
	if nil == arg {
		return
	}

	id := arg["id"].(string)
	ret.Data = map[string]any{
		"av": model.GetAttributeView(id),
	}
}

func searchAttributeView(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, _ := util.JsonArg(c, ret)
	if nil == arg {
		return
	}

	keyword := arg["keyword"].(string)
	currentAvID := ""
	if nil != arg["avID"] {
		currentAvID = arg["avID"].(string)
	}
	currentBlockID := ""
	if nil != arg["blockID"] {
		currentBlockID = arg["blockID"].(string)
	}
	var excludes []string
	if nil != arg["excludes"] {
		for _, e := range arg["excludes"].([]any) {
			excludes = append(excludes, e.(string))
		}
	}
	results := model.SearchAttributeView(keyword, excludes, currentAvID, currentBlockID)
	ret.Data = map[string]any{
		"results": results,
	}
}

func renderSnapshotAttributeView(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}

	index := arg["snapshot"].(string)
	id := arg["id"].(string)
	view, attrView, err := model.RenderRepoSnapshotAttributeView(index, id)
	if err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}

	var views []*av.ViewData
	for _, v := range attrView.Views {
		views = append(views, &av.ViewData{
			ID:               v.ID,
			Icon:             v.Icon,
			Name:             v.Name,
			Desc:             v.Desc,
			HideAttrViewName: v.HideAttrViewName,
			Type:             v.LayoutType,
			PageSize:         v.PageSize,
		})
	}

	ret.Data = map[string]any{
		"name":              attrView.Name,
		"id":                attrView.ID,
		"viewType":          view.GetType(),
		"viewID":            view.GetID(),
		"views":             views,
		"view":              view,
		"isMirror":          av.IsMirror(attrView.ID),
		"newItemTemplates":  attrView.NewItemTemplates,
		"defaultTemplateID": attrView.DefaultTemplateID,
	}
}

func renderHistoryAttributeView(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}

	id := arg["id"].(string)
	created := arg["created"].(string)
	blockIDArg := arg["blockID"]
	var blockID string
	if nil != blockIDArg {
		blockID = blockIDArg.(string)
	}
	viewIDArg := arg["viewID"]
	var viewID string
	if nil != viewIDArg {
		viewID = viewIDArg.(string)
	}
	page := 1
	pageArg := arg["page"]
	if nil != pageArg {
		page = int(pageArg.(float64))
	}

	pageSize := -1
	pageSizeArg := arg["pageSize"]
	if nil != pageSizeArg {
		pageSize = int(pageSizeArg.(float64))
	}

	query := ""
	queryArg := arg["query"]
	if nil != queryArg {
		query = queryArg.(string)
	}

	groupPaging := map[string]any{}
	groupPagingArg := arg["groupPaging"]
	if nil != groupPagingArg {
		groupPaging = groupPagingArg.(map[string]any)
	}

	view, attrView, err := model.RenderHistoryAttributeView(blockID, id, viewID, query, page, pageSize, groupPaging, created)
	if err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}

	var views []*av.ViewData
	for _, v := range attrView.Views {
		views = append(views, &av.ViewData{
			ID:               v.ID,
			Icon:             v.Icon,
			Name:             v.Name,
			Desc:             v.Desc,
			HideAttrViewName: v.HideAttrViewName,
			Type:             v.LayoutType,
			PageSize:         v.PageSize,
		})
	}

	ret.Data = map[string]any{
		"name":              attrView.Name,
		"id":                attrView.ID,
		"viewType":          view.GetType(),
		"viewID":            view.GetID(),
		"views":             views,
		"view":              view,
		"isMirror":          av.IsMirror(attrView.ID),
		"newItemTemplates":  attrView.NewItemTemplates,
		"defaultTemplateID": attrView.DefaultTemplateID,
	}
}

func renderAttributeView(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		c.JSON(http.StatusOK, ret)
		return
	}

	id := arg["id"].(string)
	blockIDArg := arg["blockID"]
	var blockID string
	if nil != blockIDArg {
		blockID = blockIDArg.(string)
	}
	viewIDArg := arg["viewID"]
	var viewID string
	if nil != viewIDArg {
		viewID = viewIDArg.(string)
	}
	page := 1
	pageArg := arg["page"]
	if nil != pageArg {
		page = int(pageArg.(float64))
	}

	pageSize := -1
	pageSizeArg := arg["pageSize"]
	if nil != pageSizeArg {
		pageSize = int(pageSizeArg.(float64))
	}

	query := ""
	queryArg := arg["query"]
	if nil != queryArg {
		query = queryArg.(string)
	}

	groupPaging := map[string]any{}
	groupPagingArg := arg["groupPaging"]
	if nil != groupPagingArg {
		groupPaging = groupPagingArg.(map[string]any)
	}

	createIfNotExist := false

	ignoreRows := false
	ignoreRowsArg := arg["ignoreRows"]
	if nil != ignoreRowsArg {
		ignoreRows = ignoreRowsArg.(bool)
	}
	targetItemID := ""
	if targetItemIDArg := arg["targetItemID"]; nil != targetItemIDArg {
		targetItemID = targetItemIDArg.(string)
	}
	targetGroupID := ""
	if targetGroupIDArg := arg["targetGroupID"]; nil != targetGroupIDArg {
		targetGroupID = targetGroupIDArg.(string)
	}

	ret = renderAttrView(blockID, id, viewID, query, page, pageSize, groupPaging, createIfNotExist, ignoreRows, targetItemID, targetGroupID)
	if ret.Code == 0 && model.IsReadOnlyRoleContext(c) {
		publishAccess := model.GetPublishAccess()
		retDataMap := ret.Data.(map[string]any)
		retDataMap["view"] = model.FilterViewByPublishAccess(c, publishAccess, retDataMap["view"].(av.Viewable))
	}

	// 大体量响应（如全量数据库视图）用 goccy 序列化后直接写字节，跳过 gin 内部基于标准库的二次序列化
	marshalBytes, marshalErr := goccyJSON.Marshal(ret)
	if nil != marshalErr || 0 == len(marshalBytes) {
		c.JSON(http.StatusOK, ret)
		return
	}
	c.Data(http.StatusOK, "application/json; charset=utf-8", marshalBytes)
}

func renderAttrView(blockID, avID, viewID, query string, page, pageSize int, groupPaging map[string]any, createIfNotExist, ignoreRows bool, targetItemID, targetGroupID string) (ret *gulu.Result) {
	ret = gulu.Ret.NewResult()
	view, attrView, target, err := model.RenderAttributeViewWithTarget(blockID, avID, viewID, query, page, pageSize, groupPaging, createIfNotExist, ignoreRows, targetItemID, targetGroupID)
	if err != nil {
		ret.Code = -1
		if errors.Is(err, av.ErrSpecTooNew) {
			ret.Msg = model.Conf.Language(215)
		} else {
			ret.Msg = err.Error()
		}
		return
	}

	var views []*av.ViewData
	for _, v := range attrView.Views {
		views = append(views, &av.ViewData{
			ID:               v.ID,
			Icon:             v.Icon,
			Name:             v.Name,
			Desc:             v.Desc,
			HideAttrViewName: v.HideAttrViewName,
			Type:             v.LayoutType,
			PageSize:         v.PageSize,
		})
	}

	retData := map[string]any{
		"name":              attrView.Name,
		"id":                attrView.ID,
		"viewType":          view.GetType(),
		"viewID":            view.GetID(),
		"views":             views,
		"view":              view,
		"isMirror":          av.IsMirror(attrView.ID),
		"newItemTemplates":  attrView.NewItemTemplates,
		"defaultTemplateID": attrView.DefaultTemplateID,
	}
	if nil != target {
		retData["target"] = target
	}
	ret.Data = retData
	return
}

func getCurrentAttrViewImages(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}

	id := arg["id"].(string)
	viewIDArg := arg["viewID"]
	var viewID string
	if nil != viewIDArg {
		viewID = viewIDArg.(string)
	}

	query := ""
	queryArg := arg["query"]
	if nil != queryArg {
		query = queryArg.(string)
	}

	images, err := model.GetCurrentAttributeViewImages(id, viewID, query)
	if err != nil {
		ret.Code = -1
		ret.Msg = err.Error()
		return
	}
	ret.Data = images
}

func getAttributeViewKeys(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}

	id, _ := arg["id"].(string)
	avID, _ := arg["avID"].(string)
	itemID, _ := arg["itemID"].(string)
	valueID, _ := arg["valueID"].(string)
	var blockAttributeViewKeys []*model.BlockAttributeViewKeys
	if "" != avID && ("" != itemID || "" != valueID) {
		blockAttributeViewKeys = model.GetAttributeViewItemKeys(avID, itemID, valueID)
	} else {
		blockAttributeViewKeys = model.GetBlockAttributeViewKeys(id)
	}
	if model.IsReadOnlyRoleContext(c) {
		publishAccess := model.GetPublishAccess()
		blockAttributeViewKeys = model.FilterBlockAttributeViewKeysByPublishAccess(c, publishAccess, blockAttributeViewKeys)
	}
	ret.Data = blockAttributeViewKeys
}

func getAttributeViewBacklinks(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)

	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}

	id, _ := arg["id"].(string)
	avID, _ := arg["avID"].(string)
	itemID, _ := arg["itemID"].(string)
	valueID, _ := arg["valueID"].(string)
	backlinks := model.GetAttributeViewBacklinks(id, avID, itemID, valueID)
	if model.IsReadOnlyRoleContext(c) {
		publishAccess := model.GetPublishAccess()
		backlinks = model.FilterAttributeViewBacklinksByPublishAccess(c, publishAccess, backlinks)
	}
	ret.Data = backlinks
}
