// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"maps"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/88250/gulu"
	"github.com/siyuan-note/siyuan/kernel/util"
)

var (
	ErrMarkdownSimulatedCrash     = errors.New("simulated Markdown transaction crash")
	markdownTransactionCrashHook  = func(string, string) error { return nil }
	markdownTransactionWriteHook  = func(string, string) error { return nil }
	markdownSaveCommitHook        = func(string, string) error { return nil }
	markdownInstallValidationHook = func(string, string) error { return nil }
	markdownIdentityDeleteHook    = func(string) error { return nil }
	markdownDurabilityHook        = func(string, string) {}
	markdownCopyValidationHook    = func(string) error { return nil }
	markdownCopyCrashHook         = func(string) error { return nil }
	markdownLinkDurabilityHook    = func(string) error { return nil }
)

type markdownFileIdentity struct {
	Size     int64       `json:"size"`
	Mtime    int64       `json:"mtime"`
	Mode     os.FileMode `json:"mode"`
	Revision string      `json:"revision"`
	SystemID string      `json:"systemID"`
}

type markdownTransaction struct {
	ID            string                      `json:"id"`
	Kind          string                      `json:"kind"`
	Phase         string                      `json:"phase"`
	Box           string                      `json:"box"`
	Source        string                      `json:"source"`
	Destination   string                      `json:"destination,omitempty"`
	Staging       string                      `json:"staging,omitempty"`
	TargetStaging string                      `json:"targetStaging,omitempty"`
	Quarantine    string                      `json:"quarantine,omitempty"`
	SourceID      markdownFileIdentity        `json:"sourceIdentity"`
	TargetID      markdownFileIdentity        `json:"targetIdentity,omitempty"`
	NewRevision   string                      `json:"newRevision,omitempty"`
	FromSort      *markdownSortSnapshotRecord `json:"fromSort,omitempty"`
	ToSort        *markdownSortSnapshotRecord `json:"toSort,omitempty"`
	RecentDocs    []*RecentDoc                `json:"recentDocs,omitempty"`
	journalPath   string
	dirPath       string
}

type markdownSortSnapshotRecord struct {
	Path    string         `json:"path"`
	Existed bool           `json:"existed"`
	Values  map[string]int `json:"values"`
}

func markdownIdentity(filePath string) (markdownFileIdentity, error) {
	file, root, err := openMarkdownFileRead(filePath)
	if err != nil {
		return markdownFileIdentity{}, err
	}
	defer root.Close()
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return markdownFileIdentity{}, err
	}
	data, err := io.ReadAll(file)
	if err != nil {
		return markdownFileIdentity{}, err
	}
	return markdownFileIdentity{
		Size: info.Size(), Mtime: info.ModTime().UnixNano(), Mode: info.Mode().Perm(), Revision: markdownRevision(data),
		SystemID: markdownPlatformFileIdentity(file, info),
	}, nil
}

func sameMarkdownIdentity(filePath string, expected markdownFileIdentity) (bool, error) {
	actual, err := markdownIdentity(filePath)
	if err != nil {
		return false, err
	}
	return actual == expected, nil
}

func beginMarkdownTransaction(kind, boxID, sourcePath, destinationPath, newRevision string) (*markdownTransaction, error) {
	sourceID, err := markdownIdentity(sourcePath)
	if err != nil {
		return nil, err
	}
	id := fmt.Sprintf("%d-%s", time.Now().UnixNano(), gulu.Rand.String(8))
	dirPath := filepath.Join(util.DataDir, boxID, ".siyuan", "markdown-transactions", id)
	if err = mkdirAllMarkdownContained(dirPath, 0700); err != nil {
		return nil, err
	}
	tx := &markdownTransaction{
		ID: id, Kind: kind, Phase: "prepared", Box: boxID, Source: sourcePath, Destination: destinationPath,
		Staging: filepath.Join(dirPath, "payload.md"), SourceID: sourceID, NewRevision: newRevision,
		Quarantine:  filepath.Join(dirPath, "source.md"),
		journalPath: filepath.Join(dirPath, "transaction.json"), dirPath: dirPath,
	}
	if destinationPath != "" {
		tx.TargetStaging = filepath.Join(filepath.Dir(destinationPath), "."+filepath.Base(destinationPath)+".copying-"+id)
	}
	if err = writeMarkdownTransaction(tx); err != nil {
		_ = finishMarkdownTransaction(tx)
		return nil, err
	}
	if err = syncMarkdownParent(tx.dirPath); err != nil {
		return nil, err
	}
	markdownDurabilityHook("transaction-parent-synced", tx.dirPath)
	return tx, nil
}

