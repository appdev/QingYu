// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org

package model

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"regexp"
	"strings"

	"github.com/BurntSushi/toml"
	"github.com/google/uuid"
	"gopkg.in/yaml.v3"
)

const MarkdownDocumentIDKey = "qingyu-document-id"

const markdownDocumentIDPlaceholder = "00000000-0000-7000-8000-000000000000"

type MarkdownDocumentIDInspection struct {
	ID    string
	State string
	Kind  string

	frontmatter markdownDocumentIDFrontmatter
}

type MarkdownDocumentIDMutation struct {
	ID      string
	Data    []byte
	Changed bool
	State   string
}

type markdownDocumentIDFrontmatter struct {
	kind         string
	bomLen       int
	contentStart int
	contentEnd   int
	blockEnd     int
	propertyFrom int
	propertyTo   int
	valueFrom    int
	valueTo      int
	found        bool
	valid        bool
}

var markdownDocumentIDLinePattern = regexp.MustCompile(`(?m)^` + regexp.QuoteMeta(MarkdownDocumentIDKey) + `[ \t]*[:=]`)

func InspectMarkdownDocumentID(data []byte) MarkdownDocumentIDInspection {
	frontmatter := inspectMarkdownDocumentIDFrontmatter(data)
	ret := MarkdownDocumentIDInspection{State: "missing", Kind: frontmatter.kind, frontmatter: frontmatter}
	if !frontmatter.valid {
		ret.State = "malformed"
		return ret
	}
	if !frontmatter.found {
		return ret
	}
	value := strings.TrimSpace(string(data[frontmatter.valueFrom:frontmatter.valueTo]))
	if frontmatter.kind == "yaml" || frontmatter.kind == "toml" {
		var decoded string
		if err := decodeFrontmatterString(frontmatter.kind, value, &decoded); err == nil {
			value = decoded
		}
	} else if frontmatter.kind == "json" {
		var decoded string
		if err := json.Unmarshal([]byte(value), &decoded); err == nil {
			value = decoded
		}
	}
	if !validMarkdownDocumentID(value) {
		ret.State = "invalid-value"
		return ret
	}
	ret.ID = strings.ToLower(value)
	ret.State = "valid"
	return ret
}

func EnsureMarkdownDocumentID(data []byte, forceNew bool) (MarkdownDocumentIDMutation, error) {
	inspection := InspectMarkdownDocumentID(data)
	if inspection.State == "valid" && !forceNew {
		return MarkdownDocumentIDMutation{ID: inspection.ID, Data: append([]byte(nil), data...), State: inspection.State}, nil
	}
	id, err := newMarkdownDocumentID()
	if err != nil {
		return MarkdownDocumentIDMutation{}, err
	}
	frontmatter := inspection.frontmatter
	if inspection.State == "malformed" || frontmatter.kind == "none" {
		newline := markdownDocumentNewline(data)
		prefix := []byte("---" + newline + MarkdownDocumentIDKey + ": " + id + newline + "---" + newline + newline)
		ret := make([]byte, 0, len(data)+len(prefix))
		ret = append(ret, data[:frontmatter.bomLen]...)
		ret = append(ret, prefix...)
		ret = append(ret, data[frontmatter.bomLen:]...)
		return MarkdownDocumentIDMutation{ID: id, Data: ret, Changed: true, State: inspection.State}, nil
	}

	var ret []byte
	if frontmatter.found {
		replacement := id
		if frontmatter.kind == "json" {
			replacement = `"` + id + `"`
		}
		ret = replaceMarkdownDocumentIDRange(data, frontmatter.valueFrom, frontmatter.valueTo, []byte(replacement))
	} else {
		ret, err = insertMarkdownDocumentID(data, frontmatter, id)
		if err != nil {
			return MarkdownDocumentIDMutation{}, err
		}
	}
	return MarkdownDocumentIDMutation{ID: id, Data: ret, Changed: true, State: inspection.State}, nil
}

func MarkdownPreviewContentRevision(data []byte) (string, error) {
	inspection := InspectMarkdownDocumentID(data)
	normalized := data
	if inspection.frontmatter.valid && inspection.frontmatter.found {
		replacement := markdownDocumentIDPlaceholder
		if inspection.frontmatter.kind == "json" {
			replacement = `"` + replacement + `"`
		}
		normalized = replaceMarkdownDocumentIDRange(data, inspection.frontmatter.valueFrom,
			inspection.frontmatter.valueTo, []byte(replacement))
	}
	sum := sha256.Sum256(normalized)
	return hex.EncodeToString(sum[:]), nil
}

