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
	"testing"

	"github.com/siyuan-note/siyuan/kernel/conf"
)

func TestInitMCPConfigDefaults(t *testing.T) {
	originalConf := Conf
	defer func() { Conf = originalConf }()

	Conf = NewAppConf()
	Conf.Api = conf.NewAPI()
	initMCPConfig(false)
	if Conf.Api.MCP.Enabled {
		t.Fatal("new workspace MCP should default to disabled")
	}
	if !Conf.Api.MCP.ReadOnly {
		t.Fatal("new workspace MCP should default to read-only")
	}

	Conf.Api.MCP = nil
	initMCPConfig(true)
	if !Conf.Api.MCP.Enabled {
		t.Fatal("existing workspace MCP should remain enabled during migration")
	}
	if Conf.Api.MCP.ReadOnly {
		t.Fatal("existing workspace MCP should retain full access during migration")
	}
}