func beginMarkdownInstallTransaction(kind, boxID, destinationPath string, data []byte, mode os.FileMode) (*markdownTransaction, error) {
	id := fmt.Sprintf("%d-%s", time.Now().UnixNano(), gulu.Rand.String(8))
	dirPath := filepath.Join(util.DataDir, boxID, ".siyuan", "markdown-transactions", id)
	if err := mkdirAllMarkdownContained(dirPath, 0700); err != nil {
		return nil, err
	}
	tx := &markdownTransaction{
		ID: id, Kind: kind, Phase: "prepared", Box: boxID, Destination: destinationPath,
		Staging: filepath.Join(dirPath, "payload.md"), NewRevision: markdownRevision(data),
		journalPath: filepath.Join(dirPath, "transaction.json"), dirPath: dirPath,
	}
	if err := writeMarkdownTransaction(tx); err != nil {
		_ = finishMarkdownTransaction(tx)
		return nil, err
	}
	if err := syncMarkdownParent(tx.dirPath); err != nil {
		return nil, err
	}
	markdownDurabilityHook("transaction-parent-synced", tx.dirPath)
	file, err := openMarkdownFileNoReplace(tx.Staging, mode)
	if err != nil {
		return nil, err
	}
	written, writeErr := file.Write(data)
	if writeErr == nil && written != len(data) {
		writeErr = io.ErrShortWrite
	}
	if writeErr == nil {
		writeErr = file.Chmod(mode)
	}
	if writeErr == nil {
		writeErr = file.Sync()
	}
	closeErr := file.Close()
	if writeErr != nil || closeErr != nil {
		return nil, errors.Join(writeErr, closeErr)
	}
	tx.TargetID, err = markdownIdentity(tx.Staging)
	if err != nil {
		return nil, err
	}
	tx.Phase = "staged"
	if err = writeMarkdownTransaction(tx); err != nil {
		return nil, err
	}
	return tx, nil
}

func installMarkdownTransaction(tx *markdownTransaction) error {
	rootPath, stageRel, err := markdownRootAndRelative(tx.Staging)
	if err != nil {
		return err
	}
	_, destinationRel, err := markdownRootAndRelative(tx.Destination)
	if err != nil {
		return err
	}
	root, err := openStableMarkdownRoot(rootPath)
	if err != nil {
		return err
	}
	if err = root.Link(stageRel, destinationRel); err != nil {
		root.Close()
		return err
	}
	if err = root.Close(); err != nil {
		return errors.Join(err, rollbackMarkdownInstall(tx))
	}
	if err = syncMarkdownParent(tx.Destination); err != nil {
		return errors.Join(err, rollbackMarkdownInstall(tx))
	}
	if err = markdownInstallValidationHook(tx.Kind, tx.Destination); err != nil {
		return errors.Join(err, rollbackMarkdownInstall(tx))
	}
	matches, identityErr := sameMarkdownIdentity(tx.Destination, tx.TargetID)
	if identityErr != nil || !matches {
		if identityErr == nil {
			identityErr = ErrMarkdownConflict
		}
		return errors.Join(identityErr, rollbackMarkdownInstall(tx))
	}
	tx.Phase = "installed"
	if err = writeMarkdownTransaction(tx); err != nil {
		return errors.Join(err, rollbackMarkdownInstall(tx))
	}
	return nil
}

func rollbackMarkdownInstall(tx *markdownTransaction) error {
	cleanupErr := removeMarkdownFileWithIdentity(tx.Destination, tx.TargetID)
	if cleanupErr != nil && !os.IsNotExist(cleanupErr) {
		return cleanupErr
	}
	tx.Phase = "staged"
	return writeMarkdownTransaction(tx)
}

func finalizeMarkdownInstall(tx *markdownTransaction) error {
	if tx.Phase == "metadata-committed" {
		if err := removeMarkdownFileWithIdentity(tx.Staging, tx.TargetID); err != nil && !os.IsNotExist(err) {
			return err
		}
		tx.Phase = "source-removed"
		if err := writeMarkdownTransaction(tx); err != nil {
			return err
		}
	}
	if tx.Phase == "source-removed" {
		if tx.TargetStaging != "" {
			if err := removeMarkdownFileWithIdentity(tx.TargetStaging, tx.TargetID); err != nil && !os.IsNotExist(err) {
				return err
			}
		}
		tx.Phase = "finalized"
		if err := writeMarkdownTransaction(tx); err != nil {
			return err
		}
	}
	return finishMarkdownTransaction(tx)
}

func writeMarkdownTransaction(tx *markdownTransaction) error {
	if err := markdownTransactionWriteHook(tx.Kind, tx.Phase); err != nil {
		return err
	}
	data, err := json.Marshal(tx)
	if err != nil {
		return err
	}
	tmpPath := tx.journalPath + ".tmp"
	_ = removeMarkdownPath(tmpPath)
	file, err := openMarkdownFileNoReplace(tmpPath, 0600)
	if err != nil {
		return err
	}
	if _, err = file.Write(data); err == nil {
		err = file.Sync()
	}
	closeErr := file.Close()
	if err != nil || closeErr != nil {
		_ = removeMarkdownPath(tmpPath)
		return errors.Join(err, closeErr)
	}
	rootPath, oldRel, err := markdownRootAndRelative(tmpPath)
	if err != nil {
		return err
	}
	_, newRel, err := markdownRootAndRelative(tx.journalPath)
	if err != nil {
		return err
	}
	root, err := openStableMarkdownRoot(rootPath)
	if err != nil {
		return err
	}
	if err = root.Rename(oldRel, newRel); err != nil {
		root.Close()
		return err
	}
	dir, err := root.Open(filepath.Dir(newRel))
	if err != nil {
		root.Close()
		return err
	}
	return errors.Join(dir.Sync(), dir.Close(), root.Close())
}

