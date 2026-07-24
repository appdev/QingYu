use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use scrypt::Params;

use crate::RepoError;

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

pub fn derive_key(password: &str, salt: &str) -> Result<[u8; 32], RepoError> {
    let params = Params::new(15, 8, 1, 32).map_err(|_| RepoError::KeyDerivationFailed)?;
    let mut key = [0_u8; 32];
    scrypt::scrypt(password.as_bytes(), salt.as_bytes(), &params, &mut key)
        .map_err(|_| RepoError::KeyDerivationFailed)?;
    Ok(key)
}

pub fn encrypt(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, RepoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| RepoError::EncryptionFailed)?;
    let mut nonce = [0_u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| RepoError::RandomnessUnavailable)?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|_| RepoError::EncryptionFailed)?;

    let mut encrypted = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    encrypted.extend_from_slice(&nonce);
    encrypted.extend_from_slice(&ciphertext);
    Ok(encrypted)
}

pub fn decrypt(encrypted: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, RepoError> {
    if encrypted.len() < NONCE_LEN + TAG_LEN {
        return Err(RepoError::DecryptionFailed);
    }

    let (nonce, ciphertext) = encrypted.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| RepoError::DecryptionFailed)?;
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| RepoError::DecryptionFailed)
}

#[cfg(test)]
mod tests {
    use super::{decrypt, derive_key, encrypt};
    use crate::{Index, RepoError};

    fn fixture_index() -> Index {
        Index {
            id: "89abcdef0123456789abcdef0123456789abcdef".to_owned(),
            memo: "Go golden oracle".to_owned(),
            created: 1_700_000_000_456,
            files: vec!["0123456789abcdef0123456789abcdef01234567".to_owned()],
            count: 1,
            size: 12,
            system_id: "oracle-system-id".to_owned(),
            system_name: "Oracle Device".to_owned(),
            system_os: "darwin".to_owned(),
            check_index_id: "fedcba9876543210fedcba9876543210fedcba98".to_owned(),
            aes_key_verify_val: String::new(),
        }
    }

    #[test]
    fn kdf_is_scrypt_32768_8_1_32() {
        let key = derive_key("oracle-password", "oracle-salt").unwrap();
        assert_eq!(key, *include_bytes!("../tests/fixtures/golden/kdf-key.bin"));
    }

    #[test]
    fn decrypts_pinned_go_aes_gcm_fixture() {
        let key = *include_bytes!("../tests/fixtures/golden/kdf-key.bin");
        let plaintext = decrypt(
            include_bytes!("../tests/fixtures/golden/aes-gcm-siyuan.bin"),
            &key,
        )
        .unwrap();

        assert_eq!(plaintext, b"siyuan");
    }

    #[test]
    fn encryption_emits_nonce_ciphertext_and_tag() {
        let key = *include_bytes!("../tests/fixtures/golden/kdf-key.bin");
        let encrypted = encrypt(b"siyuan", &key).unwrap();

        assert_eq!(encrypted.len(), 12 + 6 + 16);
        assert_eq!(decrypt(&encrypted, &key).unwrap(), b"siyuan");
    }

    #[test]
    fn wrong_key_is_a_typed_decryption_failure() {
        let error = decrypt(
            include_bytes!("../tests/fixtures/golden/aes-gcm-siyuan.bin"),
            &[0_u8; 32],
        )
        .unwrap_err();

        assert!(matches!(error, RepoError::DecryptionFailed));
    }

    #[test]
    fn truncated_ciphertext_is_rejected_without_slicing() {
        let key = *include_bytes!("../tests/fixtures/golden/kdf-key.bin");

        for length in 0..28 {
            let error = decrypt(&vec![0_u8; length], &key).unwrap_err();
            assert!(matches!(error, RepoError::DecryptionFailed));
        }
    }

    #[test]
    fn index_key_verification_matches_dejavu_semantics() {
        let key = *include_bytes!("../tests/fixtures/golden/kdf-key.bin");
        let mut index = fixture_index();

        assert!(index.verify_aes_key(&[0_u8; 32]));
        index.init_aes_key_verify_val(&key).unwrap();
        assert!(index.verify_aes_key(&key));
        assert!(!index.verify_aes_key(&[0_u8; 32]));
        index.aes_key_verify_val = "not base64".to_owned();
        assert!(!index.verify_aes_key(&key));
    }
}
