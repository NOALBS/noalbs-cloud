use db::{switcher_preferences, switchers, users, workspaces};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn preferences_are_private_upserted_exported_and_cascaded(pool: PgPool) {
    let user = users::create(&pool).await.unwrap();
    let other = users::create(&pool).await.unwrap();
    let workspace = workspaces::create(
        &pool,
        user.id,
        "Studio",
        &format!("w-{}", Uuid::new_v4().simple()),
    )
    .await
    .unwrap();
    let switcher = switchers::create(&pool, workspace.id, "Main", b"preferences-test")
        .await
        .unwrap();
    assert!(
        switcher_preferences::get(&pool, user.id, switcher.id)
            .await
            .unwrap()
            .is_none()
    );
    switcher_preferences::put(&pool, user.id, switcher.id, &json!({"layout": 1}))
        .await
        .unwrap();
    assert!(
        switcher_preferences::get(&pool, other.id, switcher.id)
            .await
            .unwrap()
            .is_none()
    );
    let row = switcher_preferences::put(&pool, user.id, switcher.id, &json!({}))
        .await
        .unwrap();
    assert_eq!(row.preferences, json!({}));
    assert_eq!(
        switcher_preferences::for_user(&pool, user.id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        db::export::account(&pool, user.id)
            .await
            .unwrap()
            .unwrap()
            .switcher_preferences
            .len(),
        1
    );
    assert!(
        switcher_preferences::put(&pool, user.id, switcher.id, &json!([]))
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM switchers WHERE id = $1")
        .bind(switcher.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        switcher_preferences::for_user(&pool, user.id)
            .await
            .unwrap()
            .is_empty()
    );
}
