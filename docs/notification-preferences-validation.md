# Notification Preferences Validation (#156)

## Overview

This document describes the Notification Preferences Validation feature implemented in the Revora smart contracts. This feature enables users to configure their notification preferences for revenue, claim, and eligibility-related events.

## Feature Description

### Notification Channels

Users can subscribe to multiple notification channels:

| Channel | Value | Description |
|---------|-------|-------------|
| OnChain | 0 | On-chain event emission only (default) |
| Email | 1 | Email notifications |
| Webhook | 2 | Webhook notifications |

### Notification Frequency

Users can choose how often they receive batched notifications:

| Frequency | Value | Description |
|------------|-------|-------------|
| Immediate | 0 | Receive notifications as they occur |
| Hourly | 1 | Hourly batch notifications |
| Daily | 2 | Daily batch notifications |
| Weekly | 3 | Weekly batch notifications |

### Notification Priority

Users can filter notifications by priority level:

| Priority | Value | Description |
|-----------|-------|-------------|
| Critical | 0 | Only critical notifications (large claims, system alerts) |
| Standard | 1 | Standard notifications including regular distributions |
| All | 2 | All notifications including minor updates |

### Notification Types

Users can enable/disable the following notification types:

- **Revenue Notifications**: Revenue reports and distribution events
- **Claim Notifications**: Claim availability and claim success events
- **Eligibility Notifications**: Blacklist/whitelist change events

## Validation Rules

The following validation rules are enforced:

1. **At least one channel must be enabled**
   - Error: `NotificationPreferencesInvalid`

2. **At least one notification type must be enabled**
   - Error: `NotificationPreferencesInvalid`

3. **Batch size constraints**
   - Minimum: 1
   - Maximum: 1000
   - Error: `NotificationBatchSizeExceeded`

4. **Webhook URL length constraint**
   - Maximum length: 2048 bytes
   - Error: `NotificationPreferencesInvalid`

## Storage

Notification preferences are stored per-user address under the `NotificationPrefs` key.

## Contract Methods

### Core Methods

```rust
// Set all notification preferences at once
pub fn set_notification_preferences(
    env: Env,
    user: Address,
    channels: Vec<NotificationChannel>,
    frequency: NotificationFrequency,
    priority: NotificationPriority,
    revenue_notifications: bool,
    claim_notifications: bool,
    eligibility_notifications: bool,
    max_batch_size: u32,
    webhook_url: Option<String>,
) -> Result<(), RevoraError>

// Get current notification preferences (returns defaults if not set)
pub fn get_notification_preferences(env: Env, user: Address) -> NotificationPreferences
```

### Channel Management

```rust
// Add a notification channel
pub fn add_notification_channel(
    env: Env,
    user: Address,
    channel: NotificationChannel,
) -> Result<(), RevoraError>

// Remove a notification channel
pub fn remove_notification_channel(
    env: Env,
    user: Address,
    channel: NotificationChannel,
) -> Result<(), RevoraError>
```

### Individual Preference Updates

```rust
// Update notification frequency
pub fn set_notification_frequency(
    env: Env,
    user: Address,
    frequency: NotificationFrequency,
) -> Result<(), RevoraError>

// Update notification priority
pub fn set_notification_priority(
    env: Env,
    user: Address,
    priority: NotificationPriority,
) -> Result<(), RevoraError>

// Update enabled notification types
pub fn set_notification_types(
    env: Env,
    user: Address,
    revenue_notifications: bool,
    claim_notifications: bool,
    eligibility_notifications: bool,
) -> Result<(), RevoraError>

// Update batch size
pub fn set_notification_batch_size(
    env: Env,
    user: Address,
    max_batch_size: u32,
) -> Result<(), RevoraError>

// Update webhook URL
pub fn set_webhook_url(
    env: Env,
    user: Address,
    webhook_url: Option<String>,
) -> Result<(), RevoraError>
```

### Helper Methods

```rust
// Check if a notification type is enabled
pub fn has_notification_type_enabled(
    env: Env,
    user: Address,
    notification_type: u32,  // 0=revenue, 1=claim, 2=eligibility
) -> bool

// Check if a channel is enabled
pub fn has_notification_channel_enabled(
    env: Env,
    user: Address,
    channel: NotificationChannel,
) -> bool

// Check if custom preferences exist
pub fn has_custom_notif_prefs(env: Env, user: Address) -> bool

// Clear preferences (revert to defaults)
pub fn clear_notification_preferences(env: Env, user: Address)
```

## Error Codes

| Error | Code | Description |
|-------|------|-------------|
| InvalidNotificationChannel | 30 | Invalid or unsupported notification channel |
| NotificationFrequencyTooHigh | 31 | Frequency exceeds maximum allowed interval |
| NotificationPreferencesInvalid | 32 | Validation failed for notification preferences |
| NotificationChannelAlreadySubscribed | 33 | Channel is already enabled |
| NotificationChannelNotFound | 34 | Channel not found for removal |
| NotificationBatchSizeExceeded | 35 | Batch size is out of allowed range |
| InvalidNotificationPriority | 36 | Invalid notification priority level |

## Events

The following events are emitted:

| Event | Symbol | Description |
|-------|--------|-------------|
| Notification Preferences Set | `notif_set` | User's full preferences were updated |
| Channel Added | `notif_add` | A notification channel was added |
| Channel Removed | `notif_rem` | A notification channel was removed |
| Batch Size Updated | `notif_bup` | Maximum batch size was changed |
| Priority Set | `notif_pri` | Notification priority was changed |

## Security Considerations

1. **Authentication**: All write operations require `require_auth()`, ensuring only the user can modify their preferences.

2. **Input Validation**: All inputs are validated before storage to prevent invalid state.

3. **Frozen Contract**: Preferences cannot be modified when the contract is frozen.

4. **Batch Size Limits**: Prevents resource exhaustion attacks via excessive batch sizes.

5. **URL Length Limits**: Prevents storage exhaustion via excessively long webhook URLs.

## Usage Example

```rust
// Set comprehensive notification preferences
let channels = vec![&env, NotificationChannel::OnChain, NotificationChannel::Email];
client.set_notification_preferences(
    &user,
    &channels,
    &NotificationFrequency::Daily,
    &NotificationPriority::Standard,
    &true,   // revenue_notifications
    &true,   // claim_notifications
    &false,  // eligibility_notifications
    &500,    // max_batch_size
    &Some(webhook_url),
)?;
```

## Default Preferences

When a user has not set custom preferences, the following defaults are returned:

- **Channel**: OnChain
- **Frequency**: Immediate
- **Priority**: Standard
- **All notification types**: Enabled
- **Batch size**: 100
- **Webhook URL**: None
