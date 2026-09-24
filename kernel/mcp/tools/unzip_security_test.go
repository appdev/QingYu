package tools

import (
	"archive/zip"
	"os"
	"path/filepath"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestExtractGuardedArchiveRejectsTraversal(t *testing.T) {
	workspace := t.TempDir()
	oldWorkspace, oldConf := util.WorkspaceDir, util.ConfDir
	util.WorkspaceDir = workspace
	util.ConfDir = filepath.Join(workspace, "conf")
	defer func() {
		util.WorkspaceDir, util.ConfDir = oldWorkspace, oldConf
	}()

	zipPath := filepath.Join(workspace, "payload.zip")
	archive, err := os.Create(zipPath)
	if err != nil {
		t.Fatal(err)
	}
	writer := zip.NewWriter(archive)
	entry, err := writer.Create("../outside.txt")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = entry.Write([]byte("must not escape")); err != nil {
		t.Fatal(err)
	}
	if err = writer.Close(); err != nil {
		t.Fatal(err)
	}
	if err = archive.Close(); err != nil {
		t.Fatal(err)
	}

	destination := filepath.Join(workspace, "out")
	if err = extractGuardedArchive(zipPath, destination); err == nil {
		t.Fatal("expected archive traversal to be rejected")
	}
	if _, err = os.Stat(filepath.Join(workspace, "outside.txt")); !os.IsNotExist(err) {
		t.Fatalf("archive created file outside destination: %v", err)
	}
}

func TestValidateArchiveLimits(t *testing.T) {
	if err := validateArchiveLimits(make([]*zip.File, maxArchiveEntries+1)); err == nil {
		t.Fatal("expected entry count limit")
	}
	if err := validateArchiveLimits([]*zip.File{{FileHeader: zip.FileHeader{Name: "large", UncompressedSize64: maxArchiveEntryBytes + 1}}}); err == nil {
		t.Fatal("expected per-entry size limit")
	}
	if err := validateArchiveLimits([]*zip.File{
		{FileHeader: zip.FileHeader{Name: "first", UncompressedSize64: maxArchiveTotalBytes / 2}},
		{FileHeader: zip.FileHeader{Name: "second", UncompressedSize64: maxArchiveTotalBytes/2 + 1}},
	}); err == nil {
		t.Fatal("expected total size limit")
	}
}
