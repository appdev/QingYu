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
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/siyuan-note/dejavu"
	"github.com/siyuan-note/eventbus"
	"github.com/siyuan-note/siyuan/kernel/conf"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func createMarkdownTestBox(t *testing.T, id string) *Box {
	t.Helper()
	box := &Box{ID: id}
	boxConf := conf.NewBoxConf()
	boxConf.Name = id
	boxConf.Closed = false
	if err := box.SaveConf(boxConf); err != nil {
		t.Fatal(err)
	}
	return box
}

func setupMarkdownTest(t *testing.T) *Box {
	t.Helper()
	originalConf := Conf
	originalDataDir := util.DataDir
	util.DataDir = filepath.Join(t.TempDir(), "data")
	Conf = NewAppConf()
	Conf.Sync = conf.NewSync()
	Conf.FileTree = conf.NewFileTree()
	Conf.NotebookCrypto = conf.NewNotebookCrypto()

	box := &Box{ID: "20260811000000-abcdefg"}
	boxConf := conf.NewBoxConf()
	boxConf.Name = "Markdown test"
	boxConf.Closed = false
	if err := box.SaveConf(boxConf); err != nil {
		t.Fatalf("save test notebook conf failed: %v", err)
	}
	t.Cleanup(func() {
		Conf = originalConf
		util.DataDir = originalDataDir
	})
	return box
}

func TestMarkdownFileLifecycle(t *testing.T) {
	box := setupMarkdownTest(t)

	created, err := CreateMarkdown(box.ID, "/", "notes")
	if err != nil {
		t.Fatal(err)
	}
	if created.Path != "/notes.md" || created.DocumentID == "" || InspectMarkdownDocumentID([]byte(created.Content)).State != "valid" {
		t.Fatalf("unexpected created document: %+v", created)
	}

	saved, err := SaveMarkdown(box.ID, created.Path, created.Content+"# Notes\n", created.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasSuffix(saved.Content, "# Notes\n") || saved.DocumentID != created.DocumentID || saved.Revision == created.Revision {
		t.Fatalf("unexpected saved document: %+v", saved)
	}
	if _, err = SaveMarkdown(box.ID, created.Path, "stale", created.Revision); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected revision conflict, got %v", err)
	}

	renamed, err := RenameMarkdown(box.ID, created.Path, "renamed.markdown")
	if err != nil {
		t.Fatal(err)
	}
	if renamed.Path != "/renamed.markdown" || renamed.Content != saved.Content || renamed.DocumentID != created.DocumentID {
		t.Fatalf("unexpected renamed document: %+v", renamed)
	}
	if err = RemoveMarkdown(box.ID, renamed.Path); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "renamed.markdown")); !os.IsNotExist(err) {
		t.Fatalf("Markdown file should be removed, got %v", err)
	}
}

func TestMarkdownDocumentIdentityLifecycle(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "identity.md")
	if err != nil {
		t.Fatal(err)
	}
	if created.DocumentID == "" {
		t.Fatal("new Markdown document has no stable ID")
	}

	renamed, err := RenameMarkdownWithRevision(box.ID, created.Path, "renamed.md", created.Revision, "identity-rename")
	if err != nil {
		t.Fatal(err)
	}
	if renamed.DocumentID != created.DocumentID {
		t.Fatal("rename changed the stable ID")
	}

	duplicated, err := DuplicateMarkdownWithOperationID(box.ID, renamed.Path, renamed.Revision, "identity-duplicate")
	if err != nil {
		t.Fatal(err)
	}
	if duplicated.DocumentID == "" || duplicated.DocumentID == renamed.DocumentID {
		t.Fatal("duplicate did not receive a distinct stable ID")
	}

	if _, err = EnsureMarkdownDocumentIdentity(box.ID, renamed.Path, "stale", "identity-stale", false); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected identity revision conflict, got %v", err)
	}
	unchanged, err := EnsureMarkdownDocumentIdentity(box.ID, renamed.Path, renamed.Revision, "identity-noop", false)
	if err != nil {
		t.Fatal(err)
	}
	if unchanged.Revision != renamed.Revision || unchanged.DocumentID != renamed.DocumentID {
		t.Fatal("valid identity no-op changed the document")
	}
	forced, err := EnsureMarkdownDocumentIdentity(box.ID, renamed.Path, renamed.Revision, "identity-force", true)
	if err != nil {
		t.Fatal(err)
	}
	if forced.DocumentID == renamed.DocumentID || forced.Revision == renamed.Revision {
		t.Fatal("force-new did not replace the stable ID")
	}

	legacyPath := filepath.Join(util.DataDir, box.ID, "legacy.md")
	if err = os.WriteFile(legacyPath, []byte("# Legacy\n"), 0644); err != nil {
		t.Fatal(err)
	}
	legacy, err := GetMarkdown(box.ID, "/legacy.md")
	if err != nil {
		t.Fatal(err)
	}
	ensured, err := EnsureMarkdownDocumentIdentity(box.ID, legacy.Path, legacy.Revision, "identity-create", false)
	if err != nil {
		t.Fatal(err)
	}
	if ensured.DocumentID == "" || !strings.HasSuffix(ensured.Content, "# Legacy\n") {
		t.Fatal("legacy document identity creation lost content")
	}
}

func TestGetMarkdownRejectsSymlinkSource(t *testing.T) {
	box := setupMarkdownTest(t)
	outside := filepath.Join(t.TempDir(), "outside.md")
	if err := os.WriteFile(outside, []byte("outside"), 0644); err != nil {
		t.Fatal(err)
	}
	symlinkOrSkip(t, outside, filepath.Join(util.DataDir, box.ID, "a.md"))
	if _, err := GetMarkdown(box.ID, "/a.md"); !errors.Is(err, ErrInvalidMarkdownPath) {
		t.Fatalf("symlink Markdown source was read: %v", err)
	}
}

func TestGetMarkdownRejectsSourceReplacedAfterCheck(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "source.md")
	if err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(util.DataDir, box.ID, "source.md")
	backup := source + ".original"
	outside := filepath.Join(t.TempDir(), "outside.md")
	if err = os.WriteFile(outside, []byte("outside"), 0644); err != nil {
		t.Fatal(err)
	}
	originalHook := markdownAfterRootComponentCheck
	markdownAfterRootComponentCheck = func(checkedPath string) error {
		if checkedPath != source {
			return nil
		}
		if err := os.Rename(source, backup); err != nil {
			return err
		}
		return os.Symlink(outside, source)
	}
	t.Cleanup(func() { markdownAfterRootComponentCheck = originalHook })
	if _, err = GetMarkdown(box.ID, created.Path); !errors.Is(err, ErrInvalidMarkdownPath) {
		t.Fatalf("replaced source was accepted: %v", err)
	}
}

func TestCreateMarkdownRejectsSymlinkParent(t *testing.T) {
	box := setupMarkdownTest(t)
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.MkdirAll(outside, 0755); err != nil {
		t.Fatal(err)
	}
	symlinkOrSkip(t, outside, filepath.Join(util.DataDir, box.ID, "linked"))
	if _, err := CreateMarkdown(box.ID, "/linked", "a.md"); !errors.Is(err, ErrInvalidMarkdownPath) {
		t.Fatalf("symlink Markdown parent was accepted: %v", err)
	}
	if _, err := os.Stat(filepath.Join(outside, "a.md")); !os.IsNotExist(err) {
		t.Fatalf("Markdown file escaped through parent symlink: %v", err)
	}
}

