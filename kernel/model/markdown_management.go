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
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"maps"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/88250/gulu"
	"github.com/siyuan-note/filelock"
	"github.com/siyuan-note/siyuan/kernel/util"
)

var (
	ErrInvalidMarkdownHistory      = errors.New("invalid Markdown history")
	markdownHistoryWriteFile       = writeMarkdownHistoryFile
	markdownHistoryRename          = renameMarkdownHistoryFile
	markdownHistoryRenameNoReplace = renameMarkdownHistoryFileNoReplace
	markdownHistoryRemove          = removeMarkdownHistoryFile
	markdownSortWriteConf          = writeSortConfMap
	markdownAfterRecycleStage      = func(string, string) error { return nil }
)

type MarkdownDocumentRef struct {
	Notebook string `json:"notebook"`
	Path     string `json:"path"`
}

func CanonicalMarkdownRef(notebook, p string) (MarkdownDocumentRef, error) {
	canonicalPath, _, err := markdownFilePath(notebook, p)
	if err != nil {
		return MarkdownDocumentRef{}, err
	}
	return MarkdownDocumentRef{Notebook: notebook, Path: canonicalPath}, nil
}

func MarkdownRecentKey(ref MarkdownDocumentRef) string {
	return "markdown:" + ref.Notebook + ":" + ref.Path
}

func fileTreeSortKey(p string) string {
	if isMarkdownFileName(path.Base(p)) {
		return "markdown:" + canonicalMarkdownPath(p)
	}
	return util.GetTreeID(p)
}

func canonicalMarkdownPath(p string) string {
	p = filepath.ToSlash(strings.TrimSpace(p))
	return path.Clean("/" + strings.TrimPrefix(p, "/"))
}

type markdownSortSnapshot struct {
	path    string
	existed bool
	values  map[string]int
}

func snapshotMarkdownSort(boxID string) (*markdownSortSnapshot, error) {
	confPath := filepath.Join(util.DataDir, boxID, ".siyuan", "sort.json")
	values, err := readSortConfMap(confPath)
	if err != nil {
		return nil, err
	}
	return &markdownSortSnapshot{path: confPath, existed: filelock.IsExist(confPath), values: maps.Clone(values)}, nil
}

func restoreMarkdownSort(snapshot *markdownSortSnapshot) error {
	if snapshot == nil {
		return nil
	}
	if !snapshot.existed {
		if err := filelock.Remove(snapshot.path); err != nil && !os.IsNotExist(err) {
			return err
		}
		return nil
	}
	return writeSortConfMap(snapshot.path, snapshot.values)
}

func addMarkdownSortKey(boxID, p string) error {
	snapshot, err := snapshotMarkdownSort(boxID)
	if err != nil {
		return err
	}
	key := fileTreeSortKey(p)
	if snapshot.values[key] != 0 {
		return nil
	}
	minSort, maxSort := 0, 0
	for _, value := range snapshot.values {
		if value <= 0 {
			continue
		}
		if minSort == 0 || value < minSort {
			minSort = value
		}
		if value > maxSort {
			maxSort = value
		}
	}
	if Conf.FileTree.CreateDocAtTop != nil && *Conf.FileTree.CreateDocAtTop {
		if minSort <= 1 {
			for existingKey, value := range snapshot.values {
				if value > 0 {
					snapshot.values[existingKey] = value + 1
				}
			}
			snapshot.values[key] = 1
		} else {
			snapshot.values[key] = minSort - 1
		}
	} else {
		snapshot.values[key] = maxSort + 1
	}
	return markdownSortWriteConf(snapshot.path, snapshot.values)
}

func removeMarkdownSortKey(boxID, p string) (*markdownSortSnapshot, error) {
	snapshot, err := snapshotMarkdownSort(boxID)
	if err != nil {
		return nil, err
	}
	delete(snapshot.values, fileTreeSortKey(p))
	if err = markdownSortWriteConf(snapshot.path, snapshot.values); err != nil {
		return nil, err
	}
	return snapshot, nil
}

type MarkdownTrashEntry struct {
	ID           string `json:"id"`
	Notebook     string `json:"notebook"`
	OriginalPath string `json:"originalPath"`
	HistoryPath  string `json:"historyPath"`
	DeletedAt    int64  `json:"deletedAt"`
	Size         int64  `json:"size"`
	Revision     string `json:"revision"`
	Mode         uint32 `json:"mode,omitempty"`
	OperationID  string `json:"operationID,omitempty"`
}

type markdownTrashRecord struct {
	entry        *MarkdownTrashEntry
	manifestPath string
}

type markdownPurgeTransaction struct {
	ID           string              `json:"id"`
	Phase        string              `json:"phase"`
	Entry        *MarkdownTrashEntry `json:"entry"`
	ManifestPath string              `json:"manifestPath"`
	PayloadPath  string              `json:"payloadPath"`
	Tombstone    string              `json:"tombstone"`
	journalPath  string
}

