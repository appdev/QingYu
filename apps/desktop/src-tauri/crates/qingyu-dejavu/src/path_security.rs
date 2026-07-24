#[cfg(windows)]
const WINDOWS_REPARSE_POINT_ATTRIBUTE: u32 =
    windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
#[cfg(all(not(windows), test))]
const WINDOWS_REPARSE_POINT_ATTRIBUTE: u32 = 0x400;

#[cfg(any(test, windows))]
pub(crate) const fn windows_attributes_are_reparse(attributes: u32) -> bool {
    attributes & WINDOWS_REPARSE_POINT_ATTRIBUTE != 0
}

pub(crate) fn cap_metadata_is_reparse(metadata: &cap_std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;

        windows_attributes_are_reparse(metadata.file_attributes())
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

pub(crate) fn std_metadata_is_reparse(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        windows_attributes_are_reparse(metadata.file_attributes())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::windows_attributes_are_reparse;

    #[test]
    fn windows_reparse_attribute_detection_is_independent_of_reparse_kind() {
        assert!(windows_attributes_are_reparse(0x400));
        assert!(windows_attributes_are_reparse(0x402));
        assert!(!windows_attributes_are_reparse(0));
        assert!(!windows_attributes_are_reparse(0x2));
    }
}
