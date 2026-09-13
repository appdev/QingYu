// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"unicode/utf16"
	"unicode/utf8"
)

const markdownAnnotationsSuffix = ".annotations.json"
const markdownAnnotationsMaxBytes = 16 * 1024 * 1024

var ErrInvalidMarkdownAnnotations = errors.New("invalid Markdown annotations")
var annotationIDPattern = regexp.MustCompile(`^[\w-]{1,128}$`)
var annotationHashPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)

type MarkdownAnnotation struct {
	ID        string `json:"id"`
	From      int    `json:"from"`
	To        int    `json:"to"`
	Quote     string `json:"quote"`
	Prefix    string `json:"prefix"`
	Suffix    string `json:"suffix"`
	Status    string `json:"status"`
	Note      string `json:"note"`
	CreatedAt int64  `json:"createdAt"`
	UpdatedAt int64  `json:"updatedAt"`
}

type MarkdownAnnotations struct {
	sourceIdentity *markdownFileIdentity
	ETag           string               `json:"etag,omitempty"`
	SchemaVersion  int                  `json:"schemaVersion"`
	DocumentID     string               `json:"documentId"`
	Revision       int64                `json:"revision"`
	ContentHash    string               `json:"contentHash"`
	Records        []MarkdownAnnotation `json:"records"`
}

func markdownAnnotationHash(content string) string {
	content = strings.TrimPrefix(content, "\ufeff")
	content = strings.ReplaceAll(strings.ReplaceAll(content, "\r\n", "\n"), "\r", "\n")
	hash := sha256.Sum256([]byte(content))
	return hex.EncodeToString(hash[:])
}

// 文档身份和标题变更只影响局部源码，复制时保留未变动区域的批注。
func rebaseMarkdownAnnotations(value *MarkdownAnnotations, before, after string) {
	normalize := func(s string) []uint16 {
		return utf16.Encode([]rune(strings.ReplaceAll(strings.ReplaceAll(strings.TrimPrefix(s, "\ufeff"), "\r\n", "\n"), "\r", "\n")))
	}
	old, next := normalize(before), normalize(after)
	prefix, suffix := 0, 0
	for prefix < len(old) && prefix < len(next) && old[prefix] == next[prefix] {
		prefix++
	}
	for suffix < len(old)-prefix && suffix < len(next)-prefix && old[len(old)-1-suffix] == next[len(next)-1-suffix] {
		suffix++
	}
	for index := range value.Records {
		record := &value.Records[index]
		if record.Status != "attached" {
			continue
		}
		if record.From >= len(old)-suffix {
			record.From += len(next) - len(old)
			record.To += len(next) - len(old)
		} else if record.To > prefix {
			record.Status = "orphaned"
			continue
		}
		if record.From < 0 || record.To > len(next) || record.From >= record.To || string(utf16.Decode(next[record.From:record.To])) != record.Quote {
			record.Status = "orphaned"
			continue
		}
		prefixRunes := []rune(string(utf16.Decode(next[:record.From])))
		if len(prefixRunes) > 64 {
			prefixRunes = prefixRunes[len(prefixRunes)-64:]
		}
		suffixRunes := []rune(string(utf16.Decode(next[record.To:])))
		if len(suffixRunes) > 64 {
			suffixRunes = suffixRunes[:64]
		}
		record.Prefix, record.Suffix = string(prefixRunes), string(suffixRunes)
	}
	value.ContentHash = markdownAnnotationHash(after)
}

func validateMarkdownAnnotations(value *MarkdownAnnotations) error {
	if value == nil || value.SchemaVersion != 1 || !annotationIDPattern.MatchString(value.DocumentID) ||
		value.Revision < 0 || value.Revision >= 9007199254740991 ||
		(value.ContentHash != "" && !annotationHashPattern.MatchString(value.ContentHash)) ||
		value.Records == nil || len(value.Records) > 10000 {
		return ErrInvalidMarkdownAnnotations
	}
	ids := map[string]bool{}
	for _, record := range value.Records {
		if !annotationIDPattern.MatchString(record.ID) || ids[record.ID] || record.From < 0 || record.To < record.From ||
			record.Quote == "" || (record.Status != "attached" && record.Status != "orphaned") ||
			len(utf16.Encode([]rune(record.Note))) > 65536 || utf8.RuneCountInString(record.Prefix) > 64 ||
			utf8.RuneCountInString(record.Suffix) > 64 || record.CreatedAt < 0 || record.UpdatedAt < 0 ||
			record.CreatedAt > 9007199254740991 || record.UpdatedAt > 9007199254740991 {
			return ErrInvalidMarkdownAnnotations
		}
		ids[record.ID] = true
	}
	return nil
}

