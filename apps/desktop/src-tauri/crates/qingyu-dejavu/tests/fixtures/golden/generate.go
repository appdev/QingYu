//go:build ignore

// Generate the Task 3 cross-language fixtures from the pinned Dejavu module.
// Run this file from the root of the pinned github.com/siyuan-note/dejavu checkout.
package main

import (
	"encoding/base64"
	"fmt"
	"os"
	"path/filepath"

	"github.com/88250/gulu"
	"github.com/klauspost/compress/zstd"
	"github.com/siyuan-note/dejavu/entity"
	"github.com/siyuan-note/encryption"
)

const (
	fileID       = "0123456789abcdef0123456789abcdef01234567"
	indexID      = "89abcdef0123456789abcdef0123456789abcdef"
	checkIndexID = "fedcba9876543210fedcba9876543210fedcba98"
)

func main() {
	if len(os.Args) != 2 {
		panic("usage: go run generate.go OUTPUT_DIR")
	}
	outputDir := os.Args[1]
	if err := os.MkdirAll(outputDir, 0o755); err != nil {
		panic(err)
	}

	key, err := encryption.KDF("oracle-password", "oracle-salt")
	if err != nil {
		panic(err)
	}
	write(outputDir, "kdf-key.bin", key)

	aesFixture, err := encryption.AesEncrypt([]byte("siyuan"), key)
	if err != nil {
		panic(err)
	}
	write(outputDir, "aes-gcm-siyuan.bin", aesFixture)

	encoder, err := zstd.NewWriter(nil,
		zstd.WithEncoderLevel(zstd.SpeedDefault),
		zstd.WithEncoderCRC(false),
		zstd.WithWindowSize(512*1024))
	if err != nil {
		panic(err)
	}
	defer encoder.Close()

	file := &entity.File{
		ID:      fileID,
		Path:    "/oracle/文档.md",
		Size:    12,
		Updated: 1700000000123,
		Chunks: []string{
			"1111111111111111111111111111111111111111",
			"2222222222222222222222222222222222222222",
		},
	}
	fileJSON, err := gulu.JSON.MarshalJSON(file)
	if err != nil {
		panic(err)
	}
	fileObject, err := encryption.AesEncrypt(encoder.EncodeAll(fileJSON, nil), key)
	if err != nil {
		panic(err)
	}
	write(outputDir, "file-object.bin", fileObject)

	verifyBytes, err := encryption.AesEncrypt([]byte("siyuan"), key)
	if err != nil {
		panic(err)
	}
	index := &entity.Index{
		ID:              indexID,
		Memo:            "Go golden oracle",
		Created:         1700000000456,
		Files:           []string{fileID},
		Count:           1,
		Size:            12,
		SystemID:        "oracle-system-id",
		SystemName:      "Oracle Device",
		SystemOS:        "darwin",
		CheckIndexID:    checkIndexID,
		AesKeyVerifyVal: base64.StdEncoding.EncodeToString(verifyBytes),
	}
	indexJSON, err := gulu.JSON.MarshalJSON(index)
	if err != nil {
		panic(err)
	}
	write(outputDir, "index-object.bin", encoder.EncodeAll(indexJSON, nil))

	fmt.Printf("file-json=%s\n", fileJSON)
	fmt.Printf("index-json=%s\n", indexJSON)
}

func write(dir, name string, data []byte) {
	if err := os.WriteFile(filepath.Join(dir, name), data, 0o644); err != nil {
		panic(err)
	}
}
