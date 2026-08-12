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

package mcp

import "github.com/siyuan-note/siyuan/kernel/mcp/tools"

// readOnlyActions 是 MCP 只读模式的服务端白名单，未声明的原生工具或动作一律拒绝。
var readOnlyActions = map[string]map[string]bool{
	"asset":     actionSet("unused", "stat"),
	"attr":      actionSet("get", "batch-get"),
	"block":     actionSet("get", "get_kramdown", "get_children", "tree_stat", "dom", "breadcrumb", "batch_get", "batch_kramdown"),
	"bookmark":  actionSet("list", "labels"),
	"database":  actionSet("search", "get", "render", "keys", "unused"),
	"document":  actionSet("get", "list", "search_docs", "info"),
	"export":    actionSet("md", "html", "preview"),
	"file":      actionSet("list", "read", "grep", "find", "stat"),
	"history":   actionSet("list", "search", "get"),
	"notebook":  actionSet("list"),
	"outline":   actionSet("get"),
	"ref":       actionSet("backlinks", "mentions"),
	"repo":      actionSet("list", "diff", "search", "file_get", "file_open"),
	"search":    actionSet("fulltext", "asset", "getasset"),
	"sql":       actionSet("", "query"),
	"sync":      actionSet("status"),
	"system":    actionSet("version", "current_time", "workspace"),
	"tag":       actionSet("list"),
	"template":  actionSet("search", "get", "render"),
	"workspace": actionSet("list", "info"),
}

func actionSet(actions ...string) map[string]bool {
	ret := make(map[string]bool, len(actions))
	for _, action := range actions {
		ret[action] = true
	}
	return ret
}

func isToolCallAllowed(tool *tools.Tool, args map[string]any, readOnly bool) bool {
	if tool == nil {
		return false
	}
	if !readOnly {
		return true
	}
	if tool.Source == "plugin" {
		return tool.ReadOnlyHint
	}
	actions := readOnlyActions[tool.Name]
	if len(actions) == 0 {
		return false
	}
	action, _ := args["action"].(string)
	return actions[action]
}

func allowedTools(readOnly bool) []*tools.Tool {
	toolList := tools.GetAllTools()
	if !readOnly {
		return toolList
	}
	ret := make([]*tools.Tool, 0, len(toolList))
	for _, tool := range toolList {
		if tool.Source == "plugin" {
			if tool.ReadOnlyHint {
				ret = append(ret, tool)
			}
			continue
		}
		actions := readOnlyActions[tool.Name]
		if len(actions) == 0 {
			continue
		}
		clone := *tool
		clone.InputSchema = tool.InputSchema
		clone.InputSchema.Properties = make(map[string]tools.Property, len(tool.InputSchema.Properties))
		for name, property := range tool.InputSchema.Properties {
			if name == "action" {
				filtered := property
				filtered.Enum = make([]string, 0, len(property.Enum))
				for _, action := range property.Enum {
					if actions[action] {
						filtered.Enum = append(filtered.Enum, action)
					}
				}
				clone.InputSchema.Properties[name] = filtered
				continue
			}
			clone.InputSchema.Properties[name] = property
		}
		ret = append(ret, &clone)
	}
	return ret
}
