package conf

import "testing"

func TestRetainedSyncProviders(t *testing.T) {
	if ProviderS3 != 0 || ProviderWebDAV != 1 || ProviderLocal != 2 {
		t.Fatalf("unexpected provider values: S3=%d WebDAV=%d Local=%d", ProviderS3, ProviderWebDAV, ProviderLocal)
	}
	if NewSync().Provider != ProviderLocal {
		t.Fatalf("new sync config must default to local provider")
	}
	for _, provider := range []int{ProviderS3, ProviderWebDAV, ProviderLocal} {
		if !IsValidSyncProvider(provider) {
			t.Fatalf("retained provider rejected: %d", provider)
		}
	}
	for _, provider := range []int{-1, 3, 4} {
		if IsValidSyncProvider(provider) {
			t.Fatalf("removed or invalid provider accepted: %d", provider)
		}
	}
}