func RecycleMarkdown(ref MarkdownDocumentRef, revision string, operationIDs ...string) (*MarkdownTrashEntry, error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	operationID, err := resolveMarkdownOperationID(operationIDs...)
	if err != nil {
		return nil, err
	}
	if err = recoverMarkdownTransactionsLocked(); err != nil {
		return nil, err
	}

	ref, err = CanonicalMarkdownRef(ref.Notebook, ref.Path)
	if err != nil {
		return nil, err
	}
	_, absPath, err := markdownFilePath(ref.Notebook, ref.Path)
	if err != nil {
		return nil, err
	}
	tx, err := beginMarkdownTransaction("recycle", ref.Notebook, absPath, "", "")
	if err != nil {
		return nil, err
	}
	if tx.SourceID.Revision != revision {
		_ = finishMarkdownTransaction(tx)
		return nil, ErrMarkdownConflict
	}
	stagingPath := tx.Staging
	tx.Phase = "source-staging"
	if err = writeMarkdownTransaction(tx); err != nil {
		return nil, errors.Join(err, finishMarkdownTransaction(tx))
	}
	if err = renameMarkdownWithinRoot(absPath, stagingPath); err != nil {
		return nil, err
	}
	tx.Phase = "staged"
	if err = writeMarkdownTransaction(tx); err != nil {
		rollbackErr := rollbackMarkdownStagedSource(tx)
		if rollbackErr == nil {
			tx.Phase = "prepared"
			rollbackErr = errors.Join(writeMarkdownTransaction(tx), finishMarkdownTransaction(tx))
		}
		return nil, errors.Join(err, rollbackErr)
	}
	if err = markdownTransactionCrashHook("recycle", "staged"); err != nil {
		return nil, err
	}
	rollbackSource := func(cause error) error {
		moveErr := moveMarkdownFileNoReplace(stagingPath, absPath, false)
		if moveErr == nil {
			return errors.Join(cause, finishMarkdownTransaction(tx))
		}
		return errors.Join(cause, moveErr)
	}
	if err = markdownAfterRecycleStage(absPath, stagingPath); err != nil {
		return nil, rollbackSource(err)
	}
	data, err := readMarkdownFileContained(stagingPath)
	if err != nil {
		return nil, rollbackSource(err)
	}
	if markdownRevision(data) != revision {
		return nil, rollbackSource(ErrMarkdownConflict)
	}
	sourceIdentity, err := markdownIdentity(stagingPath)
	if err != nil {
		return nil, rollbackSource(err)
	}

	now := time.Now()
	if err = ensureMarkdownHistoryRoot(); err != nil {
		return nil, rollbackSource(err)
	}
	batchName := fmt.Sprintf("%s-%d-%s-delete", now.Format("2006-01-02-150405"), now.UnixNano(), gulu.Rand.String(8))
	batchPath := filepath.Join(util.HistoryDir, batchName)
	if err = mkdirMarkdownHistoryPath(batchPath, 0755, false); err != nil {
		return nil, rollbackSource(err)
	}
	tx.Destination = batchPath
	if err = writeMarkdownTransaction(tx); err != nil {
		_ = removeMarkdownHistoryAll(batchPath)
		return nil, rollbackSource(err)
	}
	cleanupBatch := func() {
		_ = removeMarkdownHistoryAll(batchPath)
	}
	historyRelPath := path.Join(batchName, ref.Notebook, strings.TrimPrefix(ref.Path, "/"))
	historyAbsPath := filepath.Join(util.HistoryDir, filepath.FromSlash(historyRelPath))
	if err = validateMarkdownHistoryPath(historyAbsPath, true); err != nil {
		cleanupBatch()
		return nil, rollbackSource(err)
	}
	if err = mkdirMarkdownHistoryPath(filepath.Dir(historyAbsPath), 0755, true); err != nil {
		cleanupBatch()
		return nil, rollbackSource(err)
	}
	if err = validateMarkdownHistoryPath(historyAbsPath, true); err != nil {
		cleanupBatch()
		return nil, rollbackSource(err)
	}
	if err = writeMarkdownHistoryFileMode(historyAbsPath, data, sourceIdentity.Mode); err != nil {
		cleanupBatch()
		return nil, rollbackSource(err)
	}
	copied, err := readMarkdownHistoryFile(historyAbsPath)
	if err != nil || markdownRevision(copied) != revision {
		cleanupBatch()
		if err != nil {
			return nil, rollbackSource(err)
		}
		return nil, rollbackSource(ErrMarkdownConflict)
	}

	entry := &MarkdownTrashEntry{
		ID:           batchName,
		Notebook:     ref.Notebook,
		OriginalPath: ref.Path,
		HistoryPath:  historyRelPath,
		DeletedAt:    now.UnixMilli(),
		Size:         int64(len(data)),
		Revision:     revision,
		Mode:         uint32(sourceIdentity.Mode),
		OperationID:  operationID,
	}
	manifestPath := filepath.Join(batchPath, "markdown.json")
	if err = writeMarkdownTrashManifest(manifestPath, []*MarkdownTrashEntry{entry}); err != nil {
		cleanupBatch()
		return nil, rollbackSource(err)
	}
	recentDocLock.Lock()
	defer recentDocLock.Unlock()
	recentSnapshot, err := loadRecentDocsRaw()
	if err != nil {
		cleanupBatch()
		return nil, rollbackSource(err)
	}
	recentSnapshot = cloneRecentDocs(recentSnapshot)
	sortSnapshot, err := snapshotMarkdownSort(ref.Notebook)
	if err != nil {
		cleanupBatch()
		return nil, rollbackSource(err)
	}
	if err = recordMarkdownTransactionMetadata(tx, sortSnapshot, nil, recentSnapshot); err != nil {
		cleanupBatch()
		return nil, rollbackSource(err)
	}
	if err = removeRecentMarkdownLocked(ref); err != nil {
		cleanupBatch()
		return nil, errors.Join(err, setRecentDocs(recentSnapshot), rollbackSource(nil))
	}
	_, err = removeMarkdownSortKey(ref.Notebook, ref.Path)
	if err != nil {
		cleanupBatch()
		return nil, errors.Join(err, setRecentDocs(recentSnapshot), rollbackSource(nil))
	}
	if err = markMarkdownMetadataCommitted(tx); err != nil {
		return nil, err
	}
	if err = markdownTransactionCrashHook("recycle", "metadata-committed"); err != nil {
		return nil, err
	}
	// 回收站记录与元数据已提交；源文件清理失败由恢复门禁重试，不能删除唯一历史副本。
	_ = finalizeMarkdownMove(tx)
	removeWorkspaceMarkdownTableAppearance(ref.Notebook, ref.Path)

	IncSync()
	pushMarkdownFileEventWithOperation("removeMarkdown", ref.Notebook, ref.Path, "", "", operationID)
	return entry, nil
}

