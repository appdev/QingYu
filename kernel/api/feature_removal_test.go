package api

import (
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

func TestRemovedProductRoutesAndPreservedBoundaries(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	ServeAPI(engine)
	routes := map[string]bool{}
	for _, route := range engine.Routes() {
		routes[route.Path] = true
		for _, prefix := range []string{"/api/ai/", "/api/agent/", "/api/riff/", "/api/account/", "/api/inbox/", "/api/graph/"} {
			if strings.HasPrefix(route.Path, prefix) {
				t.Fatalf("removed route remains registered: %s", route.Path)
			}
		}
	}
	for _, removed := range []string{
		"/api/filetree/createDailyNote", "/api/block/appendDailyNoteBlock", "/api/block/prependDailyNoteBlock",
		"/api/cloud/getCloudSpace", "/api/cloud/setCloudReminder",
		"/api/setting/getCloudUser", "/api/setting/logoutCloudUser", "/api/setting/login2faCloudUser",
		"/api/asset/uploadCloud", "/api/asset/uploadCloudByAssetsPaths",
		"/api/repo/purgeCloudRepo", "/api/repo/getCloudRepoTagSnapshots", "/api/repo/getCloudRepoSnapshots",
		"/api/repo/removeCloudRepoTagSnapshot", "/api/repo/uploadCloudSnapshot", "/api/repo/downloadCloudSnapshot",
	} {
		if routes[removed] {
			t.Fatalf("removed official-cloud route remains registered: %s", removed)
		}
	}
	for _, preserved := range []string{
		"/api/sync/setSyncProviderS3", "/api/sync/setSyncProviderWebDAV", "/api/sync/setSyncProviderLocal",
		"/api/system/loginAuth", "/api/system/setAPIToken", "/api/system/setMCP",
	} {
		if !routes[preserved] {
			t.Fatalf("preserved route is missing: %s", preserved)
		}
	}
}
