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
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/conf"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/treenode"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func setupFileTreeAPIBox(t *testing.T) string {
	t.Helper()
	originalConf, originalDataDir, originalBlockTreeDBPath := model.Conf, util.DataDir, util.BlockTreeDBPath
	util.DataDir = filepath.Join(t.TempDir(), "data")
	util.BlockTreeDBPath = filepath.Join(filepath.Dir(util.DataDir), "blocktree.db")
	model.Conf = model.NewAppConf()
	model.Conf.Sync = conf.NewSync()
	model.Conf.FileTree = conf.NewFileTree()
	model.Conf.NotebookCrypto = conf.NewNotebookCrypto()
	treenode.InitBlockTree(true)
	box := &model.Box{ID: "20260811000000-abcdefg"}
	boxConf := conf.NewBoxConf()
	boxConf.Name = "File tree API test"
	boxConf.Closed = false
	if err := box.SaveConf(boxConf); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		treenode.CloseDatabase()
		model.Conf, util.DataDir, util.BlockTreeDBPath = originalConf, originalDataDir, originalBlockTreeDBPath
		if originalBlockTreeDBPath != "" {
			treenode.InitBlockTree(false)
		}
	})
	return box.ID
}

func TestChangeSortPersistsMixedNativeAndMarkdownKeys(t *testing.T) {
	boxID := setupFileTreeAPIBox(t)
	for _, name := range []string{"20260811000001-bcdefgh.sy", "a.md"} {
		if err := os.WriteFile(filepath.Join(util.DataDir, boxID, name), []byte("fixture"), 0644); err != nil {
			t.Fatal(err)
		}
	}
	ret := postMarkdownHandler(t, changeSort,
		`{"notebook":"`+boxID+`","paths":["/20260811000001-bcdefgh.sy","/a.md"]}`)
	if code, _ := ret["code"].(float64); code != 0 {
		t.Fatalf("mixed sort was rejected: %#v", ret)
	}
	data, err := os.ReadFile(filepath.Join(util.DataDir, boxID, ".siyuan", "sort.json"))
	if err != nil {
		t.Fatal(err)
	}
	var sorts map[string]int
	if err = json.Unmarshal(data, &sorts); err != nil {
		t.Fatal(err)
	}
	if sorts["20260811000001-bcdefgh"] != 1 || sorts["markdown:/a.md"] != 2 {
		t.Fatalf("unexpected mixed sort keys: %#v", sorts)
	}
}

func TestChangeSortEchoesMarkdownOperationID(t *testing.T) {
	boxID := setupFileTreeAPIBox(t)
	if err := os.WriteFile(filepath.Join(util.DataDir, boxID, "a.md"), []byte("fixture"), 0644); err != nil {
		t.Fatal(err)
	}
	ret := postMarkdownHandler(t, changeSort,
		`{"notebook":"`+boxID+`","paths":["/a.md"],"operationID":"sort-client"}`)
	data, _ := ret["data"].(map[string]any)
	if ret["code"] != float64(0) || data["operationID"] != "sort-client" {
		t.Fatalf("operation ID was not echoed: %#v", ret)
	}
}
