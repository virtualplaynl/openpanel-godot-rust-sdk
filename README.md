# [OpenPanel](https://openpanel.dev/) Rust SDK for use with Godot Engine and godot-rust

## Features

- Track events
- Identify users
- Increment and decrement properties
- Filter events

## Usage

> [!CAUTION]
> This documentation is not updated for this Godot version yet. Do not use like this!
> As of the time of writing, only up to godot-rust 0.3.5 is tested.

Set your env vars in `.env` file:

```
OPENPANEL_TRACK_URL=https://api.openpanel.dev/track
OPENPANEL_CLIENT_ID=<YOUR_CLIENT_ID>
OPENPANEL_CLIENT_SECRET=<YOUR_CLIENT_SECRET>
```

as shown in [.env_sample](.env_sample)

### Track events

Simple example of tracking an event:

```rust
use openpanel_sdk::sdk::Tracker;


async fn can_track_event() -> anyhow::Result<()> {
    let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
    let mut properties = HashMap::new();

    properties.insert("name".to_string(), "rust".to_string());

    let response = tracker
        .track("test_event".to_string(), None, Some(properties), None)
        .await?;

    assert_eq!(response.status(), 200);

    Ok(())
}
```

### Identify users

Simple way to identify users:

```rust
async fn can_identify_user() -> anyhow::Result<()> {
    let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
    let response = tracker.identify(user.into()).await?;

    assert_eq!(response.status(), 200);
}
```

To make this work, `AppUser` needs to be converted into a `user::IdentifyUser` by implementing the `From` trait:

```rust
struct Address {
    pub street: String,
    pub city: String,
    pub zip: String,
}

struct AppUser {
    pub id: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub address: Address,
}

impl From<Address> for HashMap<String, String> {
    fn from(address: Address) -> Self {
        let mut properties = HashMap::new();

        properties.insert("street".to_string(), address.street);
        properties.insert("city".to_string(), address.city);
        properties.insert("zip".to_string(), address.zip);

        properties
    }
}

impl From<AppUser> for user::IdentifyUser {
    fn from(app_user: AppUser) -> Self {
        Self {
            profile_id: app_user.id,
            email: app_user.email,
            first_name: app_user.first_name,
            last_name: app_user.last_name,
            properties: app_user.address.into(),
        }
    }
}
```

### Filtering events

Filters are used to prevent sending events to OpenPanel in certain cases.
You can filter events by passing a `filter` function to the `track` method:

```rust
async fn can_apply_filter_track_event() -> anyhow::Result<()> {
    let filter = |properties: HashMap<String, String>| properties.contains_key("name");
    let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
    let mut properties = HashMap::new();

    properties.insert("name".to_string(), "rust".to_string());

    let response = tracker
        .track("test_event".to_string(), Some(properties), Some(&filter))
        .await;

    // Event won't be sent to OpenPanel, so an Err is returned
    assert!(response.is_err());

    Ok(())
}
```

### Revenue tracking

Revenue tracking is done easily:

```rust
async fn can_track_revenue() -> anyhow::Result<()> {
    let tracker = Tracker::try_new_from_env()?.with_default_headers()?;
    let response = tracker.revenue(100, None).await?;

    assert_eq!(response.status(), 200);

    Ok(())
}
```

If you need to add the device ID, fetch it like this before passing it to the `revenue` method:

```rust
async fn can_fetch_device_id() -> anyhow::Result<()> {
    let tracker = Tracker::try_new_from_env()?
        .with_default_headers()?
        .with_header("user-agent".to_string(), "some".to_string())?;
    let id = tracker.fetch_device_id().await?;

    assert!(!id.is_empty());

    Ok(())
}
```

For more examples, see the [tests](tests) directory.

## Testing

run `cargo test`
