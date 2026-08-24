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
	"encoding/json"
	"errors"
	"io"
	"maps"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/88250/go-humanize"
	"github.com/88250/gulu"
	"github.com/siyuan-note/filelock"
	"github.com/siyuan-note/siyuan/kernel/filesys"
	"github.com/siyuan-note/siyuan/kernel/util"
)

var (
	ErrMarkdownConflict             = errors.New("markdown file has been modified")
	ErrMarkdownEncryptedNotebook    = errors.New("encrypted notebooks do not support Markdown files")
	ErrInvalidMarkdownPath          = errors.New("invalid Markdown path")
	ErrInvalidMarkdownOperationID   = errors.New("invalid Markdown operation ID")
	markdownFileOperationLock       sync.Mutex
	markdownPushEvent               = util.PushEvent
	markdownBeforeDestinationCommit = func(string) error { return nil }
	markdownAfterRevisionCheck      = func(string) error { return nil }
	markdownAfterRootComponentCheck = func(string) error { return nil }
	markdownBeforeChangeSortCommit  = func() {}
)

type MarkdownDocument struct {
	Path        string `json:"path"`
	Name        string `json:"name"`
	Content     string `json:"content"`
	Revision    string `json:"revision"`
	Mtime       int64  `json:"mtime"`
	OperationID string `json:"operationID,omitempty"`
}

func CreateMarkdown(boxID, parentPath, name string, autoName ...bool) (ret *MarkdownDocument, err error) {
	auto := len(autoName) > 0 && autoName[0]
	return createMarkdown(boxID, parentPath, name, auto, "")
}

func CreateMarkdownWithOperationID(boxID, parentPath, name string, autoName bool, operationID string) (*MarkdownDocument, error) {
	return createMarkdown(boxID, parentPath, name, autoName, operationID)
}

func createMarkdown(boxID, parentPath, name string, autoName bool, requestedOperationID string) (ret *MarkdownDocument, err error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	operationID, err := resolveMarkdownOperationID(requestedOperationID)
	if err != nil {
		return nil, err
	}
	if err = recoverMarkdownTransactionsLocked(); err != nil {
		return nil, err
	}

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
	if autoName {
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
	if err = mkdirAllMarkdownContained(filepath.Dir(absPath), 0755); err != nil {
		return nil, err
	}
	file, openErr := openMarkdownFileNoReplace(absPath, 0644)
	if openErr != nil {
		if os.IsExist(openErr) {
			return nil, errors.New("Markdown file already exists")
		}
		return nil, openErr
	}
	if chmodErr := file.Chmod(0644); chmodErr != nil {
		_ = file.Close()
		_ = cleanupMarkdownCreatedFile(absPath)
		return nil, chmodErr
	}
	if syncErr := file.Sync(); syncErr != nil {
		_ = file.Close()
		_ = cleanupMarkdownCreatedFile(absPath)
		return nil, syncErr
	}
	if closeErr := file.Close(); closeErr != nil {
		return nil, closeErr
	}
	if err = syncMarkdownParent(absPath); err != nil {
		return nil, err
	}
	if err = addMarkdownSortKey(boxID, canonicalPath); err != nil {
		return nil, errors.Join(err, cleanupMarkdownCreatedFile(absPath))
	}

	IncSync()
	pushMarkdownFileEventWithOperation("createMarkdown", boxID, canonicalPath, "", "", operationID)
	ret, err = markdownDocument(canonicalPath, nil, absPath)
	if ret != nil {
		ret.OperationID = operationID
	}
	return ret, err
}

func cleanupMarkdownCreatedFile(filePath string) error {
	identity, err := markdownIdentity(filePath)
	if os.IsNotExist(err) {
		return nil
	}
	if err != nil {
		return err
	}
	return removeMarkdownFileWithIdentity(filePath, identity)
}

func GetMarkdown(boxID, p string) (ret *MarkdownDocument, err error) {
	markdownFileOperationLock.Lock()
	if err = recoverMarkdownTransactionsLocked(); err != nil {
		markdownFileOperationLock.Unlock()
		return nil, err
	}
	markdownFileOperationLock.Unlock()
	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return nil, err
	}
	data, err := readMarkdownFileContained(absPath)
	if err != nil {
		return nil, err
	}
	return markdownDocument(canonicalPath, data, absPath)
}

func SaveMarkdown(boxID, p, content, revision string) (ret *MarkdownDocument, err error) {
	return SaveMarkdownWithOperationID(boxID, p, content, revision, "")
}