func TestCreateMarkdownRejectsNotebookRootReplacedAfterCheck(t *testing.T) {
	box := setupMarkdownTest(t)
	boxRoot := filepath.Join(util.DataDir, box.ID)
	originalRoot := boxRoot + "-original"
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.MkdirAll(outside, 0755); err != nil {
		t.Fatal(err)
	}
	originalHook := markdownAfterRootComponentCheck
	markdownAfterRootComponentCheck = func(checkedPath string) error {
		if checkedPath != boxRoot {
			return nil
		}
		if err := os.Rename(boxRoot, originalRoot); err != nil {
			return err
		}
		return os.Symlink(outside, boxRoot)
	}
	t.Cleanup(func() { markdownAfterRootComponentCheck = originalHook })
	if _, err := CreateMarkdown(box.ID, "/", "a.md"); !errors.Is(err, ErrInvalidMarkdownPath) {
		t.Fatalf("replaced notebook root was accepted: %v", err)
	}
	if _, err := os.Stat(filepath.Join(outside, "a.md")); !os.IsNotExist(err) {
		t.Fatalf("creation escaped replaced root: %v", err)
	}
}

func TestCreateMarkdownRejectsParentReplacedAfterCheck(t *testing.T) {
	box := setupMarkdownTest(t)
	parent := filepath.Join(util.DataDir, box.ID, "notes")
	if err := os.MkdirAll(parent, 0755); err != nil {
		t.Fatal(err)
	}
	originalParent := parent + "-original"
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.MkdirAll(outside, 0755); err != nil {
		t.Fatal(err)
	}
	originalHook := markdownAfterRootComponentCheck
	markdownAfterRootComponentCheck = func(checkedPath string) error {
		if checkedPath != parent {
			return nil
		}
		if err := os.Rename(parent, originalParent); err != nil {
			return err
		}
		return os.Symlink(outside, parent)
	}
	t.Cleanup(func() { markdownAfterRootComponentCheck = originalHook })
	if _, err := CreateMarkdown(box.ID, "/notes", "a.md"); !errors.Is(err, ErrInvalidMarkdownPath) {
		t.Fatalf("replaced parent was accepted: %v", err)
	}
	if _, err := os.Stat(filepath.Join(outside, "a.md")); !os.IsNotExist(err) {
		t.Fatalf("creation escaped replaced parent: %v", err)
	}
}

func TestRenameMarkdownRejectsSymlinkTarget(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(t.TempDir(), "outside.md")
	if err = os.WriteFile(outside, []byte("outside"), 0644); err != nil {
		t.Fatal(err)
	}
	symlinkOrSkip(t, outside, filepath.Join(util.DataDir, box.ID, "b.md"))
	if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "b.md", created.Revision); !errors.Is(err, ErrInvalidMarkdownPath) {
		t.Fatalf("symlink target was accepted: %v", err)
	}
	if got, readErr := os.ReadFile(outside); readErr != nil || string(got) != "outside" {
		t.Fatalf("outside target changed: %q, %v", got, readErr)
	}
}

func TestRestoreDeletedMarkdownRejectsSymlinkTargetParent(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	data := []byte("source")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	entry, err := RecycleMarkdown(MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(t.TempDir(), "outside")
	if err = os.MkdirAll(outside, 0755); err != nil {
		t.Fatal(err)
	}
	symlinkOrSkip(t, outside, filepath.Join(util.DataDir, box.ID, "linked"))
	if _, err = RestoreDeletedMarkdown(entry.ID, box.ID, "/linked", "a.md"); !errors.Is(err, ErrInvalidMarkdownPath) {
		t.Fatalf("symlink restore target was accepted: %v", err)
	}
	if _, err = os.Stat(filepath.Join(outside, "a.md")); !os.IsNotExist(err) {
		t.Fatalf("restored file escaped through parent symlink: %v", err)
	}
}

func TestDuplicateAndMoveMarkdown(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/folder", "notes.md")
	if err != nil {
		t.Fatal(err)
	}
	saved, err := SaveMarkdown(box.ID, created.Path, created.Content+"# Notes\n", created.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = CreateMarkdown(box.ID, "/folder", "notes 2.md"); err != nil {
		t.Fatal(err)
	}
	duplicated, err := DuplicateMarkdown(box.ID, saved.Path, saved.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if duplicated.Path != "/folder/notes 3.md" || duplicated.Content == saved.Content || duplicated.DocumentID == saved.DocumentID {
		t.Fatalf("unexpected duplicate: %+v", duplicated)
	}
	if strings.TrimSpace(strings.Replace(duplicated.Content, duplicated.DocumentID, saved.DocumentID, 1)) != strings.TrimSpace(saved.Content) {
		t.Fatalf("duplicate changed bytes other than the stable ID: %q", duplicated.Content)
	}
	if _, err = DuplicateMarkdown(box.ID, saved.Path, created.Revision); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected duplicate conflict, got %v", err)
	}

	if err = os.MkdirAll(filepath.Join(util.DataDir, box.ID, "archive"), 0755); err != nil {
		t.Fatal(err)
	}
	moved, err := MoveMarkdown(box.ID, saved.Path, saved.Revision, box.ID, "/archive")
	if err != nil {
		t.Fatal(err)
	}
	if moved.Path != "/archive/notes.md" || moved.Content != saved.Content {
		t.Fatalf("unexpected moved document: %+v", moved)
	}
	if _, err = GetMarkdown(box.ID, saved.Path); !os.IsNotExist(err) {
		t.Fatalf("source Markdown file should be moved, got %v", err)
	}
	if _, err = CreateMarkdown(box.ID, "/folder", "notes.md"); err != nil {
		t.Fatal(err)
	}
	if _, err = MoveMarkdown(box.ID, moved.Path, moved.Revision, box.ID, "/folder"); err == nil {
		t.Fatal("expected move to reject an existing destination")
	}
}

func TestDuplicateMarkdownPreservesMode(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Windows does not expose portable Unix permission bits")
	}
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(util.DataDir, box.ID, "a.md")
	if err = os.Chmod(source, 0440); err != nil {
		t.Fatal(err)
	}
	duplicate, err := DuplicateMarkdown(box.ID, created.Path, created.Revision)
	if err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(filepath.Join(util.DataDir, box.ID, strings.TrimPrefix(duplicate.Path, "/")))
	if err != nil {
		t.Fatal(err)
	}
	if got := info.Mode().Perm(); got != 0440 {
		t.Fatalf("duplicate mode = %#o", got)
	}
	if err = os.MkdirAll(filepath.Join(util.DataDir, box.ID, "target"), 0755); err != nil {
		t.Fatal(err)
	}
	moved, err := MoveMarkdown(box.ID, duplicate.Path, duplicate.Revision, box.ID, "/target")
	if err != nil {
		t.Fatal(err)
	}
	info, err = os.Stat(filepath.Join(util.DataDir, box.ID, strings.TrimPrefix(moved.Path, "/")))
	if err != nil {
		t.Fatal(err)
	}
	if got := info.Mode().Perm(); got != 0440 {
		t.Fatalf("moved mode = %#o", got)
	}
}

func TestDuplicateMarkdownTargetValidationDoesNotDeleteReplacement(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "duplicate-validation.md")
	if err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(util.DataDir, box.ID, "duplicate-validation 2.md")
	originalHook := markdownInstallValidationHook
	markdownInstallValidationHook = func(kind, targetPath string) error {
		if kind != "duplicate" {
			return nil
		}
		if err := os.Remove(targetPath); err != nil {
			return err
		}
		return os.WriteFile(targetPath, []byte("replacement"), 0644)
	}
	t.Cleanup(func() { markdownInstallValidationHook = originalHook })
	if _, err = DuplicateMarkdown(box.ID, created.Path, created.Revision); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected identity conflict, got %v", err)
	}
	data, err := os.ReadFile(target)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "replacement" {
		t.Fatalf("replacement was deleted: %q", data)
	}
	journals, err := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions", "*", "transaction.json"))
	if err != nil || len(journals) == 0 {
		t.Fatalf("failed install did not retain journal: %v, %#v", err, journals)
	}
}

