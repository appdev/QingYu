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

package mcp

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/conf"
	"github.com/siyuan-note/siyuan/kernel/mcp/tools"
	"github.com/siyuan-note/siyuan/kernel/model"
)

func TestReadOnlyToolListFiltersActions(t *testing.T) {
	toolList := allowedTools(true)
	for _, tool := range toolList {
		if tool.Name != "document" {
			continue
		}
		action := tool.InputSchema.Properties["action"]
		for _, value := range action.Enum {
			if value == "create" || value == "delete" || value == "move" {
				t.Fatalf("write action %q remains in read-only document schema", value)
			}
		}
		return
	}
	t.Fatal("document tool missing from read-only tool list")
}

func TestReadOnlyToolCallPermissions(t *testing.T) {
	tests := []struct {
		name    string
		tool    string
		action  string
		allowed bool
	}{
		{name: "document read", tool: "document", action: "get", allowed: true},
		{name: "document write", tool: "document", action: "create", allowed: false},
		{name: "sql query", tool: "sql", action: "query", allowed: true},
		{name: "legacy sql query", tool: "sql", allowed: true},
		{name: "unzip", tool: "unzip", allowed: false},
		{name: "unknown action", tool: "document", action: "unknown", allowed: false},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := isToolCallAllowed(tools.LookupTool(test.tool), map[string]any{"action": test.action}, true); got != test.allowed {
				t.Fatalf("isToolCallAllowed() = %v, want %v", got, test.allowed)
			}
		})
	}
}

func TestPluginReadOnlyHint(t *testing.T) {
	readOnlyPlugin := &tools.Tool{Name: "plugin__test__read", Source: "plugin", ReadOnlyHint: true}
	writePlugin := &tools.Tool{Name: "plugin__test__write", Source: "plugin"}
	if !isToolCallAllowed(readOnlyPlugin, nil, true) {
		t.Fatal("read-only plugin tool should be allowed")
	}
	if isToolCallAllowed(writePlugin, nil, true) {
		t.Fatal("plugin tool without read-only hint should be denied")
	}
}

func TestMCPEnabledMiddleware(t *testing.T) {
	originalConf := model.Conf
	defer func() { model.Conf = originalConf }()
	model.Conf = model.NewAppConf()
	model.Conf.Api = conf.NewAPI()
	model.Conf.Api.MCP = conf.NewMCP(false, true)

	gin.SetMode(gin.TestMode)
	engine := gin.New()
	engine.GET("/", checkEnabled, func(c *gin.Context) {
		c.Status(http.StatusNoContent)
	})

	recorder := httptest.NewRecorder()
	engine.ServeHTTP(recorder, httptest.NewRequest(http.MethodGet, "/", nil))
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("disabled MCP status = %d, want %d", recorder.Code, http.StatusForbidden)
	}

	model.Conf.Api.MCP.Enabled = true
	recorder = httptest.NewRecorder()
	engine.ServeHTTP(recorder, httptest.NewRequest(http.MethodGet, "/", nil))
	if recorder.Code != http.StatusNoContent {
		t.Fatalf("enabled MCP status = %d, want %d", recorder.Code, http.StatusNoContent)
	}
}
