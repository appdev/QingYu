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
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/88250/go-humanize"
	"github.com/siyuan-note/filelock"
	"github.com/siyuan-note/siyuan/kernel/filesys"
	"github.com/siyuan-note/siyuan/kernel/util"
)

var (
	ErrMarkdownConflict          = errors.New("markdown file has been modified")
	ErrMarkdownEncryptedNotebook = errors.New("encrypted notebooks do not support Markdown files")
	markdownFileOperationLock    sync.Mutex
)

type MarkdownDocument struct {
	Path     string `json:"path"`
	Name     string `json:"name"`
	Content  string `json:"content"`
	Revision string `json:"revision"`
	Mtime    int64  `json:"mtime"`
}

func CreateMarkdown(boxID, parentPath, name string, autoName ...bool) (ret *MarkdownDocument, err error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()

	name = strings.TrimSpace(name)
	if "" == name {
		return nil, errors.New("Markdown file name must not be empty")
	}
	if strings.ContainsAny(name, "/\\") || util.IsReservedFilename(name) {
		return nil, errors.New("invalid Markdown file name")
	}
	if "" == filepath.Ext(name) {
		name += ".md"
	}
	if !isMarkdownFileName(name) {
		return nil, errors.New("Markdown file name must end with .md or .markdown")
	}

	parentPath = filepath.ToSlash(strings.TrimSpace(parentPath))
	if strings.HasSuffix(strings.ToLower(parentPath), ".sy") {
		parentPath = strings.TrimSuffix(parentPath, filepath.Ext(parentPath))
	}
	p := path.Join(parentPath, name)
	if 0 < len(autoName) && autoName[0] {
		ext := filepath.Ext(name)
		baseName := strings.TrimSuffix(name, ext)
		for index := 2; ; index++ {
			_, candidateAbsPath, pathErr := markdownFilePath(boxID, p)
			if pathErr != nil {
				return nil, pathErr
			}
			if !filelock.IsExist(candidateAbsPath) {
				break
			}
			p = path.Join(parentPath, baseName+" "+strconv.Itoa(index)+ext)
		}
	}
	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return nil, err
	}
	if err = os.MkdirAll(filepath.Dir(absPath), 0755); err != nil {
		return nil, err
	}
	file, openErr := os.OpenFile(absPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0644)
	if openErr != nil {
		if os.IsExist(openErr) {
			return nil, errors.New("Markdown file already exists")
		}
		return nil, openErr
	}
	if closeErr := file.Close(); closeErr != nil {
		return nil, closeErr
	}

	IncSync()
	pushMarkdownFileEvent("createMarkdown", boxID, canonicalPath, "")
	return markdownDocument(canonicalPath, nil, absPath)
}

func GetMarkdown(boxID, p string) (ret *MarkdownDocument, err error) {
	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return nil, err
	}
	data, err := filelock.ReadFile(absPath)
	if err != nil {
		return nil, err
	}
	return markdownDocument(canonicalPath, data, absPath)
}

func SaveMarkdown(boxID, p, content, revision string) (ret *MarkdownDocument, err error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()

	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return nil, err
	}
	current, err := filelock.ReadFile(absPath)
	if err != nil {
		return nil, err
	}
	if markdownRevision(current) != revision {
		return nil, ErrMarkdownConflict
	}
	data := []byte(content)
	if err = filelock.WriteFile(absPath, data); err != nil {
		return nil, err
	}

	IncSync()
	pushMarkdownFileEvent("saveMarkdown", boxID, canonicalPath, "")
	return markdownDocument(canonicalPath, data, absPath)
}

func RenameMarkdown(boxID, p, name string) (ret *MarkdownDocument, err error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()

	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return nil, err
	}
	name = strings.TrimSpace(name)
	if "" == filepath.Ext(name) {
		name += ".md"
	}
	if strings.ContainsAny(name, "/\\") || util.IsReservedFilename(name) || !isMarkdownFileName(name) {
		return nil, errors.New("invalid Markdown file name")
	}
	newPath := path.Join(path.Dir(canonicalPath), name)
	canonicalNewPath, newAbsPath, err := markdownFilePath(boxID, newPath)
	if err != nil {
		return nil, err
	}
	if filelock.IsExist(newAbsPath) {
		return nil, errors.New("Markdown file already exists")
	}
	if err = filelock.Rename(absPath, newAbsPath); err != nil {
		return nil, err
	}

	IncSync()
	pushMarkdownFileEvent("renameMarkdown", boxID, canonicalNewPath, canonicalPath)
	return GetMarkdown(boxID, canonicalNewPath)
}

func RemoveMarkdown(boxID, p string) (err error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()

	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return err
	}
	if err = filelock.Remove(absPath); err != nil {
		return err
	}

	IncSync()
	pushMarkdownFileEvent("removeMarkdown", boxID, canonicalPath, "")
	return nil
}

func markdownFilePath(boxID, p string) (canonicalPath, absPath string, err error) {
	if nil == Conf.Box(boxID) {
		return "", "", ErrBoxNotFound
	}
	if IsEncryptedBox(boxID) {
		return "", "", ErrMarkdownEncryptedNotebook
	}
	normalized, err := filesys.ValidateBoxRelativePath(boxID, p)
	if err != nil {
		return "", "", err
	}
	if "" == normalized || !isMarkdownFileName(filepath.Base(normalized)) {
		return "", "", errors.New("Markdown path must end with .md or .markdown")
	}
	canonicalPath = "/" + filepath.ToSlash(normalized)
	absPath = filepath.Join(util.DataDir, boxID, normalized)
	return
}

func markdownDocument(p string, data []byte, absPath string) (ret *MarkdownDocument, err error) {
	info, err := os.Stat(absPath)
	if err != nil {
		return nil, err
	}
	return &MarkdownDocument{
		Path:     p,
		Name:     path.Base(p),
		Content:  string(data),
		Revision: markdownRevision(data),
		Mtime:    info.ModTime().UnixMilli(),
	}, nil
}

func markdownRevision(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func isMarkdownFileName(name string) bool {
	ext := strings.ToLower(filepath.Ext(name))
	return ".md" == ext || ".markdown" == ext
}

func pushMarkdownFileEvent(cmd, boxID, p, oldPath string) {
	evt := util.NewCmdResult(cmd, 0, util.PushModeBroadcast)
	evt.Data = map[string]any{
		"box":     boxID,
		"path":    p,
		"oldPath": oldPath,
		"time":    time.Now().UnixMilli(),
	}
	util.PushEvent(evt)
}

func markdownFileDisplayInfo(boxID string, fileInfo *FileInfo) *File {
	modified := time.Unix(0, 0)
	if info, err := os.Stat(filepath.Join(util.DataDir, boxID, filepath.FromSlash(strings.TrimPrefix(fileInfo.path, "/")))); err == nil {
		modified = info.ModTime()
	}
	return &File{
		Path:    fileInfo.path,
		Name:    fileInfo.name,
		DocType: "markdown",
		Size:    uint64(fileInfo.size),
		HSize:   humanize.BytesCustomCeil(uint64(fileInfo.size), 2),
		Mtime:   modified.Unix(),
		CTime:   modified.Unix(),
		HMtime:  modified.Format("2006-01-02 15:04:05"),
		HCtime:  modified.Format("2006-01-02 15:04:05"),
	}
}