func ListDeletedMarkdown() ([]*MarkdownTrashEntry, error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	if err := recoverMarkdownTransactionsLocked(); err != nil {
		return nil, err
	}
	records, err := loadMarkdownTrashRecords()
	if err != nil {
		return nil, err
	}
	ret := make([]*MarkdownTrashEntry, 0, len(records))
	for _, record := range records {
		entry := *record.entry
		ret = append(ret, &entry)
	}
	sort.Slice(ret, func(i, j int) bool {
		return ret[i].DeletedAt > ret[j].DeletedAt
	})
	return ret, nil
}

func GetDeletedMarkdown(id string) (*MarkdownTrashEntry, []byte, error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	if err := recoverMarkdownTransactionsLocked(); err != nil {
		return nil, nil, err
	}
	return getDeletedMarkdown(id)
}

func getDeletedMarkdown(id string) (*MarkdownTrashEntry, []byte, error) {
	record, err := findMarkdownTrashRecord(id)
	if err != nil {
		return nil, nil, err
	}
	historyPath, err := validateMarkdownTrashEntry(record)
	if err != nil {
		return nil, nil, err
	}
	data, err := readMarkdownHistoryFile(historyPath)
	if err != nil {
		return nil, nil, err
	}
	if int64(len(data)) != record.entry.Size || markdownRevision(data) != record.entry.Revision {
		return nil, nil, ErrMarkdownConflict
	}
	entry := *record.entry
	return &entry, data, nil
}

func RestoreDeletedMarkdown(id, toNotebook, toParentPath, name string, operationIDs ...string) (*MarkdownDocument, error) {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	operationID, err := resolveMarkdownOperationID(operationIDs...)
	if err != nil {
		return nil, err
	}
	if err = recoverMarkdownTransactionsLocked(); err != nil {
		return nil, err
	}

	trashEntry, data, err := getDeletedMarkdown(id)
	if err != nil {
		return nil, err
	}
	name = strings.TrimSpace(name)
	if name == "" || strings.ContainsAny(name, "/\\") || util.IsReservedFilename(name) || !isMarkdownFileName(name) {
		return nil, errors.New("invalid Markdown file name")
	}
	toParentPath = filepath.ToSlash(strings.TrimSpace(toParentPath))
	if strings.HasSuffix(strings.ToLower(toParentPath), ".sy") {
		toParentPath = strings.TrimSuffix(toParentPath, filepath.Ext(toParentPath))
	}
	canonicalPath, absPath, err := markdownFilePath(toNotebook, path.Join(toParentPath, name))
	if err != nil {
		return nil, err
	}
	if err = mkdirAllMarkdownContained(filepath.Dir(absPath), 0755); err != nil {
		return nil, err
	}
	sortSnapshot, err := snapshotMarkdownSort(toNotebook)
	if err != nil {
		return nil, err
	}
	mode := os.FileMode(0644)
	if trashEntry.Mode != 0 {
		mode = os.FileMode(trashEntry.Mode)
	}
	tx, err := beginMarkdownInstallTransaction("restore", toNotebook, absPath, data, mode)
	if err != nil {
		return nil, err
	}
	if err = recordMarkdownTransactionMetadata(tx, sortSnapshot, nil, nil); err != nil {
		return nil, err
	}
	if err = installMarkdownTransaction(tx); err != nil {
		if os.IsExist(err) {
			_ = finishMarkdownTransaction(tx)
			return nil, os.ErrExist
		}
		return nil, err
	}
	if err = addMarkdownSortKey(toNotebook, canonicalPath); err != nil {
		return nil, errors.Join(err, rollbackMarkdownInstall(tx), restoreMarkdownSort(sortSnapshot))
	}
	if err = markMarkdownMetadataCommitted(tx); err != nil {
		return nil, errors.Join(err, rollbackMarkdownInstall(tx), restoreMarkdownSort(sortSnapshot))
	}
	_ = finalizeMarkdownInstall(tx)

	IncSync()
	pushMarkdownFileEventWithOperation("createMarkdown", toNotebook, canonicalPath, "", "", operationID)
	document, err := markdownDocument(canonicalPath, data, absPath)
	if document != nil {
		document.OperationID = operationID
	}
	return document, err
}

