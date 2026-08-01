use std::{fs, path::Path};

use argon2::{
    password_hash::{PasswordHasher as _, SaltString},
    Algorithm, Argon2, Params, Version,
};
use qingyu_kernel::{
    paths::KernelPaths,
    server::{
        OwnerPasswordInitializationError, OwnerPasswordRehash, OwnerPasswordUpdateError,
        OwnerPasswordVerification, ServerAuthenticationStatus, ServerAuthenticationStore,
    },
};
use tempfile::tempdir;

const OWNER_PASSWORD: &str = "Correct-Horse-Battery-Staple!7";

fn fixture_paths(root: &Path) -> KernelPaths {
    let workspace = root.join("workspace");
    let config = root.join("config");
    let cache = root.join("cache");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&cache).unwrap();
    KernelPaths::desktop(&workspace, &config, &cache).unwrap()
}

#[test]
fn owner_password_initialization_is_durable_one_time_and_argon2id_hashed() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let store = ServerAuthenticationStore::open(paths.config_root()).unwrap();

    assert_eq!(
        store.status().unwrap(),
        ServerAuthenticationStatus::NeedsInitialization
    );
    store
        .initialize_owner_password(OWNER_PASSWORD.to_owned())
        .unwrap();
    assert_eq!(store.status().unwrap(), ServerAuthenticationStatus::Ready);
    assert_eq!(
        store.verify_owner_password(OWNER_PASSWORD).unwrap(),
        OwnerPasswordVerification::Authorized {
            needs_rehash: false
        }
    );
    assert_eq!(
        store
            .verify_owner_password("Incorrect-Owner-Password!9")
            .unwrap(),
        OwnerPasswordVerification::Rejected
    );

    let serialized =
        fs::read_to_string(temporary.path().join("config/owner-auth-v1.json")).unwrap();
    assert!(serialized.contains("$argon2id$"));
    assert!(!serialized.contains(OWNER_PASSWORD));

    let reopened = ServerAuthenticationStore::open(paths.config_root()).unwrap();
    assert_eq!(
        reopened.status().unwrap(),
        ServerAuthenticationStatus::Ready
    );
    assert_eq!(
        reopened
            .initialize_owner_password("Another-Sufficiently-Long-Password!2".to_owned())
            .unwrap_err(),
        OwnerPasswordInitializationError::AlreadyInitialized
    );
    assert_eq!(
        reopened.verify_owner_password(OWNER_PASSWORD).unwrap(),
        OwnerPasswordVerification::Authorized {
            needs_rehash: false
        }
    );
}

#[test]
fn valid_legacy_argon2_parameters_are_authorized_but_require_rehash() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let weak = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(4096, 1, 1, Some(32)).unwrap(),
    );
    let salt = SaltString::encode_b64(&[7_u8; 16]).unwrap();
    let hash = weak
        .hash_password(OWNER_PASSWORD.as_bytes(), &salt)
        .unwrap()
        .to_string();
    fs::write(
        temporary.path().join("config/owner-auth-v1.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "passwordHash": hash,
        }))
        .unwrap(),
    )
    .unwrap();

    let store = ServerAuthenticationStore::open(paths.config_root()).unwrap();
    assert_eq!(
        store.verify_owner_password(OWNER_PASSWORD).unwrap(),
        OwnerPasswordVerification::Authorized { needs_rehash: true }
    );
}

#[test]
fn legacy_password_hash_is_replaced_with_the_current_policy_using_the_verified_password() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let weak = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(4096, 1, 1, Some(32)).unwrap(),
    );
    let salt = SaltString::encode_b64(&[7_u8; 16]).unwrap();
    let hash = weak
        .hash_password(OWNER_PASSWORD.as_bytes(), &salt)
        .unwrap()
        .to_string();
    let authentication_file = temporary.path().join("config/owner-auth-v1.json");
    fs::write(
        &authentication_file,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "passwordHash": hash,
        }))
        .unwrap(),
    )
    .unwrap();
    let original = fs::read(&authentication_file).unwrap();

    let store = ServerAuthenticationStore::open(paths.config_root()).unwrap();
    assert_eq!(
        store
            .rehash_owner_password("Incorrect-Owner-Password-Material!9".to_owned())
            .unwrap_err(),
        OwnerPasswordUpdateError::InvalidCurrentPassword
    );
    assert_eq!(fs::read(&authentication_file).unwrap(), original);
    assert_eq!(
        store
            .rehash_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap(),
        OwnerPasswordRehash::Updated
    );
    assert_eq!(
        store.verify_owner_password(OWNER_PASSWORD).unwrap(),
        OwnerPasswordVerification::Authorized {
            needs_rehash: false
        }
    );
    let updated = fs::read_to_string(authentication_file).unwrap();
    assert!(updated.contains("m=19456,t=2,p=1"));
    assert!(!updated.contains(OWNER_PASSWORD));
}

