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
	"bytes"
	"encoding/json"
	"errors"
	"os"
	"path"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/siyuan-note/filelock"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func setupMarkdownManagementTest(t *testing.T) *Box {
	t.Helper()
	box := setupMarkdownTest(t)
	originalHistoryDir := util.HistoryDir
	util.HistoryDir = filepath.Join(filepath.Dir(util.DataDir), "history")
	t.Cleanup(func() {
		util.HistoryDir = originalHistoryDir
	})
	return box
}

func writeMarkdownManagementFixture(t *testing.T, boxID, p string, data []byte) string {
	t.Helper()
	_, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.MkdirAll(filepath.Dir(absPath), 0755); err != nil {
		t.Fatal(err)
	}
	if err = filelock.WriteFile(absPath, data); err != nil {
		t.Fatal(err)
	}
	return absPath
}

func symlinkOrSkip(t *testing.T, oldname, newname string) {
	t.Helper()
	if err := os.Symlink(oldname, newname); err != nil {
		t.Skipf("symlink is not available: %v", err)
	}
}

func mutateMarkdownTrashManifest(t *testing.T, mutate func(*MarkdownTrashEntry)) {
	t.Helper()
	manifests, err := filepath.Glob(filepath.Join(util.HistoryDir, "*-delete", "markdown.json"))
	if err != nil || len(manifests) != 1 {
		t.Fatalf("unexpected manifests %v: %v", manifests, err)
	}
	data, err := filelock.ReadFile(manifests[0])
	if err != nil {
		t.Fatal(err)
	}
	var entries []*MarkdownTrashEntry
	if err = json.Unmarshal(data, &entries); err != nil {
		t.Fatal(err)
	}
	mutate(entries[0])
	data, err = json.Marshal(entries)
	if err != nil {
		t.Fatal(err)
	}
	if err = filelock.WriteFile(manifests[0], data); err != nil {
		t.Fatal(err)
	}
}

func TestMarkdownRecentKeyIsNamespacedAndCanonical(t *testing.T) {
	box := setupMarkdownTest(t)
	ref, err := CanonicalMarkdownRef(box.ID, "notes/a.md")
	if err != nil {
		t.Fatal(err)
	}
	if got, want := MarkdownRecentKey(ref), "markdown:"+box.ID+":/notes/a.md"; got != want {
		t.Fatalf("got %q, want %q", got, want)
	}
}

func TestCanonicalMarkdownRefRejectsUnsafePaths(t *testing.T) {
	box := setupMarkdownTest(t)
	for _, p := range []string{"../a.md", "/../../a.md", "/a.txt"} {
		t.Run(p, func(t *testing.T) {
			if _, err := CanonicalMarkdownRef(box.ID, p); err == nil {
				t.Fatalf("unsafe Markdown path %q was accepted", p)
			}
		})
	}
}

func TestCanonicalMarkdownRefRejectsSymlinkNotebookRoot(t *testing.T) {
	box := setupMarkdownTest(t)
	boxRoot := filepath.Join(util.DataDir, box.ID)
	if err := os.RemoveAll(boxRoot); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.MkdirAll(outside, 0755); err != nil {
		t.Fatal(err)
	}
	symlinkOrSkip(t, outside, boxRoot)
	if _, err := CanonicalMarkdownRef(box.ID, "/a.md"); !errors.Is(err, ErrInvalidMarkdownPath) {
		t.Fatalf("symlink notebook root was accepted: %v", err)
	}
}

func TestRecycleMarkdownRejectsSymlinkHistoryRoot(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.MkdirAll(outside, 0755); err != nil {
		t.Fatal(err)
	}
	symlinkOrSkip(t, outside, util.HistoryDir)
	if _, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data)); !errors.Is(err, ErrInvalidMarkdownHistory) {
		t.Fatalf("symlink history root was accepted: %v", err)
	}
	entries, err := os.ReadDir(outside)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 0 {
		t.Fatalf("history escaped through symlink root: %v", entries)
	}
}