func PurgeDeletedMarkdown(id string, operationIDs ...string) error {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	operationID, err := resolveMarkdownOperationID(operationIDs...)
	if err != nil {
		return err
	}
	if err = recoverMarkdownTransactionsLocked(); err != nil {
		return err
	}

	record, err := findMarkdownTrashRecord(id)
	if err != nil {
		return err
	}
	historyPath, err := validateMarkdownTrashEntry(record)
	if err != nil {
		return err
	}
	entries, err := readMarkdownTrashManifest(record.manifestPath)
	if err != nil {
		return err
	}
	remaining := make([]*MarkdownTrashEntry, 0, len(entries)-1)
	for _, entry := range entries {
		if entry.ID != id {
			remaining = append(remaining, entry)
		}
	}
	purgeTx, err := beginMarkdownPurgeTransaction(record, historyPath)
	if err != nil {
		return err
	}
	tombstonePath := purgeTx.Tombstone
	if err = markdownHistoryRenameNoReplace(historyPath, tombstonePath); err != nil {
		_ = finishMarkdownPurgeTransaction(purgeTx)
		return err
	}
	purgeTx.Phase = "tombstoned"
	if err = writeMarkdownPurgeTransaction(purgeTx); err != nil {
		return err
	}
	if err = markdownTransactionCrashHook("purge", "tombstoned"); err != nil {
		return err
	}
	rollback := func(cause error) error {
		rollbackErr := markdownHistoryRenameNoReplace(tombstonePath, historyPath)
		if rollbackErr == nil {
			rollbackErr = finishMarkdownPurgeTransaction(purgeTx)
		}
		return errors.Join(cause, rollbackErr)
	}
	if err = writeMarkdownTrashManifest(record.manifestPath, remaining); err != nil {
		return rollback(err)
	}
	purgeTx.Phase = "committed"
	journalUpdated := writeMarkdownPurgeTransaction(purgeTx) == nil
	if journalUpdated {
		if err = markdownTransactionCrashHook("purge", "committed"); err != nil {
			return err
		}
	}
	cleanupErr := markdownHistoryRemove(tombstonePath)
	if os.IsNotExist(cleanupErr) {
		cleanupErr = nil
	}
	batchPath := filepath.Dir(record.manifestPath)
	if len(remaining) == 0 {
		_ = removeMarkdownHistoryFile(record.manifestPath)
	}
	pruneEmptyMarkdownHistoryDirs(filepath.Dir(historyPath), batchPath)
	if len(remaining) == 0 {
		_ = removeMarkdownHistoryFile(batchPath)
	}
	if cleanupErr == nil && journalUpdated {
		_ = finishMarkdownPurgeTransaction(purgeTx)
	}
	pushMarkdownFileEventWithOperation("purgeMarkdown", record.entry.Notebook, record.entry.OriginalPath, "", "",
		operationID)
	return nil
}

func beginMarkdownPurgeTransaction(record *markdownTrashRecord, payloadPath string) (*markdownPurgeTransaction, error) {
	id := fmt.Sprintf("%d-%s", time.Now().UnixNano(), gulu.Rand.String(8))
	dirPath := filepath.Join(util.HistoryDir, ".markdown-transactions")
	if err := mkdirMarkdownHistoryPath(dirPath, 0700, true); err != nil {
		return nil, err
	}
	tx := &markdownPurgeTransaction{
		ID: id, Phase: "prepared", Entry: record.entry, ManifestPath: record.manifestPath, PayloadPath: payloadPath,
		Tombstone: payloadPath + ".purging-" + id, journalPath: filepath.Join(dirPath, id+".json"),
	}
	if err := writeMarkdownPurgeTransaction(tx); err != nil {
		return nil, err
	}
	return tx, nil
}