func TestDuplicateMarkdownCleansOwnTargetAfterPostLinkFailure(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "post-link.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownInstallValidationHook
	markdownInstallValidationHook = func(kind, _ string) error {
		if kind == "duplicate" {
			return errors.New("post-link validation failed")
		}
		return nil
	}
	t.Cleanup(func() { markdownInstallValidationHook = originalHook })
	if _, err = DuplicateMarkdown(box.ID, created.Path, created.Revision); err == nil {
		t.Fatal("expected post-link failure")
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "post-link 2.md")); !os.IsNotExist(err) {
		t.Fatalf("owned target survived failed install: %v", err)
	}
	journals, _ := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions", "*", "transaction.json"))
	if len(journals) == 0 {
		t.Fatal("failed install discarded its journal")
	}
}

func TestRenameMarkdownCleansCopiedTargetAfterValidationFailure(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "copy-validation.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownCopyValidationHook
	markdownCopyValidationHook = func(string) error { return errors.New("copy validation failed") }
	t.Cleanup(func() { markdownCopyValidationHook = originalHook })
	if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "copy-target.md", created.Revision); err == nil {
		t.Fatal("expected copy validation failure")
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "copy-target.md")); !os.IsNotExist(err) {
		t.Fatalf("owned copy survived failed validation: %v", err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "copy-validation.md")); err != nil {
		t.Fatalf("source changed after copy failure: %v", err)
	}
	journals, _ := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions", "*", "transaction.json"))
	if len(journals) == 0 {
		t.Fatal("failed copy discarded its journal")
	}
}

func TestRenameMarkdownRecoversDurablePrivateCopyBeforeTargetVisible(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "private-copy.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownCopyCrashHook
	markdownCopyCrashHook = func(phase string) error {
		if phase == "target-created" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "private-target.md", created.Revision); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "private-target.md")); !os.IsNotExist(err) {
		t.Fatalf("target became visible before durable install: %v", err)
	}
	markdownCopyCrashHook = originalHook
	t.Cleanup(func() { markdownCopyCrashHook = originalHook })
	originalDeleteHook := markdownIdentityDeleteHook
	markdownIdentityDeleteHook = func(filePath string) error {
		if strings.Contains(filePath, ".private-target.md.copying-") {
			return errors.New("private cleanup failed")
		}
		return nil
	}
	if err = RecoverMarkdownTransactions(); err == nil {
		t.Fatal("expected guarded private cleanup failure")
	}
	privateCopies, _ := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".private-target.md.copying-*"))
	if len(privateCopies) == 0 {
		t.Fatal("failed identity delete discarded private recovery target")
	}
	markdownIdentityDeleteHook = originalDeleteHook
	t.Cleanup(func() { markdownIdentityDeleteHook = originalDeleteHook })
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	privateCopies, _ = filepath.Glob(filepath.Join(util.DataDir, box.ID, ".private-target.md.copying-*"))
	if len(privateCopies) != 0 {
		t.Fatalf("private copies survived recovery: %#v", privateCopies)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "private-copy.md")); err != nil {
		t.Fatalf("source changed during private copy: %v", err)
	}
}

func TestRenameMarkdownCleansBothTargetsAfterLinkDurabilityFailure(t *testing.T) {
	for _, phase := range []string{"close", "sync"} {
		t.Run(phase, func(t *testing.T) {
			box := setupMarkdownTest(t)
			created, err := CreateMarkdown(box.ID, "/", "link-failure.md")
			if err != nil {
				t.Fatal(err)
			}
			originalHook := markdownLinkDurabilityHook
			markdownLinkDurabilityHook = func(current string) error {
				if current == phase {
					return errors.New("link durability failure")
				}
				return nil
			}
			if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "link-target.md", created.Revision); err == nil {
				t.Fatal("expected link durability failure")
			}
			markdownLinkDurabilityHook = originalHook
			t.Cleanup(func() { markdownLinkDurabilityHook = originalHook })
			if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "link-target.md")); !os.IsNotExist(err) {
				t.Fatalf("destination survived link failure: %v", err)
			}
			privateCopies, _ := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".link-target.md.copying-*"))
			if len(privateCopies) != 0 {
				t.Fatalf("target staging survived link failure: %#v", privateCopies)
			}
			journals, _ := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions", "*", "transaction.json"))
			if len(journals) == 0 {
				t.Fatal("target-created journal was removed before cleanup recovery")
			}
			if err = RecoverMarkdownTransactions(); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestMoveMarkdownMigratesSortKeysAcrossNotebooks(t *testing.T) {
	box := setupMarkdownTest(t)
	otherBox := createMarkdownTestBox(t, "20260811000001-bcdefgh")
	if err := os.MkdirAll(filepath.Join(util.DataDir, otherBox.ID, "target"), 0755); err != nil {
		t.Fatal(err)
	}
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	if err = writeSortConfMap(filepath.Join(util.DataDir, box.ID, ".siyuan", "sort.json"),
		map[string]int{"markdown:/a.md": 3}); err != nil {
		t.Fatal(err)
	}
	if err = writeSortConfMap(filepath.Join(util.DataDir, otherBox.ID, ".siyuan", "sort.json"),
		map[string]int{"markdown:/target/existing.md": 4}); err != nil {
		t.Fatal(err)
	}

	moved, err := MoveMarkdown(box.ID, created.Path, created.Revision, otherBox.ID, "/target")
	if err != nil {
		t.Fatal(err)
	}
	if moved.Path != "/target/a.md" {
		t.Fatalf("unexpected moved path: %q", moved.Path)
	}
	fromSort, err := readSortConfMap(filepath.Join(util.DataDir, box.ID, ".siyuan", "sort.json"))
	if err != nil {
		t.Fatal(err)
	}
	toSort, err := readSortConfMap(filepath.Join(util.DataDir, otherBox.ID, ".siyuan", "sort.json"))
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := fromSort["markdown:/a.md"]; ok || toSort["markdown:/target/a.md"] == 0 {
		t.Fatalf("sort keys were not migrated: from=%#v to=%#v", fromSort, toSort)
	}
}

