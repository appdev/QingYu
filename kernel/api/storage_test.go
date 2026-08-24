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
	"os"
	"path/filepath"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestRecentDocTimestampHandlersRejectAmbiguousOrIncompleteIdentity(t *testing.T) {
	setupFileTreeAPIBox(t)
	handlers := []struct {
		name    string
		handler gin.HandlerFunc
	}{
		{name: "open", handler: updateRecentDocOpenTime},
		{name: "view", handler: updateRecentDocViewTime},
		{name: "close", handler: updateRecentDocCloseTime},
	}
	invalidBodies := []string{
		`{}`,
		`{"rootID":"20260811000001-bcdefgh","kind":"markdown","notebook":"20260811000000-abcdefg","path":"/a.md"}`,
		`{"kind":"markdown","notebook":"20260811000000-abcdefg"}`,
		`{"kind":"markdown","path":"/a.md"}`,
		`{"kind":"native","notebook":"20260811000000-abcdefg","path":"/a.md"}`,
	}
	for _, handler := range handlers {
		for _, body := range invalidBodies {
			t.Run(handler.name+"/"+body, func(t *testing.T) {
				assertMarkdownHandlerRejects(t, handler.handler, body)
			})
		}
	}
}

func TestRecentDocTimestampHandlersAcceptMarkdownIdentity(t *testing.T) {
	tests := []struct {
		name    string
		handler gin.HandlerFunc
		sortBy  string
	}{
		{name: "open", handler: updateRecentDocOpenTime, sortBy: "openAt"},
		{name: "view", handler: updateRecentDocViewTime, sortBy: "viewedAt"},
		{name: "close", handler: updateRecentDocCloseTime, sortBy: "closedAt"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			boxID := setupFileTreeAPIBox(t)
			model.Conf.FileTree.RecentDocsMaxListCount = 32
			if err := os.WriteFile(filepath.Join(util.DataDir, boxID, "a.md"), []byte("a"), 0644); err != nil {
				t.Fatal(err)
			}
			ret := postMarkdownHandler(t, test.handler,
				`{"kind":"markdown","notebook":"`+boxID+`","path":"/a.md"}`)
			if code, _ := ret["code"].(float64); code != 0 {
				t.Fatalf("Markdown timestamp update was rejected: %#v", ret)
			}
			docs, err := model.GetRecentDocs(test.sortBy)
			if err != nil {
				t.Fatal(err)
			}
			if len(docs) != 1 || docs[0].Kind != "markdown" || docs[0].Path != "/a.md" {
				t.Fatalf("Markdown timestamp was not recorded: %#v", docs)
			}
		})
	}
}

func TestRecentDocTimestampHandlersKeepNativeRequestShape(t *testing.T) {
	setupFileTreeAPIBox(t)
	ret := postMarkdownHandler(t, updateRecentDocOpenTime, `{"rootID":"20260811000001-bcdefgh"}`)
	if code, _ := ret["code"].(float64); code != 0 {
		t.Fatalf("legacy native timestamp request was rejected: %#v", ret)
	}
}