func DocumentCardRatio(documentID string) float64 {
	sum := sha256.Sum256([]byte(documentID))
	value := binary.BigEndian.Uint64(sum[:8])
	normalized := float64(value) / float64(^uint64(0))
	return 1.05 + normalized*0.35
}

func newMarkdownDocumentID() (string, error) {
	id, err := uuid.NewV7()
	if err != nil {
		return "", err
	}
	return strings.ToLower(id.String()), nil
}

func validMarkdownDocumentID(value string) bool {
	id, err := uuid.Parse(strings.TrimSpace(value))
	return err == nil && id.Version() == 7 && strings.ToLower(id.String()) == strings.ToLower(strings.TrimSpace(value))
}

func inspectMarkdownDocumentIDFrontmatter(data []byte) markdownDocumentIDFrontmatter {
	bomLen := 0
	if bytes.HasPrefix(data, []byte{0xef, 0xbb, 0xbf}) {
		bomLen = 3
	}
	ret := markdownDocumentIDFrontmatter{kind: "none", bomLen: bomLen, valid: true}
	source := data[bomLen:]
	if bytes.HasPrefix(source, []byte("---\n")) || bytes.HasPrefix(source, []byte("---\r\n")) {
		return inspectFencedMarkdownDocumentID(data, bomLen, "---", "yaml")
	}
	if bytes.HasPrefix(source, []byte("+++\n")) || bytes.HasPrefix(source, []byte("+++\r\n")) {
		return inspectFencedMarkdownDocumentID(data, bomLen, "+++", "toml")
	}
	if len(source) > 0 && source[0] == '{' {
		return inspectJSONMarkdownDocumentID(data, bomLen)
	}
	if bytes.HasPrefix(source, []byte("---")) || bytes.HasPrefix(source, []byte("+++")) {
		ret.valid = false
		ret.kind = "none"
	}
	return ret
}

func inspectFencedMarkdownDocumentID(data []byte, bomLen int, delimiter, kind string) markdownDocumentIDFrontmatter {
	ret := markdownDocumentIDFrontmatter{kind: kind, bomLen: bomLen}
	openingEnd := bytes.IndexByte(data[bomLen:], '\n')
	if openingEnd < 0 {
		return ret
	}
	ret.contentStart = bomLen + openingEnd + 1
	lineStart := ret.contentStart
	for lineStart <= len(data) {
		lineEnd := bytes.IndexByte(data[lineStart:], '\n')
		if lineEnd < 0 {
			lineEnd = len(data)
		} else {
			lineEnd += lineStart
		}
		trimmed := strings.TrimSpace(string(data[lineStart:lineEnd]))
		if trimmed == delimiter {
			ret.contentEnd = lineStart
			ret.blockEnd = lineEnd
			if lineEnd < len(data) {
				ret.blockEnd++
			}
			break
		}
		if lineEnd == len(data) {
			return ret
		}
		lineStart = lineEnd + 1
	}
	if ret.contentEnd == 0 {
		return ret
	}
	content := data[ret.contentStart:ret.contentEnd]
	metadata := map[string]any{}
	var err error
	if kind == "yaml" {
		err = yaml.Unmarshal(content, &metadata)
	} else {
		err = toml.Unmarshal(content, &metadata)
	}
	if err != nil {
		return ret
	}
	ret.valid = true
	propertyFrom, propertyTo, valueFrom, valueTo, count := findFencedDocumentIDProperty(data, ret.contentStart, ret.contentEnd, kind)
	if count > 1 {
		ret.valid = false
		return ret
	}
	if count == 1 {
		ret.found = true
		ret.propertyFrom, ret.propertyTo = propertyFrom, propertyTo
		ret.valueFrom, ret.valueTo = valueFrom, valueTo
	}
	return ret
}

