package conf

import "testing"

func TestRetainedSyncProviders(t *testing.T) {
	if ProviderS3 != 0 || ProviderWebDAV != 1 {
		t.Fatalf("unexpected provider values: S3=%d WebDAV=%d", ProviderS3, ProviderWebDAV)
	}
	if NewSync().Provider != ProviderS3 || NewSync().Enabled {
		t.Fatalf("new sync config must default to S3 with sync disabled")
	}
	for _, provider := range []int{ProviderS3, ProviderWebDAV} {
		if !IsValidSyncProvider(provider) {
			t.Fatalf("retained provider rejected: %d", provider)
		}
	}
	for _, provider := range []int{-1, 2, 3, 4} {
		if IsValidSyncProvider(provider) {
			t.Fatalf("removed or invalid provider accepted: %d", provider)
		}
	}
}

func TestNormalizeSyncProvider(t *testing.T) {
	for _, provider := range []int{-1, 0, 1, 2, 3, 4} {
		for _, enabled := range []bool{false, true} {
			sync := &Sync{Provider: provider, Enabled: enabled, CloudName: "main"}
			sync.NormalizeProvider()
			wantProvider, wantEnabled := provider, enabled
			if !IsValidSyncProvider(provider) {
				wantProvider, wantEnabled = ProviderS3, false
			}
			if sync.Provider != wantProvider || sync.Enabled != wantEnabled || sync.CloudName != "main" {
				t.Fatalf("unexpected migration for provider=%d enabled=%v: %+v", provider, enabled, sync)
			}
		}
	}
}
