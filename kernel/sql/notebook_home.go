// 轻语 · 明窗净几，字字轻语
// Copyright (c) 2020-present, b3log.org
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

package sql

import (
	"database/sql"
	"errors"
	"regexp"
	"strings"
)

type NotebookHomeRow struct {
	Box     string
	Title   string
	Content string
	Updated int64
}

func initNotebookHomeTables(target *sql.DB) error {
	statements := []string{
		"CREATE TABLE IF NOT EXISTS notebook_homes (box PRIMARY KEY, title, content, updated)",
		"CREATE VIRTUAL TABLE IF NOT EXISTS notebook_homes_fts USING fts5(box UNINDEXED, title, content, updated UNINDEXED, content='notebook_homes', content_rowid='rowid', tokenize=\"" + ftsTokenize() + "\")",
	}
	for _, statement := range statements {
		if _, err := target.Exec(statement); err != nil {
			return err
		}
	}
	return nil
}

func UpsertNotebookHome(box, title, content string, updated int64) error {
	target := notebookHomeDB(box)
	if nil == target {
		return nil
	}
	tx, err := target.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	var rowID, oldUpdated int64
	var oldTitle, oldContent string
	err = tx.QueryRow("SELECT rowid, title, content, updated FROM notebook_homes WHERE box = ?", box).
		Scan(&rowID, &oldTitle, &oldContent, &oldUpdated)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	if err == nil {
		if _, err = tx.Exec("INSERT INTO notebook_homes_fts(notebook_homes_fts, rowid, box, title, content, updated) VALUES('delete', ?, ?, ?, ?, ?)",
			rowID, box, oldTitle, oldContent, oldUpdated); err != nil {
			return err
		}
		if _, err = tx.Exec("UPDATE notebook_homes SET title = ?, content = ?, updated = ? WHERE rowid = ?", title, content, updated, rowID); err != nil {
			return err
		}
	} else {
		result, insertErr := tx.Exec("INSERT INTO notebook_homes(box, title, content, updated) VALUES(?, ?, ?, ?)", box, title, content, updated)
		if insertErr != nil {
			return insertErr
		}
		rowID, err = result.LastInsertId()
		if err != nil {
			return err
		}
	}
	if _, err = tx.Exec("INSERT INTO notebook_homes_fts(rowid, box, title, content, updated) VALUES(?, ?, ?, ?, ?)",
		rowID, box, title, content, updated); err != nil {
		return err
	}
	return tx.Commit()
}

func DeleteNotebookHome(box string) error {
	target := notebookHomeDB(box)
	if nil == target {
		return nil
	}
	tx, err := target.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	var rowID, updated int64
	var title, content string
	if err = tx.QueryRow("SELECT rowid, title, content, updated FROM notebook_homes WHERE box = ?", box).
		Scan(&rowID, &title, &content, &updated); errors.Is(err, sql.ErrNoRows) {
		return nil
	} else if err != nil {
		return err
	}
	if _, err = tx.Exec("INSERT INTO notebook_homes_fts(notebook_homes_fts, rowid, box, title, content, updated) VALUES('delete', ?, ?, ?, ?, ?)",
		rowID, box, title, content, updated); err != nil {
		return err
	}
	if _, err = tx.Exec("DELETE FROM notebook_homes WHERE rowid = ?", rowID); err != nil {
		return err
	}
	return tx.Commit()
}

func SearchNotebookHomesInBox(box, query string, method int) ([]*NotebookHomeRow, error) {
	target := notebookHomeDB(box)
	if nil == target || strings.TrimSpace(query) == "" || method == 2 {
		return nil, nil
	}
	if method == 3 {
		pattern, err := regexp.Compile(query)
		if err != nil {
			return nil, err
		}
		rows, err := target.Query("SELECT box, title, content, updated FROM notebook_homes WHERE box = ?", box)
		if err != nil {
			return nil, err
		}
		defer rows.Close()
		var ret []*NotebookHomeRow
		for rows.Next() {
			row := &NotebookHomeRow{}
			if err = rows.Scan(&row.Box, &row.Title, &row.Content, &row.Updated); err != nil {
				return nil, err
			}
			if pattern.MatchString(row.Title) || pattern.MatchString(row.Content) {
				ret = append(ret, row)
			}
		}
		return ret, rows.Err()
	}
	match := query
	if method == 0 {
		parts := strings.Fields(query)
		for i, part := range parts {
			parts[i] = `"` + strings.ReplaceAll(part, `"`, `""`) + `"`
		}
		match = strings.Join(parts, " AND ")
	}
	rows, err := target.Query("SELECT h.box, h.title, h.content, h.updated FROM notebook_homes_fts f JOIN notebook_homes h ON h.rowid = f.rowid WHERE notebook_homes_fts MATCH ? AND h.box = ? ORDER BY rank", match, box)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var ret []*NotebookHomeRow
	for rows.Next() {
		row := &NotebookHomeRow{}
		if err = rows.Scan(&row.Box, &row.Title, &row.Content, &row.Updated); err != nil {
			return nil, err
		}
		ret = append(ret, row)
	}
	return ret, rows.Err()
}

func notebookHomeDB(box string) *sql.DB {
	if IsEncryptedBoxFn != nil && IsEncryptedBoxFn(box) {
		return GetEncryptedDB(box)
	}
	return db
}
