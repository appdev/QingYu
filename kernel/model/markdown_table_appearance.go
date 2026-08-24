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
	"strings"
	"sync"
	"time"

	"github.com/88250/gulu"
	"github.com/siyuan-note/filelock"
	"github.com/siyuan-note/logging"
	"github.com/siyuan-note/siyuan/kernel/util"
)

const markdownTableAppearanceSchemaVersion = 1

var markdownTableAppearanceLock sync.Mutex

type MarkdownTableAppearanceAttributes struct {
	WidthMode string `json:"widthMode,omitempty"`
}

type MarkdownTableAppearanceStructure struct {
	ColumnCount       int    `json:"columnCount,omitempty"`
	HeaderFingerprint string `json:"headerFingerprint,omitempty"`
}

type MarkdownTableAppearanceRecord struct {
	TableID            string                            `json:"tableId"`
	ContentFingerprint string                            `json:"contentFingerprint,omitempty"`
	ContextFingerprint string                            `json:"contextFingerprint,omitempty"`
	Structure          MarkdownTableAppearanceStructure  `json:"structure,omitempty"`
	OrdinalHint        int                               `json:"ordinalHint,omitempty"`
	Attributes         MarkdownTableAppearanceAttributes `json:"attributes"`
	Version            int64                             `json:"version"`
	UpdatedAt          int64                             `json:"updatedAt"`
	LastMatchedAt      int64                             `json:"lastMatchedAt,omitempty"`
	DeletedAt          int64                             `json:"deletedAt,omitempty"`
}

type MarkdownTableAppearanceDocument struct {
	Revision int64                                     `json:"revision"`
	Tables   map[string]*MarkdownTableAppearanceRecord `json:"tables"`
}

type MarkdownTableAppearanceStore struct {
	SchemaVersion int                                         `json:"schemaVersion"`
	Documents     map[string]*MarkdownTableAppearanceDocument `json:"documents"`
}

type MarkdownTableAppearancePatch struct {
	ContentFingerprint *string `json:"contentFingerprint,omitempty"`
	ContextFingerprint *string `json:"contextFingerprint,omitempty"`
	ColumnCount        *int    `json:"columnCount,omitempty"`
	HeaderFingerprint  *string `json:"headerFingerprint,omitempty"`
	OrdinalHint        *int    `json:"ordinalHint,omitempty"`
	WidthMode          *string `json:"widthMode,omitempty"`
	LastMatchedAt      *int64  `json:"lastMatchedAt,omitempty"`
	Deleted            *bool   `json:"deleted,omitempty"`
}

type MarkdownTableAppearancePatchResult struct {
	DocumentRevision int64                          `json:"documentRevision"`
	Record           *MarkdownTableAppearanceRecord `json:"record"`
}

func markdownTableAppearancePath() string {
	return filepath.Join(util.DataDir, "storage", "markdown-table-appearance.json")
}

func workspaceMarkdownTableAppearanceDocumentKey(boxID, p string) string {
	return "workspace:" + boxID + ":" + p
}

func migrateWorkspaceMarkdownTableAppearance(fromBoxID, fromPath, toBoxID, toPath string) {
	fromKey := workspaceMarkdownTableAppearanceDocumentKey(fromBoxID, fromPath)
	toKey := workspaceMarkdownTableAppearanceDocumentKey(toBoxID, toPath)
	if err := MigrateMarkdownTableAppearanceDocument(fromKey, toKey); err != nil {
		logging.LogErrorf("migrate Markdown table appearance [%s] to [%s] failed: %s", fromKey, toKey, err)
	}
}

func removeWorkspaceMarkdownTableAppearance(boxID, p string) {
	documentKey := workspaceMarkdownTableAppearanceDocumentKey(boxID, p)
	if err := RemoveMarkdownTableAppearanceDocument(documentKey); err != nil {
		logging.LogErrorf("remove Markdown table appearance [%s] failed: %s", documentKey, err)
	}
}

func emptyMarkdownTableAppearanceStore() *MarkdownTableAppearanceStore {
	return &MarkdownTableAppearanceStore{
		SchemaVersion: markdownTableAppearanceSchemaVersion,
		Documents:     map[string]*MarkdownTableAppearanceDocument{},
	}
}

func validateMarkdownTableAppearanceDocumentKey(documentKey string) error {
	if documentKey == "" || len(documentKey) > 2048 ||
		(!strings.HasPrefix(documentKey, "workspace:") && !strings.HasPrefix(documentKey, "external:")) {
		return errors.New("invalid Markdown table appearance document key")
	}
	return nil
}

