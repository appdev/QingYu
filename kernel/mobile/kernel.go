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

package mobile

import (
	"encoding/json"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/88250/gulu"
	"github.com/88250/lute/ast"
	"github.com/siyuan-note/filelock"
	"github.com/siyuan-note/logging"
	"github.com/siyuan-note/siyuan/kernel/cache"
	"github.com/siyuan-note/siyuan/kernel/job"
	"github.com/siyuan-note/siyuan/kernel/model"
	"github.com/siyuan-note/siyuan/kernel/plugin"
	"github.com/siyuan-note/siyuan/kernel/server"
	"github.com/siyuan-note/siyuan/kernel/sql"
	"github.com/siyuan-note/siyuan/kernel/util"
	_ "golang.org/x/mobile/bind"
)

type exportFileLease struct {
	LeaseID string `json:"leaseID"`
	Path    string `json:"path"`
	Name    string `json:"name"`
	Size    int64  `json:"size"`
}

var exportFileLeases = struct {
	sync.Mutex
	boxIDs map[string]string
}{boxIDs: map[string]string{}}

func StartKernelFast(container, appDir, workspaceBaseDir, localIPs string) {
	go server.Serve(true, model.Conf.CookieKey)
}

func StartKernel(container, appDir, workspaceBaseDir, timezoneID, localIPs, lang, osVer string) {
	SetTimezone(container, appDir, timezoneID)
	util.Mode = "prod"
	util.MobileOSVer = osVer
	util.LocalIPs = strings.Split(localIPs, ",")
	util.BootMobile(container, appDir, workspaceBaseDir, lang)

	model.InitConf()
	go server.Serve(false, model.Conf.CookieKey)
	go func() {
		model.InitAppearance()
		sql.InitDatabase(false)
		sql.InitHistoryDatabase(false)
		sql.InitAssetContentDatabase(false)
		sql.SetCaseSensitive(model.Conf.Search.CaseSensitive)
		sql.SetIndexAssetPath(model.Conf.Search.IndexAssetPath)

		model.BootSyncData()
		model.InitBoxes()
		util.LoadAssetsTexts()

		util.SetBooted()
		util.PushClearAllMsg()

		job.StartCron()
		go model.AutoGenerateFileHistory()
		go cache.LoadAssets()
		go plugin.InitManager()
	}()
}

func Language(num int) string {
	return model.Conf.Language(num)
}

func ShowMsg(msg string, timeout int) {
	util.PushMsg(msg, timeout)
}

func IsHttpServing() bool {
	return util.HttpServing
}

func SetHttpServerPort(port int) {
	filelock.AndroidServerPort = port
}

func UpdateLocalIPs(localIPs string) {
	util.LocalIPs = strings.Split(localIPs, ",")
}

func LanSyncActive() bool {
	return false
}

func GetCurrentWorkspacePath() string {
	return util.WorkspaceDir
}

func GetAssetAbsPath(asset string) (ret string) {
	ret, err := model.GetAssetAbsPath(asset)
	if err != nil {
		logging.LogErrorf("get asset [%s] abs path failed: %s", asset, err)
		ret = asset
	}
	return
}

func GetMimeTypeByExt(ext string) string {
	return util.GetMimeTypeByExt(ext)
}

func SetTimezone(container, appDir, timezoneID string) {
	if "ios" == container {
		os.Setenv("ZONEINFO", filepath.Join(appDir, "app", "zoneinfo.zip"))
	}
	z, err := time.LoadLocation(strings.TrimSpace(timezoneID))
	if err != nil {
		fmt.Printf("load location failed: %s\n", err)
		time.Local = time.FixedZone("CST", 8*3600)
		return
	}
	time.Local = z
}

func DisableFeature(feature string) {
	util.DisableFeature(feature)
}

func FilepathBase(path string) string {
	return filepath.Base(path)
}

func FilterUploadFileName(name string) string {
	return util.FilterUploadFileName(name)
}

func AssetName(name string) string {
	return util.AssetName(name, ast.NewNodeID())
}

func HTML2Markdown(html string) string {
	return util.NewLute().HTML2Md(html)
}

func Unzip(zipFilePath, destination string) {
	if err := gulu.Zip.Unzip(zipFilePath, destination); nil != err {
		logging.LogErrorf("unzip [%s] failed: %s", zipFilePath, err)
		panic(err)
	}
}