func TestRecycleMarkdownRejectsHistoryRootReplacedAfterCheck(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	if err := os.MkdirAll(util.HistoryDir, 0755); err != nil {
		t.Fatal(err)
	}
	originalHistory := util.HistoryDir + "-original"
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.MkdirAll(outside, 0755); err != nil {
		t.Fatal(err)
	}
	originalHook := markdownAfterRootComponentCheck
	markdownAfterRootComponentCheck = func(checkedPath string) error {
		if checkedPath != util.HistoryDir {
			return nil
		}
		if err := os.Rename(util.HistoryDir, originalHistory); err != nil {
			return err
		}
		return os.Symlink(outside, util.HistoryDir)
	}
	t.Cleanup(func() { markdownAfterRootComponentCheck = originalHook })
	if _, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data)); !errors.Is(err, ErrInvalidMarkdownHistory) {
		t.Fatalf("replaced history root was accepted: %v", err)
	}
	entries, err := os.ReadDir(outside)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 0 {
		t.Fatalf("history escaped replaced root: %v", entries)
	}
}

func TestGetDeletedMarkdownRejectsSymlinkHistoryFile(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	historyPath := filepath.Join(util.HistoryDir, filepath.FromSlash(entry.HistoryPath))
	if err = os.Remove(historyPath); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(t.TempDir(), "outside.md")
	if err = os.WriteFile(outside, data, 0644); err != nil {
		t.Fatal(err)
	}
	symlinkOrSkip(t, outside, historyPath)
	if _, _, err = GetDeletedMarkdown(entry.ID); !errors.Is(err, ErrInvalidMarkdownHistory) {
		t.Fatalf("symlink history payload was accepted: %v", err)
	}
}

func TestListDeletedMarkdownRejectsSymlinkManifest(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	manifestPath := filepath.Join(util.HistoryDir, entry.ID, "markdown.json")
	manifest, err := os.ReadFile(manifestPath)
	if err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(t.TempDir(), "markdown.json")
	if err = os.WriteFile(outside, manifest, 0644); err != nil {
		t.Fatal(err)
	}
	if err = os.Remove(manifestPath); err != nil {
		t.Fatal(err)
	}
	symlinkOrSkip(t, outside, manifestPath)
	if _, err = ListDeletedMarkdown(); !errors.Is(err, ErrInvalidMarkdownHistory) {
		t.Fatalf("symlink manifest was accepted: %v", err)
	}
}

func TestFileTreeSortKeyNamespacesMarkdownPaths(t *testing.T) {
	if got, want := fileTreeSortKey("notes/a.md"), "markdown:/notes/a.md"; got != want {
		t.Fatalf("got %q, want %q", got, want)
	}
	if got, want := fileTreeSortKey("/20260820000000-abcdefg.sy"), "20260820000000-abcdefg"; got != want {
		t.Fatalf("got %q, want %q", got, want)
	}
}

func TestRecycleMarkdownPreservesBytesBeforeDelete(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	original := []byte{0xef, 0xbb, 0xbf, '#', ' ', 'x', '\r', '\n'}
	absPath := writeMarkdownManagementFixture(t, box.ID, "/a.md", original)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(original))
	if err != nil {
		t.Fatal(err)
	}
	if filelock.IsExist(absPath) {
		t.Fatal("source still exists")
	}
	gotEntry, got, err := GetDeletedMarkdown(entry.ID)
	if err != nil {
		t.Fatal(err)
	}
	if gotEntry.OriginalPath != "/a.md" || !bytes.Equal(got, original) {
		t.Fatalf("deleted Markdown changed: entry=%#v bytes=%x", gotEntry, got)
	}
}

func TestRecycleMarkdownStagesBeforeRevisionValidation(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	source := filepath.Join(util.DataDir, box.ID, "a.md")
	originalHook := markdownAfterRecycleStage
	markdownAfterRecycleStage = func(_, staging string) error {
		if _, err := os.Stat(source); !os.IsNotExist(err) {
			t.Fatalf("source still visible after staging: %v", err)
		}
		return os.WriteFile(staging, []byte("changed"), 0644)
	}
	t.Cleanup(func() { markdownAfterRecycleStage = originalHook })
	if _, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data)); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected revision conflict, got %v", err)
	}
	if got, err := os.ReadFile(source); err != nil || string(got) != "changed" {
		t.Fatalf("staged bytes were not restored: %q, %v", got, err)
	}
}

