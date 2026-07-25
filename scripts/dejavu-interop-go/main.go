package main

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"sync/atomic"

	"github.com/siyuan-note/dejavu"
	"github.com/siyuan-note/dejavu/cloud"
	"github.com/siyuan-note/logging"
)

const maxRequestBytes = 1024 * 1024
const latestRef = "refs/latest"

type request struct {
	Operation                string `json:"operation"`
	DeviceID                 string `json:"deviceId"`
	DataPath                 string `json:"dataPath"`
	RepoPath                 string `json:"repoPath"`
	HistoryPath              string `json:"historyPath"`
	TempPath                 string `json:"tempPath"`
	KeyBase64                string `json:"keyBase64"`
	CloudRoot                string `json:"cloudRoot"`
	FailBeforeRefPublication *bool  `json:"failBeforeRefPublication"`
}

type response struct {
	IndexID   *string `json:"indexId"`
	Upserts   int     `json:"upserts"`
	Removes   int     `json:"removes"`
	Conflicts int     `json:"conflicts"`
	ErrorCode *string `json:"errorCode"`
}

type safeError struct {
	code string
}

func (err *safeError) Error() string {
	return err.code
}

type failBeforeLatestCloud struct {
	cloud.Cloud
	enabled bool
	failed  atomic.Bool
}

func (wrapper *failBeforeLatestCloud) rejectLatest(filePath string) error {
	if wrapper.enabled && filePath == latestRef && wrapper.failed.CompareAndSwap(false, true) {
		return errors.New("injected ref publication failure")
	}
	return nil
}

func (wrapper *failBeforeLatestCloud) UploadObject(filePath string, overwrite bool) (int64, error) {
	if err := wrapper.rejectLatest(filePath); err != nil {
		return 0, err
	}
	return wrapper.Cloud.UploadObject(filePath, overwrite)
}

func (wrapper *failBeforeLatestCloud) UploadBytes(filePath string, data []byte, overwrite bool) (int64, error) {
	if err := wrapper.rejectLatest(filePath); err != nil {
		return 0, err
	}
	return wrapper.Cloud.UploadBytes(filePath, data, overwrite)
}

func main() {
	logging.SetLogLevel("off")
	logging.SetLogToStdout(false)
	logging.SetLogPath(os.DevNull)

	result, err := execute()
	failed := err != nil
	if failed {
		code := "operation_failed"
		var safe *safeError
		if errors.As(err, &safe) {
			code = safe.code
		}
		result = response{ErrorCode: &code}
	}
	if encodeErr := json.NewEncoder(os.Stdout).Encode(result); encodeErr != nil {
		os.Exit(1)
	}
	if failed {
		fmt.Fprintf(os.Stderr, "dejavu-interop: request failed (%s)\n", *result.ErrorCode)
		os.Exit(1)
	}
}

func execute() (response, error) {
	req, err := readRequest()
	if err != nil {
		return response{}, err
	}
	key, err := validateRequest(req)
	if err != nil {
		return response{}, err
	}

	newRepo := func(remote cloud.Cloud) (*dejavu.Repo, error) {
		return dejavu.NewRepo(
			req.DataPath,
			req.RepoPath,
			req.HistoryPath,
			req.TempPath,
			req.DeviceID,
			req.DeviceID,
			runtime.GOOS,
			key,
			nil,
			remote,
		)
	}
	if req.Operation == "inspect" {
		repo, openErr := newRepo(nil)
		if openErr != nil {
			return response{}, openErr
		}
		latest, latestErr := repo.Latest()
		if errors.Is(latestErr, dejavu.ErrNotFoundIndex) {
			return response{}, nil
		}
		if latestErr != nil {
			return response{}, latestErr
		}
		return response{IndexID: &latest.ID}, nil
	}

	cloudParent := filepath.Dir(req.CloudRoot)
	cloudDir := filepath.Base(req.CloudRoot)
	local := cloud.NewLocal(&cloud.BaseCloud{Conf: &cloud.Conf{
		Dir:           cloudDir,
		UserID:        "0",
		AvailableSize: math.MaxInt64,
		Local: &cloud.ConfLocal{
			Endpoint:       cloudParent,
			ConcurrentReqs: 4,
		},
	}})
	wrapped := &failBeforeLatestCloud{
		Cloud:   local,
		enabled: *req.FailBeforeRefPublication,
	}
	repo, err := newRepo(wrapped)
	if err != nil {
		return response{}, err
	}

	if _, err = repo.Index("[Interop] Current working tree", false, nil); err != nil {
		return response{}, err
	}
	merged, _, err := repo.Sync(nil)
	if err != nil {
		if wrapped.failed.Load() {
			return response{}, &safeError{code: "ref_publication_injected"}
		}
		return response{}, err
	}
	latest, err := repo.Latest()
	if err != nil {
		return response{}, err
	}
	return response{
		IndexID:   &latest.ID,
		Upserts:   len(merged.Upserts),
		Removes:   len(merged.Removes),
		Conflicts: len(merged.Conflicts),
	}, nil
}

func readRequest() (request, error) {
	limited := io.LimitReader(os.Stdin, maxRequestBytes+1)
	data, err := io.ReadAll(limited)
	if err != nil {
		return request{}, &safeError{code: "request_read_failed"}
	}
	if len(data) > maxRequestBytes {
		return request{}, &safeError{code: "request_too_large"}
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	var raw json.RawMessage
	if err = decoder.Decode(&raw); err != nil {
		return request{}, &safeError{code: "request_invalid"}
	}
	var trailing any
	if err = decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		return request{}, &safeError{code: "request_invalid"}
	}
	var fields map[string]json.RawMessage
	if err = json.Unmarshal(raw, &fields); err != nil || len(fields) != 9 {
		return request{}, &safeError{code: "request_invalid"}
	}
	for _, name := range []string{
		"operation",
		"deviceId",
		"dataPath",
		"repoPath",
		"historyPath",
		"tempPath",
		"keyBase64",
		"cloudRoot",
		"failBeforeRefPublication",
	} {
		if _, exists := fields[name]; !exists {
			return request{}, &safeError{code: "request_invalid"}
		}
	}
	strict := json.NewDecoder(bytes.NewReader(raw))
	strict.DisallowUnknownFields()
	var req request
	if err = strict.Decode(&req); err != nil {
		return request{}, &safeError{code: "request_invalid"}
	}
	return req, nil
}

func validateRequest(req request) ([]byte, error) {
	if req.Operation != "index-and-sync" && req.Operation != "inspect" {
		return nil, &safeError{code: "operation_invalid"}
	}
	if req.DeviceID == "" || req.FailBeforeRefPublication == nil {
		return nil, &safeError{code: "request_invalid"}
	}
	for _, path := range []string{
		req.DataPath,
		req.RepoPath,
		req.HistoryPath,
		req.TempPath,
		req.CloudRoot,
	} {
		if !filepath.IsAbs(path) {
			return nil, &safeError{code: "path_invalid"}
		}
	}
	key, err := base64.StdEncoding.Strict().DecodeString(req.KeyBase64)
	if err != nil || len(key) != 32 || base64.StdEncoding.EncodeToString(key) != req.KeyBase64 {
		return nil, &safeError{code: "key_invalid"}
	}
	return key, nil
}