// GetExportFilePath 解析导出文件绝对路径，绕过 HTTP 层以避免锁屏密码拦截。
// exportPath 格式为 "/export/xxx.zip" 或 "assets/xxx"。
// 返回文件在磁盘上的绝对路径，以便原生端分块拷贝，避免大文件内存溢出。
// 解析失败返回空字符串。
func GetExportFilePath(exportPath string) (ret string) {
	var absPath string
	if after, ok := strings.CutPrefix(exportPath, "/export/"); ok {
		fileName := after
		if decoded, err := url.PathUnescape(fileName); err == nil {
			fileName = decoded
		}
		fileName = filepath.Clean(fileName)
		if strings.HasPrefix(fileName, "..") {
			logging.LogWarnf("get export file path [%s] blocked: path traversal attempt [%s]", exportPath, fileName)
			return
		}
		// 加密导出受控路径（<boxID>/<kind>/<file>）：必须经注册表校验且 box 已解锁，否则 fail-closed
		if model.IsManagedEncryptedExportPath(fileName) {
			artifact, ok := model.ResolveManagedExportForMobile(fileName)
			if !ok {
				logging.LogWarnf("get export file path [%s] blocked: managed export not available or box locked", exportPath)
				return
			}
			return artifact
		}
		absPath = filepath.Join(util.TempDir, "export", fileName)
		exportBaseDir := filepath.Join(util.TempDir, "export")
		if !gulu.File.IsSubPath(exportBaseDir, absPath) {
			logging.LogWarnf("get export file path [%s] blocked: path [%s] is outside export base dir [%s]", exportPath, absPath, exportBaseDir)
			return
		}
	} else if strings.HasPrefix(exportPath, "assets/") {
		var err error
		absPath, err = model.GetAssetAbsPath(exportPath)
		if nil != err {
			logging.LogErrorf("get asset abs path [%s] failed: %s", exportPath, err)
			return
		}
	} else {
		logging.LogWarnf("get export file path [%s] failed: unsupported path prefix", exportPath)
		return
	}

	if "" == absPath {
		logging.LogWarnf("get export file path [%s] failed: resolved to empty abs path", exportPath)
		return
	}
	return absPath
}

// AcquireExportFile 为安卓端解析导出文件，并在复制加密导出产物期间阻止笔记本锁定。
func AcquireExportFile(exportPath string) (ret string) {
	absPath := GetExportFilePath(exportPath)
	if "" == absPath {
		return
	}
	info, err := os.Stat(absPath)
	if err != nil || !info.Mode().IsRegular() {
		logging.LogWarnf("acquire export file [%s] failed: file is unavailable", exportPath)
		return
	}

	boxID := ""
	if relativePath, ok := strings.CutPrefix(exportPath, "/export/"); ok {
		if decoded, decodeErr := url.PathUnescape(relativePath); decodeErr == nil {
			relativePath = decoded
		}
		relativePath = filepath.Clean(relativePath)
		if model.IsManagedEncryptedExportPath(relativePath) {
			resolvedBoxID, _, resolved := model.ResolveManagedEncryptedExport(relativePath)
			if !resolved {
				return
			}
			model.HoldBoxReadLock(resolvedBoxID)
			resolvedPath, available := model.ResolveManagedExportForMobile(relativePath)
			if !available || resolvedPath != absPath {
				model.ReleaseBoxReadLock(resolvedBoxID)
				return
			}
			boxID = resolvedBoxID
		}
	}

	leaseID := ast.NewNodeID()
	lease := &exportFileLease{LeaseID: leaseID, Path: absPath, Name: filepath.Base(absPath), Size: info.Size()}
	data, err := json.Marshal(lease)
	if err != nil {
		if "" != boxID {
			model.ReleaseBoxReadLock(boxID)
		}
		logging.LogErrorf("marshal export file lease failed: %s", err)
		return
	}
	exportFileLeases.Lock()
	exportFileLeases.boxIDs[leaseID] = boxID
	exportFileLeases.Unlock()
	return string(data)
}

// ReleaseExportFile 释放 AcquireExportFile 获取的加密笔记本读锁。
func ReleaseExportFile(leaseID string) {
	exportFileLeases.Lock()
	boxID, ok := exportFileLeases.boxIDs[leaseID]
	if ok {
		delete(exportFileLeases.boxIDs, leaseID)
	}
	exportFileLeases.Unlock()
	if ok && "" != boxID {
		model.ReleaseBoxReadLock(boxID)
	}
}

func Exit() {
	os.Exit(logging.ExitCodeOk)
}
