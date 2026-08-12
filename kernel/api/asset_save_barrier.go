// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package api

import (
	"errors"
	"net/http"
	"sync"
	"time"

	"github.com/88250/gulu"
	"github.com/gin-gonic/gin"
	"github.com/siyuan-note/siyuan/kernel/util"
)

type pendingMarkdownSaveBarrier struct {
	acked    map[string]struct{}
	done     chan struct{}
	err      error
	expected map[string]struct{}
	closed   bool
}

type markdownSaveBarrierManager struct {
	mu      sync.Mutex
	pending map[string]*pendingMarkdownSaveBarrier
}

func newMarkdownSaveBarrierManager() *markdownSaveBarrierManager {
	return &markdownSaveBarrierManager{pending: map[string]*pendingMarkdownSaveBarrier{}}
}

func (manager *markdownSaveBarrierManager) begin(id string, sessionIDs []string) func(time.Duration) error {
	expected := map[string]struct{}{}
	for _, sessionID := range sessionIDs {
		if sessionID != "" {
			expected[sessionID] = struct{}{}
		}
	}
	pending := &pendingMarkdownSaveBarrier{
		acked:    map[string]struct{}{},
		done:     make(chan struct{}),
		expected: expected,
	}
	if len(expected) == 0 {
		pending.closed = true
		close(pending.done)
	}
	manager.mu.Lock()
	manager.pending[id] = pending
	manager.mu.Unlock()

	return func(timeout time.Duration) error {
		timer := time.NewTimer(timeout)
		defer timer.Stop()
		select {
		case <-pending.done:
		case <-timer.C:
			manager.finish(id, errors.New("waiting for Markdown saves timed out"))
			<-pending.done
		}
		manager.mu.Lock()
		delete(manager.pending, id)
		err := pending.err
		manager.mu.Unlock()
		return err
	}
}

func (manager *markdownSaveBarrierManager) finish(id string, err error) {
	manager.mu.Lock()
	defer manager.mu.Unlock()
	pending := manager.pending[id]
	if pending == nil || pending.closed {
		return
	}
	pending.err = err
	pending.closed = true
	close(pending.done)
}

func (manager *markdownSaveBarrierManager) ack(id, sessionID string, success bool) bool {
	manager.mu.Lock()
	defer manager.mu.Unlock()
	pending := manager.pending[id]
	if pending == nil {
		return false
	}
	if _, ok := pending.expected[sessionID]; !ok {
		return false
	}
	if pending.closed {
		return true
	}
	if !success {
		pending.err = errors.New("a Markdown document could not be saved")
		pending.closed = true
		close(pending.done)
		return true
	}
	pending.acked[sessionID] = struct{}{}
	if len(pending.acked) == len(pending.expected) {
		pending.closed = true
		close(pending.done)
	}
	return true
}

var markdownSaveBarriers = newMarkdownSaveBarrierManager()

func awaitMarkdownSaveBarrier(timeout time.Duration) error {
	id := gulu.Rand.String(16)
	wait := markdownSaveBarriers.begin(id, util.MainWebSocketSessionIDs())
	util.BroadcastByType("main", "flushMarkdownForAssetScan", 0, "", map[string]any{"id": id})
	return wait(timeout)
}

func ackMarkdownSaveBarrier(c *gin.Context) {
	ret := gulu.Ret.NewResult()
	defer c.JSON(http.StatusOK, ret)
	arg, ok := util.JsonArg(c, ret)
	if !ok {
		return
	}
	var id, sessionID string
	var success bool
	if !util.ParseJsonArgs(arg, ret,
		util.BindJsonArg("id", &id, true, true),
		util.BindJsonArg("sessionId", &sessionID, true, true),
		util.BindJsonArg("success", &success, true, false),
	) {
		return
	}
	if !markdownSaveBarriers.ack(id, sessionID, success) {
		ret.Code = -1
		ret.Msg = "Markdown save barrier is no longer active"
	}
}