func TestMoveMarkdownRollsBackFileAndSortMetadata(t *testing.T) {
	box := setupMarkdownTest(t)
	otherBox := createMarkdownTestBox(t, "20260811000001-bcdefgh")
	if err := os.MkdirAll(filepath.Join(util.DataDir, otherBox.ID, "target"), 0755); err != nil {
		t.Fatal(err)
	}
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	fromConfPath := filepath.Join(util.DataDir, box.ID, ".siyuan", "sort.json")
	toConfPath := filepath.Join(util.DataDir, otherBox.ID, ".siyuan", "sort.json")
	fromBefore := map[string]int{"markdown:/a.md": 3}
	toBefore := map[string]int{"markdown:/target/existing.md": 4}
	if err = writeSortConfMap(fromConfPath, fromBefore); err != nil {
		t.Fatal(err)
	}
	if err = writeSortConfMap(toConfPath, toBefore); err != nil {
		t.Fatal(err)
	}
	originalWriter := markdownSortWriteConf
	markdownSortWriteConf = func(string, map[string]int) error { return errors.New("metadata write failed") }
	t.Cleanup(func() { markdownSortWriteConf = originalWriter })

	if _, err = MoveMarkdown(box.ID, created.Path, created.Revision, otherBox.ID, "/target"); err == nil {
		t.Fatal("expected metadata failure")
	}
	if _, err = GetMarkdown(box.ID, "/a.md"); err != nil {
		t.Fatalf("source was not restored: %v", err)
	}
	if _, err = GetMarkdown(otherBox.ID, "/target/a.md"); !os.IsNotExist(err) {
		t.Fatalf("target survived rollback: %v", err)
	}
	fromAfter, err := readSortConfMap(fromConfPath)
	if err != nil {
		t.Fatal(err)
	}
	toAfter, err := readSortConfMap(toConfPath)
	if err != nil {
		t.Fatal(err)
	}
	if fromAfter["markdown:/a.md"] != fromBefore["markdown:/a.md"] ||
		toAfter["markdown:/target/existing.md"] != toBefore["markdown:/target/existing.md"] {
		t.Fatalf("sort metadata changed on rollback: from=%#v to=%#v", fromAfter, toAfter)
	}
}

func TestMoveMarkdownReportsRollbackCollisionAndPreservesRecoveryFile(t *testing.T) {
	box := setupMarkdownTest(t)
	otherBox := createMarkdownTestBox(t, "20260811000001-abcdefg")
	if err := os.MkdirAll(filepath.Join(util.DataDir, otherBox.ID, "target"), 0755); err != nil {
		t.Fatal(err)
	}
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(util.DataDir, box.ID, "a.md")
	originalWriter := markdownSortWriteConf
	markdownSortWriteConf = func(string, map[string]int) error {
		if err := os.WriteFile(source, []byte("competitor"), 0600); err != nil {
			return err
		}
		return errors.New("metadata failed")
	}
	t.Cleanup(func() { markdownSortWriteConf = originalWriter })
	if _, err = MoveMarkdown(box.ID, created.Path, created.Revision, otherBox.ID, "/target"); !errors.Is(err, os.ErrExist) {
		t.Fatalf("rollback collision was hidden: %v", err)
	}
	recoveryFiles, globErr := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions", "*", "payload.md"))
	if globErr != nil || len(recoveryFiles) != 1 {
		t.Fatalf("recovery staging missing: %v, %v", recoveryFiles, globErr)
	}
	if got, readErr := os.ReadFile(recoveryFiles[0]); readErr != nil || string(got) != created.Content {
		t.Fatalf("recovery staging was not retained: %q, %v", got, readErr)
	}
	journals, globErr := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions", "*", "transaction.json"))
	if globErr != nil || len(journals) != 1 {
		t.Fatalf("recovery journal missing: %v, %v", journals, globErr)
	}
}

func TestMoveMarkdownPublishesOneCompleteManagementEvent(t *testing.T) {
	box := setupMarkdownTest(t)
	otherBox := createMarkdownTestBox(t, "20260811000001-bcdefgh")
	if err := os.MkdirAll(filepath.Join(util.DataDir, otherBox.ID, "target"), 0755); err != nil {
		t.Fatal(err)
	}
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	var events []*util.Result
	originalPushEvent := markdownPushEvent
	markdownPushEvent = func(event *util.Result) { events = append(events, event) }
	t.Cleanup(func() { markdownPushEvent = originalPushEvent })

	moved, err := MoveMarkdown(box.ID, created.Path, created.Revision, otherBox.ID, "/target", "client-op")
	if err != nil {
		t.Fatal(err)
	}
	if moved.OperationID != "client-op" {
		t.Fatalf("operation ID was not returned: %#v", moved)
	}
	if len(events) != 1 || events[0].Cmd != "renameMarkdown" {
		t.Fatalf("unexpected Markdown move events: %#v", events)
	}
	data, ok := events[0].Data.(map[string]any)
	if !ok {
		t.Fatalf("unexpected event data: %#v", events[0].Data)
	}
	if data["kind"] != "markdown" || data["box"] != otherBox.ID || data["path"] != "/target/a.md" ||
		data["oldBox"] != box.ID || data["oldPath"] != "/a.md" {
		t.Fatalf("incomplete Markdown move envelope: %#v", data)
	}
	if data["operationID"] != "client-op" || data["time"].(int64) <= 0 {
		t.Fatalf("missing event identity or time: %#v", data)
	}
}

func TestMoveMarkdownPublishesNothingWhenMetadataFails(t *testing.T) {
	box := setupMarkdownTest(t)
	otherBox := createMarkdownTestBox(t, "20260811000001-bcdefgh")
	if err := os.MkdirAll(filepath.Join(util.DataDir, otherBox.ID, "target"), 0755); err != nil {
		t.Fatal(err)
	}
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	var events []*util.Result
	originalPushEvent := markdownPushEvent
	markdownPushEvent = func(event *util.Result) { events = append(events, event) }
	originalWriter := markdownSortWriteConf
	markdownSortWriteConf = func(string, map[string]int) error { return errors.New("metadata write failed") }
	t.Cleanup(func() {
		markdownPushEvent = originalPushEvent
		markdownSortWriteConf = originalWriter
	})

	if _, err = MoveMarkdown(box.ID, created.Path, created.Revision, otherBox.ID, "/target"); err == nil {
		t.Fatal("expected metadata failure")
	}
	if len(events) != 0 {
		t.Fatalf("failed Markdown move published events: %#v", events)
	}
}

func TestMoveMarkdownRejectsMissingTargetDirectory(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = MoveMarkdown(box.ID, created.Path, created.Revision, box.ID, "/missing"); err == nil {
		t.Fatal("missing target directory was accepted")
	}
	if _, err = GetMarkdown(box.ID, created.Path); err != nil {
		t.Fatalf("source changed after rejected move: %v", err)
	}
}

func TestRenameMarkdownMigratesSortKey(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	confPath := filepath.Join(util.DataDir, box.ID, ".siyuan", "sort.json")
	if err = writeSortConfMap(confPath, map[string]int{"markdown:/a.md": 7}); err != nil {
		t.Fatal(err)
	}
	if _, err = RenameMarkdown(box.ID, created.Path, "b.md"); err != nil {
		t.Fatal(err)
	}
	conf, err := readSortConfMap(confPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := conf["markdown:/a.md"]; ok || conf["markdown:/b.md"] != 7 {
		t.Fatalf("sort key was not renamed: %#v", conf)
	}
}

func TestRenameMarkdownReportsRollbackCollision(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(util.DataDir, box.ID, "a.md")
	originalWriter := markdownSortWriteConf
	markdownSortWriteConf = func(string, map[string]int) error {
		if err := os.WriteFile(source, []byte("competitor"), 0600); err != nil {
			return err
		}
		return errors.New("sort failed")
	}
	t.Cleanup(func() { markdownSortWriteConf = originalWriter })
	if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "b.md", created.Revision); !errors.Is(err, os.ErrExist) {
		t.Fatalf("rollback collision was hidden: %v", err)
	}
	recoveryFiles, err := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions", "*", "payload.md"))
	if err != nil || len(recoveryFiles) != 1 {
		t.Fatalf("recovery staging missing: %v, %v", recoveryFiles, err)
	}
	journals, err := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions", "*", "transaction.json"))
	if err != nil || len(journals) != 1 {
		t.Fatalf("recovery journal missing: %v, %v", journals, err)
	}
}