func SaveMarkdownWithOperationID(boxID, p, content, revision, requestedOperationID string) (ret *MarkdownDocument, err error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	operationID, err := resolveMarkdownOperationID(requestedOperationID)
	if err != nil {
		return nil, err
	}
	if err = recoverMarkdownTransactionsLocked(); err != nil {
		return nil, err
	}

	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return nil, err
	}
	current, err := readMarkdownFileContained(absPath)
	if err != nil {
		return nil, err
	}
	if markdownRevision(current) != revision {
		return nil, ErrMarkdownConflict
	}
	data := []byte(content)
	tx, err := beginMarkdownTransaction("save", boxID, absPath, "", markdownRevision(data))
	if err != nil {
		return nil, err
	}
	if err = stageMarkdownSave(tx, data); err != nil {
		return nil, err
	}
	if err = markdownTransactionCrashHook("save", "staged"); err != nil {
		return nil, err
	}
	if err = commitMarkdownSave(tx); err != nil {
		return nil, err
	}
	// 文件已安装即为逻辑提交；事务清理可由恢复门禁重试，不阻止成功通知。
	_ = finalizeMarkdownSave(tx)

	IncSync()
	pushMarkdownFileEventWithOperation("saveMarkdown", boxID, canonicalPath, "", "", operationID)
	ret, err = markdownDocument(canonicalPath, data, absPath)
	if ret != nil {
		ret.OperationID = operationID
	}
	return ret, err
}

func RenameMarkdown(boxID, p, name string) (ret *MarkdownDocument, err error) {
	return renameMarkdown(boxID, p, name, "", false)
}

func RenameMarkdownWithRevision(boxID, p, name, revision string, operationIDs ...string) (ret *MarkdownDocument, err error) {
	return renameMarkdown(boxID, p, name, revision, true, operationIDs...)
}

func renameMarkdown(boxID, p, name, revision string, checkRevision bool, operationIDs ...string) (ret *MarkdownDocument, err error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	operationID, err := resolveMarkdownOperationID(operationIDs...)
	if err != nil {
		return nil, err
	}
	if err = recoverMarkdownTransactionsLocked(); err != nil {
		return nil, err
	}

	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return nil, err
	}
	if checkRevision {
		data, readErr := readMarkdownFileContained(absPath)
		if readErr != nil {
			return nil, readErr
		}
		if markdownRevision(data) != revision {
			return nil, ErrMarkdownConflict
		}
		if err = markdownAfterRevisionCheck(absPath); err != nil {
			return nil, err
		}
		data, err = readMarkdownFileContained(absPath)
		if err != nil {
			return nil, err
		}
		if markdownRevision(data) != revision {
			return nil, ErrMarkdownConflict
		}
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
	sortSnapshot, err := snapshotMarkdownSort(boxID)
	if err != nil {
		return nil, err
	}
	recentDocLock.Lock()
	defer recentDocLock.Unlock()
	recentSnapshot, err := loadRecentDocsRaw()
	if err != nil {
		return nil, err
	}
	recentSnapshot = cloneRecentDocs(recentSnapshot)
	tx, err := beginMarkdownTransaction("rename", boxID, absPath, newAbsPath, "")
	if err != nil {
		return nil, err
	}
	if err = recordMarkdownTransactionMetadata(tx, sortSnapshot, nil, recentSnapshot); err != nil {
		return nil, err
	}
	if err = markdownBeforeDestinationCommit(newAbsPath); err != nil {
		_ = finishMarkdownTransaction(tx)
		return nil, err
	}
	if err = copyMarkdownTransactionFile(tx); err != nil {
		return nil, err
	}
	if err = markdownTransactionCrashHook("rename", "copied"); err != nil {
		return nil, err
	}
	if err = stageMarkdownTransactionSource(tx); err != nil {
		return nil, err
	}
	if err = markdownTransactionCrashHook("rename", "source-staged"); err != nil {
		return nil, err
	}
	sortValues := maps.Clone(sortSnapshot.values)
	oldSortKey, newSortKey := fileTreeSortKey(canonicalPath), fileTreeSortKey(canonicalNewPath)
	oldSort := sortValues[oldSortKey]
	delete(sortValues, oldSortKey)
	if oldSort == 0 {
		oldSort = len(sortValues) + 1
	}
	sortValues[newSortKey] = oldSort
	if err = markdownSortWriteConf(sortSnapshot.path, sortValues); err != nil {
		rollbackErr := rollbackMarkdownTransactionFiles(tx)
		if rollbackErr == nil {
			_ = finishMarkdownTransaction(tx)
		}
		return nil, errors.Join(err, rollbackErr, restoreMarkdownSort(sortSnapshot))
	}
	if err = moveRecentMarkdownLocked(
		MarkdownDocumentRef{Notebook: boxID, Path: canonicalPath},
		MarkdownDocumentRef{Notebook: boxID, Path: canonicalNewPath},
	); err != nil {
		rollbackErr := rollbackMarkdownTransactionFiles(tx)
		if rollbackErr == nil {
			_ = finishMarkdownTransaction(tx)
		}
		return nil, errors.Join(err, rollbackErr, restoreMarkdownSort(sortSnapshot),
			setRecentDocs(recentSnapshot))
	}
	if err = markMarkdownMetadataCommitted(tx); err != nil {
		return nil, err
	}
	if err = markdownTransactionCrashHook("rename", "metadata-committed"); err != nil {
		return nil, err
	}
	_ = finalizeMarkdownMove(tx)
	migrateWorkspaceMarkdownTableAppearance(boxID, canonicalPath, boxID, canonicalNewPath)

	IncSync()
	pushMarkdownFileEventWithOperation("renameMarkdown", boxID, canonicalNewPath, canonicalPath, boxID, operationID)
	data, readErr := readMarkdownFileContained(newAbsPath)
	if readErr != nil {
		return nil, readErr
	}
	ret, err = markdownDocument(canonicalNewPath, data, newAbsPath)
	if ret != nil {
		ret.OperationID = operationID
	}
	return ret, err
}

