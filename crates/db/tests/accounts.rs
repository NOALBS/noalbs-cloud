//! Sessions and login connections, as managed from the account page.

use chrono::{Duration, Utc};
use db::{
    oauth2_connections::{self, OAuth2Provider, UnlinkOutcome, UpsertOAuth2Connection},
    sessions, users,
};
use sqlx::PgPool;
use uuid::Uuid;

async fn connect(pool: &PgPool, user_id: Uuid, provider: OAuth2Provider, name: &str) -> Uuid {
    oauth2_connections::upsert_for_user(
        pool,
        user_id,
        UpsertOAuth2Connection {
            provider,
            access_token: "token",
            refresh_token: None,
            token_type: None,
            expires_at: None,
            scopes: &[],
            provider_id: name,
            provider_username: name,
            provider_email: None,
            provider_avatar_url: None,
        },
    )
    .await
    .unwrap()
    .unwrap()
    .id
}

async fn sign_in(pool: &PgPool, connection_id: Uuid, agent: &str) -> Uuid {
    sessions::create(
        pool,
        connection_id,
        Utc::now() + Duration::days(30),
        Some(agent),
    )
    .await
    .unwrap()
    .id
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn sessions_are_listed_and_revoked_per_user(pool: PgPool) {
    let alice = users::create(&pool).await.unwrap().id;
    let bob = users::create(&pool).await.unwrap().id;
    let twitch = connect(&pool, alice, OAuth2Provider::Twitch, "alice").await;
    let bobs = connect(&pool, bob, OAuth2Provider::Twitch, "bob").await;
    let laptop = sign_in(&pool, twitch, "Firefox").await;
    let phone = sign_in(&pool, twitch, "Safari").await;
    let tablet = sign_in(&pool, twitch, "Chrome").await;
    let bobs_session = sign_in(&pool, bobs, "Edge").await;

    let listed = sessions::list_for_user(&pool, alice).await.unwrap();
    assert_eq!(listed.len(), 3);
    assert!(listed.iter().all(|session| session.provider == "TWITCH"));

    // Nobody can sign out someone else's session.
    assert!(
        !sessions::delete_for_user(&pool, alice, bobs_session)
            .await
            .unwrap()
    );
    assert!(
        sessions::delete_for_user(&pool, alice, phone)
            .await
            .unwrap()
    );
    assert_eq!(
        sessions::delete_others(&pool, alice, laptop).await.unwrap(),
        1
    );

    let left: Vec<Uuid> = sessions::list_for_user(&pool, alice)
        .await
        .unwrap()
        .into_iter()
        .map(|session| session.id)
        .collect();
    assert_eq!(left, vec![laptop]);
    assert!(!left.contains(&tablet));
    assert_eq!(sessions::list_for_user(&pool, bob).await.unwrap().len(), 1);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn unlinking_keeps_the_current_session_and_never_the_last_connection(pool: PgPool) {
    let alice = users::create(&pool).await.unwrap().id;
    let twitch = connect(&pool, alice, OAuth2Provider::Twitch, "alice").await;
    let kick = connect(&pool, alice, OAuth2Provider::Kick, "alice_kick").await;
    let here = sign_in(&pool, twitch, "Firefox").await;
    let elsewhere = sign_in(&pool, twitch, "Safari").await;

    let outcome = oauth2_connections::unlink(&pool, alice, twitch, here)
        .await
        .unwrap();
    assert_eq!(outcome, UnlinkOutcome::Unlinked);

    // This browser stays signed in (now through Kick); the other Twitch one is gone.
    let sessions = sessions::list_for_user(&pool, alice).await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, here);
    assert_eq!(sessions[0].provider, "KICK");
    assert!(!sessions.iter().any(|session| session.id == elsewhere));
    assert_eq!(
        sessions::user_id_for_active_session(&pool, here)
            .await
            .unwrap(),
        Some(alice)
    );

    assert_eq!(
        oauth2_connections::unlink(&pool, alice, kick, here)
            .await
            .unwrap(),
        UnlinkOutcome::LastConnection
    );
    let bob = users::create(&pool).await.unwrap().id;
    assert_eq!(
        oauth2_connections::unlink(&pool, bob, kick, here)
            .await
            .unwrap(),
        UnlinkOutcome::NotFound
    );
}