#[test]
fn owner_password_change_is_atomic_and_requires_the_current_password_and_a_valid_replacement() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let store = ServerAuthenticationStore::open(paths.config_root()).unwrap();
    store
        .initialize_owner_password(OWNER_PASSWORD.to_owned())
        .unwrap();
    let authentication_file = temporary.path().join("config/owner-auth-v1.json");
    let original = fs::read(&authentication_file).unwrap();

    assert_eq!(
        store
            .change_owner_password(
                "Incorrect-Owner-Password-Material!9".to_owned(),
                "has space".to_owned(),
            )
            .unwrap_err(),
        OwnerPasswordUpdateError::InvalidCurrentPassword
    );
    assert_eq!(fs::read(&authentication_file).unwrap(), original);
    assert_eq!(
        store
            .change_owner_password(OWNER_PASSWORD.to_owned(), "has space".to_owned())
            .unwrap_err(),
        OwnerPasswordUpdateError::InvalidNewPassword
    );
    assert_eq!(fs::read(&authentication_file).unwrap(), original);

    store
        .change_owner_password(OWNER_PASSWORD.to_owned(), "!".to_owned())
        .unwrap();
    assert_eq!(
        store.verify_owner_password("!").unwrap(),
        OwnerPasswordVerification::Authorized {
            needs_rehash: false
        }
    );

    store
        .change_owner_password("!".to_owned(), "New-Owner-Password-Material!8".to_owned())
        .unwrap();
    assert_eq!(
        store.verify_owner_password(OWNER_PASSWORD).unwrap(),
        OwnerPasswordVerification::Rejected
    );
    assert_eq!(
        store
            .verify_owner_password("New-Owner-Password-Material!8")
            .unwrap(),
        OwnerPasswordVerification::Authorized {
            needs_rehash: false
        }
    );
}

#[test]
fn owner_passwords_accept_only_printable_non_space_ascii() {
    for candidate in [
        "",
        " ",
        " leading",
        "trailing ",
        "middle space",
        "\t",
        "\n",
        "\0",
        "\u{7f}",
        "\u{a0}",
        "中文",
        "Ａ",
        "😀",
    ] {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        let store = ServerAuthenticationStore::open(paths.config_root()).unwrap();
        assert_eq!(
            store
                .initialize_owner_password(candidate.to_owned())
                .unwrap_err(),
            OwnerPasswordInitializationError::InvalidPassword,
            "candidate {candidate:?} must be rejected",
        );
    }

    for candidate in ["!", "A", "0", "~", "Owner-Secret!1"] {
        let temporary = tempdir().unwrap();
        let paths = fixture_paths(temporary.path());
        ServerAuthenticationStore::open(paths.config_root())
            .unwrap()
            .initialize_owner_password(candidate.to_owned())
            .unwrap();
    }

    let accepted = tempdir().unwrap();
    let accepted_paths = fixture_paths(accepted.path());
    ServerAuthenticationStore::open(accepted_paths.config_root())
        .unwrap()
        .initialize_owner_password("x".repeat(1024))
        .unwrap();

    let rejected = tempdir().unwrap();
    let rejected_paths = fixture_paths(rejected.path());
    assert_eq!(
        ServerAuthenticationStore::open(rejected_paths.config_root())
            .unwrap()
            .initialize_owner_password("x".repeat(1025))
            .unwrap_err(),
        OwnerPasswordInitializationError::InvalidPassword,
    );
}

#[test]
fn malformed_persistent_state_fails_closed_instead_of_reentering_initialization() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    fs::write(
        temporary.path().join("config/owner-auth-v1.json"),
        br#"{"schemaVersion":1,"passwordHash":"not-a-password-hash"}"#,
    )
    .unwrap();

    assert!(ServerAuthenticationStore::open(paths.config_root()).is_err());
}

#[test]
fn replacing_the_retained_config_directory_fails_closed() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let store = ServerAuthenticationStore::open(paths.config_root()).unwrap();
    fs::rename(
        temporary.path().join("config"),
        temporary.path().join("displaced-config"),
    )
    .unwrap();
    fs::create_dir(temporary.path().join("config")).unwrap();

    assert!(store.status().is_err());
    assert_eq!(
        store
            .initialize_owner_password(OWNER_PASSWORD.to_owned())
            .unwrap_err(),
        OwnerPasswordInitializationError::StateUnavailable
    );
    assert!(!temporary.path().join("config/owner-auth-v1.json").exists());
}

#[cfg(unix)]
#[test]
fn symlinked_owner_state_fails_closed_without_reading_its_target() {
    use std::os::unix::fs::symlink;

    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let target = temporary.path().join("outside-secret.json");
    fs::write(
        &target,
        br#"{"schemaVersion":1,"passwordHash":"outside-secret"}"#,
    )
    .unwrap();
    symlink(&target, temporary.path().join("config/owner-auth-v1.json")).unwrap();

    let error = ServerAuthenticationStore::open(paths.config_root()).unwrap_err();
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains("outside-secret"));
    assert!(!rendered.contains(target.to_string_lossy().as_ref()));
}

#[cfg(unix)]
#[test]
fn owner_state_is_private_to_the_container_user() {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let store = ServerAuthenticationStore::open(paths.config_root()).unwrap();
    store
        .initialize_owner_password(OWNER_PASSWORD.to_owned())
        .unwrap();

    let mode = fs::metadata(temporary.path().join("config/owner-auth-v1.json"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn authentication_store_debug_and_errors_never_expose_passwords_or_paths() {
    let temporary = tempdir().unwrap();
    let paths = fixture_paths(temporary.path());
    let store = ServerAuthenticationStore::open(paths.config_root()).unwrap();
    store
        .initialize_owner_password(OWNER_PASSWORD.to_owned())
        .unwrap();

    let rejected = store
        .verify_owner_password("Rejected-Owner-Password-Material!0")
        .unwrap();
    assert_eq!(rejected, OwnerPasswordVerification::Rejected);
    let rendered = format!("{store:?} {rejected:?}");
    assert!(!rendered.contains(OWNER_PASSWORD));
    assert!(!rendered.contains("Rejected-Owner-Password-Material!0"));
    assert!(!rendered.contains(temporary.path().to_string_lossy().as_ref()));

    let update_error = store
        .change_owner_password(
            "rejected current password material".to_owned(),
            "new secret password material".to_owned(),
        )
        .unwrap_err();
    let update_rendered = format!("{update_error:?} {update_error}");
    assert!(!update_rendered.contains("rejected current password material"));
    assert!(!update_rendered.contains("new secret password material"));
}