func DuplicateMarkdown(boxID, p, revision string) (ret *MarkdownDocument, err error) {
	return DuplicateMarkdownWithOperationID(boxID, p, revision, "")
}

func DuplicateMarkdownWithOperationID(boxID, p, revision, requestedOperationID string) (ret *MarkdownDocument, err error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	operationID, err := resolveMarkdownOperationID(requestedOperationID)
	if err != nil {
		return nil, err
	}
	if err = recoverMarkdownTransactionsLocked(); err != nil {
		return nil, err
	}

	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return nil, err
	}
	data, err := readMarkdownFileContained(absPath)
	if err != nil {
		return nil, err
	}
	if markdownRevision(data) != revision {
		return nil, ErrMarkdownConflict
	}
	if err = markdownAfterRevisionCheck(absPath); err != nil {
		return nil, err
	}
	data, err = readMarkdownFileContained(absPath)
	if err != nil {
		return nil, err
	}
	if markdownRevision(data) != revision {
		return nil, ErrMarkdownConflict
	}
	sourceIdentity, err := markdownIdentity(absPath)
	if err != nil {
		return nil, err
	}
	sortSnapshot, err := snapshotMarkdownSort(boxID)
	if err != nil {
		return nil, err
	}

	ext := filepath.Ext(canonicalPath)
	baseName := strings.TrimSuffix(path.Base(canonicalPath), ext)
	parentPath := path.Dir(canonicalPath)
	for index := 2; ; index++ {
		newPath := path.Join(parentPath, baseName+" "+strconv.Itoa(index)+ext)
		canonicalNewPath, newAbsPath, pathErr := markdownFilePath(boxID, newPath)
		if pathErr != nil {
			return nil, pathErr
		}
		tx, beginErr := beginMarkdownInstallTransaction("duplicate", boxID, newAbsPath, data, sourceIdentity.Mode)
		if beginErr != nil {
			return nil, beginErr
		}
		if err = recordMarkdownTransactionMetadata(tx, sortSnapshot, nil, nil); err != nil {
			return nil, err
		}
		installErr := installMarkdownTransaction(tx)
		if os.IsExist(installErr) {
			_ = finishMarkdownTransaction(tx)
			continue
		}
		if installErr != nil {
			return nil, installErr
		}
		if err = addMarkdownSortKey(boxID, canonicalNewPath); err != nil {
			return nil, errors.Join(err, rollbackMarkdownInstall(tx), restoreMarkdownSort(sortSnapshot))
		}
		if err = markMarkdownMetadataCommitted(tx); err != nil {
			return nil, errors.Join(err, rollbackMarkdownInstall(tx), restoreMarkdownSort(sortSnapshot))
		}
		_ = finalizeMarkdownInstall(tx)

		IncSync()
		pushMarkdownFileEventWithOperation("createMarkdown", boxID, canonicalNewPath, "", "", operationID)
		ret, err = markdownDocument(canonicalNewPath, data, newAbsPath)
		if ret != nil {
			ret.OperationID = operationID
		}
		return ret, err
	}
}

