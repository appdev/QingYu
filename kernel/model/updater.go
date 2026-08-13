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

// CheckUpdate 保留旧 API 调用兼容性。轻语自有更新服务上线前不检查或下载应用更新。
func CheckUpdate(showMsg bool) {
}

// getNewVerInstallPkgPath 保留关闭流程的内部兼容接口，不向桌面宿主返回安装包。
func getNewVerInstallPkgPath() string {
	return ""
}

// skipNewVerInstallPkg 禁止旧配置重新启用上游应用更新。
func skipNewVerInstallPkg() bool {
	return true
}