func TestMoveMarkdownRollsBackWhenRecentMetadataWriteFails(t *testing.T) {
	box := setupMarkdownTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 32
	otherBox := createMarkdownTestBox(t, "20260811000001-bcdefgh")
	if err := os.MkdirAll(filepath.Join(util.DataDir, otherBox.ID, "target"), 0755); err != nil {
		t.Fatal(err)
	}
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	fromRef := MarkdownDocumentRef{Notebook: box.ID, Path: created.Path}
	toRef := MarkdownDocumentRef{Notebook: otherBox.ID, Path: "/target/a.md"}
	if err = UpdateRecentMarkdownOpenTime(fromRef); err != nil {
		t.Fatal(err)
	}
	originalWriter := markdownRecentWriteDocs
	markdownRecentWriteDocs = func([]*RecentDoc) error { return errors.New("recent write failed") }
	t.Cleanup(func() { markdownRecentWriteDocs = originalWriter })

	if _, err = MoveMarkdown(box.ID, created.Path, created.Revision, otherBox.ID, "/target"); err == nil {
		t.Fatal("expected recent metadata failure")
	}
	if _, err = GetMarkdown(box.ID, created.Path); err != nil {
		t.Fatalf("source was not restored: %v", err)
	}
	if findRecentMarkdownTest(t, fromRef) == nil || findRecentMarkdownTest(t, toRef) != nil {
		t.Fatalf("recent metadata was not rolled back: from=%#v to=%#v",
			findRecentMarkdownTest(t, fromRef), findRecentMarkdownTest(t, toRef))
	}
}

func TestRenameMarkdownMigratesRecentRecord(t *testing.T) {
	box := setupMarkdownTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 32
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	fromRef := MarkdownDocumentRef{Notebook: box.ID, Path: created.Path}
	toRef := MarkdownDocumentRef{Notebook: box.ID, Path: "/b.md"}
	if err = UpdateRecentMarkdownOpenTime(fromRef); err != nil {
		t.Fatal(err)
	}
	if _, err = RenameMarkdown(box.ID, created.Path, "b.md"); err != nil {
		t.Fatal(err)
	}
	if findRecentMarkdownTest(t, fromRef) != nil || findRecentMarkdownTest(t, toRef) == nil {
		t.Fatalf("recent record was not renamed: from=%#v to=%#v",
			findRecentMarkdownTest(t, fromRef), findRecentMarkdownTest(t, toRef))
	}
}

func TestRenameMarkdownWithRevisionRejectsStaleContent(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	saved, err := SaveMarkdown(box.ID, created.Path, "current", created.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = RenameMarkdownWithRevision(box.ID, saved.Path, "b.md", created.Revision); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected stale rename conflict, got %v", err)
	}
	if _, err = GetMarkdown(box.ID, "/a.md"); err != nil {
		t.Fatalf("source changed after stale rename: %v", err)
	}
	if _, err = GetMarkdown(box.ID, "/b.md"); !os.IsNotExist(err) {
		t.Fatalf("target exists after stale rename: %v", err)
	}
	if _, err = RenameMarkdownWithRevision(box.ID, saved.Path, "b.md", saved.Revision); err != nil {
		t.Fatalf("matching revision was rejected: %v", err)
	}
}

func TestRenameMarkdownDoesNotOverwriteDestinationCreatedDuringCommit(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = SaveMarkdown(box.ID, created.Path, "source", created.Revision); err != nil {
		t.Fatal(err)
	}
	destination := filepath.Join(util.DataDir, box.ID, "b.md")
	originalHook := markdownBeforeDestinationCommit
	markdownBeforeDestinationCommit = func(string) error {
		return os.WriteFile(destination, []byte("competitor"), 0600)
	}
	t.Cleanup(func() { markdownBeforeDestinationCommit = originalHook })
	if _, err = RenameMarkdownWithRevision(box.ID, "/a.md", "b.md", markdownRevision([]byte("source"))); !os.IsExist(err) {
		t.Fatalf("expected destination collision, got %v", err)
	}
	if got, readErr := os.ReadFile(destination); readErr != nil || string(got) != "competitor" {
		t.Fatalf("destination was overwritten: %q, %v", got, readErr)
	}
	if got, readErr := os.ReadFile(filepath.Join(util.DataDir, box.ID, "a.md")); readErr != nil || string(got) != "source" {
		t.Fatalf("source was not preserved: %q, %v", got, readErr)
	}
	markdownBeforeDestinationCommit = originalHook
	if _, err = CreateMarkdown(box.ID, "/", "after-collision.md"); err != nil {
		t.Fatalf("destination collision left recovery gate blocked: %v", err)
	}
	if got, readErr := os.ReadFile(destination); readErr != nil || string(got) != "competitor" {
		t.Fatalf("recovery touched competitor: %q, %v", got, readErr)
	}
}

func TestRenameMarkdownRejectsInvalidOperationIDBeforeMutation(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	for _, operationID := range []string{"has space", strings.Repeat("a", 65), "非ASCII"} {
		if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "b.md", created.Revision, operationID); !errors.Is(err, ErrInvalidMarkdownOperationID) {
			t.Fatalf("operation ID %q was accepted: %v", operationID, err)
		}
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "a.md")); err != nil {
		t.Fatalf("source mutated: %v", err)
	}
}

func TestMoveMarkdownDetectsWriteAfterRevisionCheck(t *testing.T) {
	box := setupMarkdownTest(t)
	if err := os.MkdirAll(filepath.Join(util.DataDir, box.ID, "target"), 0755); err != nil {
		t.Fatal(err)
	}
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownAfterRevisionCheck
	markdownAfterRevisionCheck = func(source string) error { return os.WriteFile(source, []byte("changed"), 0644) }
	t.Cleanup(func() { markdownAfterRevisionCheck = originalHook })
	if _, err = MoveMarkdown(box.ID, created.Path, created.Revision, box.ID, "/target"); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("expected revision conflict, got %v", err)
	}
	if got, readErr := os.ReadFile(filepath.Join(util.DataDir, box.ID, "a.md")); readErr != nil || string(got) != "changed" {
		t.Fatalf("new source bytes were lost: %q, %v", got, readErr)
	}
	if _, statErr := os.Stat(filepath.Join(util.DataDir, box.ID, "target", "a.md")); !os.IsNotExist(statErr) {
		t.Fatalf("target exists after conflict: %v", statErr)
	}
}

func TestSaveMarkdownRecoversStagedTransactionOnNextEntry(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "save-crash.md")
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
	if _, err = SaveMarkdown(box.ID, created.Path, "new content", created.Revision); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	recovered, err := GetMarkdown(box.ID, created.Path)
	if err != nil {
		t.Fatal(err)
	}
	if recovered.Content != created.Content {
		t.Fatalf("staged save was committed during recovery: %q", recovered.Content)
	}
}

func TestRenameMarkdownRecoversCopiedTransactionByRollback(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "rename-crash.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "rename" && phase == "copied" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "renamed.md", created.Revision); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "rename-crash.md")); err != nil {
		t.Fatalf("source was not retained: %v", err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "renamed.md")); !os.IsNotExist(err) {
		t.Fatalf("copied target was not rolled back: %v", err)
	}
}