func readMarkdownAnnotations(documentPath string) (*MarkdownAnnotations, error) {
	file, root, err := openMarkdownFileRead(documentPath + markdownAnnotationsSuffix)
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	defer file.Close()
	defer root.Close()
	before, err := file.Stat()
	if err != nil {
		return nil, err
	}
	data, err := io.ReadAll(io.LimitReader(file, markdownAnnotationsMaxBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > markdownAnnotationsMaxBytes {
		return nil, ErrInvalidMarkdownAnnotations
	}
	var value MarkdownAnnotations
	if err = json.Unmarshal(data, &value); err != nil {
		return nil, errors.Join(ErrInvalidMarkdownAnnotations, err)
	}
	hash := sha256.Sum256(data)
	value.ETag = hex.EncodeToString(hash[:])
	after, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if before.Size() != after.Size() || before.ModTime() != after.ModTime() {
		return nil, ErrMarkdownConflict
	}
	value.sourceIdentity = &markdownFileIdentity{Size: after.Size(), Mtime: after.ModTime().UnixNano(),
		Mode: after.Mode().Perm(), Revision: markdownRevision(data), SystemID: markdownPlatformFileIdentity(file, after)}
	return &value, validateMarkdownAnnotations(&value)
}

// 配套文件的暂存身份在正文提交前记录；恢复时仅安装或移除身份一致的文件。
type markdownAnnotationChange struct {
	Source  string                `json:"source,omitempty"`
	Target  string                `json:"target,omitempty"`
	Staging string                `json:"staging,omitempty"`
	Old     *markdownFileIdentity `json:"old,omitempty"`
	New     markdownFileIdentity  `json:"new"`
	Backup  string                `json:"backup,omitempty"`
}

func stageMarkdownAnnotations(tx *markdownTransaction, source, target string, value *MarkdownAnnotations) error {
	if value == nil {
		if source != target {
			if _, err := os.Lstat(target); err == nil {
				return os.ErrExist
			} else if !os.IsNotExist(err) {
				return err
			}
		}
		return nil
	}
	if err := validateMarkdownAnnotations(value); err != nil {
		return err
	}
	change := &markdownAnnotationChange{Source: source, Target: target,
		Staging: filepath.Join(filepath.Dir(target), "."+filepath.Base(target)+".pending-"+tx.ID),
		Backup:  filepath.Join(tx.dirPath, "annotations-old.json")}
	if source != "" {
		identity, err := markdownIdentity(source)
		if err != nil && !os.IsNotExist(err) {
			return err
		}
		if err == nil {
			change.Old = &identity
		}
		if value.sourceIdentity != nil {
			if change.Old == nil || *change.Old != *value.sourceIdentity {
				return ErrMarkdownConflict
			}
			change.Old = value.sourceIdentity
		}
	}
	if source != target {
		if _, err := os.Lstat(target); err == nil {
			return os.ErrExist
		} else if !os.IsNotExist(err) {
			return err
		}
	}
	persisted := *value
	persisted.ETag = ""
	data, err := json.Marshal(&persisted)
	if err != nil || len(data) > markdownAnnotationsMaxBytes {
		return errors.Join(ErrInvalidMarkdownAnnotations, err)
	}
	file, err := openMarkdownFileNoReplace(change.Staging, 0600)
	if err != nil {
		return err
	}
	_, writeErr := file.Write(data)
	err = errors.Join(writeErr, file.Sync(), file.Close())
	if err != nil {
		return err
	}
	change.New, err = markdownIdentity(change.Staging)
	if err != nil {
		return err
	}
	tx.Annotations = change
	return errors.Join(syncMarkdownParent(change.Staging), writeMarkdownTransaction(tx))
}

func commitMarkdownAnnotations(tx *markdownTransaction) error {
	a := tx.Annotations
	if a == nil {
		return nil
	}
	if a.Target == "" {
		if a.Old == nil {
			return nil
		}
		err := removeMarkdownFileWithIdentity(a.Source, *a.Old)
		if os.IsNotExist(err) {
			return nil
		}
		return err
	}
	installed, targetErr := sameMarkdownIdentity(a.Target, a.New)
	if !installed {
		if a.Source == a.Target && a.Old != nil {
			backedUp, backupErr := sameMarkdownIdentity(a.Backup, *a.Old)
			if !backedUp {
				if !os.IsNotExist(backupErr) {
					return errors.Join(ErrMarkdownConflict, backupErr)
				}
				matches, err := sameMarkdownIdentity(a.Source, *a.Old)
				if err != nil || !matches {
					return errors.Join(ErrMarkdownConflict, err)
				}
				if err = moveMarkdownFileNoReplace(a.Source, a.Backup, false); err != nil {
					return err
				}
			}
		} else if !os.IsNotExist(targetErr) {
			return errors.Join(ErrMarkdownConflict, targetErr)
		}
		matches, err := sameMarkdownIdentity(a.Staging, a.New)
		if err != nil || !matches {
			return errors.Join(ErrMarkdownConflict, err)
		}
		if err = linkMarkdownFileNoReplace(a.Staging, a.Target); err != nil {
			return err
		}
		if err = markdownTransactionCrashHook("annotations", "installed"); err != nil {
			return err
		}
	}
	if a.Source != "" && a.Source != a.Target && a.Old != nil {
		if err := removeMarkdownFileWithIdentity(a.Source, *a.Old); err != nil && !os.IsNotExist(err) {
			return err
		}
	}
	return nil
}

func cleanupMarkdownAnnotations(tx *markdownTransaction) error {
	if tx.Annotations == nil || tx.Annotations.Staging == "" {
		return nil
	}
	err := removeMarkdownFileWithIdentity(tx.Annotations.Staging, tx.Annotations.New)
	if os.IsNotExist(err) {
		return nil
	}
	return err
}

func prepareMarkdownAnnotationSave(tx *markdownTransaction, value *MarkdownAnnotations, content string) error {
	if err := validateMarkdownAnnotations(value); err != nil {
		return err
	}
	text := utf16.Encode([]rune(strings.ReplaceAll(strings.ReplaceAll(strings.TrimPrefix(content, "\ufeff"), "\r\n", "\n"), "\r", "\n")))
	for _, record := range value.Records {
		if record.Status == "attached" && (record.To > len(text) || record.From >= record.To ||
			string(utf16.Decode(text[record.From:record.To])) != record.Quote) {
			return ErrInvalidMarkdownAnnotations
		}
	}
	current, err := readMarkdownAnnotations(tx.Source)
	if err != nil {
		return err
	}
	if current == nil && value.Revision != 0 || current != nil &&
		(current.Revision != value.Revision || current.DocumentID != value.DocumentID || current.ETag != value.ETag) {
		return ErrMarkdownConflict
	}
	next := *value
	if current != nil {
		next.sourceIdentity = current.sourceIdentity
	}
	next.Revision++
	next.ContentHash = markdownAnnotationHash(content)
	return stageMarkdownAnnotations(tx, tx.Source+markdownAnnotationsSuffix, tx.Source+markdownAnnotationsSuffix, &next)
}

func prepareMarkdownAnnotationMove(tx *markdownTransaction) error {
	value, err := readMarkdownAnnotations(tx.Source)
	if err != nil {
		return err
	}
	if tx.Source == tx.Destination {
		return fmt.Errorf("%w: same annotation destination", ErrMarkdownConflict)
	}
	return stageMarkdownAnnotations(tx, tx.Source+markdownAnnotationsSuffix, tx.Destination+markdownAnnotationsSuffix, value)
}
