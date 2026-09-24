package filesys

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestValidateTreePathRejectsInternalFiles(t *testing.T) {
	boxID := "20060102150405-1a2b3c4"
	for _, path := range []string{
		"/.siyuan/conf.json",
		"/.siyuan/sort.json",
		"/assets/20060102150405-1a2b3c4.sy",
		"/not-a-document.sy",
		"/20060102150405-1a2b3c4.txt",
		"/20060102150405-1a2b3c4/child.sy",
	} {
		if err := validateTreePath(boxID, path); err == nil {
			t.Fatalf("validateTreePath(%q) unexpectedly succeeded", path)
		}
	}
}

func TestValidateTreePathAcceptsDocuments(t *testing.T) {
	boxID := "20060102150405-1a2b3c4"
	for _, path := range []string{
		"/20060102150405-1a2b3c4.sy",
		"/20060102150406-2b3c4d5/20060102150407-3c4d5e6.sy",
	} {
		if err := validateTreePath(boxID, path); err != nil {
			t.Fatalf("validateTreePath(%q) failed: %v", path, err)
		}
	}
}

func TestValidateTreeFilePathRejectsSymlink(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symbolic links require elevated privileges on Windows")
	}
	oldDataDir := util.DataDir
	dataDir := t.TempDir()
	util.DataDir = dataDir
	defer func() { util.DataDir = oldDataDir }()

	boxID := "20060102150405-1a2b3c4"
	linkID := "20060102150406-2b3c4d5"
	docID := "20060102150407-3c4d5e6"
	boxDir := filepath.Join(dataDir, boxID)
	outsideDir := t.TempDir()
	if err := os.MkdirAll(boxDir, 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outsideDir, filepath.Join(boxDir, linkID)); err != nil {
		t.Skipf("symbolic links are unavailable: %v", err)
	}
	if _, err := validateTreeFilePath(boxID, "/"+linkID+"/"+docID+".sy", true); err == nil {
		t.Fatal("expected symlink path to be rejected")
	}
}
