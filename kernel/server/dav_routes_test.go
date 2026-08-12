// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package server

import (
	"testing"

	"github.com/gin-gonic/gin"
)

func TestDAVRouteBoundary(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	serveDAV(engine)

	routes := map[string]bool{}
	for _, route := range engine.Routes() {
		routes[route.Path] = true
	}
	for _, removed := range []string{"/.well-known/caldav", "/caldav/*path"} {
		if routes[removed] {
			t.Fatalf("removed CalDAV route remains registered: %s", removed)
		}
	}
	for _, preserved := range []string{"/webdav/*path", "/.well-known/carddav", "/carddav/*path"} {
		if !routes[preserved] {
			t.Fatalf("preserved DAV route is missing: %s", preserved)
		}
	}
}