func MoveMarkdown(boxID, p, revision, toBoxID, toParentPath string, operationIDs ...string) (ret *MarkdownDocument, err error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	operationID, err := resolveMarkdownOperationID(operationIDs...)
	if err != nil {
		return nil, err
	}
	if err = recoverMarkdownTransactionsLocked(); err != nil {
		return nil, err
	}

	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return nil, err
	}
	data, err := readMarkdownFileContained(absPath)
	if err != nil {
		return nil, err
	}
	if markdownRevision(data) != revision {
		return nil, ErrMarkdownConflict
	}
	if err = markdownAfterRevisionCheck(absPath); err != nil {
		return nil, err
	}
	data, err = readMarkdownFileContained(absPath)
	if err != nil {
		return nil, err
	}
	if markdownRevision(data) != revision {
		return nil, ErrMarkdownConflict
	}

	toParentPath = filepath.ToSlash(strings.TrimSpace(toParentPath))
	if strings.HasSuffix(strings.ToLower(toParentPath), ".sy") {
		toParentPath = strings.TrimSuffix(toParentPath, filepath.Ext(toParentPath))
	}
	toParentPath = path.Clean("/" + strings.TrimPrefix(toParentPath, "/"))
	newPath := path.Join(toParentPath, path.Base(canonicalPath))
	canonicalNewPath, newAbsPath, err := markdownFilePath(toBoxID, newPath)
	if err != nil {
		return nil, err
	}
	rootPath, newRelPath, err := markdownRootAndRelative(newAbsPath)
	if err != nil {
		return nil, err
	}
	targetParent, _, err := openStableMarkdownParent(rootPath, newRelPath)
	if err != nil {
		return nil, err
	}
	_ = targetParent.Close()
	if boxID == toBoxID && canonicalPath == canonicalNewPath {
		document, documentErr := markdownDocument(canonicalPath, data, absPath)
		if document != nil {
			document.OperationID = operationID
		}
		return document, documentErr
	}
	fromSortSnapshot, err := snapshotMarkdownSort(boxID)
	if err != nil {
		return nil, err
	}
	toSortSnapshot := fromSortSnapshot
	if boxID != toBoxID {
		toSortSnapshot, err = snapshotMarkdownSort(toBoxID)
		if err != nil {
			return nil, err
		}
	}
	recentDocLock.Lock()
	defer recentDocLock.Unlock()
	recentSnapshot, err := loadRecentDocsRaw()
	if err != nil {
		return nil, err
	}
	recentSnapshot = cloneRecentDocs(recentSnapshot)
	if err = mkdirAllMarkdownContained(filepath.Dir(newAbsPath), 0755); err != nil {
		return nil, err
	}
	tx, err := beginMarkdownTransaction("move", boxID, absPath, newAbsPath, "")
	if err != nil {
		return nil, err
	}
	if err = recordMarkdownTransactionMetadata(tx, fromSortSnapshot, toSortSnapshot, recentSnapshot); err != nil {
		return nil, err
	}
	if err = markdownBeforeDestinationCommit(newAbsPath); err != nil {
		_ = finishMarkdownTransaction(tx)
		return nil, err
	}
	if err = copyMarkdownTransactionFile(tx); err != nil {
		return nil, err
	}
	if err = markdownTransactionCrashHook("move", "copied"); err != nil {
		return nil, err
	}
	if err = stageMarkdownTransactionSource(tx); err != nil {
		return nil, err
	}
	if err = markdownTransactionCrashHook("move", "source-staged"); err != nil {
		return nil, err
	}
	rollback := func() error {
		fileErr := rollbackMarkdownTransactionFiles(tx)
		errs := []error{fileErr, restoreMarkdownSort(fromSortSnapshot)}
		if boxID != toBoxID {
			errs = append(errs, restoreMarkdownSort(toSortSnapshot))
		}
		if fileErr == nil {
			errs = append(errs, finishMarkdownTransaction(tx))
		}
		return errors.Join(errs...)
	}
	fromValues := maps.Clone(fromSortSnapshot.values)
	toValues := fromValues
	if boxID != toBoxID {
		toValues = maps.Clone(toSortSnapshot.values)
	}
	fromKey, toKey := fileTreeSortKey(canonicalPath), fileTreeSortKey(canonicalNewPath)
	movedSort := fromValues[fromKey]
	delete(fromValues, fromKey)
	if movedSort == 0 {
		for _, value := range toValues {
			if value >= movedSort {
				movedSort = value + 1
			}
		}
	}
	toValues[toKey] = movedSort
	if err = markdownSortWriteConf(fromSortSnapshot.path, fromValues); err != nil {
		return nil, errors.Join(err, rollback())
	}
	if boxID != toBoxID {
		if err = markdownSortWriteConf(toSortSnapshot.path, toValues); err != nil {
			return nil, errors.Join(err, rollback())
		}
	}
	if err = moveRecentMarkdownLocked(
		MarkdownDocumentRef{Notebook: boxID, Path: canonicalPath},
		MarkdownDocumentRef{Notebook: toBoxID, Path: canonicalNewPath},
	); err != nil {
		return nil, errors.Join(err, rollback(), setRecentDocs(recentSnapshot))
	}
	if err = markMarkdownMetadataCommitted(tx); err != nil {
		return nil, err
	}
	if err = markdownTransactionCrashHook("move", "metadata-committed"); err != nil {
		return nil, err
	}
	document, err := markdownDocument(canonicalNewPath, data, newAbsPath)
	if err != nil {
		return nil, errors.Join(err, rollback(), setRecentDocs(recentSnapshot))
	}
	_ = finalizeMarkdownMove(tx)
	migrateWorkspaceMarkdownTableAppearance(boxID, canonicalPath, toBoxID, canonicalNewPath)

	document.OperationID = operationID
	IncSync()
	pushMarkdownFileEventWithOperation("renameMarkdown", toBoxID, canonicalNewPath, canonicalPath, boxID, operationID)
	return document, nil
}