func removeMarkdownPath(filePath string) error {
	rootPath, relPath, err := markdownRootAndRelative(filePath)
	if err != nil {
		return err
	}
	root, err := openStableMarkdownRoot(rootPath)
	if err != nil {
		return err
	}
	defer root.Close()
	return root.Remove(relPath)
}

func finishMarkdownTransaction(tx *markdownTransaction) error {
	if tx == nil {
		return nil
	}
	rootPath, relPath, err := markdownRootAndRelative(tx.dirPath)
	if err != nil {
		return err
	}
	root, err := openStableMarkdownRoot(rootPath)
	if err != nil {
		return err
	}
	defer root.Close()
	if err = root.RemoveAll(relPath); err != nil {
		return err
	}
	return syncMarkdownParent(tx.dirPath)
}

func stageMarkdownSave(tx *markdownTransaction, data []byte) error {
	file, err := openMarkdownFileNoReplace(tx.Staging, tx.SourceID.Mode)
	if err != nil {
		return err
	}
	if _, err = file.Write(data); err == nil {
		err = file.Chmod(tx.SourceID.Mode)
	}
	if err == nil {
		err = file.Sync()
	}
	closeErr := file.Close()
	if err != nil || closeErr != nil {
		return errors.Join(err, closeErr)
	}
	staged, err := markdownIdentity(tx.Staging)
	if err != nil {
		return err
	}
	if staged.Revision != tx.NewRevision {
		return ErrMarkdownConflict
	}
	tx.TargetID = staged
	tx.Phase = "staged"
	return writeMarkdownTransaction(tx)
}

func commitMarkdownSave(tx *markdownTransaction) error {
	matches, err := sameMarkdownIdentity(tx.Source, tx.SourceID)
	if err != nil || !matches {
		if err != nil {
			return err
		}
		return ErrMarkdownConflict
	}
	tx.Phase = "source-isolating"
	if err = writeMarkdownTransaction(tx); err != nil {
		return errors.Join(err, finishMarkdownTransaction(tx))
	}
	if err = renameMarkdownWithinRoot(tx.Source, tx.Quarantine); err != nil {
		return err
	}
	matches, err = sameMarkdownIdentity(tx.Quarantine, tx.SourceID)
	if err != nil || !matches {
		rollbackErr := moveMarkdownFileNoReplace(tx.Quarantine, tx.Source, false)
		if err == nil {
			err = ErrMarkdownConflict
		}
		return errors.Join(err, rollbackErr)
	}
	tx.Phase = "source-isolated"
	if err = writeMarkdownTransaction(tx); err != nil {
		rollbackErr := rollbackMarkdownSaveIsolation(tx)
		if rollbackErr == nil {
			rollbackErr = persistMarkdownSaveRollback(tx)
		}
		return errors.Join(err, rollbackErr)
	}
	if err = markdownSaveCommitHook("source-isolated", tx.Source); err != nil {
		return err
	}
	return commitMarkdownSaveIsolated(tx)
}

func rollbackMarkdownSaveIsolation(tx *markdownTransaction) error {
	matches, err := sameMarkdownIdentity(tx.Quarantine, tx.SourceID)
	if err != nil || !matches {
		return errors.Join(ErrMarkdownConflict, err)
	}
	if _, err = os.Lstat(tx.Source); err == nil {
		return os.ErrExist
	} else if !os.IsNotExist(err) {
		return err
	}
	return renameMarkdownWithinRoot(tx.Quarantine, tx.Source)
}

func commitMarkdownSaveIsolated(tx *markdownTransaction) error {
	quarantineMatches, err := sameMarkdownIdentity(tx.Quarantine, tx.SourceID)
	if err != nil || !quarantineMatches {
		return errors.Join(ErrMarkdownConflict, err)
	}
	stageMatches, err := sameMarkdownIdentity(tx.Staging, tx.TargetID)
	if err != nil || !stageMatches {
		return errors.Join(ErrMarkdownConflict, err)
	}
	rootPath, stageRel, err := markdownRootAndRelative(tx.Staging)
	if err != nil {
		return err
	}
	_, sourceRel, err := markdownRootAndRelative(tx.Source)
	if err != nil {
		return err
	}
	root, err := openStableMarkdownRoot(rootPath)
	if err != nil {
		return err
	}
	if err = root.Link(stageRel, sourceRel); err != nil {
		root.Close()
		return err
	}
	if err = root.Close(); err != nil {
		return rollbackMarkdownSaveCommit(tx, err)
	}
	if err = markdownSaveCommitHook("linked", tx.Source); err != nil {
		return rollbackMarkdownSaveCommit(tx, err)
	}
	if err = syncMarkdownParent(tx.Source); err != nil {
		return rollbackMarkdownSaveCommit(tx, err)
	}
	matches, err := sameMarkdownIdentity(tx.Source, tx.TargetID)
	if err != nil || !matches {
		if err == nil {
			err = ErrMarkdownConflict
		}
		return rollbackMarkdownSaveCommit(tx, err)
	}
	tx.Phase = "installed"
	if err = writeMarkdownTransaction(tx); err != nil {
		return rollbackMarkdownSaveCommit(tx, err)
	}
	return nil
}

