use sha1::{Digest, Sha1};
use time::OffsetDateTime;

use base64::Engine;

use crate::crypto::{decrypt, encrypt};
use crate::RepoError;

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct File {
    pub id: String,
    pub path: String,
    pub size: i64,
    pub updated: i64,
    pub chunks: Vec<String>,
}

impl File {
    pub fn new(path: impl Into<String>, size: i64, updated: i64) -> Self {
        let path = path.into();
        let id = sha1_hex(format!("{path}{}", updated / 1000).as_bytes());
        Self {
            id,
            path,
            size,
            updated,
            chunks: Vec::new(),
        }
    }

    pub fn sec_updated(&self) -> i64 {
        self.updated / 1000
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Chunk {
    pub id: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Index {
    pub id: String,
    pub memo: String,
    pub created: i64,
    pub files: Vec<String>,
    pub count: usize,
    pub size: i64,
    #[serde(rename = "systemID")]
    pub system_id: String,
    #[serde(rename = "systemName")]
    pub system_name: String,
    #[serde(rename = "systemOS")]
    pub system_os: String,
    #[serde(rename = "checkIndexID")]
    pub check_index_id: String,
    #[serde(rename = "aesKeyVerifyVal")]
    pub aes_key_verify_val: String,
}

impl Index {
    pub fn init_aes_key_verify_val(&mut self, key: &[u8; 32]) -> Result<(), RepoError> {
        let encrypted = encrypt(b"siyuan", key)?;
        self.aes_key_verify_val = base64::engine::general_purpose::STANDARD.encode(encrypted);
        Ok(())
    }

    pub fn verify_aes_key(&self, key: &[u8; 32]) -> bool {
        if self.aes_key_verify_val.is_empty() {
            return true;
        }

        let Ok(encrypted) =
            base64::engine::general_purpose::STANDARD.decode(&self.aes_key_verify_val)
        else {
            return false;
        };
        matches!(decrypt(&encrypted, key), Ok(plaintext) if plaintext == b"siyuan")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CheckIndex {
    pub id: String,
    #[serde(rename = "indexID")]
    pub index_id: String,
    pub files: Vec<CheckIndexFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CheckIndexFile {
    pub id: String,
    pub chunks: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeResult {
    pub time: OffsetDateTime,
    pub upserts: Vec<File>,
    pub removes: Vec<File>,
    pub conflicts: Vec<File>,
}

impl MergeResult {
    pub fn data_changed(&self) -> bool {
        !self.upserts.is_empty() || !self.removes.is_empty() || !self.conflicts.is_empty()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrafficStat {
    pub download_file_count: usize,
    pub download_chunk_count: usize,
    pub download_bytes: i64,
    pub upload_file_count: usize,
    pub upload_chunk_count: usize,
    pub upload_bytes: i64,
    pub api_get: usize,
    pub api_put: usize,
}

pub fn sha1_hex(data: &[u8]) -> String {
    format!("{:x}", Sha1::digest(data))
}

pub fn random_hash() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(sha1_hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::{File, Index};

    impl Index {
        fn fixture() -> Self {
            Self {
                id: "index-id".to_owned(),
                memo: "fixture".to_owned(),
                created: 1_700_000_000_123,
                files: vec!["file-id".to_owned()],
                count: 1,
                size: 6,
                system_id: "system-id".to_owned(),
                system_name: "QingYu".to_owned(),
                system_os: "macOS".to_owned(),
                check_index_id: "check-index-id".to_owned(),
                aes_key_verify_val: "verify-value".to_owned(),
            }
        }
    }

    #[test]
    fn file_id_matches_dejavu_path_plus_second_timestamp() {
        let file = File::new("/doc.txt", 6, 1_700_000_000_123);
        assert_eq!(file.id, "3b6e9cfa0638699ff9f954594602518645ec38a0");
        assert_eq!(file.sec_updated(), 1_700_000_000);
    }

    #[test]
    fn index_json_uses_go_field_names() {
        let value = serde_json::to_value(Index::fixture()).unwrap();
        assert!(value.get("systemID").is_some());
        assert!(value.get("systemName").is_some());
        assert!(value.get("systemOS").is_some());
        assert!(value.get("checkIndexID").is_some());
        assert!(value.get("aesKeyVerifyVal").is_some());
        assert!(value.get("system_id").is_none());
    }

    #[test]
    fn sha1_hex_matches_known_digest() {
        assert_eq!(
            super::sha1_hex(b"QingYu"),
            "df129aa05f45aafefc227d0e1726b7971155d6e4"
        );
    }

    #[test]
    fn random_hash_returns_a_sha1_identifier() {
        let hash = super::random_hash().unwrap();
        assert_eq!(hash.len(), 40);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn merge_result_reports_whether_file_data_changed() {
        let mut result = super::MergeResult {
            time: time::OffsetDateTime::UNIX_EPOCH,
            upserts: Vec::new(),
            removes: Vec::new(),
            conflicts: Vec::new(),
        };
        assert!(!result.data_changed());

        result
            .upserts
            .push(File::new("/changed.txt", 1, 1_700_000_000_123));
        assert!(result.data_changed());
    }
}
