// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"

	"github.com/88250/lute/ast"
	"github.com/siyuan-note/logging"
	ksql "github.com/siyuan-note/siyuan/kernel/sql"
	"github.com/siyuan-note/siyuan/kernel/util"
)

const (
	notebookHomePath = ".qingyu/home.md"
)

var (
	ErrInvalidNotebookInternalPath = errors.New("invalid notebook internal path")
	ErrNotebookHomeConflict        = errors.New("notebook home has been modified")
	notebookHomeLock               sync.Mutex
	notebookRecoveryNamePattern    = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,127}\.md$`)
)

type NotebookHomeDocument struct {
	Notebook    string `json:"notebook"`
	Name        string `json:"name"`
	Content     string `json:"content"`
	Revision    string `json:"revision"`
	Mtime       int64  `json:"mtime"`
	Exists      bool   `json:"exists"`
	OperationID string `json:"operationID,omitempty"`
}

func GetNotebookHome(boxID string) (*NotebookHomeDocument, error) {
	if err := validateNotebookHomeBox(boxID); err != nil {
		return nil, err
	}
	data, err := ReadNotebookInternalFile(boxID, notebookHomePath)
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	return notebookHomeDocument(boxID, data, err == nil, "")
}

func SaveNotebookHome(boxID, content, revision, requestedOperationID string) (*NotebookHomeDocument, error) {
	if err := validateNotebookHomeBox(boxID); err != nil {
		return nil, err
	}
	operationID, err := resolveMarkdownOperationID(requestedOperationID)
	if err != nil {
		return nil, err
	}

	notebookHomeLock.Lock()
	defer notebookHomeLock.Unlock()

	current, readErr := ReadNotebookInternalFile(boxID, notebookHomePath)
	if readErr != nil && !errors.Is(readErr, os.ErrNotExist) {
		return nil, readErr
	}
	if markdownRevision(current) != revision {
		return nil, ErrNotebookHomeConflict
	}

	data := []byte(content)
	if strings.TrimSpace(content) == "" {
		if readErr == nil {
			if err = removeNotebookInternalFile(boxID, notebookHomePath); err != nil {
				return nil, err
			}
		}
		data = nil
	} else if err = WriteNotebookInternalFile(boxID, notebookHomePath, data); err != nil {
		return nil, err
	}

	if len(data) == 0 {
		if indexErr := ksql.DeleteNotebookHome(boxID); indexErr != nil {
			logging.LogWarnf("delete notebook home index [%s] failed: %s", boxID, indexErr)
		}
	} else if indexErr := indexNotebookHomeContent(boxID, string(data)); indexErr != nil {
		logging.LogWarnf("index notebook home [%s] failed: %s", boxID, indexErr)
	}
	IncSync()
	evt := util.NewCmdResult("saveNotebookHome", 0, util.PushModeBroadcast)
	evt.Data = map[string]any{
		"kind":        "notebook-home",
		"box":         boxID,
		"operationID": operationID,
		"revision":    markdownRevision(data),
		"time":        time.Now().UnixMilli(),
	}
	util.PushEvent(evt)
	return notebookHomeDocument(boxID, data, len(data) > 0, operationID)
}

func ReadNotebookInternalFile(boxID, relativePath string) ([]byte, error) {
	absPath, err := notebookInternalFilePath(boxID, relativePath)
	if err != nil {
		return nil, err
	}
	if IsEncryptedBox(boxID) {
		HoldBoxReadLock(boxID)
		defer ReleaseBoxReadLock(boxID)
	}
	if err = validatePathWithoutSymlinks(filepath.Join(util.DataDir, boxID), absPath, false); err != nil {
		return nil, errors.Join(ErrInvalidNotebookInternalPath, err)
	}
	data, err := os.ReadFile(absPath)
	if err != nil {
		return nil, err
	}
	if !IsEncryptedBox(boxID) {
		return data, nil
	}
	dek, err := GetDEKIfUnlocked(boxID)
	if err != nil {
		return nil, err
	}
	return DecryptNotebookInternalFile(boxID, relativePath, dek, data)
}

func WriteNotebookInternalFile(boxID, relativePath string, data []byte) error {
	absPath, err := notebookInternalFilePath(boxID, relativePath)
	if err != nil {
		return err
	}
	boxRoot := filepath.Join(util.DataDir, boxID)
	if IsEncryptedBox(boxID) {
		HoldBoxReadLock(boxID)
		defer ReleaseBoxReadLock(boxID)
		dek, dekErr := GetDEKIfUnlocked(boxID)
		if dekErr != nil {
			return dekErr
		}
		data, err = EncryptNotebookInternalFile(boxID, relativePath, dek, data)
		if err != nil {
			return err
		}
	}
	if err = mkdirAllMarkdownContained(filepath.Dir(absPath), 0755); err != nil {
		return errors.Join(ErrInvalidNotebookInternalPath, err)
	}
	if err = validatePathWithoutSymlinks(boxRoot, absPath, true); err != nil {
		return errors.Join(ErrInvalidNotebookInternalPath, err)
	}

	tmp, err := os.CreateTemp(filepath.Dir(absPath), ".qingyu-stage-*")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	committed := false
	defer func() {
		_ = tmp.Close()
		if !committed {
			_ = os.Remove(tmpPath)
		}
	}()
	if err = tmp.Chmod(0600); err != nil {
		return err
	}
	if _, err = tmp.Write(data); err != nil {
		return err
	}
	if err = tmp.Sync(); err != nil {
		return err
	}
	if err = tmp.Close(); err != nil {
		return err
	}
	if err = validatePathWithoutSymlinks(boxRoot, absPath, true); err != nil {
		return errors.Join(ErrInvalidNotebookInternalPath, err)
	}
	if err = os.Rename(tmpPath, absPath); err != nil {
		return err
	}
	committed = true
	return syncMarkdownParent(absPath)
}

func EncryptNotebookInternalFile(boxID, relativePath string, dek, plaintext []byte) ([]byte, error) {
	if _, err := notebookInternalFilePath(boxID, relativePath); err != nil {
		return nil, err
	}
	key := util.DeriveSubKey(dek, "qingyu/notebook-home")
	return util.EncryptWithAAD(key, plaintext, []byte(notebookInternalAAD(boxID, relativePath)))
}

func DecryptNotebookInternalFile(boxID, relativePath string, dek, ciphertext []byte) ([]byte, error) {
	if _, err := notebookInternalFilePath(boxID, relativePath); err != nil {
		return nil, err
	}
	key := util.DeriveSubKey(dek, "qingyu/notebook-home")
	return util.DecryptWithAAD(key, ciphertext, []byte(notebookInternalAAD(boxID, relativePath)))
}

func notebookInternalAAD(boxID, relativePath string) string {
	return "qingyu:v1:notebook-home:" + boxID + ":" + filepath.ToSlash(relativePath)
}

func notebookInternalFilePath(boxID, relativePath string) (string, error) {
	if !ast.IsNodeIDPattern(boxID) {
		return "", ErrInvalidNotebookInternalPath
	}
	relativePath = filepath.ToSlash(relativePath)
	allowed := relativePath == notebookHomePath || relativePath == ".qingyu/home.json" ||
		relativePath == notebookRootMigrationMarkerPath
	if strings.HasPrefix(relativePath, ".qingyu/recovery/") {
		allowed = notebookRecoveryNamePattern.MatchString(strings.TrimPrefix(relativePath, ".qingyu/recovery/"))
	}
	if !allowed || filepath.IsAbs(relativePath) || strings.Contains(relativePath, "\\") {
		return "", ErrInvalidNotebookInternalPath
	}
	absPath := filepath.Join(util.DataDir, boxID, filepath.FromSlash(relativePath))
	boxRoot := filepath.Join(util.DataDir, boxID)
	rel, err := filepath.Rel(boxRoot, absPath)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "", ErrInvalidNotebookInternalPath
	}
	return absPath, nil
}

func removeNotebookInternalFile(boxID, relativePath string) error {
	absPath, err := notebookInternalFilePath(boxID, relativePath)
	if err != nil {
		return err
	}
	if IsEncryptedBox(boxID) {
		HoldBoxReadLock(boxID)
		defer ReleaseBoxReadLock(boxID)
		if _, err = GetDEKIfUnlocked(boxID); err != nil {
			return err
		}
	}
	if err = validatePathWithoutSymlinks(filepath.Join(util.DataDir, boxID), absPath, true); err != nil {
		return errors.Join(ErrInvalidNotebookInternalPath, err)
	}
	if err = os.Remove(absPath); err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return syncMarkdownParent(absPath)
}

func notebookHomeDocument(boxID string, data []byte, exists bool, operationID string) (*NotebookHomeDocument, error) {
	name := boxID
	if boxConf := (&Box{ID: boxID}).GetConf(); nil != boxConf && boxConf.Name != "" {
		name = boxConf.Name
	}
	mtime := int64(0)
	if exists {
		if info, err := os.Stat(filepath.Join(util.DataDir, boxID, filepath.FromSlash(notebookHomePath))); err == nil {
			mtime = info.ModTime().UnixMilli()
		}
	}
	return &NotebookHomeDocument{
		Notebook:    boxID,
		Name:        name,
		Content:     string(data),
		Revision:    markdownRevision(data),
		Mtime:       mtime,
		Exists:      exists,
		OperationID: operationID,
	}, nil
}

func validateNotebookHomeBox(boxID string) error {
	if !ast.IsNodeIDPattern(boxID) {
		return fmt.Errorf("invalid notebook ID [%s]", boxID)
	}
	if nil != Conf && nil == Conf.Box(boxID) {
		return ErrBoxNotFound
	}
	boxRoot := filepath.Join(util.DataDir, boxID)
	info, err := os.Lstat(boxRoot)
	if err != nil {
		return err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return ErrInvalidNotebookInternalPath
	}
	return nil
}

func IndexNotebookHome(boxID string) error {
	home, err := GetNotebookHome(boxID)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return ksql.DeleteNotebookHome(boxID)
		}
		return err
	}
	if !home.Exists || strings.TrimSpace(home.Content) == "" {
		return ksql.DeleteNotebookHome(boxID)
	}
	return ksql.UpsertNotebookHome(boxID, home.Name, home.Content, home.Mtime)
}

func indexNotebookHomeContent(boxID, content string) error {
	document, err := notebookHomeDocument(boxID, []byte(content), true, "")
	if err != nil {
		return err
	}
	return ksql.UpsertNotebookHome(boxID, document.Name, content, document.Mtime)
}