func writeMarkdownPurgeTransaction(tx *markdownPurgeTransaction) error {
	data, err := json.Marshal(tx)
	if err != nil {
		return err
	}
	tmpPath := tx.journalPath + ".tmp"
	_ = removeMarkdownHistoryFile(tmpPath)
	if err = markdownHistoryWriteFile(tmpPath, data); err != nil {
		return err
	}
	if err = markdownHistoryRename(tmpPath, tx.journalPath); err != nil {
		return err
	}
	root, err := openMarkdownHistoryRoot()
	if err != nil {
		return err
	}
	relDir, err := markdownHistoryRelative(filepath.Dir(tx.journalPath))
	if err != nil {
		root.Close()
		return err
	}
	dir, err := root.Open(relDir)
	if err != nil {
		root.Close()
		return err
	}
	return errors.Join(dir.Sync(), dir.Close(), root.Close())
}

func finishMarkdownPurgeTransaction(tx *markdownPurgeTransaction) error {
	if tx == nil {
		return nil
	}
	err := removeMarkdownHistoryFile(tx.journalPath)
	if os.IsNotExist(err) {
		return nil
	}
	return err
}

func recoverMarkdownPurgeTransactionsLocked() error {
	dirPath := filepath.Join(util.HistoryDir, ".markdown-transactions")
	root, err := openMarkdownHistoryRoot()
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}
		return err
	}
	relDir, err := markdownHistoryRelative(dirPath)
	if err != nil {
		root.Close()
		return err
	}
	dir, err := root.Open(relDir)
	if os.IsNotExist(err) {
		root.Close()
		return nil
	}
	if err != nil {
		root.Close()
		return err
	}
	entries, err := dir.ReadDir(-1)
	_ = dir.Close()
	_ = root.Close()
	if err != nil {
		return err
	}
	var recoveryErrors []error
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".json") {
			continue
		}
		journalPath := filepath.Join(dirPath, entry.Name())
		data, readErr := readMarkdownHistoryFile(journalPath)
		if readErr != nil {
			recoveryErrors = append(recoveryErrors, readErr)
			continue
		}
		var tx markdownPurgeTransaction
		if readErr = json.Unmarshal(data, &tx); readErr != nil {
			recoveryErrors = append(recoveryErrors, readErr)
			continue
		}
		tx.journalPath = journalPath
		manifestContainsEntry := false
		manifestEntries, manifestErr := readMarkdownTrashManifest(tx.ManifestPath)
		if manifestErr == nil {
			for _, manifestEntry := range manifestEntries {
				if manifestEntry != nil && tx.Entry != nil && manifestEntry.ID == tx.Entry.ID {
					manifestContainsEntry = true
					break
				}
			}
		} else if !os.IsNotExist(manifestErr) {
			recoveryErrors = append(recoveryErrors, manifestErr)
			continue
		}
		if manifestContainsEntry {
			if _, statErr := os.Lstat(tx.PayloadPath); statErr == nil {
				recoveryErrors = append(recoveryErrors, ErrMarkdownConflict)
				continue
			}
			if renameErr := markdownHistoryRenameNoReplace(tx.Tombstone, tx.PayloadPath); renameErr != nil && !os.IsNotExist(renameErr) {
				recoveryErrors = append(recoveryErrors, renameErr)
				continue
			}
		} else {
			if removeErr := markdownHistoryRemove(tx.Tombstone); removeErr != nil && !os.IsNotExist(removeErr) {
				recoveryErrors = append(recoveryErrors, removeErr)
				continue
			}
		}
		if finishErr := finishMarkdownPurgeTransaction(&tx); finishErr != nil {
			recoveryErrors = append(recoveryErrors, finishErr)
		}
	}
	return errors.Join(recoveryErrors...)
}

func writeMarkdownTrashManifest(manifestPath string, entries []*MarkdownTrashEntry) error {
	if err := validateMarkdownHistoryPath(manifestPath, true); err != nil {
		return err
	}
	data, err := json.MarshalIndent(entries, "", "  ")
	if err != nil {
		return err
	}
	tmpPath := manifestPath + ".tmp"
	if err = validateMarkdownHistoryPath(tmpPath, true); err != nil {
		return err
	}
	if info, lstatErr := os.Lstat(tmpPath); lstatErr == nil && info.Mode()&os.ModeSymlink != 0 {
		return ErrInvalidMarkdownHistory
	}
	_ = removeMarkdownHistoryFile(tmpPath)
	if err = markdownHistoryWriteFile(tmpPath, data); err != nil {
		_ = removeMarkdownHistoryFile(tmpPath)
		return err
	}
	if err = markdownHistoryRename(tmpPath, manifestPath); err != nil {
		_ = removeMarkdownHistoryFile(tmpPath)
		return err
	}
	return nil
}

func readMarkdownTrashManifest(manifestPath string) ([]*MarkdownTrashEntry, error) {
	if err := validateMarkdownHistoryPath(manifestPath, false); err != nil {
		return nil, err
	}
	data, err := readMarkdownHistoryFile(manifestPath)
	if err != nil {
		return nil, err
	}
	return decodeMarkdownTrashManifest(data)
}

