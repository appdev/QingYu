use qingyu_dejavu::{UPSTREAM_DEJAVU_COMMIT, UPSTREAM_SIYUAN_COMMIT};

#[test]
fn source_baselines_are_pinned() {
    assert_eq!(
        UPSTREAM_DEJAVU_COMMIT,
        "8462fe30163c6e6e95ae2da832cfe76058e0e830"
    );
    assert_eq!(
        UPSTREAM_SIYUAN_COMMIT,
        "eef10568384e2e7cf547adb029ae46a72e43c287"
    );
}