func TestRecycleMarkdownPreservesStagingWhenRollbackTargetOccupied(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	source := filepath.Join(util.DataDir, box.ID, "a.md")
	var staging string
	originalHook := markdownAfterRecycleStage
	markdownAfterRecycleStage = func(_, stagingPath string) error {
		staging = stagingPath
		if err := os.WriteFile(source, []byte("competitor"), 0600); err != nil {
			return err
		}
		return errors.New("after-stage failure")
	}
	t.Cleanup(func() { markdownAfterRecycleStage = originalHook })
	if _, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data)); !errors.Is(err, os.ErrExist) {
		t.Fatalf("rollback collision was hidden: %v", err)
	}
	if got, err := os.ReadFile(source); err != nil || string(got) != "competitor" {
		t.Fatalf("rollback overwrote competitor: %q, %v", got, err)
	}
	if got, err := os.ReadFile(staging); err != nil || !bytes.Equal(got, data) {
		t.Fatalf("staging recovery bytes missing: %q, %v", got, err)
	}
	if _, err := os.Stat(filepath.Join(filepath.Dir(staging), "transaction.json")); err != nil {
		t.Fatalf("recovery journal missing: %v", err)
	}
}

func TestRecycleMarkdownRecoversStagedTransaction(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/recycle-crash.md", data)
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "recycle" && phase == "staged" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	ref := MarkdownDocumentRef{Notebook: box.ID, Path: "/recycle-crash.md"}
	if _, err := RecycleMarkdown(ref, markdownRevision(data)); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err := RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	if got, err := os.ReadFile(filepath.Join(util.DataDir, box.ID, "recycle-crash.md")); err != nil || !bytes.Equal(got, data) {
		t.Fatalf("recycle source was not recovered: %q, %v", got, err)
	}
}

func TestRecycleMarkdownRecoveryFinishesMetadataCommit(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("committed delete")
	writeMarkdownManagementFixture(t, box.ID, "/committed-delete.md", data)
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "recycle" && phase == "metadata-committed" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	ref := MarkdownDocumentRef{Notebook: box.ID, Path: "/committed-delete.md"}
	if _, err := RecycleMarkdown(ref, markdownRevision(data)); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err := RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(util.DataDir, box.ID, "committed-delete.md")); !os.IsNotExist(err) {
		t.Fatalf("logically deleted source was restored: %v", err)
	}
	entries, err := ListDeletedMarkdown()
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 || entries[0].OriginalPath != ref.Path {
		t.Fatalf("trash commit was lost: %#v", entries)
	}
}

func TestRecycleMarkdownKeepsHistoryWhenCommittedSourceCleanupFails(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("committed cleanup failure")
	writeMarkdownManagementFixture(t, box.ID, "/cleanup-failure.md", data)
	originalHook := markdownIdentityDeleteHook
	markdownIdentityDeleteHook = func(filePath string) error {
		if strings.Contains(filePath, "markdown-transactions") && strings.HasSuffix(filePath, "payload.md") {
			return errors.New("source cleanup failed")
		}
		return nil
	}
	t.Cleanup(func() { markdownIdentityDeleteHook = originalHook })
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/cleanup-failure.md"}, markdownRevision(data))
	if err != nil {
		t.Fatalf("logical delete reported cleanup failure: %v", err)
	}
	if entry == nil {
		t.Fatal("missing committed trash entry")
	}
	entries, err := ListDeletedMarkdown()
	if err == nil {
		t.Fatal("active cleanup fault should keep recovery pending")
	}
	markdownIdentityDeleteHook = originalHook
	entries, err = ListDeletedMarkdown()
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 || entries[0].ID != entry.ID {
		t.Fatalf("committed history was removed: %#v", entries)
	}
}

func TestRecycleMarkdownSyncsHistoryPayloadParentBeforeMetadataCommit(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("history durability")
	writeMarkdownManagementFixture(t, box.ID, "/history-durable.md", data)
	originalDurabilityHook := markdownDurabilityHook
	originalWriteHook := markdownTransactionWriteHook
	historyParentSynced := false
	markdownDurabilityHook = func(event, _ string) {
		if event == "history-payload-parent-synced" {
			historyParentSynced = true
		}
	}
	markdownTransactionWriteHook = func(kind, phase string) error {
		if kind == "recycle" && phase == "metadata-committed" && !historyParentSynced {
			return errors.New("metadata committed before history payload parent sync")
		}
		return nil
	}
	t.Cleanup(func() {
		markdownDurabilityHook = originalDurabilityHook
		markdownTransactionWriteHook = originalWriteHook
	})
	if _, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/history-durable.md"}, markdownRevision(data)); err != nil {
		t.Fatal(err)
	}
}

