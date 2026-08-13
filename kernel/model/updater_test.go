// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package model

import (
	"testing"

	"github.com/siyuan-note/siyuan/kernel/conf"
	"github.com/siyuan-note/siyuan/kernel/util"
)

func TestApplicationUpdateRemainsDisabledForLegacyConfiguration(t *testing.T) {
	old := Conf
	oldContainer := util.Container
	t.Cleanup(func() {
		Conf = old
		util.Container = oldContainer
	})
	Conf = &AppConf{System: &conf.System{DownloadInstallPkg: true}}
	util.Container = util.ContainerStd
	if !skipNewVerInstallPkg() {
		t.Fatal("legacy configuration must not enable application updates")
	}
	if got := getNewVerInstallPkgPath(); got != "" {
		t.Fatalf("unexpected application update package %q", got)
	}
}