func validateMarkdownTableAppearanceTableID(tableID string) error {
	if tableID == "" || len(tableID) > 128 {
		return errors.New("invalid Markdown table appearance table ID")
	}
	return nil
}

func validateMarkdownTableAppearanceFingerprint(value string) error {
	if len(value) > 256 {
		return errors.New("invalid Markdown table appearance fingerprint")
	}
	return nil
}

func validateMarkdownTableAppearancePatch(patch MarkdownTableAppearancePatch) error {
	if patch.ContentFingerprint != nil {
		if err := validateMarkdownTableAppearanceFingerprint(*patch.ContentFingerprint); err != nil {
			return err
		}
	}
	if patch.ContextFingerprint != nil {
		if err := validateMarkdownTableAppearanceFingerprint(*patch.ContextFingerprint); err != nil {
			return err
		}
	}
	if patch.HeaderFingerprint != nil {
		if err := validateMarkdownTableAppearanceFingerprint(*patch.HeaderFingerprint); err != nil {
			return err
		}
	}
	if patch.ColumnCount != nil && (*patch.ColumnCount < 0 || *patch.ColumnCount > 1024) {
		return errors.New("invalid Markdown table appearance column count")
	}
	if patch.OrdinalHint != nil && (*patch.OrdinalHint < 0 || *patch.OrdinalHint > 1000000) {
		return errors.New("invalid Markdown table appearance ordinal")
	}
	if patch.WidthMode != nil && *patch.WidthMode != "auto" && *patch.WidthMode != "even" {
		return errors.New("invalid Markdown table appearance width mode")
	}
	return nil
}

func loadMarkdownTableAppearanceStore() *MarkdownTableAppearanceStore {
	store := emptyMarkdownTableAppearanceStore()
	storePath := markdownTableAppearancePath()
	data, err := filelock.ReadFile(storePath)
	if os.IsNotExist(err) {
		return store
	}
	if err != nil {
		logging.LogErrorf("read Markdown table appearance storage failed: %s", err)
		return store
	}
	if err = gulu.JSON.UnmarshalJSON(data, store); err == nil &&
		store.SchemaVersion == markdownTableAppearanceSchemaVersion && store.Documents != nil {
		return store
	}

	backupPath := fmt.Sprintf("%s.corrupt.%d", storePath, time.Now().UnixMilli())
	if renameErr := os.Rename(storePath, backupPath); renameErr != nil {
		logging.LogErrorf("preserve corrupt Markdown table appearance storage failed: %s", renameErr)
	}
	logging.LogErrorf("load Markdown table appearance storage failed, preserved at [%s]", backupPath)
	return emptyMarkdownTableAppearanceStore()
}

func saveMarkdownTableAppearanceStore(store *MarkdownTableAppearanceStore) error {
	store.SchemaVersion = markdownTableAppearanceSchemaVersion
	if store.Documents == nil {
		store.Documents = map[string]*MarkdownTableAppearanceDocument{}
	}
	data, err := gulu.JSON.MarshalIndentJSON(store, "", "  ")
	if err != nil {
		return err
	}
	storePath := markdownTableAppearancePath()
	if err = os.MkdirAll(filepath.Dir(storePath), 0755); err != nil {
		return err
	}
	return filelock.WriteFile(storePath, data)
}

func cloneMarkdownTableAppearanceDocument(document *MarkdownTableAppearanceDocument) *MarkdownTableAppearanceDocument {
	if document == nil {
		return &MarkdownTableAppearanceDocument{Tables: map[string]*MarkdownTableAppearanceRecord{}}
	}
	data, err := gulu.JSON.MarshalJSON(document)
	if err != nil {
		return &MarkdownTableAppearanceDocument{Revision: document.Revision, Tables: map[string]*MarkdownTableAppearanceRecord{}}
	}
	ret := &MarkdownTableAppearanceDocument{}
	if err = gulu.JSON.UnmarshalJSON(data, ret); err != nil || ret.Tables == nil {
		return &MarkdownTableAppearanceDocument{Revision: document.Revision, Tables: map[string]*MarkdownTableAppearanceRecord{}}
	}
	return ret
}

func GetMarkdownTableAppearance(documentKey string) (*MarkdownTableAppearanceDocument, error) {
	if err := validateMarkdownTableAppearanceDocumentKey(documentKey); err != nil {
		return nil, err
	}
	markdownTableAppearanceLock.Lock()
	defer markdownTableAppearanceLock.Unlock()
	store := loadMarkdownTableAppearanceStore()
	return cloneMarkdownTableAppearanceDocument(store.Documents[documentKey]), nil
}

