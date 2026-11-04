use chrono::{Duration, Utc};
use db::{
    connect::{self, Claim},
    switchers, users, workspaces,
};
use sqlx::PgPool;

async fn fixture(pool: &PgPool) -> (uuid::Uuid, uuid::Uuid, uuid::Uuid) {
    let user = users::create(pool).await.unwrap();
    let workspace = workspaces::create(pool, user.id, "Connect workspace", "connect-workspace")
        .await
        .unwrap();
    let switcher = switchers::create(pool, workspace.id, "Main", b"connect token")
        .await
        .unwrap();
    (user.id, workspace.id, switcher.id)
}

async fn create_pairing(pool: &PgPool, user_code: &str, expires_at: chrono::DateTime<Utc>) {
    connect::create_pairing(
        pool,
        &format!("device-{user_code}"),
        user_code,
        "Studio PC",
        "linux",
        "1.2.3",
        expires_at,
    )
    .await
    .unwrap();
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn claim_is_pending_until_confirmed_and_then_creates_device(pool: PgPool) {
    let (user_id, _, switcher_id) = fixture(&pool).await;
    create_pairing(&pool, "ABCD2345", Utc::now() + Duration::minutes(10)).await;

    assert!(matches!(
        connect::claim_pairing(&pool, "device-ABCD2345", "token-hash")
            .await
            .unwrap(),
        Claim::Pending
    ));
    assert!(
        connect::confirm_pairing(&pool, "abcd-2345", switcher_id, user_id, "Workspace Owner")
            .await
            .unwrap()
    );
    let event_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM switcher_events WHERE switcher_id = $1")
            .bind(switcher_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(event_count, 0);
    let Claim::Claimed {
        device,
        confirmed_by_name,
    } = connect::claim_pairing(&pool, "device-ABCD2345", "token-hash")
        .await
        .unwrap()
    else {
        panic!("confirmed pairing should be claimed");
    };
    assert_eq!(device.switcher_id, switcher_id);
    assert_eq!(device.name, "Studio PC");
    assert_eq!(device.created_by, Some(user_id));
    assert_eq!(confirmed_by_name.as_deref(), Some("Workspace Owner"));
    assert!(matches!(
        connect::claim_pairing(&pool, "device-ABCD2345", "another-hash")
            .await
            .unwrap(),
        Claim::Expired
    ));
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn confirmed_pairing_survives_confirmer_deletion(pool: PgPool) {
    let (_, _, switcher_id) = fixture(&pool).await;
    let confirmer = users::create(&pool).await.unwrap();
    create_pairing(&pool, "RSTU6789", Utc::now() + Duration::minutes(10)).await;
    assert!(
        connect::confirm_pairing(
            &pool,
            "RSTU-6789",
            switcher_id,
            confirmer.id,
            "Former member"
        )
        .await
        .unwrap()
    );

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(confirmer.id)
        .execute(&pool)
        .await
        .unwrap();

    let Claim::Claimed {
        device,
        confirmed_by_name,
    } = connect::claim_pairing(&pool, "device-RSTU6789", "device-token")
        .await
        .unwrap()
    else {
        panic!("confirmed pairing should still be claimed");
    };
    assert_eq!(device.switcher_id, switcher_id);
    assert_eq!(device.created_by, None);
    assert_eq!(confirmed_by_name.as_deref(), Some("Former member"));
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn long_confirmer_name_is_returned_when_pairing_is_claimed(pool: PgPool) {
    let (user_id, _, switcher_id) = fixture(&pool).await;
    let confirmer_name = "x".repeat(300);
    create_pairing(&pool, "UVWX2345", Utc::now() + Duration::minutes(10)).await;

    assert!(
        connect::confirm_pairing(&pool, "UVWX-2345", switcher_id, user_id, &confirmer_name)
            .await
            .unwrap()
    );

    let Claim::Claimed {
        confirmed_by_name, ..
    } = connect::claim_pairing(&pool, "device-UVWX2345", "long-name-token")
        .await
        .unwrap()
    else {
        panic!("confirmed pairing should be claimable");
    };
    assert_eq!(confirmed_by_name.as_deref(), Some(confirmer_name.as_str()));
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn claim_of_expired_or_unknown_pairing_is_expired(pool: PgPool) {
    create_pairing(&pool, "EFGH5678", Utc::now() - Duration::seconds(1)).await;
    for code in ["device-EFGH5678", "unknown"] {
        assert!(matches!(
            connect::claim_pairing(&pool, code, "token-hash")
                .await
                .unwrap(),
            Claim::Expired
        ));
    }
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn revoke_is_scoped_to_its_switcher(pool: PgPool) {
    let (user_id, workspace_id, switcher_id) = fixture(&pool).await;
    let other_switcher = switchers::create(&pool, workspace_id, "Other", b"other token")
        .await
        .unwrap();
    create_pairing(&pool, "JKLM2345", Utc::now() + Duration::minutes(10)).await;
    assert!(
        connect::confirm_pairing(&pool, "JKLM-2345", switcher_id, user_id, "Workspace Owner")
            .await
            .unwrap()
    );
    let Claim::Claimed { device, .. } =
        connect::claim_pairing(&pool, "device-JKLM2345", "revoke-token")
            .await
            .unwrap()
    else {
        panic!("confirmed pairing should be claimed");
    };

    assert!(
        !connect::revoke(&pool, other_switcher.id, device.id)
            .await
            .unwrap()
    );
    assert!(
        connect::revoke(&pool, switcher_id, device.id)
            .await
            .unwrap()
    );
    assert!(
        !connect::revoke(&pool, switcher_id, device.id)
            .await
            .unwrap()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn revoked_device_is_not_found_by_token(pool: PgPool) {
    let (user_id, _, switcher_id) = fixture(&pool).await;
    create_pairing(&pool, "NPQR6789", Utc::now() + Duration::minutes(10)).await;
    connect::confirm_pairing(&pool, "NPQR6789", switcher_id, user_id, "Workspace Owner")
        .await
        .unwrap();
    let Claim::Claimed { device, .. } =
        connect::claim_pairing(&pool, "device-NPQR6789", "revoked-token")
            .await
            .unwrap()
    else {
        panic!("confirmed pairing should be claimed");
    };
    assert!(
        connect::by_token_hash(&pool, &device.token_hash)
            .await
            .unwrap()
            .is_some()
    );
    connect::revoke(&pool, switcher_id, device.id)
        .await
        .unwrap();
    assert!(
        connect::by_token_hash(&pool, &device.token_hash)
            .await
            .unwrap()
            .is_none()
    );
}
