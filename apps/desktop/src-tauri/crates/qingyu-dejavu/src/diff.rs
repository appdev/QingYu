use std::collections::HashMap;

use crate::entity::File;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexFileDiff {
    pub adds_left: Vec<File>,
    pub updates_left: Vec<File>,
    pub updates_right: Vec<File>,
    pub removes_right: Vec<File>,
}

pub fn diff_upsert_remove(left: &[File], right: &[File]) -> (Vec<File>, Vec<File>) {
    let left_by_path = files_by_path(left);
    let right_by_path = files_by_path(right);
    let mut upserts = Vec::new();
    let mut removes = Vec::new();

    for (path, left_file) in &left_by_path {
        match right_by_path.get(path) {
            Some(right_file) if equal_file(left_file, right_file) => {}
            Some(_) | None => upserts.push((*left_file).clone()),
        }
    }

    for (path, right_file) in &right_by_path {
        if !left_by_path.contains_key(path) {
            removes.push((*right_file).clone());
        }
    }

    sort_files(&mut upserts);
    sort_files(&mut removes);
    (upserts, removes)
}

pub fn diff_index_files(left: &[File], right: &[File]) -> IndexFileDiff {
    let left_by_path = files_by_path(left);
    let right_by_path = files_by_path(right);
    let mut diff = IndexFileDiff::default();

    for (path, left_file) in &left_by_path {
        match right_by_path.get(path) {
            None => diff.adds_left.push((*left_file).clone()),
            Some(right_file) if !equal_file(left_file, right_file) => {
                diff.updates_left.push((*left_file).clone());
                diff.updates_right.push((**right_file).clone());
            }
            Some(_) => {}
        }
    }

    for (path, right_file) in &right_by_path {
        if !left_by_path.contains_key(path) {
            diff.removes_right.push((*right_file).clone());
        }
    }

    sort_files(&mut diff.adds_left);
    sort_files(&mut diff.updates_left);
    sort_files(&mut diff.updates_right);
    sort_files(&mut diff.removes_right);
    diff
}

fn equal_file(left: &File, right: &File) -> bool {
    left.path == right.path && left.sec_updated() == right.sec_updated()
}

fn files_by_path(files: &[File]) -> HashMap<&str, &File> {
    files.iter().map(|file| (file.path.as_str(), file)).collect()
}

fn sort_files(files: &mut [File]) {
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.id.cmp(&right.id))
    });
}

#[cfg(test)]
mod tests {
    use super::{diff_index_files, diff_upsert_remove};
    use crate::entity::File;

    fn file(path: &str, updated: i64) -> File {
        File::new(path, 1, updated)
    }

    #[test]
    fn equal_second_timestamps_are_equal() {
        let left = vec![file("/doc.txt", 1_700_000_000_999)];
        let right = vec![file("/doc.txt", 1_700_000_000_123)];

        let (upserts, removes) = diff_upsert_remove(&left, &right);
        assert!(upserts.is_empty());
        assert!(removes.is_empty());

        let index_diff = diff_index_files(&left, &right);
        assert!(index_diff.adds_left.is_empty());
        assert!(index_diff.updates_left.is_empty());
        assert!(index_diff.updates_right.is_empty());
        assert!(index_diff.removes_right.is_empty());
    }

    #[test]
    fn different_second_timestamps_are_upserts() {
        let left_file = file("/doc.txt", 1_700_000_001_123);
        let right_file = file("/doc.txt", 1_700_000_000_123);

        let (upserts, removes) =
            diff_upsert_remove(std::slice::from_ref(&left_file), std::slice::from_ref(&right_file));
        assert_eq!(upserts, vec![left_file.clone()]);
        assert!(removes.is_empty());

        let index_diff = diff_index_files(&[left_file.clone()], &[right_file.clone()]);
        assert_eq!(index_diff.updates_left, vec![left_file]);
        assert_eq!(index_diff.updates_right, vec![right_file]);
    }

    #[test]
    fn path_removal_appears_only_in_removes() {
        let removed = file("/gone.txt", 1_700_000_000_123);

        let (upserts, removes) = diff_upsert_remove(&[], std::slice::from_ref(&removed));
        assert!(upserts.is_empty());
        assert_eq!(removes, vec![removed.clone()]);

        let index_diff = diff_index_files(&[], std::slice::from_ref(&removed));
        assert!(index_diff.adds_left.is_empty());
        assert!(index_diff.updates_left.is_empty());
        assert!(index_diff.updates_right.is_empty());
        assert_eq!(index_diff.removes_right, vec![removed]);
    }

    #[test]
    fn diff_results_are_sorted_by_path() {
        let left = vec![
            file("/z-new.txt", 1_700_000_000_123),
            file("/b-new.txt", 1_700_000_000_123),
            file("/z-updated.txt", 1_700_000_001_123),
            file("/b-updated.txt", 1_700_000_001_123),
        ];
        let right = vec![
            file("/z-removed.txt", 1_700_000_000_123),
            file("/b-removed.txt", 1_700_000_000_123),
            file("/z-updated.txt", 1_700_000_000_123),
            file("/b-updated.txt", 1_700_000_000_123),
        ];

        let (upserts, removes) = diff_upsert_remove(&left, &right);
        assert_eq!(
            upserts.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(),
            vec!["/b-new.txt", "/b-updated.txt", "/z-new.txt", "/z-updated.txt"]
        );
        assert_eq!(
            removes.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(),
            vec!["/b-removed.txt", "/z-removed.txt"]
        );

        let index_diff = diff_index_files(&left, &right);
        assert_eq!(
            index_diff
                .adds_left
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/b-new.txt", "/z-new.txt"]
        );
        assert_eq!(
            index_diff
                .updates_left
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/b-updated.txt", "/z-updated.txt"]
        );
        assert_eq!(
            index_diff
                .updates_right
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/b-updated.txt", "/z-updated.txt"]
        );
        assert_eq!(
            index_diff
                .removes_right
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/b-removed.txt", "/z-removed.txt"]
        );
    }
}
