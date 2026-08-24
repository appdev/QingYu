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

package model

import (
	"errors"
	"os"
	"path"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/88250/lute/parse"
	"github.com/siyuan-note/siyuan/kernel/cache"
	"github.com/siyuan-note/siyuan/kernel/conf"
	"github.com/siyuan-note/siyuan/kernel/filesys"
	"github.com/siyuan-note/siyuan/kernel/treenode"
	"github.com/siyuan-note/siyuan/kernel/util"
)

type fileOperationTestFixture struct {
	box        *Box
	sourcePath string
	targetPath string
	sourceID   string
	childID    string
}

func setupFileOperationTest(t *testing.T) *fileOperationTestFixture {
	originalConf := Conf
	originalDataDir := util.DataDir
	originalBlockTreeDBPath := util.BlockTreeDBPath
	originalTimeLangs := util.TimeLangs
	tempDir := t.TempDir()
	util.DataDir = filepath.Join(tempDir, "data")
	util.BlockTreeDBPath = filepath.Join(tempDir, "blocktree.db")
	Conf = NewAppConf()
	Conf.Lang = "en"
	Conf.Sync = conf.NewSync()
	Conf.FileTree = conf.NewFileTree()
	Conf.NotebookCrypto = conf.NewNotebookCrypto()
	util.TimeLangs = map[string]map[string]any{"en": {
		"albl": "ago", "blbl": "from now", "now": "now", "1s": "1 second", "xs": "%d seconds",
		"1m": "1 minute", "xh": "%d hours", "1h": "1 hour", "1d": "1 day", "xd": "%d days",
		"1w": "1 week", "xw": "%d weeks", "1M": "1 month", "xM": "%d months", "1y": "1 year",
		"2y": "2 years", "xy": "%d years", "max": "long ago",
	}}

	box := &Box{ID: "20260718000000-abcdefg"}
	boxConf := conf.NewBoxConf()
	boxConf.Name = "File operation test"
	boxConf.Closed = false
	if err := box.SaveConf(boxConf); err != nil {
		t.Fatalf("save test notebook conf failed: %v", err)
	}

	treenode.InitBlockTree(true)
	sourcePath := "/20260718000001-abcdefg.sy"
	targetPath := "/20260718000002-abcdefg.sy"
	sourceTree := treenode.NewTree(box.ID, sourcePath, "/Source", "Source")
	targetTree := treenode.NewTree(box.ID, targetPath, "/Target", "Target")
	for _, tree := range []*parse.Tree{sourceTree, targetTree} {
		if _, err := filesys.WriteTree(tree); err != nil {
			t.Fatalf("write test tree failed: %v", err)
		}
		treenode.UpsertBlockTree(tree)
	}

	t.Cleanup(func() {
		cache.RemoveTreeData(sourceTree.ID)
		cache.RemoveTreeData(targetTree.ID)
		cache.RemoveDocIAL(sourceTree.Path)
		cache.RemoveDocIAL(targetTree.Path)
		treenode.CloseDatabase()
		Conf = originalConf
		util.DataDir = originalDataDir
		util.BlockTreeDBPath = originalBlockTreeDBPath
		util.TimeLangs = originalTimeLangs
		if "" != originalBlockTreeDBPath {
			treenode.InitBlockTree(false)
		}
	})

	return &fileOperationTestFixture{
		box:        box,
		sourcePath: sourcePath,
		targetPath: targetPath,
		sourceID:   sourceTree.ID,
		childID:    sourceTree.Root.FirstChild.ID,
	}
}

func TestListDocTreeRunsMarkdownRecoveryGate(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "recovery-gate.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "save" && phase == "staged" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	if _, err = SaveMarkdown(box.ID, created.Path, "recovered", created.Revision); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if _, _, err = ListDocTree(box.ID, "/", util.SortModeNameASC, false, 100); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(util.DataDir, box.ID, "recovery-gate.md"))
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "" {
		t.Fatalf("first list committed a pre-installed transaction: %q", data)
	}
}