func decodeMarkdownTrashManifest(data []byte) ([]*MarkdownTrashEntry, error) {
	var entries []*MarkdownTrashEntry
	if err := json.Unmarshal(data, &entries); err != nil {
		return nil, ErrInvalidMarkdownHistory
	}
	return entries, nil
}

func loadMarkdownTrashRecords() ([]*markdownTrashRecord, error) {
	root, err := openMarkdownHistoryRoot()
	if os.IsNotExist(err) {
		return []*markdownTrashRecord{}, nil
	}
	if err != nil {
		return nil, err
	}
	defer root.Close()
	dir, err := root.Open(".")
	if err != nil {
		return nil, err
	}
	dirEntries, err := dir.ReadDir(-1)
	_ = dir.Close()
	if err != nil {
		return nil, err
	}
	records := make([]*markdownTrashRecord, 0)
	seen := map[string]struct{}{}
	for _, dirEntry := range dirEntries {
		if !strings.HasSuffix(dirEntry.Name(), "-delete") {
			continue
		}
		info, lstatErr := root.Lstat(dirEntry.Name())
		if lstatErr != nil || info.Mode()&os.ModeSymlink != 0 || !info.IsDir() {
			if lstatErr != nil {
				return nil, lstatErr
			}
			return nil, ErrInvalidMarkdownHistory
		}
		checkedPath := filepath.Join(util.HistoryDir, dirEntry.Name())
		if lstatErr = markdownAfterRootComponentCheck(checkedPath); lstatErr != nil {
			return nil, lstatErr
		}
		batchRoot, openErr := root.OpenRoot(dirEntry.Name())
		if openErr != nil {
			return nil, errors.Join(ErrInvalidMarkdownHistory, openErr)
		}
		opened, statErr := batchRoot.Stat(".")
		after, afterErr := root.Lstat(dirEntry.Name())
		if statErr != nil || afterErr != nil || after.Mode()&os.ModeSymlink != 0 || !os.SameFile(info, opened) || !os.SameFile(opened, after) {
			batchRoot.Close()
			return nil, errors.Join(ErrInvalidMarkdownHistory, statErr, afterErr)
		}
		manifestInfo, manifestErr := batchRoot.Lstat("markdown.json")
		if os.IsNotExist(manifestErr) {
			batchRoot.Close()
			continue
		}
		if manifestErr != nil || manifestInfo.Mode()&os.ModeSymlink != 0 || !manifestInfo.Mode().IsRegular() {
			batchRoot.Close()
			return nil, errors.Join(ErrInvalidMarkdownHistory, manifestErr)
		}
		manifestFile, readErr := batchRoot.Open("markdown.json")
		if readErr != nil {
			batchRoot.Close()
			return nil, readErr
		}
		manifestData, readErr := io.ReadAll(manifestFile)
		_ = manifestFile.Close()
		_ = batchRoot.Close()
		if readErr != nil {
			return nil, readErr
		}
		entries, readErr := decodeMarkdownTrashManifest(manifestData)
		if readErr != nil {
			return nil, readErr
		}
		manifestPath := filepath.Join(checkedPath, "markdown.json")
		for _, entry := range entries {
			if entry == nil || entry.ID == "" {
				return nil, ErrInvalidMarkdownHistory
			}
			if _, ok := seen[entry.ID]; ok {
				return nil, ErrInvalidMarkdownHistory
			}
			seen[entry.ID] = struct{}{}
			records = append(records, &markdownTrashRecord{entry: entry, manifestPath: manifestPath})
		}
	}
	return records, nil
}

func findMarkdownTrashRecord(id string) (*markdownTrashRecord, error) {
	if id == "" || strings.ContainsAny(id, "/\\") {
		return nil, ErrInvalidMarkdownHistory
	}
	records, err := loadMarkdownTrashRecords()
	if err != nil {
		return nil, err
	}
	for _, record := range records {
		if record.entry.ID == id {
			return record, nil
		}
	}
	return nil, os.ErrNotExist
}

func validateMarkdownTrashEntry(record *markdownTrashRecord) (string, error) {
	if record == nil || record.entry == nil {
		return "", ErrInvalidMarkdownHistory
	}
	entry := record.entry
	if entry.Notebook == "" || entry.Notebook == "." || entry.Notebook == ".." ||
		path.Base(entry.Notebook) != entry.Notebook || strings.ContainsAny(entry.Notebook, "/\\") {
		return "", ErrInvalidMarkdownHistory
	}
	originalPath := "/" + strings.TrimPrefix(filepath.ToSlash(entry.OriginalPath), "/")
	if originalPath != entry.OriginalPath || path.Clean(originalPath) != originalPath || !isMarkdownFileName(path.Base(originalPath)) {
		return "", ErrInvalidMarkdownHistory
	}
	batchName := filepath.Base(filepath.Dir(record.manifestPath))
	expectedRelPath := path.Join(batchName, entry.Notebook, strings.TrimPrefix(originalPath, "/"))
	if filepath.ToSlash(entry.HistoryPath) != expectedRelPath {
		return "", ErrInvalidMarkdownHistory
	}
	historyPath := filepath.Join(util.HistoryDir, filepath.FromSlash(entry.HistoryPath))
	rel, err := filepath.Rel(util.HistoryDir, historyPath)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) || filepath.IsAbs(rel) {
		return "", ErrInvalidMarkdownHistory
	}
	if err = validateMarkdownHistoryPath(historyPath, false); err != nil {
		return "", err
	}
	return historyPath, nil
}