func rollbackMarkdownSaveCommit(tx *markdownTransaction, cause error) error {
	var rollbackErrors []error
	matches, err := sameMarkdownIdentity(tx.Source, tx.TargetID)
	if err == nil && matches {
		rollbackErrors = append(rollbackErrors, removeMarkdownFileWithIdentity(tx.Source, tx.TargetID))
	} else if err != nil && !os.IsNotExist(err) {
		rollbackErrors = append(rollbackErrors, err)
	} else if err == nil {
		rollbackErrors = append(rollbackErrors, ErrMarkdownConflict)
	}
	if errors.Join(rollbackErrors...) == nil {
		rollbackErrors = append(rollbackErrors, rollbackMarkdownSaveIsolation(tx))
	}
	if errors.Join(rollbackErrors...) == nil {
		tx.Phase = "rolled-back"
		rollbackErrors = append(rollbackErrors, writeMarkdownTransaction(tx))
	}
	return errors.Join(cause, errors.Join(rollbackErrors...))
}

func persistMarkdownSaveRollback(tx *markdownTransaction) error {
	tx.Phase = "rolled-back"
	if err := writeMarkdownTransaction(tx); err != nil {
		return err
	}
	return finalizeMarkdownSaveRollback(tx)
}

func finalizeMarkdownSaveRollback(tx *markdownTransaction) error {
	if err := removeMarkdownFileWithIdentity(tx.Staging, tx.TargetID); err != nil && !os.IsNotExist(err) {
		return err
	}
	return finishMarkdownTransaction(tx)
}

func recoverMarkdownSavePrecommit(tx *markdownTransaction) error {
	sourceMatchesOld, oldErr := sameMarkdownIdentity(tx.Source, tx.SourceID)
	quarantineMatches, quarantineErr := sameMarkdownIdentity(tx.Quarantine, tx.SourceID)
	sourceMatchesNew, newErr := sameMarkdownIdentity(tx.Source, tx.TargetID)
	if sourceMatchesOld && os.IsNotExist(quarantineErr) {
		return persistMarkdownSaveRollback(tx)
	}
	if os.IsNotExist(oldErr) && os.IsNotExist(newErr) && quarantineMatches {
		if err := rollbackMarkdownSaveIsolation(tx); err != nil {
			return err
		}
		return persistMarkdownSaveRollback(tx)
	}
	if sourceMatchesNew && quarantineMatches {
		if err := rollbackMarkdownSaveCommit(tx, nil); err != nil {
			return err
		}
		return finalizeMarkdownSaveRollback(tx)
	}
	return errors.Join(ErrMarkdownConflict, oldErr, newErr, quarantineErr)
}

func finalizeMarkdownSave(tx *markdownTransaction) error {
	if tx.Phase == "installed" {
		if err := removeMarkdownFileWithIdentity(tx.Quarantine, tx.SourceID); err != nil && !os.IsNotExist(err) {
			return err
		}
		tx.Phase = "source-removed"
		if err := writeMarkdownTransaction(tx); err != nil {
			return err
		}
		if err := markdownTransactionCrashHook("save", "source-removed"); err != nil {
			return err
		}
	}
	if tx.Phase == "source-removed" {
		if err := removeMarkdownFileWithIdentity(tx.Staging, tx.TargetID); err != nil && !os.IsNotExist(err) {
			return err
		}
		tx.Phase = "finalized"
		if err := writeMarkdownTransaction(tx); err != nil {
			return err
		}
		if err := markdownTransactionCrashHook("save", "finalized"); err != nil {
			return err
		}
	}
	return finishMarkdownTransaction(tx)
}

func markMarkdownMetadataCommitted(tx *markdownTransaction) error {
	tx.Phase = "metadata-committed"
	return writeMarkdownTransaction(tx)
}

func finalizeMarkdownMove(tx *markdownTransaction) error {
	if tx.Phase == "metadata-committed" {
		if err := removeMarkdownFileWithIdentity(tx.Staging, tx.SourceID); err != nil && !os.IsNotExist(err) {
			return err
		}
		tx.Phase = "source-removed"
		if err := writeMarkdownTransaction(tx); err != nil {
			return err
		}
		if err := markdownTransactionCrashHook(tx.Kind, "source-removed"); err != nil {
			return err
		}
	}
	if tx.Phase == "source-removed" {
		if tx.TargetStaging != "" {
			if err := removeMarkdownFileWithIdentity(tx.TargetStaging, tx.TargetID); err != nil && !os.IsNotExist(err) {
				return err
			}
		}
		tx.Phase = "finalized"
		if err := writeMarkdownTransaction(tx); err != nil {
			return err
		}
		if err := markdownTransactionCrashHook(tx.Kind, "finalized"); err != nil {
			return err
		}
	}
	return finishMarkdownTransaction(tx)
}

