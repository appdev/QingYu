package util

import (
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/gin-gonic/gin"
)

func TestQingYuKernelIdentityDefaults(t *testing.T) {
	if FixedPort != "9806" {
		t.Fatalf("FixedPort = %q, want 9806", FixedPort)
	}
	if UserAgent != "QingYu/"+Ver {
		t.Fatalf("UserAgent = %q, want QingYu/%s", UserAgent, Ver)
	}
}

func TestQingYuWorkspaceHistoryUsesIsolatedConfig(t *testing.T) {
	oldHomeDir := HomeDir
	HomeDir = t.TempDir()
	t.Cleanup(func() { HomeDir = oldHomeDir })

	workspace := filepath.Join(HomeDir, "workspace")
	for _, dir := range []string{workspace, filepath.Join(HomeDir, ".config", "qingyu"), filepath.Join(HomeDir, ".config", "siyuan")} {
		if err := os.MkdirAll(dir, 0755); err != nil {
			t.Fatal(err)
		}
	}
	if err := WriteWorkspacePaths([]string{workspace}); err != nil {
		t.Fatal(err)
	}

	qingYuHistory := filepath.Join(HomeDir, ".config", "qingyu", "workspace.json")
	if _, err := os.Stat(qingYuHistory); err != nil {
		t.Fatalf("QingYu workspace history was not written to the isolated config: %v", err)
	}
	legacyHistory := filepath.Join(HomeDir, ".config", "siyuan", "workspace.json")
	if _, err := os.Stat(legacyHistory); !os.IsNotExist(err) {
		t.Fatalf("legacy SiYuan workspace history was modified: %v", err)
	}

	paths, err := ReadWorkspacePaths()
	if err != nil {
		t.Fatal(err)
	}
	if len(paths) != 1 || paths[0] != workspace {
		t.Fatalf("ReadWorkspacePaths() = %v, want [%s]", paths, workspace)
	}
}

func TestQingYuNativeUserAgentIsNotTreatedAsBrowser(t *testing.T) {
	gin.SetMode(gin.TestMode)
	c, _ := gin.CreateTestContext(httptest.NewRecorder())
	c.Request = httptest.NewRequest("GET", "/", nil)
	c.Request.Header.Set("User-Agent", "QingYu/3.7.3 Electron")
	if IsBrowserRequest(c) {
		t.Fatal("QingYu native User-Agent was treated as a browser")
	}

	c.Request.Header.Set("User-Agent", "SiYuan/3.7.3 Electron")
	if IsBrowserRequest(c) {
		t.Fatal("legacy SiYuan native User-Agent compatibility was removed")
	}

	c.Request.Header.Set("User-Agent", "Mozilla/5.0")
	if !IsBrowserRequest(c) {
		t.Fatal("browser User-Agent was treated as a native app")
	}
}
