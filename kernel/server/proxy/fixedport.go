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

package proxy

import (
	"crypto/tls"
	"errors"
	"net"
	"net/http"
	"net/http/httputil"

	"github.com/siyuan-note/logging"
	"github.com/siyuan-note/siyuan/kernel/util"
	"github.com/soheilhy/cmux"
)

func InitFixedPortService(host string, certPath, keyPath string) {
	if util.FixedPort != util.ServerPort {
		if util.IsPortOpen(util.FixedPort) {
			return
		}

		addr := host + ":" + util.FixedPort

		// 启动固定端口的反向代理服务器，让浏览器扩展或外部程序无需配置动态端口即可调用内核接口。
		proxy := httputil.NewSingleHostReverseProxy(util.ServerURL)
		proxy.Transport = &http.Transport{
			TLSClientConfig: &tls.Config{InsecureSkipVerify: true},
		}

		if "" != certPath {
			logging.LogInfof("fixed port service [%s] is running (HTTP/HTTPS dual mode)", addr)

			ln, listenErr := net.Listen("tcp", addr)
			if listenErr != nil {
				logging.LogWarnf("boot fixed port service [%s] failed: %s", addr, listenErr)
				return
			}

			if _, _, serveErr := util.ServeMultiplexed(ln, proxy, certPath, keyPath, nil, nil); serveErr != nil {
				if !errors.Is(serveErr, cmux.ErrListenerClosed) && !errors.Is(serveErr, http.ErrServerClosed) {
					logging.LogWarnf("fixed port cmux serve error: %s", serveErr)
				}
			}
		} else {
			logging.LogInfof("fixed port service [%s] is running", addr)
			if proxyErr := http.ListenAndServe(addr, proxy); nil != proxyErr {
				logging.LogWarnf("boot fixed port service [%s] failed: %s", util.ServerURL, proxyErr)
			}
		}
		logging.LogInfof("fixed port service [%s] is stopped", addr)
	}
}