func copyMarkdownTransactionFile(tx *markdownTransaction) error {
	tx.Phase = "target-creating"
	if err := writeMarkdownTransaction(tx); err != nil {
		return err
	}
	if err := copyMarkdownFileNoReplace(tx.Source, tx.TargetStaging); err != nil {
		return err
	}
	targetID, err := markdownIdentity(tx.TargetStaging)
	if err != nil {
		return err
	}
	tx.TargetID = targetID
	tx.Phase = "target-created"
	if err = writeMarkdownTransaction(tx); err != nil {
		return err
	}
	if err = markdownCopyCrashHook("target-created"); err != nil {
		return err
	}
	if err = linkMarkdownFileNoReplace(tx.TargetStaging, tx.Destination); err != nil {
		if os.IsExist(err) {
			if cleanupErr := cleanupMarkdownTargetStaging(tx); cleanupErr == nil {
				tx.Phase = "target-collision"
				if writeMarkdownTransaction(tx) == nil {
					_ = finishMarkdownTransaction(tx)
				}
			}
			return err
		}
		cleanupErr := cleanupMarkdownTransactionTargets(tx)
		return errors.Join(err, cleanupErr)
	}
	cleanup := func(cause error) error {
		return errors.Join(cause, cleanupMarkdownTransactionTargets(tx))
	}
	if err = markdownCopyValidationHook(tx.Destination); err != nil {
		return cleanup(err)
	}
	if targetID.Revision != tx.SourceID.Revision {
		return cleanup(ErrMarkdownConflict)
	}
	matches, err := sameMarkdownIdentity(tx.Source, tx.SourceID)
	if err != nil || !matches {
		if err != nil {
			return cleanup(err)
		}
		return cleanup(ErrMarkdownConflict)
	}
	tx.Phase = "copied"
	if err = writeMarkdownTransaction(tx); err != nil {
		return cleanup(err)
	}
	return nil
}

func linkMarkdownFileNoReplace(sourcePath, destinationPath string) error {
	rootPath, sourceRel, err := markdownRootAndRelative(sourcePath)
	if err != nil {
		return err
	}
	destinationRoot, destinationRel, err := markdownRootAndRelative(destinationPath)
	if err != nil || destinationRoot != rootPath {
		return errors.Join(ErrInvalidMarkdownPath, err)
	}
	root, err := openStableMarkdownRoot(rootPath)
	if err != nil {
		return err
	}
	if err = root.Link(sourceRel, destinationRel); err != nil {
		root.Close()
		return err
	}
	if err = markdownLinkDurabilityHook("close"); err != nil {
		_ = root.Close()
		return err
	}
	if err = root.Close(); err != nil {
		return err
	}
	if err = markdownLinkDurabilityHook("sync"); err != nil {
		return err
	}
	return syncMarkdownParent(destinationPath)
}

func cleanupMarkdownTransactionTargets(tx *markdownTransaction) error {
	var cleanupErrors []error
	for _, targetPath := range []string{tx.Destination, tx.TargetStaging} {
		if targetPath == "" {
			continue
		}
		if err := removeMarkdownFileWithIdentity(targetPath, tx.TargetID); err != nil && !os.IsNotExist(err) {
			cleanupErrors = append(cleanupErrors, err)
		}
	}
	return errors.Join(cleanupErrors...)
}

func cleanupMarkdownTargetStaging(tx *markdownTransaction) error {
	if tx.TargetStaging == "" {
		return nil
	}
	if err := removeMarkdownFileWithIdentity(tx.TargetStaging, tx.TargetID); err != nil && !os.IsNotExist(err) {
		return err
	}
	return nil
}

func recoverMarkdownCopyTarget(tx *markdownTransaction) error {
	if tx.Phase == "target-collision" {
		if err := cleanupMarkdownTargetStaging(tx); err != nil {
			return err
		}
		return finishMarkdownTransaction(tx)
	}
	if tx.Phase == "target-creating" {
		identity, err := markdownIdentity(tx.TargetStaging)
		if os.IsNotExist(err) {
			return finishMarkdownTransaction(tx)
		}
		if err != nil {
			return err
		}
		// 私有路径在创建前已写入 journal；恢复时锁定当前文件身份，再通过 quarantine 二次校验删除。
		tx.TargetID = identity
	}
	destinationID, destinationErr := markdownIdentity(tx.Destination)
	if destinationErr == nil && destinationID != tx.TargetID {
		if err := cleanupMarkdownTargetStaging(tx); err != nil {
			return err
		}
		tx.Phase = "target-collision"
		if err := writeMarkdownTransaction(tx); err != nil {
			return err
		}
		return finishMarkdownTransaction(tx)
	}
	if destinationErr != nil && !os.IsNotExist(destinationErr) {
		return destinationErr
	}
	if err := cleanupMarkdownTransactionTargets(tx); err != nil {
		return err
	}
	return finishMarkdownTransaction(tx)
}

func recordMarkdownTransactionMetadata(tx *markdownTransaction, fromSort, toSort *markdownSortSnapshot, recentDocs []*RecentDoc) error {
	if fromSort != nil {
		tx.FromSort = &markdownSortSnapshotRecord{Path: fromSort.path, Existed: fromSort.existed, Values: maps.Clone(fromSort.values)}
	}
	if toSort != nil && toSort != fromSort {
		tx.ToSort = &markdownSortSnapshotRecord{Path: toSort.path, Existed: toSort.existed, Values: maps.Clone(toSort.values)}
	}
	tx.RecentDocs = cloneRecentDocs(recentDocs)
	return writeMarkdownTransaction(tx)
}

func restoreMarkdownTransactionMetadata(tx *markdownTransaction) error {
	var restoreErrors []error
	for _, record := range []*markdownSortSnapshotRecord{tx.FromSort, tx.ToSort} {
		if record == nil {
			continue
		}
		restoreErrors = append(restoreErrors, restoreMarkdownSort(&markdownSortSnapshot{
			path: record.Path, existed: record.Existed, values: maps.Clone(record.Values),
		}))
	}
	if tx.RecentDocs != nil {
		recentDocLock.Lock()
		restoreErrors = append(restoreErrors, setRecentDocs(cloneRecentDocs(tx.RecentDocs)))
		recentDocLock.Unlock()
	}
	return errors.Join(restoreErrors...)
}