func PatchMarkdownTableAppearance(
	documentKey, tableID string,
	patch MarkdownTableAppearancePatch,
) (*MarkdownTableAppearancePatchResult, error) {
	if err := validateMarkdownTableAppearanceDocumentKey(documentKey); err != nil {
		return nil, err
	}
	if err := validateMarkdownTableAppearanceTableID(tableID); err != nil {
		return nil, err
	}
	if err := validateMarkdownTableAppearancePatch(patch); err != nil {
		return nil, err
	}

	markdownTableAppearanceLock.Lock()
	defer markdownTableAppearanceLock.Unlock()
	store := loadMarkdownTableAppearanceStore()
	document := store.Documents[documentKey]
	if document == nil {
		document = &MarkdownTableAppearanceDocument{Tables: map[string]*MarkdownTableAppearanceRecord{}}
		store.Documents[documentKey] = document
	}
	if document.Tables == nil {
		document.Tables = map[string]*MarkdownTableAppearanceRecord{}
	}
	record := document.Tables[tableID]
	if record == nil {
		record = &MarkdownTableAppearanceRecord{TableID: tableID, Attributes: MarkdownTableAppearanceAttributes{WidthMode: "auto"}}
		document.Tables[tableID] = record
	}
	if patch.ContentFingerprint != nil {
		record.ContentFingerprint = *patch.ContentFingerprint
	}
	if patch.ContextFingerprint != nil {
		record.ContextFingerprint = *patch.ContextFingerprint
	}
	if patch.ColumnCount != nil {
		record.Structure.ColumnCount = *patch.ColumnCount
	}
	if patch.HeaderFingerprint != nil {
		record.Structure.HeaderFingerprint = *patch.HeaderFingerprint
	}
	if patch.OrdinalHint != nil {
		record.OrdinalHint = *patch.OrdinalHint
	}
	if patch.WidthMode != nil {
		record.Attributes.WidthMode = *patch.WidthMode
	}
	if patch.LastMatchedAt != nil {
		record.LastMatchedAt = *patch.LastMatchedAt
	}
	if patch.Deleted != nil {
		if *patch.Deleted {
			record.DeletedAt = time.Now().UnixMilli()
		} else {
			record.DeletedAt = 0
		}
	}
	record.Version++
	record.UpdatedAt = time.Now().UnixMilli()
	document.Revision++
	if err := saveMarkdownTableAppearanceStore(store); err != nil {
		return nil, err
	}
	return &MarkdownTableAppearancePatchResult{
		DocumentRevision: document.Revision,
		Record:           cloneMarkdownTableAppearanceDocument(document).Tables[tableID],
	}, nil
}

func MigrateMarkdownTableAppearanceDocument(fromKey, toKey string) error {
	if fromKey == toKey {
		return nil
	}
	if err := validateMarkdownTableAppearanceDocumentKey(fromKey); err != nil {
		return err
	}
	if err := validateMarkdownTableAppearanceDocumentKey(toKey); err != nil {
		return err
	}
	markdownTableAppearanceLock.Lock()
	defer markdownTableAppearanceLock.Unlock()
	store := loadMarkdownTableAppearanceStore()
	from := store.Documents[fromKey]
	if from == nil {
		return nil
	}
	to := store.Documents[toKey]
	if to == nil {
		store.Documents[toKey] = from
	} else {
		if to.Tables == nil {
			to.Tables = map[string]*MarkdownTableAppearanceRecord{}
		}
		for tableID, record := range from.Tables {
			current := to.Tables[tableID]
			if current == nil || current.UpdatedAt < record.UpdatedAt {
				to.Tables[tableID] = record
			}
		}
		to.Revision++
	}
	delete(store.Documents, fromKey)
	return saveMarkdownTableAppearanceStore(store)
}

func RemoveMarkdownTableAppearanceDocument(documentKey string) error {
	if err := validateMarkdownTableAppearanceDocumentKey(documentKey); err != nil {
		return err
	}
	markdownTableAppearanceLock.Lock()
	defer markdownTableAppearanceLock.Unlock()
	store := loadMarkdownTableAppearanceStore()
	if _, ok := store.Documents[documentKey]; !ok {
		return nil
	}
	delete(store.Documents, documentKey)
	return saveMarkdownTableAppearanceStore(store)
}
