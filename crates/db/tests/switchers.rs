//! Database tests for switchers, stream servers and chat commands.

use std::collections::BTreeMap;

use db::{
    chat_commands::{self, ChatCommandRole},
    stream_servers::{self, Details, StreamServerKind},
    switchers::{self, Behaviour, SecretChange, StreamLink, Triggers},
    users, workspaces,
};
use sqlx::PgPool;
use uuid::Uuid;

async fn workspace(pool: &PgPool) -> Uuid {
    let owner = users::create(pool).await.unwrap();
    workspaces::create(pool, owner.id, "Nomad Live", "nomad-live")
        .await
        .unwrap()
        .id
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn switchers_are_limited_by_plan(pool: PgPool) {
    let workspace_id = workspace(&pool).await;

    let first = switchers::create_within_plan(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();
    assert!(first.is_some());
    // Free allows one switcher.
    let second = switchers::create_within_plan(&pool, workspace_id, "Bike cam", b"b")
        .await
        .unwrap();
    assert!(second.is_none());

    workspaces::set_plan(&pool, workspace_id, "pro")
        .await
        .unwrap();
    let second = switchers::create_within_plan(&pool, workspace_id, "Bike cam", b"b")
        .await
        .unwrap();
    assert!(second.is_some());

    let listed = switchers::list_summaries(&pool, workspace_id)
        .await
        .unwrap();
    assert_eq!(
        listed.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["Main", "Bike cam"]
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn stream_servers_are_limited_by_plan(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let switcher = switchers::create(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();
    let kind = StreamServerKind::Belabox;
    let none = Details::default();
    let add = |name: &'static str| {
        stream_servers::create_within_plan(&pool, switcher.id, name, kind, "https://a", &none)
    };

    // Free allows three per switcher.
    for name in ["A", "B", "C"] {
        assert!(add(name).await.unwrap().is_some());
    }
    assert!(add("D").await.unwrap().is_none());

    workspaces::set_plan(&pool, workspace_id, "pro")
        .await
        .unwrap();
    let d = add("D").await.unwrap().unwrap();
    assert_eq!(d.priority, 3);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn triggers_default_and_can_be_turned_off(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let switcher = switchers::create(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();

    // Defaults: LOW at 800 kbps and 2500 ms; the offline triggers are off.
    assert_eq!(switcher.trigger_low, Some(800));
    assert_eq!(switcher.trigger_offline, None);
    assert_eq!(switcher.trigger_rtt, Some(2500));
    assert_eq!(switcher.trigger_rtt_offline, None);

    let updated = switchers::set_triggers(
        &pool,
        switcher.id,
        Triggers {
            low: Some(1000),
            offline: Some(150),
            rtt: None,
            rtt_offline: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        (
            updated.trigger_low,
            updated.trigger_offline,
            updated.trigger_rtt
        ),
        (Some(1000), Some(150), None)
    );

    // 0 is rejected by the schema: NULL means off.
    let zero = Triggers {
        low: Some(0),
        offline: None,
        rtt: None,
        rtt_offline: None,
    };
    assert!(
        switchers::set_triggers(&pool, switcher.id, zero)
            .await
            .is_err()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn recording_scenes_default_empty_and_round_trip(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let switcher = switchers::create(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();
    assert!(switcher.recording_scene_ids.is_empty());

    let scenes = vec![Uuid::new_v4(), Uuid::new_v4()];
    let updated = switchers::set_behaviour(
        &pool,
        switcher.id,
        Behaviour {
            bitrate_switcher_enabled: switcher.bitrate_switcher_enabled,
            only_switch_when_streaming: switcher.only_switch_when_streaming,
            instantly_switch_on_recover: switcher.instantly_switch_on_recover,
            auto_switch_notification: switcher.auto_switch_notification,
            retry_attempts: switcher.retry_attempts,
            auto_stop_on_raid: switcher.auto_stop_on_raid,
            announce_raid: switcher.announce_raid,
            record_while_streaming: switcher.record_while_streaming,
            recording_scene_ids: scenes.clone(),
            switch_to_starting: switcher.switch_to_starting,
            switch_to_live: switcher.switch_to_live,
            offline_timeout: switcher.offline_timeout,
            ending_timeout: switcher.ending_timeout,
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.recording_scene_ids, scenes);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn obs_password_can_be_kept_cleared_or_replaced(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let switcher = switchers::create(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();

    let set = switchers::set_obs(&pool, switcher.id, 4455, SecretChange::Set(vec![1, 2, 3]))
        .await
        .unwrap();
    assert_eq!(set.password.as_deref(), Some([1, 2, 3].as_slice()));

    let kept = switchers::set_obs(&pool, switcher.id, 4456, SecretChange::Keep)
        .await
        .unwrap();
    assert_eq!(
        (kept.port, kept.password.as_deref()),
        (4456, Some([1, 2, 3].as_slice()))
    );

    let cleared = switchers::set_obs(&pool, switcher.id, 4456, SecretChange::Clear)
        .await
        .unwrap();
    assert_eq!(cleared.password, None);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn belabox_key_can_be_kept_cleared_or_replaced(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let switcher = switchers::create(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();
    let off = StreamLink {
        start: false,
        stop: false,
    };
    assert_eq!(switcher.belabox_remote_key, None);
    assert!(!switcher.belabox_start_with_stream && !switcher.belabox_stop_with_stream);

    let start_only = StreamLink {
        start: true,
        stop: false,
    };
    let set = switchers::set_belabox(
        &pool,
        switcher.id,
        SecretChange::Set(vec![4, 5]),
        start_only,
    )
    .await
    .unwrap();
    assert_eq!(set.belabox_remote_key.as_deref(), Some([4, 5].as_slice()));
    assert!(set.belabox_start_with_stream && !set.belabox_stop_with_stream);

    let kept = switchers::set_belabox(&pool, switcher.id, SecretChange::Keep, off)
        .await
        .unwrap();
    assert_eq!(kept.belabox_remote_key.as_deref(), Some([4, 5].as_slice()));
    assert!(!kept.belabox_start_with_stream);

    let cleared = switchers::set_belabox(&pool, switcher.id, SecretChange::Clear, off)
        .await
        .unwrap();
    assert_eq!(cleared.belabox_remote_key, None);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn belabox_interface_labels_are_replaced_as_a_whole(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let switcher = switchers::create(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();
    assert!(switcher.interface_labels().is_empty());

    let labels = BTreeMap::from([
        ("a1b2".to_owned(), "Phone A".to_owned()),
        ("c3d4".to_owned(), "Phone B".to_owned()),
    ]);
    let saved = switchers::set_belabox_interface_labels(&pool, switcher.id, &labels)
        .await
        .unwrap();
    assert_eq!(saved.interface_labels(), labels);

    let fewer = BTreeMap::from([("c3d4".to_owned(), "Phone B".to_owned())]);
    let saved = switchers::set_belabox_interface_labels(&pool, switcher.id, &fewer)
        .await
        .unwrap();
    assert_eq!(saved.interface_labels(), fewer);
    let read = switchers::get(&pool, switcher.id).await.unwrap().unwrap();
    assert_eq!(read.interface_labels(), fewer);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn moblin_password_can_be_kept_cleared_or_replaced(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let switcher = switchers::create(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();
    let off = StreamLink {
        start: false,
        stop: false,
    };
    assert_eq!(switcher.moblin_password, None);
    assert!(!switcher.moblin_start_with_stream && !switcher.moblin_stop_with_stream);

    let stop_only = StreamLink {
        start: false,
        stop: true,
    };
    let set = switchers::set_moblin(&pool, switcher.id, SecretChange::Set(vec![7]), stop_only)
        .await
        .unwrap();
    assert_eq!(set.moblin_password.as_deref(), Some([7].as_slice()));
    assert!(!set.moblin_start_with_stream && set.moblin_stop_with_stream);

    let kept = switchers::set_moblin(&pool, switcher.id, SecretChange::Keep, off)
        .await
        .unwrap();
    assert_eq!(kept.moblin_password.as_deref(), Some([7].as_slice()));
    assert!(!kept.moblin_stop_with_stream);

    let cleared = switchers::set_moblin(&pool, switcher.id, SecretChange::Clear, off)
        .await
        .unwrap();
    assert_eq!(cleared.moblin_password, None);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn stream_servers_append_and_reorder(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let switcher = switchers::create(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();
    let kind = StreamServerKind::Belabox;

    let none = Details::default();
    let a = stream_servers::create_last(&pool, switcher.id, "A", kind, "https://a", &none)
        .await
        .unwrap();
    let b = stream_servers::create_last(&pool, switcher.id, "B", kind, "https://b", &none)
        .await
        .unwrap();
    // A newer kind, with kind-specific details.
    let rtmp = Details {
        application: Some("live".into()),
        key: Some("feed1".into()),
        ..Details::default()
    };
    let c = stream_servers::create_last(
        &pool,
        switcher.id,
        "C",
        StreamServerKind::Irlhosting,
        "https://c",
        &rtmp,
    )
    .await
    .unwrap();
    assert_eq!((a.priority, b.priority, c.priority), (0, 1, 2));
    assert_eq!(c.details(), rtmp);
    assert_eq!(a.details(), Details::default());
    // Added by a later migration.
    let ws = stream_servers::create_last(
        &pool,
        switcher.id,
        "WS",
        StreamServerKind::IrlhostingWebsocket,
        "wss://ws",
        &none,
    )
    .await
    .unwrap();
    assert_eq!(ws.kind, StreamServerKind::IrlhostingWebsocket);
    assert!(
        stream_servers::delete(&pool, switcher.id, ws.id)
            .await
            .unwrap()
    );

    assert!(
        stream_servers::reorder(&pool, switcher.id, &[c.id, a.id, b.id])
            .await
            .unwrap()
    );
    let names = stream_servers::by_switcher_id(&pool, switcher.id)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect::<Vec<_>>();
    assert_eq!(names, ["C", "A", "B"]);

    // Missing, duplicated or foreign IDs change nothing.
    assert!(
        !stream_servers::reorder(&pool, switcher.id, &[a.id, b.id])
            .await
            .unwrap()
    );
    assert!(
        !stream_servers::reorder(&pool, switcher.id, &[a.id, a.id, b.id, c.id])
            .await
            .unwrap()
    );
    assert!(
        !stream_servers::reorder(&pool, switcher.id, &[a.id, b.id, Uuid::new_v4()])
            .await
            .unwrap()
    );

    assert!(
        stream_servers::delete(&pool, switcher.id, b.id)
            .await
            .unwrap()
    );
    assert!(
        !stream_servers::delete(&pool, Uuid::new_v4(), a.id)
            .await
            .unwrap()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn commands_merge_overrides_and_aliases(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let switcher = switchers::create(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();

    chat_commands::set_override(&pool, switcher.id, "start", false, None)
        .await
        .unwrap();
    chat_commands::set_override(
        &pool,
        switcher.id,
        "privacy",
        true,
        Some(ChatCommandRole::Moderator),
    )
    .await
    .unwrap();
    // A new switcher has NOALBS's default aliases: !b, !f, !ss, !bbm.
    let commands = chat_commands::list_for_switcher(&pool, switcher.id)
        .await
        .unwrap();
    let aliases = |name: &str| {
        commands
            .iter()
            .find(|c| c.command == name)
            .unwrap()
            .aliases
            .clone()
    };
    assert_eq!(aliases("bitrate"), ["b"]);
    assert_eq!(aliases("fix"), ["f"]);
    assert_eq!(aliases("switch"), ["ss"]);
    assert_eq!(aliases("bbs"), ["bbm"]);

    chat_commands::add_alias(&pool, switcher.id, "br", "bitrate")
        .await
        .unwrap();
    assert!(
        chat_commands::add_alias(&pool, switcher.id, "b", "switch")
            .await
            .is_err()
    );

    let commands = chat_commands::list_for_switcher(&pool, switcher.id)
        .await
        .unwrap();
    let find = |name: &str| commands.iter().find(|c| c.command == name).unwrap();

    assert_eq!(commands.len(), 38);
    assert_eq!(find("start").enabled, Some(false));
    assert_eq!(find("privacy").min_role, Some(ChatCommandRole::Moderator));
    assert_eq!(find("privacy").default_role, ChatCommandRole::Admin);
    assert_eq!(find("bitrate").aliases, ["b", "br"]);
    assert_eq!(find("switch").enabled, None);

    assert!(
        chat_commands::remove_alias(&pool, switcher.id, "b")
            .await
            .unwrap()
    );
    assert!(
        !chat_commands::remove_alias(&pool, switcher.id, "b")
            .await
            .unwrap()
    );
}