func ensureMarkdownHistoryRoot() error {
	info, err := os.Lstat(util.HistoryDir)
	if err == nil {
		if info.Mode()&os.ModeSymlink != 0 || !info.IsDir() {
			return ErrInvalidMarkdownHistory
		}
		root, openErr := openStableRoot(util.HistoryDir, ErrInvalidMarkdownHistory)
		if openErr != nil {
			return openErr
		}
		return root.Close()
	}
	if !os.IsNotExist(err) {
		return err
	}
	parentPath, leaf := filepath.Dir(util.HistoryDir), filepath.Base(util.HistoryDir)
	parent, err := openStableRoot(parentPath, ErrInvalidMarkdownHistory)
	if err != nil {
		return err
	}
	defer parent.Close()
	if err = parent.Mkdir(leaf, 0755); err != nil && !os.IsExist(err) {
		return err
	}
	dir, err := parent.Open(".")
	if err != nil {
		return err
	}
	if err = errors.Join(dir.Sync(), dir.Close()); err != nil {
		return err
	}
	markdownDurabilityHook("mkdir-parent-synced", util.HistoryDir)
	root, err := openStableRoot(util.HistoryDir, ErrInvalidMarkdownHistory)
	if err != nil {
		return err
	}
	return root.Close()
}

func validateMarkdownHistoryPath(targetPath string, allowMissing bool) error {
	if err := validatePathWithoutSymlinks(util.HistoryDir, targetPath, allowMissing); err != nil {
		if errors.Is(err, ErrInvalidMarkdownPath) {
			return ErrInvalidMarkdownHistory
		}
		return err
	}
	return nil
}

func openMarkdownHistoryRoot() (*os.Root, error) {
	if err := ensureMarkdownHistoryRoot(); err != nil {
		return nil, err
	}
	return openStableRoot(util.HistoryDir, ErrInvalidMarkdownHistory)
}

func markdownHistoryRelative(targetPath string) (string, error) {
	rel, err := filepath.Rel(util.HistoryDir, targetPath)
	if err != nil || filepath.IsAbs(rel) || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "", ErrInvalidMarkdownHistory
	}
	return rel, nil
}

func validateStableHistoryComponents(root *os.Root, relPath string, allowMissing bool) error {
	err := validateStableRootComponents(root, util.HistoryDir, relPath, allowMissing)
	if errors.Is(err, ErrInvalidMarkdownPath) {
		return ErrInvalidMarkdownHistory
	}
	return err
}

func mkdirMarkdownHistoryPath(targetPath string, mode os.FileMode, all bool) error {
	relPath, err := markdownHistoryRelative(targetPath)
	if err != nil {
		return err
	}
	if all {
		return mkdirStableDirectories(util.HistoryDir, relPath, mode, ErrInvalidMarkdownHistory)
	}
	parent, leaf, err := openStableParent(util.HistoryDir, relPath, ErrInvalidMarkdownHistory)
	if err != nil {
		return err
	}
	defer parent.Close()
	if err = parent.Mkdir(leaf, mode); err != nil {
		return err
	}
	dir, err := parent.Open(".")
	if err != nil {
		return err
	}
	if err = errors.Join(dir.Sync(), dir.Close()); err != nil {
		return err
	}
	markdownDurabilityHook("mkdir-parent-synced", targetPath)
	return nil
}

func writeMarkdownHistoryFile(targetPath string, data []byte) error {
	return writeMarkdownHistoryFileMode(targetPath, data, 0644)
}

func writeMarkdownHistoryFileMode(targetPath string, data []byte, mode os.FileMode) error {
	relPath, err := markdownHistoryRelative(targetPath)
	if err != nil {
		return err
	}
	root, leaf, err := openStableParent(util.HistoryDir, relPath, ErrInvalidMarkdownHistory)
	if err != nil {
		return err
	}
	defer root.Close()
	if info, lstatErr := root.Lstat(leaf); lstatErr == nil && info.Mode()&os.ModeSymlink != 0 {
		return ErrInvalidMarkdownHistory
	}
	file, err := root.OpenFile(leaf, os.O_WRONLY|os.O_CREATE|os.O_EXCL, mode)
	if err != nil {
		return err
	}
	if _, err = file.Write(data); err == nil {
		err = file.Chmod(mode)
	}
	if err == nil {
		err = file.Sync()
	}
	if closeErr := file.Close(); err != nil || closeErr != nil {
		return errors.Join(err, closeErr)
	}
	if err = syncMarkdownHistoryParent(targetPath); err != nil {
		return err
	}
	markdownDurabilityHook("history-payload-parent-synced", targetPath)
	return nil
}

