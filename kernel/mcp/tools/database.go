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
	"fmt"
	"strings"

	"github.com/siyuan-note/siyuan/kernel/av"
	"github.com/siyuan-note/siyuan/kernel/model"
)

var DatabaseTool = &Tool{
	Name:        "database",
	Description: "Read existing databases. Actions: search, get, render, keys, unused.",
	InputSchema: ToolSchema{
		Type: "object",
		Properties: map[string]Property{
			"action":   {Type: "string", Description: "Operation", Enum: []string{"search", "get", "render", "keys", "unused"}},
			"keyword":  {Type: "string", Description: "Search keyword (for search)"},
			"id":       {Type: "string", Description: "Attribute view ID (for get, render, keys)"},
			"viewID":   {Type: "string", Description: "View ID (for render)"},
			"query":    {Type: "string", Description: "Filter query (for render)"},
			"page":     {Type: "number", Description: "Page number (default 1)"},
			"pageSize": {Type: "number", Description: "Results per page (default 50)"},
		},
		Required: []string{"action"},
	},
	Handler: databaseHandler,
}

func init() {
	register(DatabaseTool)
}

func databaseHandler(args map[string]any) (CallToolResult, error) {
	action, _ := args["action"].(string)
	switch action {
	case "search":
		return databaseSearch(args)
	case "get":
		return databaseGet(args)
	case "render":
		return databaseRender(args)
	case "keys":
		return databaseKeys(args)
	case "unused":
		return databaseUnused(args)
	}
	return CallToolResult{
		Content: []ContentItem{{Type: "text", Text: "unknown action '" + action + "', expected one of: [search, get, render, keys, unused]"}},
		IsError: true,
	}, nil
}

func databaseSearch(args map[string]any) (CallToolResult, error) {
	keyword, _ := args["keyword"].(string)
	if keyword == "" {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "keyword is required"}}, IsError: true}, nil
	}

	results := model.SearchAttributeView(keyword, nil, "", "")
	if len(results) == 0 {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "no attribute views found"}}}, nil
	}

	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("Attribute views matching '%s' (%d):\n\n", keyword, len(results)))
	for _, r := range results {
		sb.WriteString(fmt.Sprintf("- %s (id: %s, hPath: %s)\n", r.AvName, r.AvID, r.HPath))
	}
	return CallToolResult{Content: []ContentItem{{Type: "text", Text: sb.String()}}}, nil
}

func databaseGet(args map[string]any) (CallToolResult, error) {
	id, _ := args["id"].(string)
	if id == "" {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "id is required"}}, IsError: true}, nil
	}

	attrView := model.GetAttributeView(id)
	if attrView == nil {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "attribute view not found: " + id}}, IsError: true}, nil
	}

	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("Attribute View: %s\n\n", id))
	sb.WriteString(fmt.Sprintf("Name: %s\n", attrView.Name))
	sb.WriteString(fmt.Sprintf("Keys (%d):\n", len(attrView.KeyValues)))
	for _, kv := range attrView.KeyValues {
		sb.WriteString(fmt.Sprintf("- %s (%s): %s\n", kv.Key.Name, kv.Key.Type, kv.Key.Icon))
	}
	sb.WriteString(fmt.Sprintf("\nViews (%d):\n", len(attrView.Views)))
	for _, v := range attrView.Views {
		sb.WriteString(fmt.Sprintf("- %s (%s, pageSize: %d)\n", v.Name, v.LayoutType, v.PageSize))
	}
	return CallToolResult{Content: []ContentItem{{Type: "text", Text: sb.String()}}}, nil
}

func databaseRender(args map[string]any) (CallToolResult, error) {
	id, _ := args["id"].(string)
	if id == "" {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "id is required"}}, IsError: true}, nil
	}
	viewID, _ := args["viewID"].(string)
	query, _ := args["query"].(string)
	page := 1
	if v, ok := args["page"].(float64); ok {
		page = int(v)
	}
	pageSize := 50
	if v, ok := args["pageSize"].(float64); ok {
		pageSize = int(v)
	}

	viewable, _, err := model.RenderAttributeView("", id, viewID, query, page, pageSize, nil, false, false)
	if err != nil {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "render failed: " + err.Error()}}, IsError: true}, nil
	}

	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("Attribute View Render (page %d):\n\n", page))
	if viewable != nil {
		if table, ok := viewable.(*av.Table); ok {
			for _, row := range table.Rows {
				vals := make([]string, 0, len(row.Cells))
				for _, cell := range row.Cells {
					vals = append(vals, fmt.Sprintf("%v", cell.Value))
				}
				sb.WriteString(strings.Join(vals, " | ") + "\n")
			}
		}
	}
	return CallToolResult{Content: []ContentItem{{Type: "text", Text: sb.String()}}}, nil
}

func databaseKeys(args map[string]any) (CallToolResult, error) {
	id, _ := args["id"].(string)
	if id == "" {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "id is required"}}, IsError: true}, nil
	}
	attrView := model.GetAttributeView(id)
	if attrView == nil {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "attribute view not found: " + id}}, IsError: true}, nil
	}
	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("Keys for %s (%s):\n\n", id, attrView.Name))
	for _, kv := range attrView.KeyValues {
		sb.WriteString(fmt.Sprintf("- %s (%s) [%s]\n", kv.Key.ID, kv.Key.Name, kv.Key.Type))
	}
	return CallToolResult{Content: []ContentItem{{Type: "text", Text: sb.String()}}}, nil
}

func databaseUnused(args map[string]any) (CallToolResult, error) {
	items := model.UnusedAttributeViews(true)
	if len(items) == 0 {
		return CallToolResult{Content: []ContentItem{{Type: "text", Text: "no unused databases found"}}}, nil
	}
	var sb strings.Builder
	sb.WriteString(fmt.Sprintf("Unused databases (%d):\n\n", len(items)))
	for _, item := range items {
		sb.WriteString(fmt.Sprintf("- %s (%s)\n", item.Item, item.Name))
	}
	return CallToolResult{Content: []ContentItem{{Type: "text", Text: sb.String()}}}, nil
}
