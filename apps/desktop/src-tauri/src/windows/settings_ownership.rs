use tauri::{PhysicalPosition, PhysicalSize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExistingSettingsAction {
    FocusExisting,
    RecreateHidden,
    ReuseHidden,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SettingsWindowOwnership {
    pub(crate) owner_label: Option<String>,
    pub(crate) owner_last_position: Option<PhysicalPosition<i32>>,
    pub(crate) relative_offset: Option<PhysicalPosition<i32>>,
    pub(crate) pending_owner_destroy: Option<String>,
}

impl SettingsWindowOwnership {
    pub(crate) fn begin_owner_destroy(&mut self, label: &str) -> bool {
        if self.owner_label.as_deref() != Some(label) {
            return false;
        }
        if self.pending_owner_destroy.as_deref() == Some(label) {
            return true;
        }
        if self.pending_owner_destroy.is_some() {
            return false;
        }
        self.pending_owner_destroy = Some(label.to_string());
        true
    }

    pub(crate) fn cancel_owner_destroy(&mut self) {
        self.pending_owner_destroy = None;
    }

    pub(crate) fn take_pending_owner_destroy(&mut self) -> Option<String> {
        self.pending_owner_destroy.take()
    }
}

fn clamp_i64(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

pub(crate) fn existing_settings_action(
    existing_owner: Option<&str>,
    requesting_owner: &str,
    visible: bool,
) -> ExistingSettingsAction {
    if visible {
        return ExistingSettingsAction::FocusExisting;
    }
    if existing_owner == Some(requesting_owner) {
        return ExistingSettingsAction::ReuseHidden;
    }
    ExistingSettingsAction::RecreateHidden
}

pub(crate) fn centered_child_position(
    owner_position: PhysicalPosition<i32>,
    owner_size: PhysicalSize<u32>,
    child_size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    let x = i64::from(owner_position.x)
        + (i64::from(owner_size.width) - i64::from(child_size.width)) / 2;
    let y = i64::from(owner_position.y)
        + (i64::from(owner_size.height) - i64::from(child_size.height)) / 2;
    PhysicalPosition::new(clamp_i64(x), clamp_i64(y))
}

pub(crate) fn relative_offset(
    owner_position: PhysicalPosition<i32>,
    child_position: PhysicalPosition<i32>,
) -> PhysicalPosition<i32> {
    PhysicalPosition::new(
        child_position.x.saturating_sub(owner_position.x),
        child_position.y.saturating_sub(owner_position.y),
    )
}

pub(crate) fn position_from_offset(
    owner_position: PhysicalPosition<i32>,
    offset: PhysicalPosition<i32>,
) -> PhysicalPosition<i32> {
    PhysicalPosition::new(
        owner_position.x.saturating_add(offset.x),
        owner_position.y.saturating_add(offset.y),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        centered_child_position, existing_settings_action, position_from_offset, relative_offset,
        ExistingSettingsAction,
    };
    use tauri::{PhysicalPosition, PhysicalSize};

    #[test]
    fn visible_singleton_is_focused_even_for_another_editor() {
        assert_eq!(
            existing_settings_action(Some("main"), "markra-editor-2", true),
            ExistingSettingsAction::FocusExisting
        );
    }

    #[test]
    fn hidden_singleton_is_recreated_only_for_another_owner() {
        assert_eq!(
            existing_settings_action(Some("main"), "markra-editor-2", false),
            ExistingSettingsAction::RecreateHidden
        );
        assert_eq!(
            existing_settings_action(Some("main"), "main", false),
            ExistingSettingsAction::ReuseHidden
        );
    }

    #[test]
    fn centers_child_and_preserves_user_selected_offset() {
        let owner = PhysicalPosition::new(100, 200);
        assert_eq!(
            centered_child_position(
                owner,
                PhysicalSize::new(1400, 900),
                PhysicalSize::new(1000, 700)
            ),
            PhysicalPosition::new(300, 300)
        );

        let child = PhysicalPosition::new(360, 340);
        let offset = relative_offset(owner, child);
        assert_eq!(offset, PhysicalPosition::new(260, 140));
        assert_eq!(
            position_from_offset(PhysicalPosition::new(180, 260), offset),
            PhysicalPosition::new(440, 400)
        );
    }

    #[test]
    fn owner_move_uses_the_saved_relative_offset() {
        let owner = PhysicalPosition::new(500, 400);
        let offset = PhysicalPosition::new(120, 80);
        assert_eq!(
            position_from_offset(owner, offset),
            PhysicalPosition::new(620, 480)
        );
    }

    #[test]
    fn pending_owner_destroy_is_scoped_to_the_settings_owner() {
        let mut ownership = super::SettingsWindowOwnership {
            owner_label: Some("main".to_string()),
            ..super::SettingsWindowOwnership::default()
        };

        assert!(ownership.begin_owner_destroy("main"));
        assert!(ownership.begin_owner_destroy("main"));
        assert!(!ownership.begin_owner_destroy("markra-editor-2"));
        assert_eq!(
            ownership.take_pending_owner_destroy(),
            Some("main".to_string())
        );
        assert_eq!(ownership.take_pending_owner_destroy(), None);
    }
}