func readMarkdownHistoryFile(targetPath string) ([]byte, error) {
	relPath, err := markdownHistoryRelative(targetPath)
	if err != nil {
		return nil, err
	}
	root, leaf, err := openStableParent(util.HistoryDir, relPath, ErrInvalidMarkdownHistory)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	info, err := root.Lstat(leaf)
	if err != nil || info.Mode()&os.ModeSymlink != 0 || !info.Mode().IsRegular() {
		return nil, errors.Join(ErrInvalidMarkdownHistory, err)
	}
	if err = markdownAfterRootComponentCheck(targetPath); err != nil {
		return nil, err
	}
	file, err := root.Open(leaf)
	if err != nil {
		return nil, errors.Join(ErrInvalidMarkdownHistory, err)
	}
	defer file.Close()
	opened, err := file.Stat()
	current, lstatErr := root.Lstat(leaf)
	if err != nil || lstatErr != nil || current.Mode()&os.ModeSymlink != 0 || !os.SameFile(opened, current) {
		return nil, errors.Join(ErrInvalidMarkdownHistory, err, lstatErr)
	}
	return io.ReadAll(file)
}

func renameMarkdownHistoryFile(oldPath, newPath string) error {
	oldRel, err := markdownHistoryRelative(oldPath)
	if err != nil {
		return err
	}
	newRel, err := markdownHistoryRelative(newPath)
	if err != nil {
		return err
	}
	if filepath.Dir(oldRel) != filepath.Dir(newRel) {
		return ErrInvalidMarkdownHistory
	}
	root, oldLeaf, err := openStableParent(util.HistoryDir, oldRel, ErrInvalidMarkdownHistory)
	if err != nil {
		return err
	}
	defer root.Close()
	if err = root.Rename(oldLeaf, filepath.Base(newRel)); err != nil {
		return err
	}
	return syncMarkdownHistoryParent(oldPath)
}

func renameMarkdownHistoryFileNoReplace(oldPath, newPath string) error {
	oldRel, err := markdownHistoryRelative(oldPath)
	if err != nil {
		return err
	}
	newRel, err := markdownHistoryRelative(newPath)
	if err != nil {
		return err
	}
	if filepath.Dir(oldRel) != filepath.Dir(newRel) {
		return ErrInvalidMarkdownHistory
	}
	root, oldLeaf, err := openStableParent(util.HistoryDir, oldRel, ErrInvalidMarkdownHistory)
	if err != nil {
		return err
	}
	defer root.Close()
	newLeaf := filepath.Base(newRel)
	if _, err = root.Lstat(newLeaf); err == nil {
		return os.ErrExist
	} else if !os.IsNotExist(err) {
		return err
	}
	if err = root.Rename(oldLeaf, newLeaf); err != nil {
		return err
	}
	return syncMarkdownHistoryParent(oldPath)
}

func removeMarkdownHistoryFile(targetPath string) error {
	relPath, err := markdownHistoryRelative(targetPath)
	if err != nil {
		return err
	}
	root, leaf, err := openStableParent(util.HistoryDir, relPath, ErrInvalidMarkdownHistory)
	if err != nil {
		return err
	}
	defer root.Close()
	if err = root.Remove(leaf); err != nil {
		return err
	}
	return syncMarkdownHistoryParent(targetPath)
}

func syncMarkdownHistoryParent(targetPath string) error {
	relPath, err := markdownHistoryRelative(targetPath)
	if err != nil {
		return err
	}
	root, _, err := openStableParent(util.HistoryDir, relPath, ErrInvalidMarkdownHistory)
	if err != nil {
		return err
	}
	defer root.Close()
	dir, err := root.Open(".")
	if err != nil {
		return err
	}
	return errors.Join(dir.Sync(), dir.Close())
}

func removeMarkdownHistoryAll(targetPath string) error {
	relPath, err := markdownHistoryRelative(targetPath)
	if err != nil {
		return err
	}
	root, err := openMarkdownHistoryRoot()
	if err != nil {
		return err
	}
	defer root.Close()
	if err = root.RemoveAll(relPath); err != nil {
		return err
	}
	return syncMarkdownHistoryParent(targetPath)
}

func pruneEmptyMarkdownHistoryDirs(from, stop string) {
	for current := from; current != stop && current != filepath.Dir(current); current = filepath.Dir(current) {
		relPath, err := markdownHistoryRelative(current)
		if err != nil {
			return
		}
		root, leaf, err := openStableParent(util.HistoryDir, relPath, ErrInvalidMarkdownHistory)
		if err != nil {
			return
		}
		dir, err := root.Open(leaf)
		if err != nil {
			root.Close()
			return
		}
		entries, err := dir.ReadDir(-1)
		_ = dir.Close()
		_ = root.Close()
		if err != nil || len(entries) != 0 {
			return
		}
		if err = removeMarkdownHistoryFile(current); err != nil {
			return
		}
	}
}