func stageMarkdownTransactionSource(tx *markdownTransaction) error {
	previousPhase := tx.Phase
	tx.Phase = "source-staging"
	if err := writeMarkdownTransaction(tx); err != nil {
		tx.Phase = previousPhase
		return err
	}
	if err := renameMarkdownWithinRoot(tx.Source, tx.Staging); err != nil {
		return err
	}
	matches, err := sameMarkdownIdentity(tx.Staging, tx.SourceID)
	if err != nil || !matches {
		rollbackErr := moveMarkdownFileNoReplace(tx.Staging, tx.Source, false)
		if err == nil {
			err = ErrMarkdownConflict
		}
		return errors.Join(err, rollbackErr)
	}
	tx.Phase = "source-staged"
	if err = writeMarkdownTransaction(tx); err != nil {
		rollbackErr := rollbackMarkdownStagedSource(tx)
		if rollbackErr == nil {
			tx.Phase = previousPhase
			rollbackErr = writeMarkdownTransaction(tx)
		}
		return errors.Join(err, rollbackErr)
	}
	return nil
}

func rollbackMarkdownStagedSource(tx *markdownTransaction) error {
	matches, err := sameMarkdownIdentity(tx.Staging, tx.SourceID)
	if err != nil || !matches {
		return errors.Join(ErrMarkdownConflict, err)
	}
	if _, err = os.Lstat(tx.Source); err == nil {
		return os.ErrExist
	} else if !os.IsNotExist(err) {
		return err
	}
	return renameMarkdownWithinRoot(tx.Staging, tx.Source)
}

func rollbackMarkdownTransactionFiles(tx *markdownTransaction) error {
	var rollbackErrors []error
	if _, err := os.Lstat(tx.Staging); err == nil {
		rollbackErrors = append(rollbackErrors, moveMarkdownFileNoReplace(tx.Staging, tx.Source, false))
	} else if !os.IsNotExist(err) {
		rollbackErrors = append(rollbackErrors, err)
	}
	if _, err := os.Lstat(tx.Destination); err == nil {
		rollbackErrors = append(rollbackErrors, removeMarkdownFileWithIdentity(tx.Destination, tx.TargetID))
	} else if !os.IsNotExist(err) {
		rollbackErrors = append(rollbackErrors, err)
	}
	if tx.TargetStaging != "" {
		if _, err := os.Lstat(tx.TargetStaging); err == nil {
			rollbackErrors = append(rollbackErrors, removeMarkdownFileWithIdentity(tx.TargetStaging, tx.TargetID))
		} else if !os.IsNotExist(err) {
			rollbackErrors = append(rollbackErrors, err)
		}
	}
	return errors.Join(rollbackErrors...)
}

func readMarkdownFileContained(filePath string) ([]byte, error) {
	file, root, err := openMarkdownFileRead(filePath)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	defer file.Close()
	return io.ReadAll(file)
}

func removeMarkdownFileWithIdentity(filePath string, expected markdownFileIdentity) error {
	if err := markdownIdentityDeleteHook(filePath); err != nil {
		return err
	}
	rootPath, relPath, err := markdownRootAndRelative(filePath)
	if err != nil {
		return err
	}
	root, leaf, err := openStableMarkdownParent(rootPath, relPath)
	if err != nil {
		return err
	}
	defer root.Close()
	quarantineLeaf := leaf + ".removing-" + gulu.Rand.String(16)
	if _, err = root.Lstat(quarantineLeaf); err == nil {
		return os.ErrExist
	} else if !os.IsNotExist(err) {
		return err
	}
	if err = root.Rename(leaf, quarantineLeaf); err != nil {
		return err
	}
	if err = syncMarkdownParent(filePath); err != nil {
		return err
	}
	quarantinePath := filepath.Join(filepath.Dir(filePath), quarantineLeaf)
	matches, identityErr := sameMarkdownIdentity(quarantinePath, expected)
	if identityErr != nil || !matches {
		rollbackErr := moveMarkdownFileNoReplace(quarantinePath, filePath, false)
		if identityErr == nil {
			identityErr = ErrMarkdownConflict
		}
		return errors.Join(identityErr, rollbackErr)
	}
	if err = root.Remove(quarantineLeaf); err != nil {
		return err
	}
	return syncMarkdownParent(filePath)
}

func renameMarkdownWithinRoot(oldPath, newPath string) error {
	oldRootPath, oldRel, err := markdownRootAndRelative(oldPath)
	if err != nil {
		return err
	}
	newRootPath, newRel, err := markdownRootAndRelative(newPath)
	if err != nil {
		return err
	}
	if oldRootPath != newRootPath {
		return ErrInvalidMarkdownPath
	}
	root, err := openStableMarkdownRoot(oldRootPath)
	if err != nil {
		return err
	}
	defer root.Close()
	if err = validateStableRootComponents(root, oldRootPath, oldRel, false); err != nil {
		return err
	}
	if err = validateStableRootComponents(root, oldRootPath, newRel, true); err != nil {
		return err
	}
	if err = root.Rename(oldRel, newRel); err != nil {
		return err
	}
	if err = syncMarkdownParent(oldPath); err != nil {
		return err
	}
	if filepath.Dir(oldPath) != filepath.Dir(newPath) {
		return syncMarkdownParent(newPath)
	}
	return nil
}

