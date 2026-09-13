package api

import (
	"encoding/json"
	"net/http/httptest"
	"strings"
	"testing"
)

import (
	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/model"
)

func TestNativeEmbedRequiresStoredBlock(t *testing.T) {
	setupFileTreeAPIBox(t)
	gin.SetMode(gin.TestMode)
	recorder := httptest.NewRecorder()
	context, _ := gin.CreateTestContext(recorder)
	context.Set(model.RoleContextKey, model.RoleAdministrator)
	context.Request = httptest.NewRequest("POST", "/api/search/searchEmbedBlock", strings.NewReader(
		`{"embedBlockID":"20260911000000-missing","stmt":"select 1","excludeIDs":[]}`))
	context.Request.Header.Set("Content-Type", "application/json")
	searchEmbedBlock(context)
	var result struct {
		Code int `json:"code"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &result); err != nil {
		t.Fatal(err)
	}
	if result.Code == 0 {
		t.Fatal("arbitrary SQL accepted without a stored embed block")
	}
}

func TestNativeDocumentCreationRoutesRemoved(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	ServeAPI(engine)
	routes := map[string]bool{}
	for _, route := range engine.Routes() {
		routes[route.Path] = true
	}
	for _, name := range []string{"createDoc", "createDocWithMd", "duplicateDoc", "heading2Doc", "li2Doc"} {
		if routes["/api/filetree/"+name] {
			t.Errorf("native creation remains available: %s", name)
		}
	}
	for _, route := range []string{"/api/block/updateBlock", "/api/block/insertBlock", "/api/transactions", "/api/markdown/create"} {
		if !routes[route] {
			t.Errorf("existing editing or Markdown creation missing: %s", route)
		}
	}
}