func writeMarkdownRecoveryJournal(boxID, sourcePath, recoveryPath string) error {
	dir := filepath.Join(util.DataDir, boxID, ".siyuan", "markdown-transactions")
	if err := validatePathWithoutSymlinks(filepath.Join(util.DataDir, boxID), dir, true); err != nil {
		return err
	}
	if err := os.MkdirAll(dir, 0755); err != nil {
		return err
	}
	data, err := json.Marshal(map[string]string{"sourcePath": sourcePath, "recoveryPath": recoveryPath})
	if err != nil {
		return err
	}
	journalPath := filepath.Join(dir, strconv.FormatInt(time.Now().UnixNano(), 10)+".json")
	file, err := os.OpenFile(journalPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	if _, err = file.Write(data); err != nil {
		_ = file.Close()
		return err
	}
	return file.Close()
}

func rollbackMarkdownFile(boxID, recoveryPath, sourcePath string) error {
	err := moveMarkdownFileNoReplace(recoveryPath, sourcePath, false)
	if err == nil {
		return nil
	}
	return errors.Join(err, writeMarkdownRecoveryJournal(boxID, sourcePath, recoveryPath))
}

func moveMarkdownFileNoReplace(sourcePath, destinationPath string, runHook bool) error {
	if runHook {
		if err := markdownBeforeDestinationCommit(destinationPath); err != nil {
			return err
		}
	}
	identity, err := markdownIdentity(sourcePath)
	if err != nil {
		return err
	}
	if err = copyMarkdownFileNoReplace(sourcePath, destinationPath); err != nil {
		return err
	}
	if err = removeMarkdownFileWithIdentity(sourcePath, identity); err != nil {
		targetID, identityErr := markdownIdentity(destinationPath)
		var cleanupErr error
		if identityErr == nil {
			cleanupErr = removeMarkdownFileWithIdentity(destinationPath, targetID)
		}
		return errors.Join(err, identityErr, cleanupErr)
	}
	return nil
}

func copyMarkdownFileNoReplace(sourcePath, destinationPath string) (retErr error) {
	identity, err := markdownIdentity(sourcePath)
	if err != nil {
		return err
	}
	source, sourceRoot, err := openMarkdownFileRead(sourcePath)
	if err != nil {
		return err
	}
	defer sourceRoot.Close()
	defer source.Close()
	destination, err := openMarkdownFileNoReplace(destinationPath, identity.Mode)
	if err != nil {
		return err
	}
	committed := false
	defer func() {
		_ = destination.Close()
		if !committed {
			if targetID, identityErr := markdownIdentity(destinationPath); identityErr == nil {
				retErr = errors.Join(retErr, removeMarkdownFileWithIdentity(destinationPath, targetID))
			}
		}
	}()
	if _, err = io.Copy(destination, source); err != nil {
		return err
	}
	if err = destination.Chmod(identity.Mode); err != nil {
		return err
	}
	if err = destination.Sync(); err != nil {
		return err
	}
	if err = destination.Close(); err != nil {
		return err
	}
	if err = syncMarkdownParent(destinationPath); err != nil {
		return err
	}
	committed = true
	return nil
}

func openMarkdownFileRead(sourcePath string) (*os.File, *os.Root, error) {
	rootPath, relPath, err := markdownRootAndRelative(sourcePath)
	if err != nil {
		return nil, nil, err
	}
	root, leaf, err := openStableMarkdownParent(rootPath, relPath)
	if err != nil {
		return nil, nil, err
	}
	info, err := root.Lstat(leaf)
	if err != nil {
		root.Close()
		return nil, nil, err
	}
	if info.Mode()&os.ModeSymlink != 0 || !info.Mode().IsRegular() {
		root.Close()
		return nil, nil, ErrInvalidMarkdownPath
	}
	if err = markdownAfterRootComponentCheck(sourcePath); err != nil {
		root.Close()
		return nil, nil, err
	}
	file, err := root.Open(leaf)
	if err != nil {
		root.Close()
		return nil, nil, errors.Join(ErrInvalidMarkdownPath, err)
	}
	opened, err := file.Stat()
	current, lstatErr := root.Lstat(leaf)
	if err != nil || lstatErr != nil || current.Mode()&os.ModeSymlink != 0 || !os.SameFile(opened, current) {
		file.Close()
		root.Close()
		return nil, nil, errors.Join(ErrInvalidMarkdownPath, err, lstatErr)
	}
	return file, root, nil
}

func markdownRootAndRelative(targetPath string) (string, string, error) {
	relData, err := filepath.Rel(util.DataDir, targetPath)
	if err != nil || filepath.IsAbs(relData) || relData == ".." || strings.HasPrefix(relData, ".."+string(filepath.Separator)) {
		return "", "", ErrInvalidMarkdownPath
	}
	components := strings.Split(relData, string(filepath.Separator))
	if len(components) < 2 || components[0] == "" {
		return "", "", ErrInvalidMarkdownPath
	}
	rootPath := filepath.Join(util.DataDir, components[0])
	relRoot, err := filepath.Rel(rootPath, targetPath)
	if err != nil {
		return "", "", err
	}
	return rootPath, relRoot, nil
}

func openMarkdownFileNoReplace(targetPath string, mode os.FileMode) (*os.File, error) {
	rootPath, relPath, err := markdownRootAndRelative(targetPath)
	if err != nil {
		return nil, err
	}
	if err = validatePathWithoutSymlinks(rootPath, targetPath, true); err != nil {
		return nil, err
	}
	parent, leaf, err := openStableMarkdownParent(rootPath, relPath)
	if err != nil {
		return nil, err
	}
	defer parent.Close()
	if info, lstatErr := parent.Lstat(leaf); lstatErr == nil && info.Mode()&os.ModeSymlink != 0 {
		return nil, ErrInvalidMarkdownPath
	}
	return parent.OpenFile(leaf, os.O_WRONLY|os.O_CREATE|os.O_EXCL, mode)
}

func mkdirAllMarkdownContained(targetPath string, mode os.FileMode) error {
	rootPath, relPath, err := markdownRootAndRelative(filepath.Join(targetPath, "placeholder.md"))
	if err != nil {
		return err
	}
	relPath = filepath.Dir(relPath)
	if err = validatePathWithoutSymlinks(rootPath, targetPath, true); err != nil {
		return err
	}
	return mkdirStableMarkdownDirectories(rootPath, relPath, mode)
}

func openStableMarkdownParent(rootPath, relPath string) (*os.Root, string, error) {
	return openStableParent(rootPath, relPath, ErrInvalidMarkdownPath)
}

func openStableParent(rootPath, relPath string, invalidErr error) (*os.Root, string, error) {
	components := strings.Split(filepath.Clean(relPath), string(filepath.Separator))
	if len(components) == 0 || components[0] == "." {
		return nil, "", invalidErr
	}
	current, err := openStableRoot(rootPath, invalidErr)
	if err != nil {
		return nil, "", err
	}
	currentPath := rootPath
	for _, component := range components[:len(components)-1] {
		info, lstatErr := current.Lstat(component)
		if lstatErr != nil || info.Mode()&os.ModeSymlink != 0 || !info.IsDir() {
			current.Close()
			return nil, "", errors.Join(invalidErr, lstatErr)
		}
		checkedPath := filepath.Join(currentPath, component)
		if err = markdownAfterRootComponentCheck(checkedPath); err != nil {
			current.Close()
			return nil, "", err
		}
		next, openErr := current.OpenRoot(component)
		if openErr != nil {
			current.Close()
			return nil, "", errors.Join(invalidErr, openErr)
		}
		opened, statErr := next.Stat(".")
		after, afterErr := current.Lstat(component)
		if statErr != nil || afterErr != nil || after.Mode()&os.ModeSymlink != 0 || !os.SameFile(info, opened) || !os.SameFile(opened, after) {
			next.Close()
			current.Close()
			return nil, "", errors.Join(invalidErr, statErr, afterErr)
		}
		current.Close()
		current, currentPath = next, checkedPath
	}
	return current, components[len(components)-1], nil
}

func mkdirStableMarkdownDirectories(rootPath, relPath string, mode os.FileMode) error {
	return mkdirStableDirectories(rootPath, relPath, mode, ErrInvalidMarkdownPath)
}

func mkdirStableDirectories(rootPath, relPath string, mode os.FileMode, invalidErr error) error {
	components := strings.Split(filepath.Clean(relPath), string(filepath.Separator))
	root, err := openStableRoot(rootPath, invalidErr)
	if err != nil {
		return err
	}
	defer root.Close()
	current, currentPath := root, rootPath
	for index, component := range components {
		info, lstatErr := current.Lstat(component)
		if os.IsNotExist(lstatErr) {
			if err = current.Mkdir(component, mode); err != nil && !os.IsExist(err) {
				return err
			}
			dir, syncErr := current.Open(".")
			if syncErr != nil {
				return syncErr
			}
			if syncErr = errors.Join(dir.Sync(), dir.Close()); syncErr != nil {
				return syncErr
			}
			markdownDurabilityHook("mkdir-parent-synced", filepath.Join(currentPath, component))
			info, lstatErr = current.Lstat(component)
		}
		if lstatErr != nil || info.Mode()&os.ModeSymlink != 0 || !info.IsDir() {
			return errors.Join(invalidErr, lstatErr)
		}
		checkedPath := filepath.Join(currentPath, component)
		if err = markdownAfterRootComponentCheck(checkedPath); err != nil {
			return err
		}
		next, openErr := current.OpenRoot(component)
		if openErr != nil {
			return errors.Join(invalidErr, openErr)
		}
		opened, statErr := next.Stat(".")
		after, afterErr := current.Lstat(component)
		if statErr != nil || afterErr != nil || after.Mode()&os.ModeSymlink != 0 || !os.SameFile(info, opened) || !os.SameFile(opened, after) {
			next.Close()
			return errors.Join(invalidErr, statErr, afterErr)
		}
		if index > 0 {
			current.Close()
		}
		current, currentPath = next, checkedPath
	}
	if current != root {
		return current.Close()
	}
	return nil
}

func openStableMarkdownRoot(rootPath string) (*os.Root, error) {
	return openStableRoot(rootPath, ErrInvalidMarkdownPath)
}

func openStableRoot(rootPath string, invalidErr error) (*os.Root, error) {
	parentPath, baseName := filepath.Dir(rootPath), filepath.Base(rootPath)
	parentRoot, err := os.OpenRoot(parentPath)
	if err != nil {
		return nil, err
	}
	defer parentRoot.Close()
	before, err := parentRoot.Lstat(baseName)
	if err != nil {
		return nil, err
	}
	if before.Mode()&os.ModeSymlink != 0 || !before.IsDir() {
		return nil, invalidErr
	}
	if err = markdownAfterRootComponentCheck(rootPath); err != nil {
		return nil, err
	}
	root, err := parentRoot.OpenRoot(baseName)
	if err != nil {
		return nil, errors.Join(invalidErr, err)
	}
	opened, err := root.Stat(".")
	if err != nil {
		root.Close()
		return nil, err
	}
	after, err := parentRoot.Lstat(baseName)
	if err != nil || after.Mode()&os.ModeSymlink != 0 || !os.SameFile(before, opened) || !os.SameFile(opened, after) {
		root.Close()
		return nil, invalidErr
	}
	return root, nil
}

func validateStableRootComponents(root *os.Root, rootPath, relPath string, allowMissing bool) error {
	currentRoot := root
	owned := []*os.Root{}
	defer func() {
		for _, opened := range owned {
			_ = opened.Close()
		}
	}()
	components := strings.Split(filepath.Clean(relPath), string(filepath.Separator))
	for index, component := range components {
		if component == "." || component == "" {
			continue
		}
		info, err := currentRoot.Lstat(component)
		if os.IsNotExist(err) && allowMissing {
			return nil
		}
		if err != nil {
			return err
		}
		if info.Mode()&os.ModeSymlink != 0 {
			return ErrInvalidMarkdownPath
		}
		checkedPath := filepath.Join(rootPath, filepath.Join(components[:index+1]...))
		if err = markdownAfterRootComponentCheck(checkedPath); err != nil {
			return err
		}
		after, err := currentRoot.Lstat(component)
		if err != nil || after.Mode()&os.ModeSymlink != 0 || !os.SameFile(info, after) {
			return ErrInvalidMarkdownPath
		}
		if index == len(components)-1 {
			return nil
		}
		if !info.IsDir() {
			return ErrInvalidMarkdownPath
		}
		nextRoot, err := currentRoot.OpenRoot(component)
		if err != nil {
			return errors.Join(ErrInvalidMarkdownPath, err)
		}
		opened, err := nextRoot.Stat(".")
		if err != nil || !os.SameFile(info, opened) {
			nextRoot.Close()
			return ErrInvalidMarkdownPath
		}
		owned = append(owned, nextRoot)
		currentRoot = nextRoot
	}
	return nil
}

func RemoveMarkdown(boxID, p string) error {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	if err := recoverMarkdownTransactionsLocked(); err != nil {
		return err
	}

	canonicalPath, absPath, err := markdownFilePath(boxID, p)
	if err != nil {
		return err
	}
	recentDocLock.Lock()
	defer recentDocLock.Unlock()
	recentSnapshot, err := loadRecentDocsRaw()
	if err != nil {
		return err
	}
	recentSnapshot = cloneRecentDocs(recentSnapshot)
	if err = removeRecentMarkdownLocked(MarkdownDocumentRef{Notebook: boxID, Path: canonicalPath}); err != nil {
		return errors.Join(err, setRecentDocs(recentSnapshot))
	}
	sortSnapshot, err := removeMarkdownSortKey(boxID, canonicalPath)
	if err != nil {
		return errors.Join(err, setRecentDocs(recentSnapshot))
	}
	if err = filelock.Remove(absPath); err != nil {
		return errors.Join(err, restoreMarkdownSort(sortSnapshot), setRecentDocs(recentSnapshot))
	}
	removeWorkspaceMarkdownTableAppearance(boxID, canonicalPath)

	IncSync()
	pushMarkdownFileEvent("removeMarkdown", boxID, canonicalPath, "")
	return nil
}

func markdownFilePath(boxID, p string) (canonicalPath, absPath string, err error) {
	boxRoot := filepath.Join(util.DataDir, boxID)
	if rootInfo, lstatErr := os.Lstat(boxRoot); lstatErr == nil && rootInfo.Mode()&os.ModeSymlink != 0 {
		return "", "", ErrInvalidMarkdownPath
	}
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
	if err = validatePathWithoutSymlinks(boxRoot, absPath, true); err != nil {
		return "", "", err
	}
	return
}

func validatePathWithoutSymlinks(rootPath, targetPath string, allowMissing bool) error {
	rootPath = filepath.Clean(rootPath)
	targetPath = filepath.Clean(targetPath)
	rel, err := filepath.Rel(rootPath, targetPath)
	if err != nil || filepath.IsAbs(rel) || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return ErrInvalidMarkdownPath
	}
	rootInfo, err := os.Lstat(rootPath)
	if err != nil {
		return err
	}
	if rootInfo.Mode()&os.ModeSymlink != 0 || !rootInfo.IsDir() {
		return ErrInvalidMarkdownPath
	}
	realRoot, err := filepath.EvalSymlinks(rootPath)
	if err != nil {
		return err
	}
	current := rootPath
	if rel != "." {
		for _, component := range strings.Split(rel, string(filepath.Separator)) {
			current = filepath.Join(current, component)
			info, lstatErr := os.Lstat(current)
			if os.IsNotExist(lstatErr) && allowMissing {
				break
			}
			if lstatErr != nil {
				return lstatErr
			}
			if info.Mode()&os.ModeSymlink != 0 {
				return ErrInvalidMarkdownPath
			}
		}
	}
	existing := current
	for {
		if _, statErr := os.Lstat(existing); statErr == nil {
			break
		} else if !os.IsNotExist(statErr) {
			return statErr
		}
		parent := filepath.Dir(existing)
		if parent == existing {
			return ErrInvalidMarkdownPath
		}
		existing = parent
	}
	realExisting, err := filepath.EvalSymlinks(existing)
	if err != nil {
		return err
	}
	realRel, err := filepath.Rel(realRoot, realExisting)
	if err != nil || filepath.IsAbs(realRel) || realRel == ".." || strings.HasPrefix(realRel, ".."+string(filepath.Separator)) {
		return ErrInvalidMarkdownPath
	}
	return nil
}