func TestRemoveDocRejectsInvalidPath(t *testing.T) {
	fixture := setupFileOperationTest(t)

	if err := RemoveDoc(fixture.box.ID, "/_REPRO_FLAT"); !errors.Is(err, ErrBlockNotFound) {
		t.Fatalf("expected invalid document path to return ErrBlockNotFound, got [%v]", err)
	}
}

func TestGetBoxesByPathsStrictRejectsInvalidPaths(t *testing.T) {
	fixture := setupFileOperationTest(t)
	tests := []struct {
		name  string
		paths []string
	}{
		{name: "empty", paths: nil},
		{name: "hpath", paths: []string{"/_REPRO_TEST/Sub_Note"}},
		{name: "hpath with extension", paths: []string{"/_REPRO_FLAT.sy"}},
		{name: "wrong parent", paths: []string{"/20260718000003-abcdefg/" + fixture.sourceID + ".sy"}},
		{name: "parent traversal", paths: []string{"/../" + fixture.sourceID + ".sy"}},
		{name: "child block", paths: []string{"/" + fixture.childID + ".sy"}},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if _, err := getBoxesByPathsStrict(test.paths); !errors.Is(err, ErrBlockNotFound) {
				t.Fatalf("expected invalid document paths [%v] to return ErrBlockNotFound, got [%v]", test.paths, err)
			}
		})
	}

	if _, err := getBoxesByPathsStrict([]string{strings.TrimPrefix(fixture.sourcePath, "/")}); err != nil {
		t.Fatalf("expected document path without leading slash to remain supported, got [%v]", err)
	}
}

func TestMoveDocsRejectsInvalidPathsBeforeMoving(t *testing.T) {
	fixture := setupFileOperationTest(t)
	newPath := path.Join(strings.TrimSuffix(fixture.targetPath, ".sy"), fixture.sourceID+".sy")
	tests := []struct {
		name      string
		fromPaths []string
	}{
		{name: "hpath", fromPaths: []string{"/_REPRO_TEST/Sub_Note"}},
		{name: "mixed", fromPaths: []string{fixture.sourcePath, "/_REPRO_TEST/Sub_Note"}},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if err := MoveDocs(test.fromPaths, fixture.box.ID, fixture.targetPath, nil); !errors.Is(err, ErrBlockNotFound) {
				t.Fatalf("expected invalid source paths [%v] to return ErrBlockNotFound, got [%v]", test.fromPaths, err)
			}
			if !fixture.box.Exist(fixture.sourcePath) {
				t.Fatalf("source document was moved for invalid source paths [%v]", test.fromPaths)
			}
			if fixture.box.Exist(newPath) {
				t.Fatalf("target document was created for invalid source paths [%v]", test.fromPaths)
			}
		})
	}
}

func TestSortSearchDocResults(t *testing.T) {
	results := []searchDocResult{
		{data: map[string]string{"hPath": "A/初中数学"}},
		{data: map[string]string{"hPath": "Z/数学"}, exact: true},
		{data: map[string]string{"hPath": "A/数学/"}, exact: true},
		{data: map[string]string{"hPath": "B/高等数学"}},
	}

	sortSearchDocResults(results)
	expected := []string{"A/数学/", "Z/数学", "A/初中数学", "B/高等数学"}
	for i, hPath := range expected {
		if hPath != results[i].data["hPath"] {
			t.Fatalf("unexpected search result order at %d: got %q, want %q", i, results[i].data["hPath"], hPath)
		}
	}
}

func TestChangeFileTreeSortKeepsMarkdownPathsSeparateFromBlockIDs(t *testing.T) {
	fixture := setupFileOperationTest(t)
	markdownPath := "/notes.md"
	if err := os.WriteFile(filepath.Join(util.DataDir, fixture.box.ID, "notes.md"), []byte("notes"), 0644); err != nil {
		t.Fatal(err)
	}

	ChangeFileTreeSort(fixture.box.ID, []string{fixture.sourcePath, markdownPath})
	conf, err := readSortConfMap(filepath.Join(util.DataDir, fixture.box.ID, ".siyuan", "sort.json"))
	if err != nil {
		t.Fatal(err)
	}
	if conf[fixture.sourceID] != 1 || conf["markdown:/notes.md"] != 2 {
		t.Fatalf("unexpected sort map: %#v", conf)
	}
}