func TestRecycleMarkdownKeepsSourceWhenManifestWriteFails(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	original := []byte("# source\n")
	absPath := writeMarkdownManagementFixture(t, box.ID, "/a.md", original)
	originalWriter := markdownHistoryWriteFile
	markdownHistoryWriteFile = func(string, []byte) error { return errors.New("write failed") }
	t.Cleanup(func() { markdownHistoryWriteFile = originalWriter })

	if _, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(original)); err == nil {
		t.Fatal("expected manifest write failure")
	}
	if !filelock.IsExist(absPath) {
		t.Fatal("source must survive a failed backup")
	}
}

func TestRecycleAndRestoreMarkdownPreserveMode(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Windows does not expose portable Unix permission bits")
	}
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	source := filepath.Join(util.DataDir, box.ID, "a.md")
	if err := os.Chmod(source, 0440); err != nil {
		t.Fatal(err)
	}
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	if entry.Mode != 0440 {
		t.Fatalf("manifest mode = %#o", entry.Mode)
	}
	restored, err := RestoreDeletedMarkdown(entry.ID, box.ID, "/", "restored.md")
	if err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(filepath.Join(util.DataDir, box.ID, strings.TrimPrefix(restored.Path, "/")))
	if err != nil {
		t.Fatal(err)
	}
	if got := info.Mode().Perm(); got != 0440 {
		t.Fatalf("restored mode = %#o", got)
	}
}

func TestPurgeDeletedMarkdownSucceedsAfterLogicalCommitWhenCleanupFails(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	originalRemove := markdownHistoryRemove
	markdownHistoryRemove = func(string) error { return errors.New("remove failed") }
	t.Cleanup(func() { markdownHistoryRemove = originalRemove })
	var events []*util.Result
	originalPushEvent := markdownPushEvent
	markdownPushEvent = func(event *util.Result) { events = append(events, event) }
	t.Cleanup(func() { markdownPushEvent = originalPushEvent })
	if err = PurgeDeletedMarkdown(entry.ID, "purge-cleanup"); err != nil {
		t.Fatalf("cleanup failure changed logical result: %v", err)
	}
	markdownHistoryRemove = originalRemove
	if _, _, err = GetDeletedMarkdown(entry.ID); !os.IsNotExist(err) {
		t.Fatalf("logically purged entry remained: %v", err)
	}
	if len(events) != 1 || events[0].Cmd != "purgeMarkdown" || events[0].Data.(map[string]any)["operationID"] != "purge-cleanup" {
		t.Fatalf("logical purge did not broadcast success: %#v", events)
	}
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatalf("orphan cleanup recovery failed: %v", err)
	}
	orphans, err := filepath.Glob(filepath.Join(util.HistoryDir, "*-delete", "*.purging-*"))
	if err != nil || len(orphans) != 0 {
		t.Fatalf("purge tombstone orphan remained: %v, %v", orphans, err)
	}
}

func TestPurgeDeletedMarkdownRecoversCrashAfterTombstone(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "purge" && phase == "tombstoned" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	if err = PurgeDeletedMarkdown(entry.ID); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	if _, got, err := GetDeletedMarkdown(entry.ID); err != nil || !bytes.Equal(got, data) {
		t.Fatalf("tombstoned payload was not restored: %q, %v", got, err)
	}
}

func TestPurgeDeletedMarkdownRestoresPayloadWhenManifestUpdateFails(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	originalRename := markdownHistoryRename
	markdownHistoryRename = func(oldPath, newPath string) error {
		if strings.HasSuffix(oldPath, ".tmp") && strings.HasSuffix(newPath, "markdown.json") {
			return errors.New("manifest update failed")
		}
		return filelock.Rename(oldPath, newPath)
	}
	if err = PurgeDeletedMarkdown(entry.ID); err == nil {
		t.Fatal("expected manifest update failure")
	}
	markdownHistoryRename = originalRename
	t.Cleanup(func() { markdownHistoryRename = originalRename })
	if _, got, getErr := GetDeletedMarkdown(entry.ID); getErr != nil || !bytes.Equal(got, data) {
		t.Fatalf("payload was not restored: %q, %v", got, getErr)
	}
}

