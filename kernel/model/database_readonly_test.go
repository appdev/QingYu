package model

import (
	"bytes"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/88250/lute/ast"
	"github.com/88250/lute/parse"
	"github.com/siyuan-note/siyuan/kernel/av"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestDatabaseWritePayload(t *testing.T) {
	for _, data := range []string{
		`<div data-type="NodeAttributeView" data-av-id="20260910000000-abcdefg"></div>`,
		`<div data-type='NodeAttributeView'></div>`,
		`<div av-id="20260910000000-abcdefg"/>`,
		`{"custom-avs":"20260910000000-abcdefg"}`,
		`{"custom-sy-av-view":"20260910000000-abcdefg"}`,
	} {
		if !databaseWritePayload(data) {
			t.Errorf("database payload accepted: %s", data)
		}
	}
	for _, data := range []string{`<div data-type="NodeTable"><table><tr><td>note</td></tr></table></div>`,
		`<div data-type="NodeParagraph">NodeAttributeView</div>`,
		`&lt;div data-type="NodeAttributeView"&gt;`, `{"name":"notes"}`} {
		if databaseWritePayload(data) {
			t.Errorf("ordinary content rejected: %s", data)
		}
	}
}

func TestDatabaseTransactionsRejectedBeforeBegin(t *testing.T) {
	for _, op := range []*Operation{{Action: "updateAttrViewCell"}, {Action: "setAttrs", AvID: "database"},
		{Action: "insert", Data: `<div data-type="NodeAttributeView"></div>`}} {
		// 没有事务环境也必须先返回只读错误，不能先执行同批普通写操作。
		tx := &Transaction{DoOperations: []*Operation{{Action: "update", Data: "normal"}, op}}
		if err := performTx(tx); err == nil || err.msg != ErrDatabaseReadOnly.Error() {
			t.Fatalf("unexpected transaction result: %v", err)
		}
	}
	root := &ast.Node{Type: ast.NodeDocument}
	root.AppendChild(&ast.Node{Type: ast.NodeAttributeView})
	if err := checkDatabaseOperations([]*Operation{{Action: "restoreCreatedDoc", Tree: &parse.Tree{Root: root}}}); !errors.Is(err, ErrDatabaseReadOnly) {
		t.Fatalf("database replay accepted: %v", err)
	}
	if err := checkDatabaseOperations([]*Operation{{Action: "create", Data: &parse.Tree{Root: root}}}); !errors.Is(err, ErrDatabaseReadOnly) {
		t.Fatalf("database document duplication accepted: %v", err)
	}
}

func TestDatabaseBatchRejectedBeforeQueueing(t *testing.T) {
	before := txQueueSize()
	transactions := []*Transaction{
		{DoOperations: []*Operation{{Action: "update", Data: "normal"}}},
		{DoOperations: []*Operation{{Action: "updateAttrViewCell"}}},
	}
	if err := PerformTransactions(&transactions); !errors.Is(err, ErrDatabaseReadOnly) {
		t.Fatalf("unexpected batch result: %v", err)
	}
	if txQueueSize() != before {
		t.Fatal("part of a rejected batch was queued")
	}
}

func TestDatabaseDocumentCreationRejectedBeforeWriting(t *testing.T) {
	dom := `<div data-type="NodeAttributeView"></div>`
	if _, err := createDoc("", "", "", dom, false); !errors.Is(err, ErrDatabaseReadOnly) {
		t.Fatalf("database document accepted: %v", err)
	}
	if _, err := createDocsByHPath("", "", dom, "", "", false); !errors.Is(err, ErrDatabaseReadOnly) {
		t.Fatalf("database path creation accepted: %v", err)
	}
}

func TestDatabaseRenderDoesNotCreateMissingDatabase(t *testing.T) {
	previous := util.DataDir
	util.DataDir = t.TempDir()
	t.Cleanup(func() { util.DataDir = previous })
	id := "20260910000000-abcdefg"
	_, _, err := RenderAttributeView("", id, "", "", 1, 50, nil, true, false)
	if !errors.Is(err, av.ErrAttributeViewNotFound) {
		t.Fatalf("unexpected missing database result: %v", err)
	}
	if _, err := os.Stat(filepath.Join(util.DataDir, "storage", "av", id+".json")); !os.IsNotExist(err) {
		t.Fatalf("render created a database: %v", err)
	}
}

func TestDatabaseExistingViewsRenderWithoutWriting(t *testing.T) {
	previous := util.DataDir
	previousLangs := util.AttrViewLangs
	previousConf := Conf
	Conf = &AppConf{Lang: "en"}
	util.DataDir = t.TempDir()
	util.AttrViewLangs = map[string]map[string]any{util.Lang: {
		"table": "Table", "gallery": "Gallery", "kanban": "Kanban", "key": "Name", "select": "Select",
	}}
	t.Cleanup(func() {
		util.DataDir = previous
		util.AttrViewLangs = previousLangs
		Conf = previousConf
	})
	database := av.NewAttributeView("20260910000000-readold")
	database.Name = "Archived database"
	database.Views[0].Table.Columns = append(database.Views[0].Table.Columns,
		&av.ViewTableColumn{BaseField: &av.BaseField{ID: "missing-field"}})
	database.Views = append(database.Views, av.NewGalleryView(), av.NewKanbanView())
	data, err := json.Marshal(database)
	if err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(util.DataDir, "storage", "av", database.ID+".json")
	if err = os.MkdirAll(filepath.Dir(file), 0755); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(file, data, 0644); err != nil {
		t.Fatal(err)
	}
	for _, view := range database.Views {
		t.Run(string(view.LayoutType), func(t *testing.T) {
			rendered, _, err := RenderAttributeView("", database.ID, view.ID, "", 1, 50, nil, false, false)
			if err != nil || rendered == nil {
				t.Fatalf("cannot read archived view: %v", err)
			}
			after, err := os.ReadFile(file)
			if err != nil || !bytes.Equal(data, after) {
				t.Fatalf("reading a view rewrote its database: %v", err)
			}
		})
	}
}

func TestDatabaseDocumentDuplicationPreservesOriginal(t *testing.T) {
	root := &ast.Node{Type: ast.NodeDocument, ID: "20260910000000-original"}
	root.AppendChild(&ast.Node{Type: ast.NodeAttributeView})
	tree := &parse.Tree{ID: root.ID, Root: root}
	if err := DuplicateDoc(tree); !errors.Is(err, ErrDatabaseReadOnly) {
		t.Fatalf("database duplication accepted: %v", err)
	}
	if tree.ID != "20260910000000-original" || root.ID != tree.ID {
		t.Fatal("rejected duplication changed the original identity")
	}
}
