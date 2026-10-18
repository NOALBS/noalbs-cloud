//! Database tests for the chat bot: channel links, bots and wanted subscriptions.

use db::{
    chat::{self, ChatPlatform, Grant, GrantStatus, PlatformAccount, WantedSubscription},
    switchers, users, workspaces,
};
use sqlx::PgPool;
use uuid::Uuid;

struct Setup {
    user: Uuid,
    workspace: Uuid,
    switcher: Uuid,
}

async fn setup(pool: &PgPool, slug: &str) -> Setup {
    let user = users::create(pool).await.unwrap().id;
    let workspace = workspaces::create(pool, user, slug, slug).await.unwrap().id;
    let switcher = switchers::create(pool, workspace, "Main", slug.as_bytes())
        .await
        .unwrap()
        .id;
    Setup {
        user,
        workspace,
        switcher,
    }
}

fn twitch(id: &'static str, login: &'static str) -> PlatformAccount<'static> {
    PlatformAccount {
        platform: ChatPlatform::Twitch,
        id,
        login,
        display_name: login,
    }
}

fn kick(id: &'static str, login: &'static str) -> PlatformAccount<'static> {
    PlatformAccount {
        platform: ChatPlatform::Kick,
        ..twitch(id, login)
    }
}

fn youtube(id: &'static str, login: &'static str) -> PlatformAccount<'static> {
    PlatformAccount {
        platform: ChatPlatform::YouTube,
        ..twitch(id, login)
    }
}

