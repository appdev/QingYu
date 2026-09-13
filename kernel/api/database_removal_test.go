package api

import (
	"testing"

	"github.com/gin-gonic/gin"
)

func TestDatabaseRoutesAreReadOnly(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	ServeAPI(engine)
	routes := map[string]bool{}
	for _, route := range engine.Routes() {
		routes[route.Path] = true
	}
	if routes["/api/query/sql"] {
		t.Error("SQL query route remains registered")
	}
	for _, route := range []string{"/api/bazaar/installBazaarWidget", "/api/bazaar/uninstallBazaarWidget", "/api/search/searchWidget"} {
		if routes[route] {
			t.Errorf("widget operation remains registered: %s", route)
		}
	}
	for _, name := range []string{
		"setAttributeViewBlockAttr", "batchSetAttributeViewBlockAttrs", "setAttrViewFilters", "setAttrViewSorts",
		"addAttributeViewKey", "removeAttributeViewKey", "sortAttributeViewViewKey", "sortAttributeViewKey",
		"addAttributeViewBlocks", "removeAttributeViewBlocks", "setDatabaseBlockView", "duplicateAttributeViewBlock",
		"appendAttributeViewDetachedBlocksWithValues", "changeAttrViewLayout", "setAttrViewGroup",
		"batchReplaceAttributeViewBlocks", "createAttributeViewItem", "removeUnusedAttributeViews", "removeUnusedAttributeView",
	} {
		if routes["/api/av/"+name] {
			t.Errorf("database write route remains: %s", name)
		}
	}
	if routes["/api/history/rollbackAttributeViewHistory"] {
		t.Error("database history can still be restored")
	}
	for _, name := range []string{"renderAttributeView", "renderHistoryAttributeView", "renderSnapshotAttributeView",
		"getAttributeViewKeys", "getAttributeViewBacklinks", "getCurrentAttrViewImages", "getMirrorDatabaseBlocks"} {
		if !routes["/api/av/"+name] {
			t.Errorf("database read route missing: %s", name)
		}
	}
	for _, name := range []string{"create", "get", "save", "move", "rename"} {
		if !routes["/api/markdown/"+name] {
			t.Errorf("Markdown route missing: %s", name)
		}
	}
}
