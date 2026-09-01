// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/siyuan-note/siyuan/kernel/conf"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestNotebookHomeStorage(t *testing.T) {
	originalDataDir := util.DataDir
	originalConf := Conf
	util.DataDir = t.TempDir()
	Conf = NewAppConf()
	Conf.Sync = conf.NewSync()
	Conf.FileTree = conf.NewFileTree()
	Conf.NotebookCrypto = conf.NewNotebookCrypto()
	t.Cleanup(func() {
		util.DataDir = originalDataDir
		Conf = originalConf
	})
	boxID := "20260831120000-home001"
	if err := os.MkdirAll(filepath.Join(util.DataDir, boxID), 0755); err != nil {
		t.Fatal(err)
	}
	boxConf := conf.NewBoxConf()
	boxConf.Name = "Home"
	boxConf.Closed = false
	if err := (&Box{ID: boxID}).SaveConf(boxConf); err != nil {
		t.Fatal(err)
	}

	home, err := GetNotebookHome(boxID)
	if err != nil {
		t.Fatal(err)
	}
	if home.Exists || home.Content != "" || home.Revision != markdownRevision(nil) {
		t.Fatalf("unexpected missing home: %#v", home)
	}

	home, err = SaveNotebookHome(boxID, "# 首页\n", home.Revision, "save-1")
	if err != nil {
		t.Fatal(err)
	}
	if !home.Exists || home.Content != "# 首页\n" || home.OperationID != "save-1" {
		t.Fatalf("unexpected saved home: %#v", home)
	}
	if _, err = SaveNotebookHome(boxID, "stale", markdownRevision(nil), "save-2"); !errors.Is(err, ErrNotebookHomeConflict) {
		t.Fatalf("expected conflict, got %v", err)
	}

	home, err = SaveNotebookHome(boxID, " \n\t", home.Revision, "save-3")
	if err != nil {
		t.Fatal(err)
	}
	if home.Exists || home.Content != "" {
		t.Fatalf("empty save must remove home: %#v", home)
	}
	if _, err = os.Stat(filepath.Join(util.DataDir, boxID, notebookHomePath)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("home file still exists: %v", err)
	}
}

func TestNotebookInternalPathValidation(t *testing.T) {
	originalDataDir := util.DataDir
	util.DataDir = t.TempDir()
	t.Cleanup(func() { util.DataDir = originalDataDir })
	boxID := "20260831120000-home002"
	if err := os.MkdirAll(filepath.Join(util.DataDir, boxID), 0755); err != nil {
		t.Fatal(err)
	}

	for _, invalid := range []string{"home.md", ".qingyu/../home.md", ".qingyu/private.txt", ".qingyu/recovery/../home.md"} {
		if _, err := notebookInternalFilePath(boxID, invalid); !errors.Is(err, ErrInvalidNotebookInternalPath) {
			t.Fatalf("path %q should be rejected, got %v", invalid, err)
		}
	}
	if err := os.Symlink(t.TempDir(), filepath.Join(util.DataDir, boxID, ".qingyu")); err != nil {
		t.Fatal(err)
	}
	if err := WriteNotebookInternalFile(boxID, notebookHomePath, []byte("secret")); !errors.Is(err, ErrInvalidNotebookInternalPath) {
		t.Fatalf("symlinked internal directory should be rejected, got %v", err)
	}
}

func TestEncryptNotebookInternalFile(t *testing.T) {
	originalDataDir := util.DataDir
	util.DataDir = t.TempDir()
	t.Cleanup(func() { util.DataDir = originalDataDir })
	boxID := "20260831120000-home003"
	if err := os.MkdirAll(filepath.Join(util.DataDir, boxID), 0755); err != nil {
		t.Fatal(err)
	}
	dek := bytes.Repeat([]byte{0x2a}, 32)
	plaintext := []byte("private notebook home")

	ciphertext, err := EncryptNotebookInternalFile(boxID, notebookHomePath, dek, plaintext)
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(ciphertext, plaintext) {
		t.Fatal("ciphertext contains plaintext")
	}
	decrypted, err := DecryptNotebookInternalFile(boxID, notebookHomePath, dek, ciphertext)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(decrypted, plaintext) {
		t.Fatalf("decrypted mismatch: %q", decrypted)
	}
	if _, err = DecryptNotebookInternalFile("20260831120000-home004", notebookHomePath, dek, ciphertext); err == nil {
		t.Fatal("another notebook AAD should fail")
	}
	if _, err = DecryptNotebookInternalFile(boxID, ".qingyu/recovery/home.md", dek, ciphertext); err == nil {
		t.Fatal("another path AAD should fail")
	}
}

func TestNotebookHomeEncryptedStorageLocksAndKeepsCiphertext(t *testing.T) {
	box := setupMarkdownManagementTest(t)
	boxConf := box.GetConf()
	boxConf.Encrypted = true
	boxConf.Closed = false
	if err := box.SaveConf(boxConf); err != nil {
		t.Fatal(err)
	}
	dek := bytes.Repeat([]byte{0x3c}, 32)
	setDEKForTest(box.ID, append([]byte(nil), dek...))
	lockForTest := func() {
		acquireBoxWriteLock(box.ID)
		lockBoxHeld(box.ID)
		releaseBoxWriteLock(box.ID)
	}
	t.Cleanup(lockForTest)

	home, err := GetNotebookHome(box.ID)
	if err != nil {
		t.Fatal(err)
	}
	const content = "加密首页正文"
	if _, err = SaveNotebookHome(box.ID, content, home.Revision, "encrypted-save"); err != nil {
		t.Fatal(err)
	}
	raw, err := os.ReadFile(filepath.Join(util.DataDir, box.ID, notebookHomePath))
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(raw, []byte(content)) {
		t.Fatal("encrypted notebook home contains plaintext")
	}

	lockForTest()
	if _, err = GetNotebookHome(box.ID); err == nil {
		t.Fatal("locked encrypted notebook home was readable")
	}
	setDEKForTest(box.ID, append([]byte(nil), dek...))
	readBack, err := GetNotebookHome(box.ID)
	if err != nil || readBack.Content != content {
		t.Fatalf("unexpected unlocked home: %#v, %v", readBack, err)
	}
}
