package mcp_test

import (
	"net/http"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/mcp"
	"github.com/siyuan-note/siyuan/kernel/mcp/tools"
)

func TestMCPServerBoundary(t *testing.T) {
	if mcp.ServerName != "QingYu" {
		t.Fatalf("MCP server name = %q, want QingYu", mcp.ServerName)
	}
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	mcp.Serve(engine)
	foundPost := false
	for _, route := range engine.Routes() {
		foundPost = foundPost || route.Method == http.MethodPost && route.Path == "/mcp"
	}
	if !foundPost {
		t.Fatal("POST /mcp must remain registered")
	}
	for _, preserved := range []string{"notebook", "document", "markdown", "block", "database", "search", "sync"} {
		if tools.LookupTool(preserved) == nil {
			t.Fatalf("preserved MCP tool is missing: %s", preserved)
		}
	}
	for _, removed := range []string{"dailynote", "inbox", "image", "frontend", "question", "todo_write", "skill", "http_request", "web_fetch", "web_search"} {
		if tools.LookupTool(removed) != nil {
			t.Fatalf("removed MCP tool remains: %s", removed)
		}
	}
}