func markdownDocument(p string, data []byte, absPath string) (ret *MarkdownDocument, err error) {
	identity, err := markdownIdentity(absPath)
	if err != nil {
		return nil, err
	}
	return &MarkdownDocument{
		Path:     p,
		Name:     path.Base(p),
		Content:  string(data),
		Revision: markdownRevision(data),
		Mtime:    identity.Mtime / int64(time.Millisecond),
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

func pushMarkdownFileEvent(cmd, boxID, p, oldPath string, oldBox ...string) {
	oldBoxID := ""
	if oldPath != "" {
		oldBoxID = boxID
	}
	if len(oldBox) > 0 {
		oldBoxID = oldBox[0]
	}
	pushMarkdownFileEventWithOperation(cmd, boxID, p, oldPath, oldBoxID, markdownOperationID())
}

func markdownOperationID(operationIDs ...string) string {
	if len(operationIDs) > 0 && operationIDs[0] != "" {
		return operationIDs[0]
	}
	return gulu.Rand.String(16)
}

func resolveMarkdownOperationID(operationIDs ...string) (string, error) {
	operationID := markdownOperationID(operationIDs...)
	if len(operationID) > 64 {
		return "", ErrInvalidMarkdownOperationID
	}
	for _, char := range operationID {
		if (char >= 'a' && char <= 'z') || (char >= 'A' && char <= 'Z') || (char >= '0' && char <= '9') ||
			char == '-' || char == '_' || char == '.' {
			continue
		}
		return "", ErrInvalidMarkdownOperationID
	}
	return operationID, nil
}

func ResolveMarkdownOperationID(operationID string) (string, error) {
	return resolveMarkdownOperationID(operationID)
}

func pushMarkdownFileEventWithOperation(cmd, boxID, p, oldPath, oldBoxID, operationID string) {
	evt := util.NewCmdResult(cmd, 0, util.PushModeBroadcast)
	evt.Data = map[string]any{
		"kind":        "markdown",
		"box":         boxID,
		"path":        p,
		"oldBox":      oldBoxID,
		"oldPath":     oldPath,
		"operationID": operationID,
		"time":        time.Now().UnixMilli(),
	}
	markdownPushEvent(evt)
}

func markdownFileDisplayInfo(boxID string, fileInfo *FileInfo) *File {
	modified := time.Unix(0, 0)
	absPath := filepath.Join(util.DataDir, boxID, filepath.FromSlash(strings.TrimPrefix(fileInfo.path, "/")))
	if file, root, err := openMarkdownFileRead(absPath); err == nil {
		if info, statErr := file.Stat(); statErr == nil {
			modified = info.ModTime()
		}
		_ = file.Close()
		_ = root.Close()
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