func syncMarkdownParent(filePath string) error {
	rootPath, relPath, err := markdownRootAndRelative(filePath)
	if err != nil {
		return err
	}
	root, _, err := openStableMarkdownParent(rootPath, relPath)
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

func RecoverMarkdownTransactions() error {
	markdownFileOperationLock.Lock()
	defer markdownFileOperationLock.Unlock()
	return recoverMarkdownTransactionsLocked()
}

func recoverMarkdownTransactionsLocked() error {
	boxes := Conf.GetBoxes()
	sort.Slice(boxes, func(i, j int) bool { return boxes[i].ID < boxes[j].ID })
	var recoveryErrors []error
	for _, box := range boxes {
		dirPath := filepath.Join(util.DataDir, box.ID, ".siyuan", "markdown-transactions")
		root, err := openStableMarkdownRoot(filepath.Join(util.DataDir, box.ID))
		if err != nil {
			recoveryErrors = append(recoveryErrors, err)
			continue
		}
		dir, err := root.Open(filepath.Join(".siyuan", "markdown-transactions"))
		if os.IsNotExist(err) {
			root.Close()
			continue
		}
		if err != nil {
			root.Close()
			recoveryErrors = append(recoveryErrors, err)
			continue
		}
		entries, err := dir.ReadDir(-1)
		_ = dir.Close()
		_ = root.Close()
		if err != nil {
			recoveryErrors = append(recoveryErrors, err)
			continue
		}
		for _, entry := range entries {
			if !entry.IsDir() {
				continue
			}
			txDir := filepath.Join(dirPath, entry.Name())
			journalPath := filepath.Join(txDir, "transaction.json")
			data, readErr := readMarkdownFileContained(journalPath)
			if os.IsNotExist(readErr) {
				continue
			}
			if readErr != nil {
				recoveryErrors = append(recoveryErrors, readErr)
				continue
			}
			var tx markdownTransaction
			if readErr = json.Unmarshal(data, &tx); readErr != nil {
				recoveryErrors = append(recoveryErrors, readErr)
				continue
			}
			tx.dirPath, tx.journalPath = txDir, journalPath
			if recoverErr := recoverMarkdownTransaction(&tx); recoverErr != nil {
				recoveryErrors = append(recoveryErrors, recoverErr)
			}
		}
	}
	recoveryErrors = append(recoveryErrors, recoverMarkdownPurgeTransactionsLocked())
	return errors.Join(recoveryErrors...)
}

func recoverMarkdownTransaction(tx *markdownTransaction) error {
	switch tx.Kind {
	case "save":
		if tx.Phase == "prepared" {
			if identity, err := markdownIdentity(tx.Staging); err == nil {
				tx.TargetID = identity
			} else if !os.IsNotExist(err) {
				return err
			}
			return recoverMarkdownSavePrecommit(tx)
		}
		if tx.Phase == "staged" || tx.Phase == "source-isolating" || tx.Phase == "source-isolated" {
			return recoverMarkdownSavePrecommit(tx)
		}
		if tx.Phase == "rolled-back" {
			return finalizeMarkdownSaveRollback(tx)
		}
		return finalizeMarkdownSave(tx)
	case "rename", "move":
		if tx.Phase == "prepared" {
			return recoverPreparedMarkdownSource(tx, tx.Staging)
		}
		if tx.Phase == "target-creating" || tx.Phase == "target-created" || tx.Phase == "target-collision" {
			return recoverMarkdownCopyTarget(tx)
		}
		if tx.Phase == "copied" {
			sourceMatches, err := sameMarkdownIdentity(tx.Source, tx.SourceID)
			if err != nil || !sourceMatches {
				if err == nil {
					err = ErrMarkdownConflict
				}
				return err
			}
			targetMatches, err := sameMarkdownIdentity(tx.Destination, tx.TargetID)
			if err != nil || !targetMatches {
				if err == nil {
					err = ErrMarkdownConflict
				}
				return err
			}
			if err = rollbackMarkdownTransactionFiles(tx); err != nil {
				return err
			}
			return finishMarkdownTransaction(tx)
		}
		if tx.Phase == "source-staging" {
			sourceMatches, sourceErr := sameMarkdownIdentity(tx.Source, tx.SourceID)
			stagingMatches, stagingErr := sameMarkdownIdentity(tx.Staging, tx.SourceID)
			if sourceMatches && os.IsNotExist(stagingErr) {
				tx.Phase = "copied"
				if err := writeMarkdownTransaction(tx); err != nil {
					return err
				}
				return recoverMarkdownTransaction(tx)
			}
			if os.IsNotExist(sourceErr) && stagingMatches {
				tx.Phase = "source-staged"
				if err := writeMarkdownTransaction(tx); err != nil {
					return err
				}
				return recoverMarkdownTransaction(tx)
			}
			return errors.Join(ErrMarkdownConflict, sourceErr, stagingErr)
		}
		if tx.Phase == "source-staged" {
			stagingMatches, err := sameMarkdownIdentity(tx.Staging, tx.SourceID)
			if err != nil || !stagingMatches {
				if err == nil {
					err = ErrMarkdownConflict
				}
				return err
			}
			if err = rollbackMarkdownTransactionFiles(tx); err != nil {
				return err
			}
			if err = restoreMarkdownTransactionMetadata(tx); err != nil {
				return err
			}
			return finishMarkdownTransaction(tx)
		}
		if tx.Phase == "metadata-committed" || tx.Phase == "source-removed" || tx.Phase == "finalized" {
			return finalizeMarkdownMove(tx)
		}
		return finishMarkdownTransaction(tx)
	case "recycle":
		if tx.Phase == "prepared" {
			return recoverPreparedMarkdownSource(tx, tx.Staging)
		}
		if tx.Phase == "source-staging" {
			sourceMatches, sourceErr := sameMarkdownIdentity(tx.Source, tx.SourceID)
			stagingMatches, stagingErr := sameMarkdownIdentity(tx.Staging, tx.SourceID)
			if sourceMatches && os.IsNotExist(stagingErr) {
				return finishMarkdownTransaction(tx)
			}
			if os.IsNotExist(sourceErr) && stagingMatches {
				tx.Phase = "staged"
				if err := writeMarkdownTransaction(tx); err != nil {
					return err
				}
				return recoverMarkdownTransaction(tx)
			}
			return errors.Join(ErrMarkdownConflict, sourceErr, stagingErr)
		}
		if tx.Phase == "staged" {
			if _, err := os.Lstat(tx.Source); err == nil {
				return ErrMarkdownConflict
			} else if !os.IsNotExist(err) {
				return err
			}
			stagingMatches, err := sameMarkdownIdentity(tx.Staging, tx.SourceID)
			if err != nil || !stagingMatches {
				if err == nil {
					err = ErrMarkdownConflict
				}
				return err
			}
			if err = renameMarkdownWithinRoot(tx.Staging, tx.Source); err != nil {
				return err
			}
			if tx.Destination != "" {
				if err = removeMarkdownHistoryAll(tx.Destination); err != nil && !os.IsNotExist(err) {
					return err
				}
			}
			if err = restoreMarkdownTransactionMetadata(tx); err != nil {
				return err
			}
		}
		if tx.Phase == "metadata-committed" || tx.Phase == "source-removed" || tx.Phase == "finalized" {
			return finalizeMarkdownMove(tx)
		}
		return finishMarkdownTransaction(tx)
	case "duplicate", "restore":
		if tx.Phase == "prepared" {
			if _, err := markdownIdentity(tx.Staging); os.IsNotExist(err) {
				return finishMarkdownTransaction(tx)
			} else if err != nil {
				return err
			}
			return ErrMarkdownConflict
		}
		if tx.Phase == "staged" {
			if _, err := os.Lstat(tx.Destination); os.IsNotExist(err) {
				if err = restoreMarkdownTransactionMetadata(tx); err != nil {
					return err
				}
				return finishMarkdownTransaction(tx)
			} else if err != nil {
				return err
			}
			matches, err := sameMarkdownIdentity(tx.Destination, tx.TargetID)
			if err != nil || !matches {
				return errors.Join(ErrMarkdownConflict, err)
			}
			tx.Phase = "installed"
		}
		if tx.Phase == "installed" {
			matches, err := sameMarkdownIdentity(tx.Destination, tx.TargetID)
			if err != nil || !matches {
				return errors.Join(ErrMarkdownConflict, err)
			}
			if err = rollbackMarkdownInstall(tx); err != nil {
				return err
			}
			if err = restoreMarkdownTransactionMetadata(tx); err != nil {
				return err
			}
			return finishMarkdownTransaction(tx)
		}
		if tx.Phase == "metadata-committed" || tx.Phase == "source-removed" || tx.Phase == "finalized" {
			if tx.Phase == "metadata-committed" {
				matches, err := sameMarkdownIdentity(tx.Destination, tx.TargetID)
				if err != nil || !matches {
					return errors.Join(ErrMarkdownConflict, err)
				}
			}
			return finalizeMarkdownInstall(tx)
		}
		return ErrInvalidMarkdownPath
	}
	if strings.TrimSpace(tx.Kind) == "" {
		return ErrInvalidMarkdownPath
	}
	return nil
}

func recoverPreparedMarkdownSource(tx *markdownTransaction, payloadPath string) error {
	if payloadPath == "" {
		return finishMarkdownTransaction(tx)
	}
	payloadMatches, payloadErr := sameMarkdownIdentity(payloadPath, tx.SourceID)
	if os.IsNotExist(payloadErr) {
		return finishMarkdownTransaction(tx)
	}
	if payloadErr != nil || !payloadMatches {
		return errors.Join(ErrMarkdownConflict, payloadErr)
	}
	if _, err := markdownIdentity(tx.Source); err == nil {
		return ErrMarkdownConflict
	} else if !os.IsNotExist(err) {
		return err
	}
	if err := renameMarkdownWithinRoot(payloadPath, tx.Source); err != nil {
		return err
	}
	if tx.Destination != "" && tx.TargetID.SystemID != "" {
		if err := removeMarkdownFileWithIdentity(tx.Destination, tx.TargetID); err != nil && !os.IsNotExist(err) {
			return err
		}
	}
	return finishMarkdownTransaction(tx)
}
