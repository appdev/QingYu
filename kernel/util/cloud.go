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

package util

var CurrentCloudRegion = 0

func IsChinaCloud() bool {
	return 0 == CurrentCloudRegion
}

func GetCloudServer() string {
	if 0 == CurrentCloudRegion {
		return chinaServer
	}
	return northAmericaServer
}

const (
	chinaServer        = "https://siyuan-sync.b3logfile.com" // 中国大陆版本信息服务地址
	northAmericaServer = "https://siyuan-cloud.liuyun.io"    // 北美版本信息服务地址

	BazaarStatServer = "https://bazaar.b3logfile.com" // 集市包统计服务地址，七牛云，全球 CDN
	BazaarOSSServer  = "https://oss.b3logfile.com"    // 云端对象存储地址，七牛云，仅用于读取集市包，全球 CDN
)