func TestChangeFileTreeSortPublishesMarkdownDirectoryEvent(t *testing.T) {
	fixture := setupFileOperationTest(t)
	markdownPath := "/notes.md"
	if err := os.WriteFile(filepath.Join(util.DataDir, fixture.box.ID, "notes.md"), []byte("notes"), 0644); err != nil {
		t.Fatal(err)
	}
	var events []*util.Result
	originalPushEvent := markdownPushEvent
	markdownPushEvent = func(event *util.Result) { events = append(events, event) }
	t.Cleanup(func() { markdownPushEvent = originalPushEvent })

	ChangeFileTreeSort(fixture.box.ID, []string{fixture.sourcePath, markdownPath}, "sort-client")

	if len(events) != 1 || events[0].Cmd != "sortMarkdown" {
		t.Fatalf("unexpected Markdown sort events: %#v", events)
	}
	data := events[0].Data.(map[string]any)
	if data["kind"] != "markdown" || data["box"] != fixture.box.ID || data["path"] != markdownPath ||
		data["operationID"] != "sort-client" {
		t.Fatalf("incomplete Markdown sort envelope: %#v", data)
	}
}

func TestChangeFileTreeSortSerializesWithMarkdownCreation(t *testing.T) {
	fixture := setupFileOperationTest(t)
	if err := os.WriteFile(filepath.Join(util.DataDir, fixture.box.ID, "a.md"), []byte("a"), 0644); err != nil {
		t.Fatal(err)
	}
	entered, release := make(chan struct{}), make(chan struct{})
	originalHook := markdownBeforeChangeSortCommit
	markdownBeforeChangeSortCommit = func() {
		close(entered)
		<-release
	}
	t.Cleanup(func() { markdownBeforeChangeSortCommit = originalHook })
	sortDone := make(chan error, 1)
	go func() {
		_, err := ChangeFileTreeSortWithOperationID(fixture.box.ID, []string{"/a.md"}, "sort-client")
		sortDone <- err
	}()
	<-entered
	createDone := make(chan error, 1)
	go func() {
		_, err := CreateMarkdown(fixture.box.ID, "/", "b.md")
		createDone <- err
	}()
	select {
	case err := <-createDone:
		t.Fatalf("Markdown creation was not serialized with sort: %v", err)
	case <-time.After(100 * time.Millisecond):
	}
	close(release)
	if err := <-sortDone; err != nil {
		t.Fatal(err)
	}
	if err := <-createDone; err != nil {
		t.Fatal(err)
	}
	sorts, err := readSortConfMap(filepath.Join(util.DataDir, fixture.box.ID, ".siyuan", "sort.json"))
	if err != nil {
		t.Fatal(err)
	}
	if sorts["markdown:/b.md"] == 0 {
		t.Fatalf("concurrent Markdown sort entry was lost: %#v", sorts)
	}
}

func TestListDocTreeUsesMixedCustomSortKeys(t *testing.T) {
	fixture := setupFileOperationTest(t)
	if err := os.WriteFile(filepath.Join(util.DataDir, fixture.box.ID, "notes.md"), []byte("notes"), 0644); err != nil {
		t.Fatal(err)
	}
	confPath := filepath.Join(util.DataDir, fixture.box.ID, ".siyuan", "sort.json")
	if err := writeSortConfMap(confPath, map[string]int{
		fixture.sourceID:                   1,
		"markdown:/notes.md":               2,
		util.GetTreeID(fixture.targetPath): 3,
	}); err != nil {
		t.Fatal(err)
	}

	docs, _, err := ListDocTree(fixture.box.ID, "/", util.SortModeCustom, false, 100)
	if err != nil {
		t.Fatal(err)
	}
	if len(docs) < 2 || docs[0].ID != fixture.sourceID || docs[1].Path != "/notes.md" {
		t.Fatalf("unexpected mixed custom order: %#v", docs)
	}
}