func TestRenameMarkdownRecoveryDoesNotUnlinkReplacement(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "replace-crash.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "rename" && phase == "copied" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "replacement.md", created.Revision); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	source := filepath.Join(util.DataDir, box.ID, "replace-crash.md")
	if err = os.WriteFile(source, []byte("replacement"), 0600); err != nil {
		t.Fatal(err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err = RecoverMarkdownTransactions(); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("replacement was not detected: %v", err)
	}
	if got, err := os.ReadFile(source); err != nil || string(got) != "replacement" {
		t.Fatalf("replacement was unlinked: %q, %v", got, err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "replacement.md")); err != nil {
		t.Fatalf("recovery evidence target missing: %v", err)
	}
}

func TestRenameMarkdownRecoveryDetectsSameContentFileReplacement(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "identity-crash.md")
	if err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(util.DataDir, box.ID, "identity-crash.md")
	before, err := os.Stat(source)
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "rename" && phase == "copied" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "identity-target.md", created.Revision); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	if err = os.Remove(source); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(source, nil, before.Mode().Perm()); err != nil {
		t.Fatal(err)
	}
	if err = os.Chtimes(source, before.ModTime(), before.ModTime()); err != nil {
		t.Fatal(err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err = RecoverMarkdownTransactions(); !errors.Is(err, ErrMarkdownConflict) {
		t.Fatalf("same-content replacement identity was not detected: %v", err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "identity-target.md")); err != nil {
		t.Fatalf("recovery evidence target missing: %v", err)
	}
}

func TestMoveMarkdownRecoversCopiedTransactionByRollback(t *testing.T) {
	box := setupMarkdownTest(t)
	otherBox := createMarkdownTestBox(t, "20260811000001-abcdefg")
	if err := os.MkdirAll(filepath.Join(util.DataDir, otherBox.ID, "target"), 0755); err != nil {
		t.Fatal(err)
	}
	created, err := CreateMarkdown(box.ID, "/", "move-crash.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "move" && phase == "copied" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	if _, err = MoveMarkdown(box.ID, created.Path, created.Revision, otherBox.ID, "/target"); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "move-crash.md")); err != nil {
		t.Fatalf("source was not retained: %v", err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, otherBox.ID, "target", "move-crash.md")); !os.IsNotExist(err) {
		t.Fatalf("copied target was not rolled back: %v", err)
	}
}

func TestRenameMarkdownRecoversCrashAfterSourceStagedForUnlink(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "unlink-crash.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "rename" && phase == "source-staged" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "unlink-target.md", created.Revision); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "unlink-crash.md")); err != nil {
		t.Fatalf("source was not restored: %v", err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "unlink-target.md")); !os.IsNotExist(err) {
		t.Fatalf("target was not rolled back: %v", err)
	}
}

func TestRenameMarkdownRecoveryRestoresCommittedMetadata(t *testing.T) {
	box := setupMarkdownTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 32
	created, err := CreateMarkdown(box.ID, "/", "metadata-crash.md")
	if err != nil {
		t.Fatal(err)
	}
	fromRef := MarkdownDocumentRef{Notebook: box.ID, Path: created.Path}
	if err = UpdateRecentMarkdownOpenTime(fromRef); err != nil {
		t.Fatal(err)
	}
	confPath := filepath.Join(util.DataDir, box.ID, ".siyuan", "sort.json")
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "rename" && phase == "metadata-committed" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "metadata-target.md", created.Revision); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	afterSort, err := readSortConfMap(confPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := afterSort[fileTreeSortKey("/metadata-target.md")]; !ok {
		t.Fatalf("committed sort metadata was rolled back: %#v", afterSort)
	}
	toRef := MarkdownDocumentRef{Notebook: box.ID, Path: "/metadata-target.md"}
	if findRecentMarkdownTest(t, fromRef) != nil || findRecentMarkdownTest(t, toRef) == nil {
		t.Fatal("committed recent metadata was rolled back")
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "metadata-crash.md")); !os.IsNotExist(err) {
		t.Fatalf("committed source survived recovery: %v", err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "metadata-target.md")); err != nil {
		t.Fatalf("committed target missing: %v", err)
	}
}

func TestSaveMarkdownCASDoesNotOverwriteReplacement(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "save-cas.md")
	if err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(util.DataDir, box.ID, "save-cas.md")
	originalHook := markdownSaveCommitHook
	markdownSaveCommitHook = func(phase, sourcePath string) error {
		if phase != "source-isolated" {
			return nil
		}
		return os.WriteFile(sourcePath, []byte("replacement"), 0644)
	}
	t.Cleanup(func() { markdownSaveCommitHook = originalHook })
	if _, err = SaveMarkdown(box.ID, created.Path, "new content", created.Revision); !errors.Is(err, os.ErrExist) {
		t.Fatalf("expected no-replace conflict, got %v", err)
	}
	got, err := os.ReadFile(source)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "replacement" {
		t.Fatalf("replacement overwritten: %q", got)
	}
}

func TestSaveMarkdownRollsBackLinkBeforeInstalledIsDurable(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "save-link.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownSaveCommitHook
	markdownSaveCommitHook = func(phase, _ string) error {
		if phase == "linked" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	if _, err = SaveMarkdown(box.ID, created.Path, "new content", created.Revision); !errors.Is(err, ErrMarkdownSimulatedCrash) {
		t.Fatalf("expected simulated crash, got %v", err)
	}
	markdownSaveCommitHook = originalHook
	t.Cleanup(func() { markdownSaveCommitHook = originalHook })
	data, err := os.ReadFile(filepath.Join(util.DataDir, box.ID, "save-link.md"))
	if err != nil {
		t.Fatal(err)
	}
	if markdownRevision(data) != created.Revision {
		t.Fatalf("non-durable link was treated as commit: %q", data)
	}
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	data, err = os.ReadFile(filepath.Join(util.DataDir, box.ID, "save-link.md"))
	if err != nil || markdownRevision(data) != created.Revision {
		t.Fatalf("recovery did not preserve old source: %q, %v", data, err)
	}
}

func TestSaveMarkdownRollsBackWhenInstalledPhaseWriteFails(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "save-installed-write.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionWriteHook
	failed := false
	markdownTransactionWriteHook = func(kind, phase string) error {
		if !failed && kind == "save" && phase == "installed" {
			failed = true
			return errors.New("installed phase write failed")
		}
		return nil
	}
	t.Cleanup(func() { markdownTransactionWriteHook = originalHook })
	if _, err = SaveMarkdown(box.ID, created.Path, "new content", created.Revision); err == nil {
		t.Fatal("expected installed phase write failure")
	}
	data, err := os.ReadFile(filepath.Join(util.DataDir, box.ID, "save-installed-write.md"))
	if err != nil || markdownRevision(data) != created.Revision {
		t.Fatalf("failed installed phase did not rollback source: %q, %v", data, err)
	}
}

func TestSaveMarkdownSourceIsolatedRecoveryRollsBackVisibleLink(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "save-recovery-link.md")
	if err != nil {
		t.Fatal(err)
	}
	originalSaveHook := markdownSaveCommitHook
	originalDeleteHook := markdownIdentityDeleteHook
	markdownSaveCommitHook = func(phase, _ string) error {
		if phase == "linked" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	markdownIdentityDeleteHook = func(filePath string) error {
		if strings.HasSuffix(filePath, "save-recovery-link.md") {
			return errors.New("rollback interrupted")
		}
		return nil
	}
	if _, err = SaveMarkdown(box.ID, created.Path, "new visible link", created.Revision); err == nil {
		t.Fatal("expected interrupted rollback")
	}
	markdownSaveCommitHook = originalSaveHook
	markdownIdentityDeleteHook = originalDeleteHook
	t.Cleanup(func() {
		markdownSaveCommitHook = originalSaveHook
		markdownIdentityDeleteHook = originalDeleteHook
	})
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(util.DataDir, box.ID, "save-recovery-link.md"))
	if err != nil || markdownRevision(data) != created.Revision {
		t.Fatalf("source-isolated recovery promoted non-durable link: %q, %v", data, err)
	}
}

