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

package tools

import (
	"errors"
	"fmt"

	"github.com/siyuan-note/siyuan/kernel/model"
)

var MarkdownTool = &Tool{
	Name:        "markdown",
	Description: "Markdown file operations. Actions: create(notebook, parentPath, name, autoName?), get(notebook, path), save(notebook, path, content, revision), rename(notebook, path, name), remove(notebook, path).",
	InputSchema: ToolSchema{
		Type: "object",
		Properties: map[string]Property{
			"action":     {Type: "string", Description: "Operation", Enum: []string{"create", "get", "save", "rename", "remove"}},
			"notebook":   {Type: "string", Description: "Notebook ID"},
			"parentPath": {Type: "string", Description: "Parent path for create"},
			"name":       {Type: "string", Description: "Markdown file name for create or rename"},
			"autoName":   {Type: "boolean", Description: "Generate a numbered name when the create target exists"},
			"path":       {Type: "string", Description: "Markdown file path for get, save, rename, or remove"},
			"content":    {Type: "string", Description: "Markdown content for save"},
			"revision":   {Type: "string", Description: "Latest revision required for save"},
		},
		Required: []string{"action"},
	},
	Handler: markdownHandler,
}

func init() {
	register(MarkdownTool)
}

func markdownHandler(args map[string]any) (CallToolResult, error) {
	action, _ := args["action"].(string)
	switch action {
	case "create":
		return markdownCreate(args)
	case "get":
		return markdownGet(args)
	case "save":
		return markdownSave(args)
	case "rename":
		return markdownRename(args)
	case "remove":
		return markdownRemove(args)
	}
	return CallToolResult{
		Content: []ContentItem{{Type: "text", Text: "unknown action '" + action + "', expected one of: [create, get, save, rename, remove]"}},
		IsError: true,
	}, nil
}

func markdownCreate(args map[string]any) (CallToolResult, error) {
	notebook, err := requiredMarkdownString(args, "notebook", false)
	if err != nil {
		return markdownError("create", err), nil
	}
	parentPath, err := requiredMarkdownString(args, "parentPath", false)
	if err != nil {
		return markdownError("create", err), nil
	}
	name, err := requiredMarkdownString(args, "name", false)
	if err != nil {
		return markdownError("create", err), nil
	}
	autoName, _ := args["autoName"].(bool)
	document, err := model.CreateMarkdown(notebook, parentPath, name, autoName)
	if err != nil {
		return markdownError("create", err), nil
	}
	return markdownDocumentResult("created", document), nil
}

func markdownGet(args map[string]any) (CallToolResult, error) {
	notebook, err := requiredMarkdownString(args, "notebook", false)
	if err != nil {
		return markdownError("get", err), nil
	}
	path, err := requiredMarkdownString(args, "path", false)
	if err != nil {
		return markdownError("get", err), nil
	}
	document, err := model.GetMarkdown(notebook, path)
	if err != nil {
		return markdownError("get", err), nil
	}
	return markdownDocumentResult("loaded", document), nil
}

func markdownSave(args map[string]any) (CallToolResult, error) {
	notebook, err := requiredMarkdownString(args, "notebook", false)
	if err != nil {
		return markdownError("save", err), nil
	}
	path, err := requiredMarkdownString(args, "path", false)
	if err != nil {
		return markdownError("save", err), nil
	}
	content, err := requiredMarkdownString(args, "content", true)
	if err != nil {
		return markdownError("save", err), nil
	}
	revision, err := requiredMarkdownString(args, "revision", false)
	if err != nil {
		return markdownError("save", err), nil
	}
	document, err := model.SaveMarkdown(notebook, path, content, revision)
	if err != nil {
		return markdownError("save", err), nil
	}
	return markdownDocumentResult("saved", document), nil
}

func markdownRename(args map[string]any) (CallToolResult, error) {
	notebook, err := requiredMarkdownString(args, "notebook", false)
	if err != nil {
		return markdownError("rename", err), nil
	}
	path, err := requiredMarkdownString(args, "path", false)
	if err != nil {
		return markdownError("rename", err), nil
	}
	name, err := requiredMarkdownString(args, "name", false)
	if err != nil {
		return markdownError("rename", err), nil
	}
	document, err := model.RenameMarkdown(notebook, path, name)
	if err != nil {
		return markdownError("rename", err), nil
	}
	return markdownDocumentResult("renamed", document), nil
}

func markdownRemove(args map[string]any) (CallToolResult, error) {
	notebook, err := requiredMarkdownString(args, "notebook", false)
	if err != nil {
		return markdownError("remove", err), nil
	}
	path, err := requiredMarkdownString(args, "path", false)
	if err != nil {
		return markdownError("remove", err), nil
	}
	if err = model.RemoveMarkdown(notebook, path); err != nil {
		return markdownError("remove", err), nil
	}
	return CallToolResult{Content: []ContentItem{{Type: "text", Text: "markdown document removed: " + path}}}, nil
}

func requiredMarkdownString(args map[string]any, name string, allowEmpty bool) (string, error) {
	value, ok := args[name].(string)
	if !ok || (!allowEmpty && value == "") {
		return "", errors.New(name + " is required")
	}
	return value, nil
}

func markdownDocumentResult(action string, document *model.MarkdownDocument) CallToolResult {
	return CallToolResult{Content: []ContentItem{{Type: "text", Text: fmt.Sprintf(
		"markdown document %s:\nPath: %s\nName: %s\nRevision: %s\nMtime: %d\nContent:\n%s",
		action, document.Path, document.Name, document.Revision, document.Mtime, document.Content,
	)}}}
}

func markdownError(action string, err error) CallToolResult {
	return CallToolResult{
		Content: []ContentItem{{Type: "text", Text: fmt.Sprintf("%s markdown failed: %s", action, err)}},
		IsError: true,
	}
}