func TestSearchDocTextMatching(t *testing.T) {
	exactCases := []struct {
		name          string
		value         string
		keyword       string
		caseSensitive bool
		expected      bool
	}{
		{name: "same case sensitive", value: "Math", keyword: "Math", caseSensitive: true, expected: true},
		{name: "different case sensitive", value: "Math", keyword: "math", caseSensitive: true, expected: false},
		{name: "different case insensitive", value: "Math", keyword: "math", caseSensitive: false, expected: true},
	}
	for _, test := range exactCases {
		t.Run("exact/"+test.name, func(t *testing.T) {
			if actual := isExactSearchDocMatch(test.value, test.keyword, test.caseSensitive); test.expected != actual {
				t.Fatalf("unexpected exact match result: got %t, want %t", actual, test.expected)
			}
		})
	}

	containsCases := []struct {
		name          string
		value         string
		keywords      []string
		caseSensitive bool
		expected      bool
	}{
		{name: "same case sensitive", value: "Math Notes", keywords: []string{"Math"}, caseSensitive: true, expected: true},
		{name: "different case sensitive", value: "Math Notes", keywords: []string{"math"}, caseSensitive: true, expected: false},
		{name: "different case insensitive", value: "Math Notes", keywords: []string{"math"}, caseSensitive: false, expected: true},
		{name: "preserve any keyword matching", value: "Math Notes", keywords: []string{"missing", "notes"}, caseSensitive: false, expected: true},
	}
	for _, test := range containsCases {
		t.Run("contains/"+test.name, func(t *testing.T) {
			if actual := containsSearchDocKeyword(test.value, test.keywords, test.caseSensitive); test.expected != actual {
				t.Fatalf("unexpected contains result: got %t, want %t", actual, test.expected)
			}
		})
	}
}

func TestBuildSearchDocsCondition(t *testing.T) {
	condition, args := buildSearchDocsCondition([]string{"O'Reilly", "100%_done\\file"}, []string{"20260720000000-abc_def"}, true, true, true)
	if strings.Contains(condition, "O'Reilly") || strings.Contains(condition, "100%_done") {
		t.Fatalf("search condition should contain placeholders instead of keywords: %q", condition)
	}
	if placeholderCount := strings.Count(condition, "?"); placeholderCount != len(args) {
		t.Fatalf("search condition placeholder/arg mismatch: %d placeholders, %d args", placeholderCount, len(args))
	}

	expectedArgs := []string{
		"%O'Reilly%", "%O'Reilly%", "%O'Reilly%", "%O'Reilly%",
		"%100\\%\\_done\\\\file%", "%100\\%\\_done\\\\file%", "%100\\%\\_done\\\\file%", "%100\\%\\_done\\\\file%",
		"%20260720000000-abc\\_def%",
	}
	for i, expected := range expectedArgs {
		if actual := args[i].(string); expected != actual {
			t.Fatalf("unexpected search arg at %d: got %q, want %q", i, actual, expected)
		}
	}
}

func TestBuildSearchDocsConditionBindsInjectionPayload(t *testing.T) {
	payload := "poc%')/**/union/**/select/**/'poc'--"
	condition, args := buildSearchDocsCondition([]string{payload}, nil, true, true, true)
	if strings.Contains(condition, payload) || strings.Contains(strings.ToLower(condition), "union") {
		t.Fatalf("search condition should not contain payload SQL: %q", condition)
	}

	expected := "%" + escapeSearchDocLikePattern(payload) + "%"
	if len(args) != 4 {
		t.Fatalf("unexpected search arg count: got %d, want 4", len(args))
	}
	for i, arg := range args {
		if actual := arg.(string); expected != actual {
			t.Fatalf("unexpected search arg at %d: got %q, want %q", i, actual, expected)
		}
	}
}
