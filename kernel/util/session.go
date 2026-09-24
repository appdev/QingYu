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

import (
	"crypto/subtle"
	"strings"
	"sync"
	"time"

	"github.com/88250/gulu"
	ginSessions "github.com/gin-contrib/sessions"
	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/logging"
)

var WrongAuthCount int

func NeedCaptcha() bool {
	return 3 < WrongAuthCount
}

// AuthCodeEquals 使用恒定时间比较认证码，避免通过响应时间差异猜测秘密。
func AuthCodeEquals(a, b string) bool {
	return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1
}

var (
	authThrottleLock sync.Mutex
	authThrottles    = map[string]*authThrottle{}
)

type authThrottle struct {
	failCount int
	lockUntil time.Time
	lastFail  time.Time
}

const (
	authThrottleMaxFail     = 5
	authThrottleLockBaseSec = 30
	authThrottleLockMaxSec  = 15 * 60
	authThrottleWindow      = 15 * time.Minute
	authThrottleMaxEntries  = 10000
)

// AuthThrottleCheck 返回 key 剩余锁定秒数，0 表示未锁定。
func AuthThrottleCheck(key string) int {
	authThrottleLock.Lock()
	defer authThrottleLock.Unlock()
	entry := authThrottles[key]
	if entry == nil {
		return 0
	}
	now := time.Now()
	if now.Before(entry.lockUntil) {
		return int(time.Until(entry.lockUntil)/time.Second) + 1
	}
	if !entry.lockUntil.IsZero() || now.Sub(entry.lastFail) >= authThrottleWindow {
		delete(authThrottles, key)
	}
	return 0
}

// AuthThrottleFail 记录认证失败，并在连续失败达到阈值后暂时锁定。
func AuthThrottleFail(key string) {
	authThrottleLock.Lock()
	defer authThrottleLock.Unlock()
	now := time.Now()
	entry := authThrottles[key]
	if entry == nil {
		if len(authThrottles) >= authThrottleMaxEntries {
			return
		}
		entry = &authThrottle{}
		authThrottles[key] = entry
	} else if now.Sub(entry.lastFail) >= authThrottleWindow {
		entry.failCount = 0
	}
	entry.lastFail = now
	entry.failCount++
	if entry.failCount <= authThrottleMaxFail {
		return
	}
	lockSeconds := authThrottleLockBaseSec << (entry.failCount - authThrottleMaxFail)
	if lockSeconds > authThrottleLockMaxSec {
		lockSeconds = authThrottleLockMaxSec
	}
	entry.lockUntil = now.Add(time.Duration(lockSeconds) * time.Second)
}

// AuthThrottleReset 清除认证成功后的失败记录。
func AuthThrottleReset(key string) {
	authThrottleLock.Lock()
	defer authThrottleLock.Unlock()
	delete(authThrottles, key)
}

// SessionData represents the session.
type SessionData struct {
	Workspaces map[string]*WorkspaceSession // <WorkspacePath, WorkspaceSession>
}

type WorkspaceSession struct {
	AccessAuthCode string
	Captcha        string
}

func (sd *SessionData) Clear(c *gin.Context) {
	session := ginSessions.Default(c)
	session.Delete("data")
	if err := session.Save(); err != nil {
		logging.LogErrorf("clear session failed: %v", err)
	}
}

// Save saves the current session of the specified context.
func (sd *SessionData) Save(c *gin.Context) error {
	session := ginSessions.Default(c)
	sessionDataBytes, err := gulu.JSON.MarshalJSON(sd)
	if err != nil {
		return err
	}
	session.Set("data", string(sessionDataBytes))
	return session.Save()
}

// GetSession returns session of the specified context.
func GetSession(c *gin.Context) *SessionData {
	ret := &SessionData{}

	session := ginSessions.Default(c)
	sessionDataStr := session.Get("data")
	if nil == sessionDataStr {
		return ret
	}

	err := gulu.JSON.UnmarshalJSON([]byte(sessionDataStr.(string)), ret)
	if err != nil {
		return ret
	}

	c.Set("session", ret)
	return ret
}

func GetWorkspaceSession(session *SessionData) (ret *WorkspaceSession) {
	ret = &WorkspaceSession{}
	if nil == session.Workspaces {
		session.Workspaces = map[string]*WorkspaceSession{}
	}
	ret = session.Workspaces[WorkspaceDir]
	if nil == ret {
		ret = &WorkspaceSession{}
		session.Workspaces[WorkspaceDir] = ret
	}
	return
}

func RemoveWorkspaceSession(session *SessionData) {
	delete(session.Workspaces, WorkspaceDir)
}

// IsNativeAppUserAgent 判断请求是否来自轻语或兼容的思源原生客户端。
func IsNativeAppUserAgent(userAgent string) bool {
	return strings.HasPrefix(userAgent, "QingYu/") || strings.HasPrefix(userAgent, "SiYuan/")
}

// IsBrowserRequest 判断请求是否来自浏览器（非原生客户端）。
func IsBrowserRequest(c *gin.Context) bool {
	return !IsNativeAppUserAgent(c.GetHeader("User-Agent"))
}