func TestPurgeDeletedMarkdownRecoveryHandlesManifestAndRollbackFailure(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/combined.md", data)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/combined.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	originalRename := markdownHistoryRename
	originalRenameNoReplace := markdownHistoryRenameNoReplace
	markdownHistoryRename = func(oldPath, newPath string) error {
		if strings.HasSuffix(oldPath, ".tmp") && strings.HasSuffix(newPath, "markdown.json") {
			return errors.New("manifest update failed")
		}
		return originalRename(oldPath, newPath)
	}
	markdownHistoryRenameNoReplace = func(oldPath, newPath string) error {
		if strings.Contains(oldPath, ".purging-") && strings.HasSuffix(newPath, "combined.md") {
			return errors.New("rollback rename failed")
		}
		return originalRenameNoReplace(oldPath, newPath)
	}
	t.Cleanup(func() {
		markdownHistoryRename = originalRename
		markdownHistoryRenameNoReplace = originalRenameNoReplace
	})
	if err = PurgeDeletedMarkdown(entry.ID); err == nil {
		t.Fatal("expected combined purge failure")
	}
	markdownHistoryRename = originalRename
	markdownHistoryRenameNoReplace = originalRenameNoReplace
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	if _, got, getErr := GetDeletedMarkdown(entry.ID); getErr != nil || !bytes.Equal(got, data) {
		t.Fatalf("combined failure was not recovered: %q, %v", got, getErr)
	}
}

func TestRestoreDeletedMarkdownRejectsExistingTarget(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	original := []byte("# deleted\n")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", original)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(original))
	if err != nil {
		t.Fatal(err)
	}
	writeMarkdownManagementFixture(t, box.ID, "/a.md", []byte("# collision\n"))
	if _, err = RestoreDeletedMarkdown(entry.ID, box.ID, "/", "a.md"); !errors.Is(err, os.ErrExist) {
		t.Fatalf("expected restore collision, got %v", err)
	}
	got, err := filelock.ReadFile(filepath.Join(util.DataDir, box.ID, "a.md"))
	if err != nil || string(got) != "# collision\n" {
		t.Fatalf("restore overwrote target: %q, %v", got, err)
	}
}

func TestRestoreDeletedMarkdownTargetValidationDoesNotDeleteReplacement(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("deleted")
	writeMarkdownManagementFixture(t, box.ID, "/restore-validation.md", data)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/restore-validation.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(util.DataDir, box.ID, "restored.md")
	originalHook := markdownInstallValidationHook
	markdownInstallValidationHook = func(kind, targetPath string) error {
		if kind != "restore" {
			return nil
		}
		if err := os.Remove(targetPath); err != nil {
			return err
		}
		return os.WriteFile(targetPath, []byte("replacement"), 0644)
	}
	t.Cleanup(func() { markdownInstallValidationHook = originalHook })
	if _, err = RestoreDeletedMarkdown(entry.ID, box.ID, "/", "restored.md"); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected identity conflict, got %v", err)
	}
	got, err := os.ReadFile(target)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "replacement" {
		t.Fatalf("replacement was deleted: %q", got)
	}
}

func TestRestoreDeletedMarkdownRejectsCorruptHistory(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	original := []byte("# deleted\n")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", original)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(original))
	if err != nil {
		t.Fatal(err)
	}
	if err = filelock.WriteFile(filepath.Join(util.HistoryDir, filepath.FromSlash(entry.HistoryPath)), []byte("corrupt")); err != nil {
		t.Fatal(err)
	}
	if _, err = RestoreDeletedMarkdown(entry.ID, box.ID, "/", "restored.md"); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected history hash mismatch, got %v", err)
	}
}