func TestSaveMarkdownRecoversAfterLogicalCommitCleanupCrash(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "save-cleanup.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "save" && phase == "source-removed" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	saved, err := SaveMarkdown(box.ID, created.Path, "committed", created.Revision)
	if err != nil {
		t.Fatalf("logical commit reported cleanup failure: %v", err)
	}
	if saved.Content != "committed" {
		t.Fatalf("unexpected saved content: %q", saved.Content)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	journals, err := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions", "*", "transaction.json"))
	if err != nil || len(journals) != 0 {
		t.Fatalf("cleanup recovery left journals: %v, %#v", err, journals)
	}
}

func TestRenameMarkdownRecoversAfterSourceRemoved(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "rename-cleanup.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionCrashHook
	markdownTransactionCrashHook = func(kind, phase string) error {
		if kind == "rename" && phase == "source-removed" {
			return ErrMarkdownSimulatedCrash
		}
		return nil
	}
	renamed, err := RenameMarkdownWithRevision(box.ID, created.Path, "rename-cleaned.md", created.Revision)
	if err != nil {
		t.Fatalf("logical commit reported cleanup failure: %v", err)
	}
	if renamed.Path != "/rename-cleaned.md" {
		t.Fatalf("unexpected rename result: %#v", renamed)
	}
	markdownTransactionCrashHook = originalHook
	t.Cleanup(func() { markdownTransactionCrashHook = originalHook })
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
}

func TestRenameMarkdownRollsBackSourceWhenStagedPhaseWriteFails(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "phase-write.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionWriteHook
	failed := false
	markdownTransactionWriteHook = func(kind, phase string) error {
		if !failed && kind == "rename" && phase == "source-staged" {
			failed = true
			return errors.New("phase write failed")
		}
		return nil
	}
	t.Cleanup(func() { markdownTransactionWriteHook = originalHook })
	if _, err = RenameMarkdownWithRevision(box.ID, created.Path, "phase-target.md", created.Revision); err == nil {
		t.Fatal("expected phase write failure")
	}
	source := filepath.Join(util.DataDir, box.ID, "phase-write.md")
	if data, readErr := os.ReadFile(source); readErr != nil || markdownRevision(data) != created.Revision {
		t.Fatalf("source was not immediately restored: %v", readErr)
	}
	markdownTransactionWriteHook = originalHook
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, box.ID, "phase-target.md")); !os.IsNotExist(err) {
		t.Fatalf("copied target survived rollback: %v", err)
	}
}

func TestSaveMarkdownRollsBackSourceWhenIsolatedPhaseWriteFails(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "save-phase.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionWriteHook
	failed := false
	markdownTransactionWriteHook = func(kind, phase string) error {
		if !failed && kind == "save" && phase == "source-isolated" {
			failed = true
			return errors.New("phase write failed")
		}
		return nil
	}
	t.Cleanup(func() { markdownTransactionWriteHook = originalHook })
	if _, err = SaveMarkdown(box.ID, created.Path, "new", created.Revision); err == nil {
		t.Fatal("expected phase write failure")
	}
	data, err := os.ReadFile(filepath.Join(util.DataDir, box.ID, "save-phase.md"))
	if err != nil || markdownRevision(data) != created.Revision {
		t.Fatalf("source was not immediately restored: %q, %v", data, err)
	}
}

func TestSaveMarkdownSyncsTransactionParentBeforeSourceIsolationIntent(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "durability-order.md")
	if err != nil {
		t.Fatal(err)
	}
	originalDurabilityHook := markdownDurabilityHook
	originalWriteHook := markdownTransactionWriteHook
	parentSynced := false
	markdownDurabilityHook = func(event, _ string) {
		if event == "transaction-parent-synced" {
			parentSynced = true
		}
	}
	markdownTransactionWriteHook = func(kind, phase string) error {
		if kind == "save" && phase == "source-isolating" && !parentSynced {
			return errors.New("source isolation intent preceded transaction parent sync")
		}
		return nil
	}
	t.Cleanup(func() {
		markdownDurabilityHook = originalDurabilityHook
		markdownTransactionWriteHook = originalWriteHook
	})
	if _, err = SaveMarkdown(box.ID, created.Path, "durable", created.Revision); err != nil {
		t.Fatal(err)
	}
}

func TestMarkdownMkdirSyncsEveryCreatedParent(t *testing.T) {
	box := setupMarkdownTest(t)
	originalHook := markdownDurabilityHook
	var synced []string
	markdownDurabilityHook = func(event, targetPath string) {
		if event == "mkdir-parent-synced" {
			synced = append(synced, targetPath)
		}
	}
	t.Cleanup(func() { markdownDurabilityHook = originalHook })
	target := filepath.Join(util.DataDir, box.ID, "one", "two", "three")
	if err := mkdirAllMarkdownContained(target, 0755); err != nil {
		t.Fatal(err)
	}
	want := []string{
		filepath.Join(util.DataDir, box.ID, "one"),
		filepath.Join(util.DataDir, box.ID, "one", "two"),
		target,
	}
	if !slices.Equal(synced, want) {
		t.Fatalf("created directory parents were not synced in order: got %#v want %#v", synced, want)
	}
}