fn wanted(platform: ChatPlatform, channel: &str, bot: &str) -> WantedSubscription {
    WantedSubscription {
        platform,
        platform_channel_id: channel.into(),
        platform_bot_id: bot.into(),
    }
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_channel_on_two_switchers_is_one_channel_and_unlinking_keeps_the_other(pool: PgPool) {
    let a = setup(&pool, "kate-live").await;
    let b = setup(&pool, "bike-crew").await;

    let first = chat::link_channel(
        &pool,
        a.switcher,
        twitch("100", "kate"),
        Grant::default(),
        a.user,
    )
    .await
    .unwrap();
    // Linking again (a fresh grant) changes nothing but the details.
    let renamed = PlatformAccount {
        display_name: "Kate",
        ..twitch("100", "kate")
    };
    chat::link_channel(&pool, a.switcher, renamed, Grant::default(), a.user)
        .await
        .unwrap();
    let second = chat::link_channel(
        &pool,
        b.switcher,
        twitch("100", "kate"),
        Grant::default(),
        b.user,
    )
    .await
    .unwrap();
    assert_eq!(first.id, second.id);

    let linked = chat::channels_for_switcher(&pool, a.switcher)
        .await
        .unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].channel.display_name, "kate");
    assert_eq!(linked[0].other_links, 1);

    assert!(
        chat::unlink_channel(&pool, a.switcher, first.id)
            .await
            .unwrap()
    );
    assert!(
        !chat::unlink_channel(&pool, a.switcher, first.id)
            .await
            .unwrap()
    );
    assert!(
        chat::channels_for_switcher(&pool, a.switcher)
            .await
            .unwrap()
            .is_empty()
    );
    let kept = chat::channels_for_switcher(&pool, b.switcher)
        .await
        .unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].other_links, 0);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn one_shared_bot_and_one_own_bot_per_platform(pool: PgPool) {
    let a = setup(&pool, "kate-live").await;

    chat::upsert_bot(
        &pool,
        None,
        twitch("1", "noalbsbot"),
        Grant::default(),
        a.user,
    )
    .await
    .unwrap();
    // Connecting another account replaces the shared bot.
    let replaced = chat::upsert_bot(&pool, None, twitch("2", "noalbs"), Grant::default(), a.user)
        .await
        .unwrap();
    assert_eq!(replaced.login, "noalbs");
    let token = b"encrypted".as_slice();
    let scopes = ["chat:write".to_owned()];
    chat::upsert_bot(
        &pool,
        None,
        kick("3", "noalbs"),
        Grant {
            access_token: Some(token),
            scopes: &scopes,
            ..Grant::default()
        },
        a.user,
    )
    .await
    .unwrap();
    chat::upsert_bot(
        &pool,
        Some(a.workspace),
        twitch("4", "katebot"),
        Grant::default(),
        a.user,
    )
    .await
    .unwrap();

    let shared = chat::bots(&pool, None).await.unwrap();
    assert_eq!(
        shared
            .iter()
            .map(|bot| (bot.platform, bot.login.as_str()))
            .collect::<Vec<_>>(),
        [
            (ChatPlatform::Twitch, "noalbs"),
            (ChatPlatform::Kick, "noalbs")
        ]
    );
    assert_eq!(shared[1].scopes, scopes);
    let own = chat::bots(&pool, Some(a.workspace)).await.unwrap();
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].status, GrantStatus::Active);

    assert!(
        chat::remove_bot(&pool, Some(a.workspace), ChatPlatform::Twitch)
            .await
            .unwrap()
    );
    assert!(
        chat::bots(&pool, Some(a.workspace))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(chat::bots(&pool, None).await.unwrap().len(), 2);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn own_bots_speak_only_on_plans_that_allow_them(pool: PgPool) {
    let a = setup(&pool, "kate-live").await;
    let b = setup(&pool, "bike-crew").await;
    chat::upsert_bot(
        &pool,
        None,
        twitch("1", "noalbsbot"),
        Grant::default(),
        a.user,
    )
    .await
    .unwrap();
    chat::upsert_bot(
        &pool,
        Some(a.workspace),
        twitch("4", "katebot"),
        Grant::default(),
        a.user,
    )
    .await
    .unwrap();
    for setup in [&a, &b] {
        chat::link_channel(
            &pool,
            setup.switcher,
            twitch("100", "kate"),
            Grant::default(),
            setup.user,
        )
        .await
        .unwrap();
    }
    chat::link_channel(
        &pool,
        b.switcher,
        kick("200", "bikes"),
        Grant::default(),
        b.user,
    )
    .await
    .unwrap();

    // Free: the shared bot speaks everywhere; Kick needs no bot to subscribe.
    let speaking = chat::channels_for_switcher(&pool, a.switcher)
        .await
        .unwrap();
    assert_eq!(speaking[0].bot_login.as_deref(), Some("noalbsbot"));
    assert!(!speaking[0].bot_is_custom);
    assert_eq!(
        chat::wanted_subscriptions(&pool).await.unwrap(),
        [
            wanted(ChatPlatform::Twitch, "100", "1"),
            wanted(ChatPlatform::Kick, "200", ""),
        ]
    );
    let kick_link = chat::channels_for_switcher(&pool, b.switcher)
        .await
        .unwrap();
    assert_eq!(kick_link[1].bot_login, None);

    // Pro: workspace A's own bot reads its link too, so the channel has two readers.
    workspaces::set_plan(&pool, a.workspace, "pro")
        .await
        .unwrap();
    let speaking = chat::channels_for_switcher(&pool, a.switcher)
        .await
        .unwrap();
    assert_eq!(speaking[0].bot_login.as_deref(), Some("katebot"));
    assert!(speaking[0].bot_is_custom);
    assert_eq!(
        chat::wanted_subscriptions(&pool).await.unwrap(),
        [
            wanted(ChatPlatform::Twitch, "100", "1"),
            wanted(ChatPlatform::Twitch, "100", "4"),
            wanted(ChatPlatform::Kick, "200", ""),
        ]
    );

    // A revoked own bot falls back to the shared one; a revoked channel isn't read.
    sqlx::query("UPDATE chat_bots SET status = 'revoked' WHERE platform_user_id = '4'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE chat_channels SET status = 'revoked' WHERE platform_channel_id = '200'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        chat::wanted_subscriptions(&pool).await.unwrap(),
        [wanted(ChatPlatform::Twitch, "100", "1")]
    );

    // Without any Twitch bot, Twitch channels can't be read.
    chat::remove_bot(&pool, None, ChatPlatform::Twitch)
        .await
        .unwrap();
    assert!(chat::wanted_subscriptions(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn racing_instances_settle_on_one_conduit(pool: PgPool) {
    let key = "twitch_conduit_id";
    // Two instances find none and each made one: the first stored wins.
    assert_eq!(
        chat::replace_platform_state(&pool, key, None, "conduit_a")
            .await
            .unwrap(),
        "conduit_a"
    );
    assert_eq!(
        chat::replace_platform_state(&pool, key, None, "conduit_b")
            .await
            .unwrap(),
        "conduit_a"
    );
    // The stored one vanished: replacing it works once.
    assert_eq!(
        chat::replace_platform_state(&pool, key, Some("conduit_a"), "conduit_c")
            .await
            .unwrap(),
        "conduit_c"
    );
    assert_eq!(
        chat::replace_platform_state(&pool, key, Some("conduit_a"), "conduit_d")
            .await
            .unwrap(),
        "conduit_c"
    );
    assert_eq!(
        chat::platform_state(&pool, key).await.unwrap().as_deref(),
        Some("conduit_c")
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn messages_find_their_switchers_and_speaking_bots(pool: PgPool) {
    let a = setup(&pool, "kate-live").await;
    let b = setup(&pool, "bike-crew").await;
    chat::upsert_bot(
        &pool,
        None,
        twitch("1", "noalbsbot"),
        Grant::default(),
        a.user,
    )
    .await
    .unwrap();
    for setup in [&a, &b] {
        chat::link_channel(
            &pool,
            setup.switcher,
            twitch("100", "kate"),
            Grant::default(),
            setup.user,
        )
        .await
        .unwrap();
    }

    let links = chat::links_for_channel(&pool, ChatPlatform::Twitch, "100")
        .await
        .unwrap();
    assert_eq!(
        links
            .iter()
            .map(|link| link.switcher_id)
            .collect::<Vec<_>>(),
        [a.switcher, b.switcher]
    );
    assert!(
        links
            .iter()
            .all(|link| link.bot_login.as_deref() == Some("noalbsbot"))
    );
    assert!(links.iter().all(|link| link.command_prefix == "!"));
    assert_eq!(
        chat::bot_user_ids(&pool, ChatPlatform::Twitch)
            .await
            .unwrap(),
        ["1"]
    );
    assert!(
        chat::links_for_channel(&pool, ChatPlatform::Kick, "100")
            .await
            .unwrap()
            .is_empty()
    );

    // Workspace members are recognised by the chat account they linked.
    assert!(
        !chat::is_member_account(&pool, a.workspace, ChatPlatform::Twitch, "55")
            .await
            .unwrap()
    );
    sqlx::query(
        "INSERT INTO oauth2_connections (user_id, provider, access_token, provider_id, provider_username)
         VALUES ($1, 'TWITCH', 'token', '55', 'kate_alt')",
    )
    .bind(a.user)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        chat::is_member_account(&pool, a.workspace, ChatPlatform::Twitch, "55")
            .await
            .unwrap()
    );
    assert!(
        !chat::is_member_account(&pool, b.workspace, ChatPlatform::Twitch, "55")
            .await
            .unwrap()
    );

    // Subscriptions are recorded once per channel and bot.
    for id in ["sub_1", "sub_2"] {
        chat::record_subscription(
            &pool,
            ChatPlatform::Twitch,
            "channel.chat.message",
            "100",
            "1",
            id,
        )
        .await
        .unwrap();
    }
    let recorded = chat::subscriptions(&pool).await.unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].platform_subscription_id, "sub_2");
    chat::forget_subscription(&pool, ChatPlatform::Twitch, "sub_2")
        .await
        .unwrap();
    assert!(chat::subscriptions(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn raids_notifications_revocations_and_ignored_chatters(pool: PgPool) {
    let a = setup(&pool, "kate-live").await;
    let b = setup(&pool, "bike-crew").await;
    chat::upsert_bot(
        &pool,
        None,
        twitch("1", "noalbsbot"),
        Grant::default(),
        a.user,
    )
    .await
    .unwrap();
    for setup in [&a, &b] {
        chat::link_channel(
            &pool,
            setup.switcher,
            twitch("100", "kate"),
            Grant::default(),
            setup.user,
        )
        .await
        .unwrap();
    }
    let kate_kick = chat::link_channel(
        &pool,
        a.switcher,
        kick("200", "kate"),
        Grant::default(),
        a.user,
    )
    .await
    .unwrap();

    // Raids matter only where a switcher stops after raiding out; announcing alone
    // doesn't need them.
    sqlx::query("UPDATE switchers SET announce_raid = true WHERE id = $1")
        .bind(a.switcher)
        .execute(&pool)
        .await
        .unwrap();
    for platform in ChatPlatform::ALL {
        assert!(
            chat::raid_channels(&pool, platform)
                .await
                .unwrap()
                .is_empty()
        );
    }
    sqlx::query("UPDATE switchers SET auto_stop_on_raid = true WHERE id = $1")
        .bind(b.switcher)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        chat::raid_channels(&pool, ChatPlatform::Twitch)
            .await
            .unwrap(),
        ["100"]
    );
    // B has no Kick channel; A's is listed once A stops after raids too.
    assert!(
        chat::raid_channels(&pool, ChatPlatform::Kick)
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::query("UPDATE switchers SET auto_stop_on_raid = true WHERE id = $1")
        .bind(a.switcher)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        chat::raid_channels(&pool, ChatPlatform::Kick)
            .await
            .unwrap(),
        ["200"]
    );

    // Where switcher A posts: both its channels; the Twitch one is shared with B.
    let channels = chat::channels_of_switcher(&pool, a.switcher).await.unwrap();
    assert_eq!(
        channels
            .iter()
            .map(|c| (c.platform, c.platform_channel_id.as_str(), c.shared))
            .collect::<Vec<_>>(),
        [
            (ChatPlatform::Twitch, "100", true),
            (ChatPlatform::Kick, "200", false)
        ]
    );
    assert_eq!(channels[0].bot_platform_user_id.as_deref(), Some("1"));
    assert_eq!(channels[1].bot_id, None);

    // Notifications are on for every link until turned off for one.
    assert!(channels.iter().all(|c| c.auto_switch_notification));
    assert!(
        chat::set_link_notification(&pool, a.switcher, kate_kick.id, false)
            .await
            .unwrap()
    );
    let notified: Vec<_> = chat::channels_of_switcher(&pool, a.switcher)
        .await
        .unwrap()
        .into_iter()
        .map(|c| (c.platform_channel_id, c.auto_switch_notification))
        .collect();
    assert_eq!(notified, [("100".into(), true), ("200".into(), false)]);
    // A channel the switcher isn't linked to: nothing to change.
    assert!(
        !chat::set_link_notification(&pool, b.switcher, kate_kick.id, false)
            .await
            .unwrap()
    );

    // The streamer revokes our app: their channel goes; the bot is someone else.
    assert_eq!(
        chat::revoke_account(&pool, ChatPlatform::Twitch, "100")
            .await
            .unwrap(),
        1
    );
    assert!(
        chat::raid_channels(&pool, ChatPlatform::Twitch)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        chat::channels_of_switcher(&pool, a.switcher)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        chat::revoke_account(&pool, ChatPlatform::Twitch, "1")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        chat::bots(&pool, None).await.unwrap()[0].status,
        GrantStatus::Revoked
    );

    // Ignored chatters, per switcher.
    chat::ignore_chatter(&pool, a.switcher, ChatPlatform::Twitch, "9", "Nightbot")
        .await
        .unwrap();
    chat::ignore_chatter(&pool, a.switcher, ChatPlatform::Twitch, "9", "nightbot")
        .await
        .unwrap();
    let ignored = chat::ignored_chatters(&pool, a.switcher).await.unwrap();
    assert_eq!(ignored.len(), 1);
    assert_eq!(ignored[0].platform_username, "nightbot");
    assert!(
        chat::is_ignored(&pool, a.switcher, ChatPlatform::Twitch, "9")
            .await
            .unwrap()
    );
    assert!(
        !chat::is_ignored(&pool, b.switcher, ChatPlatform::Twitch, "9")
            .await
            .unwrap()
    );
    assert!(
        chat::unignore_chatter_named(&pool, a.switcher, ChatPlatform::Twitch, "NightBot")
            .await
            .unwrap()
    );
    assert!(
        !chat::unignore_chatter(&pool, a.switcher, ChatPlatform::Twitch, "9")
            .await
            .unwrap()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn kick_tokens_close_to_expiry_are_found(pool: PgPool) {
    let a = setup(&pool, "kate-live").await;
    let token = b"encrypted".as_slice();
    let soon = chrono::Utc::now() + chrono::Duration::minutes(30);
    let later = chrono::Utc::now() + chrono::Duration::hours(5);
    let bot = chat::upsert_bot(
        &pool,
        None,
        kick("2", "noalbs"),
        Grant {
            access_token: Some(token),
            refresh_token: Some(token),
            expires_at: Some(soon),
            scopes: &[],
        },
        a.user,
    )
    .await
    .unwrap();
    chat::link_channel(
        &pool,
        a.switcher,
        kick("200", "kate"),
        Grant {
            access_token: Some(token),
            refresh_token: Some(token),
            expires_at: Some(later),
            scopes: &[],
        },
        a.user,
    )
    .await
    .unwrap();

    let due = chat::tokens_expiring(&pool, chrono::Utc::now() + chrono::Duration::hours(1))
        .await
        .unwrap();
    assert_eq!(due, [chat::TokenOwner::Bot(bot.id)]);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn the_environment_id_is_made_once(pool: PgPool) {
    let first = chat::environment_id(&pool).await.unwrap();
    assert_eq!(first.len(), 12);
    assert_eq!(chat::environment_id(&pool).await.unwrap(), first);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn youtube_channels_are_followed_not_subscribed(pool: PgPool) {
    let a = setup(&pool, "kate-live").await;
    let b = setup(&pool, "bike-crew").await;
    let token = b"encrypted".as_slice();
    let grant = Grant {
        access_token: Some(token),
        refresh_token: Some(token),
        expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
        scopes: &[],
    };
    let channel = chat::link_channel(&pool, a.switcher, youtube("UCkate", "kate"), grant, a.user)
        .await
        .unwrap();
    chat::link_channel(&pool, b.switcher, youtube("UCkate", "kate"), grant, b.user)
        .await
        .unwrap();

    // Nobody to answer with yet.
    assert!(chat::youtube_channels(&pool).await.unwrap().is_empty());
    chat::upsert_bot(&pool, None, youtube("UCbot", "noalbs"), grant, a.user)
        .await
        .unwrap();
    let mut both = [a.switcher, b.switcher];
    both.sort();
    assert_eq!(
        chat::youtube_channels(&pool).await.unwrap(),
        [chat::YouTubeChannel {
            id: channel.id,
            platform_channel_id: "UCkate".into(),
            switcher_ids: both.to_vec(),
        }]
    );
    // The reconciler has nothing to subscribe on YouTube.
    assert!(chat::wanted_subscriptions(&pool).await.unwrap().is_empty());

    // A suspended workspace's switcher drops out.
    sqlx::query(
        "UPDATE workspaces SET suspended_at = now(), suspended_reason = 'test' WHERE id = $1",
    )
    .bind(b.workspace)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        chat::youtube_channels(&pool).await.unwrap()[0].switcher_ids,
        [a.switcher]
    );

    // Tokens say which platform refreshes them.
    let mut tx = pool.begin().await.unwrap();
    let stored = chat::lock_tokens(&mut tx, chat::TokenOwner::Channel(channel.id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.platform, ChatPlatform::YouTube);
    drop(tx);

    chat::revoke(&pool, chat::TokenOwner::Channel(channel.id))
        .await
        .unwrap();
    assert!(chat::youtube_channels(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn unlinked_channels_go_with_their_grants_once_unsubscribed(pool: PgPool) {
    let a = setup(&pool, "kate-live").await;
    let b = setup(&pool, "bike-crew").await;
    let token = b"encrypted".as_slice();
    let grant = Grant {
        access_token: Some(token),
        refresh_token: Some(token),
        expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
        scopes: &[],
    };
    let kate = chat::link_channel(&pool, a.switcher, kick("200", "kate"), grant, a.user)
        .await
        .unwrap();
    chat::link_channel(&pool, b.switcher, kick("200", "kate"), grant, b.user)
        .await
        .unwrap();
    let bike = chat::link_channel(&pool, b.switcher, youtube("UCbike", "bike"), grant, b.user)
        .await
        .unwrap();
    chat::record_subscription(
        &pool,
        ChatPlatform::Kick,
        "chat.message.sent",
        "200",
        "",
        "sub-1",
    )
    .await
    .unwrap();
    assert_eq!(chat::delete_unlinked_channels(&pool).await.unwrap(), 0);

    // Still linked to the other switcher.
    chat::unlink_channel(&pool, a.switcher, kate.id)
        .await
        .unwrap();
    assert_eq!(chat::delete_unlinked_channels(&pool).await.unwrap(), 0);

    // Unlinked everywhere, but its subscription isn't gone yet.
    chat::unlink_everywhere(&pool, kate.id).await.unwrap();
    assert_eq!(chat::delete_unlinked_channels(&pool).await.unwrap(), 0);
    assert!(chat::channel(&pool, kate.id).await.unwrap().is_some());

    chat::forget_subscription(&pool, ChatPlatform::Kick, "sub-1")
        .await
        .unwrap();
    assert_eq!(chat::delete_unlinked_channels(&pool).await.unwrap(), 1);
    assert!(chat::channel(&pool, kate.id).await.unwrap().is_none());
    assert!(chat::channel(&pool, bike.id).await.unwrap().is_some());

    // Deleting the workspace takes the links, and then the channel goes too.
    workspaces::delete(&pool, b.workspace).await.unwrap();
    assert_eq!(chat::delete_unlinked_channels(&pool).await.unwrap(), 1);
    assert!(chat::channel(&pool, bike.id).await.unwrap().is_none());

    // Linking again starts over with a fresh grant.
    let again = chat::link_channel(&pool, a.switcher, kick("200", "kate"), grant, a.user)
        .await
        .unwrap();
    assert_ne!(again.id, kate.id);
    assert_eq!(chat::delete_unlinked_channels(&pool).await.unwrap(), 0);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn revoking_a_grant_clears_its_tokens(pool: PgPool) {
    let a = setup(&pool, "kate-live").await;
    let token = b"encrypted".as_slice();
    let grant = Grant {
        access_token: Some(token),
        refresh_token: Some(token),
        expires_at: Some(chrono::Utc::now() + chrono::Duration::minutes(5)),
        scopes: &[],
    };
    let bot = chat::upsert_bot(&pool, None, kick("2", "noalbs"), grant, a.user)
        .await
        .unwrap();
    let kate = chat::link_channel(&pool, a.switcher, kick("200", "kate"), grant, a.user)
        .await
        .unwrap();
    let mia = chat::link_channel(&pool, a.switcher, youtube("UCmia", "mia"), grant, a.user)
        .await
        .unwrap();

    async fn tokens(pool: &PgPool, owner: chat::TokenOwner) -> chat::StoredTokens {
        let mut tx = pool.begin().await.unwrap();
        chat::lock_tokens(&mut tx, owner).await.unwrap().unwrap()
    }
    let cleared = |stored: chat::StoredTokens| {
        stored.access_token.is_none()
            && stored.refresh_token.is_none()
            && stored.expires_at.is_none()
    };

    // A refused refresh.
    chat::revoke(&pool, chat::TokenOwner::Bot(bot.id))
        .await
        .unwrap();
    assert!(cleared(tokens(&pool, chat::TokenOwner::Bot(bot.id)).await));
    // The account took our access away.
    assert_eq!(
        chat::revoke_account(&pool, ChatPlatform::Kick, "200")
            .await
            .unwrap(),
        1
    );
    assert!(cleared(
        tokens(&pool, chat::TokenOwner::Channel(kate.id)).await
    ));
    // Others keep theirs, and revoked grants aren't refreshed.
    assert!(!cleared(
        tokens(&pool, chat::TokenOwner::Channel(mia.id)).await
    ));
    let soon = chrono::Utc::now() + chrono::Duration::hours(1);
    assert_eq!(
        chat::tokens_expiring(&pool, soon).await.unwrap(),
        [chat::TokenOwner::Channel(mia.id)]
    );
    // Revoked channels stay linked so the Chat page can ask to connect them again.
    assert_eq!(
        chat::channel(&pool, kate.id).await.unwrap().unwrap().status,
        GrantStatus::Revoked
    );
}