func findFencedDocumentIDProperty(data []byte, from, to int, kind string) (propertyFrom, propertyTo, valueFrom, valueTo, count int) {
	for cursor := from; cursor < to; {
		lineEnd := bytes.IndexByte(data[cursor:to], '\n')
		if lineEnd < 0 {
			lineEnd = to
		} else {
			lineEnd += cursor
		}
		line := data[cursor:lineEnd]
		match := markdownDocumentIDLinePattern.FindIndex(line)
		if match != nil && match[0] == 0 {
			count++
			if count == 1 {
				propertyFrom = cursor
				propertyTo = lineEnd
				separator := byte(':')
				if kind == "toml" {
					separator = '='
				}
				separatorAt := bytes.IndexByte(line, separator)
				valueFrom = cursor + separatorAt + 1
				for valueFrom < lineEnd && (data[valueFrom] == ' ' || data[valueFrom] == '\t') {
					valueFrom++
				}
				valueTo = cursor + frontmatterInlineValueEnd(line[separatorAt+1:]) + separatorAt + 1
				for valueTo > valueFrom && (data[valueTo-1] == ' ' || data[valueTo-1] == '\t') {
					valueTo--
				}
			}
		}
		if lineEnd == to {
			break
		}
		cursor = lineEnd + 1
	}
	return
}

func frontmatterInlineValueEnd(value []byte) int {
	quote := byte(0)
	escaped := false
	for i, char := range value {
		if escaped {
			escaped = false
			continue
		}
		if char == '\\' && quote == '"' {
			escaped = true
			continue
		}
		if quote != 0 {
			if char == quote {
				quote = 0
			}
			continue
		}
		if char == '\'' || char == '"' {
			quote = char
			continue
		}
		if char == '#' {
			return i
		}
	}
	return len(value)
}

func inspectJSONMarkdownDocumentID(data []byte, bomLen int) markdownDocumentIDFrontmatter {
	ret := markdownDocumentIDFrontmatter{kind: "json", bomLen: bomLen}
	decoder := json.NewDecoder(bytes.NewReader(data[bomLen:]))
	var metadata map[string]any
	if err := decoder.Decode(&metadata); err != nil {
		return ret
	}
	ret.contentStart = bomLen
	ret.contentEnd = bomLen + int(decoder.InputOffset())
	ret.blockEnd = ret.contentEnd
	propertyFrom, propertyTo, valueFrom, valueTo, count, ok := findJSONDocumentIDProperty(data, bomLen, ret.contentEnd)
	if !ok || count > 1 {
		return ret
	}
	ret.valid = true
	if count == 1 {
		ret.found = true
		ret.propertyFrom, ret.propertyTo = propertyFrom, propertyTo
		ret.valueFrom, ret.valueTo = valueFrom, valueTo
	}
	return ret
}

func findJSONDocumentIDProperty(data []byte, from, to int) (propertyFrom, propertyTo, valueFrom, valueTo, count int, ok bool) {
	cursor := from
	for cursor < to && isJSONWhitespace(data[cursor]) {
		cursor++
	}
	if cursor >= to || data[cursor] != '{' {
		return 0, 0, 0, 0, 0, false
	}
	cursor++
	for {
		for cursor < to && isJSONWhitespace(data[cursor]) {
			cursor++
		}
		if cursor < to && data[cursor] == '}' {
			return propertyFrom, propertyTo, valueFrom, valueTo, count, true
		}
		memberStart := cursor
		keyEnd, key, valid := scanJSONString(data, cursor, to)
		if !valid {
			return 0, 0, 0, 0, 0, false
		}
		cursor = keyEnd
		for cursor < to && isJSONWhitespace(data[cursor]) {
			cursor++
		}
		if cursor >= to || data[cursor] != ':' {
			return 0, 0, 0, 0, 0, false
		}
		cursor++
		for cursor < to && isJSONWhitespace(data[cursor]) {
			cursor++
		}
		currentValueFrom := cursor
		currentValueTo, valid := scanJSONValue(data, cursor, to)
		if !valid {
			return 0, 0, 0, 0, 0, false
		}
		cursor = currentValueTo
		memberEnd := cursor
		if key == MarkdownDocumentIDKey {
			count++
			if count == 1 {
				propertyFrom, propertyTo = memberStart, memberEnd
				valueFrom, valueTo = currentValueFrom, currentValueTo
			}
		}
		for cursor < to && isJSONWhitespace(data[cursor]) {
			cursor++
		}
		if cursor < to && data[cursor] == ',' {
			cursor++
			continue
		}
		if cursor < to && data[cursor] == '}' {
			return propertyFrom, propertyTo, valueFrom, valueTo, count, true
		}
		return 0, 0, 0, 0, 0, false
	}
}