func TestSaveMarkdownPreparedRecoveryRollsBackUncommittedPayload(t *testing.T) {
	box := setupMarkdownTest(t)
	created, err := CreateMarkdown(box.ID, "/", "prepared-payload.md")
	if err != nil {
		t.Fatal(err)
	}
	originalHook := markdownTransactionWriteHook
	failed := false
	markdownTransactionWriteHook = func(kind, phase string) error {
		if !failed && kind == "save" && phase == "staged" {
			failed = true
			return errors.New("staged phase write failed")
		}
		return nil
	}
	if _, err = SaveMarkdown(box.ID, created.Path, "unclassified", created.Revision); err == nil {
		t.Fatal("expected staged phase write failure")
	}
	markdownTransactionWriteHook = originalHook
	t.Cleanup(func() { markdownTransactionWriteHook = originalHook })
	if err = RecoverMarkdownTransactions(); err != nil {
		t.Fatalf("prepared recovery did not rollback: %v", err)
	}
	payloads, _ := filepath.Glob(filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions", "*", "payload.md"))
	if len(payloads) != 0 {
		t.Fatalf("rolled back payload survived: %#v", payloads)
	}
	data, err := os.ReadFile(filepath.Join(util.DataDir, box.ID, "prepared-payload.md"))
	if err != nil || markdownRevision(data) != created.Revision {
		t.Fatalf("prepared recovery changed old source: %q, %v", data, err)
	}
}

func TestRecycleAndRestoreMarkdownDoesNotResurrectRecentRecord(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 32
	data := []byte("deleted")
	writeMarkdownManagementFixture(t, box.ID, "/a.md", data)
	ref := MarkdownDocumentRef{Notebook: box.ID, Path: "/a.md"}
	if err := UpdateRecentMarkdownOpenTime(ref); err != nil {
		t.Fatal(err)
	}
	entry, err := RecycleMarkdown(ref, markdownRevision(data))
	if err != nil {
		t.Fatal(err)
	}
	if findRecentMarkdownTest(t, ref) != nil {
		t.Fatal("recycled Markdown recent record survived")
	}
	if _, err = RestoreDeletedMarkdown(entry.ID, box.ID, "/", "a.md"); err != nil {
		t.Fatal(err)
	}
	if findRecentMarkdownTest(t, ref) != nil {
		t.Fatal("restore resurrected stale recent timestamps")
	}
}

func TestRemoveMarkdownRemovesRecentRecord(t *testing.T) {
	box := setupMarkdownTest(t)
	Conf.FileTree.RecentDocsMaxListCount = 32
	created, err := CreateMarkdown(box.ID, "/", "a.md")
	if err != nil {
		t.Fatal(err)
	}
	ref := MarkdownDocumentRef{Notebook: box.ID, Path: created.Path}
	if err = UpdateRecentMarkdownOpenTime(ref); err != nil {
		t.Fatal(err)
	}
	if err = RemoveMarkdown(box.ID, created.Path); err != nil {
		t.Fatal(err)
	}
	if findRecentMarkdownTest(t, ref) != nil {
		t.Fatal("removed Markdown recent record survived")
	}
}

func TestMarkdownFilesIncludedInSyncSnapshots(t *testing.T) {
	originalConf, originalLangs := Conf, util.Langs
	Conf = NewAppConf()
	Conf.Lang = "en"
	util.Langs = map[string]map[int]string{"en": {158: "Indexing %s", 159: "Reading %d/%d", 160: "Writing %d/%d"}}
	t.Cleanup(func() {
		Conf, util.Langs = originalConf, originalLangs
	})

	root := t.TempDir()
	dataDir := filepath.Join(root, "data")
	markdownPath := filepath.Join(dataDir, "20260811000000-abcdefg", "sync.md")
	assetPath := filepath.Join(dataDir, "assets", "sync.png")
	for _, dir := range []string{filepath.Dir(markdownPath), filepath.Dir(assetPath)} {
		if err := os.MkdirAll(dir, 0755); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(markdownPath, []byte("![image](assets/sync.png)\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(assetPath, []byte("image"), 0644); err != nil {
		t.Fatal(err)
	}

	repo, err := dejavu.NewRepo(
		dataDir,
		filepath.Join(root, "repo"),
		filepath.Join(root, "history"),
		filepath.Join(root, "temp"),
		"device-id",
		"device-name",
		"darwin",
		bytes.Repeat([]byte{1}, 32),
		nil,
		nil,
	)
	if err != nil {
		t.Fatal(err)
	}
	indexContext := map[string]any{eventbus.CtxPushMsg: eventbus.CtxPushMsgToNone}
	index, err := repo.Index("Markdown sync test", false, indexContext)
	if err != nil {
		t.Fatal(err)
	}
	files, err := repo.GetFiles(index)
	if err != nil {
		t.Fatal(err)
	}
	paths := map[string]bool{}
	for _, file := range files {
		paths[file.Path] = true
	}
	if !paths["/20260811000000-abcdefg/sync.md"] || !paths["/assets/sync.png"] {
		t.Fatalf("Markdown and its asset must be included in the sync snapshot: %v", paths)
	}

	if err = os.WriteFile(markdownPath, []byte("updated\n"), 0644); err != nil {
		t.Fatal(err)
	}
	updatedTime := time.Now().Add(2 * time.Second)
	if err = os.Chtimes(markdownPath, updatedTime, updatedTime); err != nil {
		t.Fatal(err)
	}
	if err = os.Remove(assetPath); err != nil {
		t.Fatal(err)
	}
	index, err = repo.Index("Markdown sync update", false, indexContext)
	if err != nil {
		t.Fatal(err)
	}
	files, err = repo.GetFiles(index)
	if err != nil {
		t.Fatal(err)
	}
	paths = map[string]bool{}
	var syncedMarkdown []byte
	for _, file := range files {
		paths[file.Path] = true
		if file.Path == "/20260811000000-abcdefg/sync.md" {
			syncedMarkdown, err = repo.OpenFile(file)
			if err != nil {
				t.Fatal(err)
			}
		}
	}
	if paths["/assets/sync.png"] {
		t.Fatalf("deleted asset must be removed from the next sync snapshot: %v", paths)
	}
	if string(syncedMarkdown) != "updated\n" {
		t.Fatalf("updated Markdown content missing from sync snapshot: %q", syncedMarkdown)
	}
}

func TestMarkdownRejectsUnsafePathAndEncryptedNotebook(t *testing.T) {
	box := setupMarkdownTest(t)

	if _, err := GetMarkdown(box.ID, "../outside.md"); err == nil {
		t.Fatal("unsafe Markdown path should be rejected")
	}
	boxConf := box.GetConf()
	boxConf.Encrypted = true
	if err := box.SaveConf(boxConf); err != nil {
		t.Fatal(err)
	}
	if _, err := CreateMarkdown(box.ID, "/", "encrypted.md"); !errors.Is(err, ErrMarkdownEncryptedNotebook) {
		t.Fatalf("expected encrypted notebook rejection, got %v", err)
	}
}

func TestMarkdownAutoName(t *testing.T) {
	box := setupMarkdownTest(t)

	first, err := CreateMarkdown(box.ID, "/", "Untitled.md", true)
	if err != nil {
		t.Fatal(err)
	}
	second, err := CreateMarkdown(box.ID, "/", "Untitled.md", true)
	if err != nil {
		t.Fatal(err)
	}
	if first.Path != "/Untitled.md" || second.Path != "/Untitled 2.md" {
		t.Fatalf("unexpected automatic Markdown names: %q, %q", first.Path, second.Path)
	}
}

func TestMarkdownWYSIWYGRoundTrip(t *testing.T) {
	markdown := "# 标题\n\n开头 **粗体** 和 *斜体*。\n\n> 引用\n\n- [x] 任务\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n```ts\nconst value = 42;\n```\n\n![图片](https://example.com/image.png)\n\n结尾。\n"
	html := MarkdownToProtylePreviewHTML(markdown)
	html = strings.Replace(html, "开头", "修改后的开头", 1)
	html = strings.Replace(html, "结尾。", "修改后的结尾。", 1)
	html = strings.Replace(html, `<code class="hljs">`, `<code class="language-ts">`, 1)
	html = strings.Replace(html, `<img src=`, `<img width="320px" src=`, 1)
	converted, err := util.NewLute().HTML2Markdown(html)
	if err != nil {
		t.Fatal(err)
	}
	for _, expected := range []string{
		"# 标题", "修改后的开头", "**粗体**", "*斜体*", "> 引用", "[X] 任务",
		"|A|B|", "```ts", "const value = 42;", "![图片](https://example.com/image.png)",
		`{: style="width: 320px;"}`, "修改后的结尾。",
	} {
		if !strings.Contains(converted, expected) {
			t.Fatalf("converted Markdown should contain %q:\n%s", expected, converted)
		}
	}
	rendered := MarkdownToProtylePreviewHTML(converted)
	if !strings.Contains(rendered, `style="width: 320px;"`) {
		t.Fatalf("rendered image should preserve its width:\n%s", rendered)
	}
}
