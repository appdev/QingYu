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

import "testing"

func TestServerInstructionsAdvertiseChineseName(t *testing.T) {
	tests := []struct {
		name   string
		result map[string]any
	}{
		{
			name: "initialize",
			result: handleInitialize(&JsonRpcRequest{
				Params: map[string]any{"protocolVersion": ProtocolVersion},
				ID:     1,
			}, newSession()).Result.(map[string]any),
		},
		{
			name:   "discover",
			result: handleDiscover(&JsonRpcRequest{ID: 1}).Result.(map[string]any),
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := test.result["instructions"]; got != ServerInstructions {
				t.Fatalf("instructions = %q, want %q", got, ServerInstructions)
			}
		})
	}
}
