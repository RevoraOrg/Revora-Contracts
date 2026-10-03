#![cfg(test)]

use crate::{assert_semver_forward, RevoraError};

#[test]
fn test_assert_semver_forward_valid() {
    // Valid upgrades (major)
    assert_eq!(assert_semver_forward((1, 0, 0), (2, 0, 0)), Ok(()));
    assert_eq!(assert_semver_forward((1, 5, 2), (2, 0, 0)), Ok(()));
    
    // Valid upgrades (minor)
    assert_eq!(assert_semver_forward((1, 0, 0), (1, 1, 0)), Ok(()));
    assert_eq!(assert_semver_forward((1, 1, 5), (1, 2, 0)), Ok(()));

    // Valid upgrades (patch)
    assert_eq!(assert_semver_forward((1, 0, 0), (1, 0, 1)), Ok(()));
    assert_eq!(assert_semver_forward((1, 1, 1), (1, 1, 2)), Ok(()));
}

#[test]
fn test_assert_semver_forward_invalid_same() {
    // Exact same version
    assert_eq!(
        assert_semver_forward((1, 0, 0), (1, 0, 0)),
        Err(RevoraError::AlreadyAtTargetVersion)
    );
    assert_eq!(
        assert_semver_forward((0, 0, 0), (0, 0, 0)),
        Err(RevoraError::AlreadyAtTargetVersion)
    );
    assert_eq!(
        assert_semver_forward((u32::MAX, u32::MAX, u32::MAX), (u32::MAX, u32::MAX, u32::MAX)),
        Err(RevoraError::AlreadyAtTargetVersion)
    );
}

#[test]
fn test_assert_semver_forward_invalid_downgrade() {
    // Downgrade (major)
    assert_eq!(
        assert_semver_forward((2, 0, 0), (1, 0, 0)),
        Err(RevoraError::MigrationDowngradeNotAllowed)
    );
    assert_eq!(
        assert_semver_forward((2, 0, 0), (1, 9, 9)),
        Err(RevoraError::MigrationDowngradeNotAllowed)
    );

    // Downgrade (minor)
    assert_eq!(
        assert_semver_forward((1, 2, 0), (1, 1, 0)),
        Err(RevoraError::MigrationDowngradeNotAllowed)
    );
    assert_eq!(
        assert_semver_forward((1, 2, 0), (1, 1, 9)),
        Err(RevoraError::MigrationDowngradeNotAllowed)
    );

    // Downgrade (patch)
    assert_eq!(
        assert_semver_forward((1, 0, 2), (1, 0, 1)),
        Err(RevoraError::MigrationDowngradeNotAllowed)
    );
}

#[test]
fn test_assert_semver_forward_boundary() {
    // From 0.0.0
    assert_eq!(assert_semver_forward((0, 0, 0), (0, 0, 1)), Ok(()));
    assert_eq!(assert_semver_forward((0, 0, 0), (0, 1, 0)), Ok(()));
    assert_eq!(assert_semver_forward((0, 0, 0), (1, 0, 0)), Ok(()));

    // To max
    assert_eq!(assert_semver_forward((0, 0, 0), (u32::MAX, u32::MAX, u32::MAX)), Ok(()));
    assert_eq!(assert_semver_forward((u32::MAX, u32::MAX, u32::MAX - 1), (u32::MAX, u32::MAX, u32::MAX)), Ok(()));
    
    // Max version downgrade
    assert_eq!(
        assert_semver_forward((u32::MAX, u32::MAX, u32::MAX), (u32::MAX, u32::MAX, u32::MAX - 1)),
        Err(RevoraError::MigrationDowngradeNotAllowed)
    );
}
