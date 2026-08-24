//go:build windows

package model

import (
	"os"
	"path/filepath"
	"testing"
)

func TestMarkdownPlatformIdentityTracksWindowsFileIndex(t *testing.T) {
	dir := t.TempDir()
	original := filepath.Join(dir, "original.md")
	linked := filepath.Join(dir, "linked.md")
	if err := os.WriteFile(original, []byte("same"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.Link(original, linked); err != nil {
		t.Fatal(err)
	}
	identity := func(name string) string {
		file, err := os.Open(name)
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		info, err := file.Stat()
		if err != nil {
			t.Fatal(err)
		}
		return markdownPlatformFileIdentity(file, info)
	}
	originalID := identity(original)
	if originalID == "" || identity(linked) != originalID {
		t.Fatal("hard links did not share a stable platform identity")
	}
	if err := os.Remove(linked); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(linked, []byte("same"), 0644); err != nil {
		t.Fatal(err)
	}
	if identity(linked) == originalID {
		t.Fatal("replacement reused the original platform identity")
	}
}