func TestGetDeletedMarkdownRejectsManifestTraversal(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	original := []byte("# deleted\n")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", original)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(original))
	if err != nil {
		t.Fatal(err)
	}
	mutateMarkdownTrashManifest(t, func(entry *MarkdownTrashEntry) {
		entry.HistoryPath = "../outside.md"
	})
	if _, _, err = GetDeletedMarkdown(entry.ID); !errors.Is(err, ErrInvalidMarkdownHistory) {
		t.Fatalf("expected invalid history path, got %v", err)
	}
}

func TestGetDeletedMarkdownRejectsNotebookTraversal(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	original := []byte("# deleted\n")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", original)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(original))
	if err != nil {
		t.Fatal(err)
	}
	mutateMarkdownTrashManifest(t, func(entry *MarkdownTrashEntry) {
		entry.Notebook = ".."
		entry.OriginalPath = "/outside.md"
		entry.HistoryPath = "outside.md"
	})
	if _, _, err = GetDeletedMarkdown(entry.ID); !errors.Is(err, ErrInvalidMarkdownHistory) {
		t.Fatalf("expected invalid notebook history path, got %v", err)
	}
}

func TestPurgeDeletedMarkdownKeepsSiblingHistory(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	firstData, secondData := []byte("first"), []byte("second")
	writeMarkdownManagementFixture(t, box.ID, "/first.md", firstData)
	first, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/first.md"}, markdownRevision(firstData))
	if err != nil {
		t.Fatal(err)
	}
	batchName := strings.Split(first.HistoryPath, "/")[0]
	second := &MarkdownTrashEntry{
		ID: first.ID + "-sibling", Notebook: box.ID, OriginalPath: "/second.md",
		HistoryPath: path.Join(batchName, box.ID, "second.md"), DeletedAt: first.DeletedAt,
		Size: int64(len(secondData)), Revision: markdownRevision(secondData), Mode: 0644,
	}
	secondHistoryPath := filepath.Join(util.HistoryDir, filepath.FromSlash(second.HistoryPath))
	if err = filelock.WriteFile(secondHistoryPath, secondData); err != nil {
		t.Fatal(err)
	}
	manifestPath := filepath.Join(util.HistoryDir, batchName, "markdown.json")
	if err = writeMarkdownTrashManifest(manifestPath, []*MarkdownTrashEntry{first, second}); err != nil {
		t.Fatal(err)
	}
	nativePath := filepath.Join(util.HistoryDir, "native-history.sy")
	if err = filelock.WriteFile(nativePath, []byte("native")); err != nil {
		t.Fatal(err)
	}

	if err = PurgeDeletedMarkdown(first.ID); err != nil {
		t.Fatal(err)
	}
	if _, _, err = GetDeletedMarkdown(second.ID); err != nil {
		t.Fatalf("sibling Markdown history was removed: %v", err)
	}
	if !filelock.IsExist(nativePath) {
		t.Fatal("native history was removed")
	}
}

func TestPurgeDeletedMarkdownRemovesEmptyBatch(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("only")
	writeMarkdownManagementFixture(t, box.ID, "/only.md", data)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/only.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	if err = PurgeDeletedMarkdown(entry.ID); err != nil {
		t.Fatal(err)
	}
	manifests, err := filepath.Glob(filepath.Join(util.HistoryDir, "*-delete", "markdown.json"))
	if err != nil {
		t.Fatal(err)
	}
	if len(manifests) != 0 {
		t.Fatalf("empty Markdown history batch survived: %v", manifests)
	}
}

func TestPurgeDeletedMarkdownPublishesAfterSuccess(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("only")
	writeMarkdownManagementFixture(t, box.ID, "/only.md", data)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/only.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	var events []*util.Result
	originalPushEvent := markdownPushEvent
	markdownPushEvent = func(event *util.Result) { events = append(events, event) }
	t.Cleanup(func() { markdownPushEvent = originalPushEvent })

	if err = PurgeDeletedMarkdown(entry.ID, "purge-client"); err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || events[0].Cmd != "purgeMarkdown" {
		t.Fatalf("unexpected Markdown purge events: %#v", events)
	}
	dataEnvelope := events[0].Data.(map[string]any)
	if dataEnvelope["box"] != box.ID || dataEnvelope["path"] != "/only.md" || dataEnvelope["operationID"] != "purge-client" {
		t.Fatalf("incomplete Markdown purge envelope: %#v", dataEnvelope)
	}
}