func scanJSONString(data []byte, from, to int) (end int, value string, ok bool) {
	if from >= to || data[from] != '"' {
		return 0, "", false
	}
	escaped := false
	for cursor := from + 1; cursor < to; cursor++ {
		if escaped {
			escaped = false
			continue
		}
		if data[cursor] == '\\' {
			escaped = true
			continue
		}
		if data[cursor] == '"' {
			if err := json.Unmarshal(data[from:cursor+1], &value); err != nil {
				return 0, "", false
			}
			return cursor + 1, value, true
		}
	}
	return 0, "", false
}

func scanJSONValue(data []byte, from, to int) (int, bool) {
	if from >= to {
		return 0, false
	}
	if data[from] == '"' {
		end, _, ok := scanJSONString(data, from, to)
		return end, ok
	}
	depth := 0
	inString := false
	escaped := false
	for cursor := from; cursor < to; cursor++ {
		char := data[cursor]
		if inString {
			if escaped {
				escaped = false
			} else if char == '\\' {
				escaped = true
			} else if char == '"' {
				inString = false
			}
			continue
		}
		switch char {
		case '"':
			inString = true
		case '{', '[':
			depth++
		case '}', ']':
			if depth == 0 {
				return trimJSONValueEnd(data, from, cursor), true
			}
			depth--
		case ',':
			if depth == 0 {
				return trimJSONValueEnd(data, from, cursor), true
			}
		}
	}
	return 0, false
}

func trimJSONValueEnd(data []byte, from, to int) int {
	for to > from && isJSONWhitespace(data[to-1]) {
		to--
	}
	return to
}

func isJSONWhitespace(char byte) bool {
	return char == ' ' || char == '\t' || char == '\r' || char == '\n'
}

func decodeFrontmatterString(kind, value string, decoded *string) error {
	if value == "" {
		return errors.New("empty value")
	}
	if kind == "yaml" {
		return yaml.Unmarshal([]byte(value), decoded)
	}
	container := struct {
		Value string `toml:"value"`
	}{}
	if _, err := toml.Decode("value = "+value, &container); err != nil {
		return err
	}
	*decoded = container.Value
	return nil
}

func insertMarkdownDocumentID(data []byte, frontmatter markdownDocumentIDFrontmatter, id string) ([]byte, error) {
	newline := markdownDocumentNewline(data)
	var insertion string
	switch frontmatter.kind {
	case "yaml":
		insertion = MarkdownDocumentIDKey + ": " + id + newline
	case "toml":
		insertion = MarkdownDocumentIDKey + ` = "` + id + `"` + newline
	case "json":
		cursor := frontmatter.contentEnd - 1
		for cursor > frontmatter.contentStart && isJSONWhitespace(data[cursor]) {
			cursor--
		}
		if cursor <= frontmatter.contentStart || data[cursor] != '}' {
			return nil, errors.New("invalid JSON Front Matter")
		}
		prefix := data[frontmatter.contentStart+1 : cursor]
		separator := ""
		if strings.TrimSpace(string(prefix)) != "" {
			separator = ","
		}
		insertion = separator + newline + `"` + MarkdownDocumentIDKey + `": "` + id + `"` + newline
		return replaceMarkdownDocumentIDRange(data, cursor, cursor, []byte(insertion)), nil
	default:
		return nil, errors.New("unsupported Front Matter kind")
	}
	return replaceMarkdownDocumentIDRange(data, frontmatter.contentEnd, frontmatter.contentEnd, []byte(insertion)), nil
}

func replaceMarkdownDocumentIDRange(data []byte, from, to int, replacement []byte) []byte {
	ret := make([]byte, 0, len(data)-(to-from)+len(replacement))
	ret = append(ret, data[:from]...)
	ret = append(ret, replacement...)
	ret = append(ret, data[to:]...)
	return ret
}

func markdownDocumentNewline(data []byte) string {
	if bytes.Contains(data, []byte("\r\n")) {
		return "\r\n"
	}
	return "\n"
}
