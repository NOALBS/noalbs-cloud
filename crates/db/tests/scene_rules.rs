//! Database tests for server combinations (scene rules).

use db::{
    scene_rules::{self, SaveOutcome, SceneRule, SceneRuleInput},
    stream_servers::{self, StreamServerKind},
    switchers, users, workspaces,
};
use sqlx::PgPool;
use uuid::Uuid;

/// A switcher with servers named `names`, in that priority order.
async fn switcher_with(pool: &PgPool, token: &[u8], names: &[&str]) -> (Uuid, Vec<Uuid>) {
    let owner = users::create(pool).await.unwrap();
    let slug = format!("w-{}", Uuid::new_v4().simple());
    let workspace = workspaces::create(pool, owner.id, "Nomad Live", &slug)
        .await
        .unwrap();
    let switcher = switchers::create(pool, workspace.id, "Main", token)
        .await
        .unwrap();
    let mut servers = Vec::new();
    for (priority, name) in names.iter().enumerate() {
        let server = stream_servers::create(pool, switcher.id, name, StreamServerKind::Belabox)
            .await
            .unwrap();
        stream_servers::reorder(
            pool,
            switcher.id,
            &[servers.clone(), vec![server.id]].concat(),
        )
        .await
        .unwrap();
        assert_eq!(priority, servers.len());
        servers.push(server.id);
    }
    (switcher.id, servers)
}

fn input(servers: &[Uuid]) -> SceneRuleInput {
    SceneRuleInput {
        server_ids: servers.to_vec(),
        enabled: true,
        scene_live: Some(Uuid::new_v4()),
        scene_low: None,
        scene_offline: None,
    }
}

fn saved(outcome: SaveOutcome) -> SceneRule {
    match outcome {
        SaveOutcome::Saved(rule) => rule,
        other => panic!("not saved: {other:?}"),
    }
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn rules_are_listed_in_order_with_their_servers(pool: PgPool) {
    let (switcher, s) = switcher_with(&pool, b"a", &["S1", "S2", "S3"]).await;

    // Given out of priority order: listed in it.
    let first = saved(
        scene_rules::create(&pool, switcher, &input(&[s[2], s[0]]))
            .await
            .unwrap(),
    );
    assert_eq!(first.server_ids, [s[0], s[2]]);
    assert_eq!(first.priority, 0);
    let second = saved(
        scene_rules::create(&pool, switcher, &input(&[s[0], s[1], s[2]]))
            .await
            .unwrap(),
    );
    assert_eq!(second.priority, 1);

    let listed = scene_rules::by_switcher_id(&pool, switcher).await.unwrap();
    assert_eq!(
        listed.iter().map(|rule| rule.id).collect::<Vec<_>>(),
        [first.id, second.id]
    );

    assert!(
        scene_rules::reorder(&pool, switcher, &[second.id, first.id])
            .await
            .unwrap()
    );
    let listed = scene_rules::by_switcher_id(&pool, switcher).await.unwrap();
    assert_eq!(
        listed.iter().map(|rule| rule.id).collect::<Vec<_>>(),
        [second.id, first.id]
    );

    // Not exactly the switcher's rules: refused.
    assert!(
        !scene_rules::reorder(&pool, switcher, &[second.id])
            .await
            .unwrap()
    );
    assert!(
        !scene_rules::reorder(&pool, switcher, &[second.id, first.id, Uuid::new_v4()])
            .await
            .unwrap()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn the_same_servers_in_any_order_are_refused(pool: PgPool) {
    let (switcher, s) = switcher_with(&pool, b"a", &["S1", "S2", "S3"]).await;
    saved(
        scene_rules::create(&pool, switcher, &input(&[s[0], s[1]]))
            .await
            .unwrap(),
    );
    assert!(matches!(
        scene_rules::create(&pool, switcher, &input(&[s[1], s[0]]))
            .await
            .unwrap(),
        SaveOutcome::Duplicate
    ));

    // Editing another rule into the same set too; editing a rule in place isn't.
    let other = saved(
        scene_rules::create(&pool, switcher, &input(&[s[1], s[2]]))
            .await
            .unwrap(),
    );
    assert!(matches!(
        scene_rules::update(&pool, switcher, other.id, &input(&[s[0], s[1]]))
            .await
            .unwrap(),
        SaveOutcome::Duplicate
    ));
    let mut off = input(&[s[2], s[1]]);
    off.enabled = false;
    let edited = saved(
        scene_rules::update(&pool, switcher, other.id, &off)
            .await
            .unwrap(),
    );
    assert!(!edited.enabled);
    assert_eq!(edited.scene_live, off.scene_live);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn servers_and_rules_of_other_switchers_are_refused(pool: PgPool) {
    let (switcher, s) = switcher_with(&pool, b"a", &["S1", "S2"]).await;
    let (other, theirs) = switcher_with(&pool, b"b", &["T1", "T2"]).await;

    assert!(matches!(
        scene_rules::create(&pool, switcher, &input(&[s[0], theirs[0]]))
            .await
            .unwrap(),
        SaveOutcome::UnknownServer
    ));

    let rule = saved(
        scene_rules::create(&pool, switcher, &input(&[s[0], s[1]]))
            .await
            .unwrap(),
    );
    assert!(matches!(
        scene_rules::update(&pool, other, rule.id, &input(&theirs))
            .await
            .unwrap(),
        SaveOutcome::NotFound
    ));
    assert!(!scene_rules::delete(&pool, other, rule.id).await.unwrap());
    assert!(scene_rules::delete(&pool, switcher, rule.id).await.unwrap());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn deleting_a_server_deletes_its_rules(pool: PgPool) {
    let (switcher, s) = switcher_with(&pool, b"a", &["S1", "S2", "S3"]).await;
    let with_s3 = saved(
        scene_rules::create(&pool, switcher, &input(&[s[0], s[2]]))
            .await
            .unwrap(),
    );
    let without = saved(
        scene_rules::create(&pool, switcher, &input(&[s[0], s[1]]))
            .await
            .unwrap(),
    );
    let _ = with_s3;

    assert!(stream_servers::delete(&pool, switcher, s[2]).await.unwrap());
    let listed = scene_rules::by_switcher_id(&pool, switcher).await.unwrap();
    assert_eq!(
        listed.iter().map(|rule| rule.id).collect::<Vec<_>>(),
        [without.id]
    );
    assert_eq!(listed[0].server_ids, [s[0], s[1]]);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_switcher_has_at_most_twenty_rules(pool: PgPool) {
    let names: Vec<String> = (0..7).map(|n| format!("S{n}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (switcher, s) = switcher_with(&pool, b"a", &names).await;

    // Every pair of 7 servers: 21 sets.
    let mut outcomes = Vec::new();
    for a in 0..s.len() {
        for b in a + 1..s.len() {
            outcomes.push(
                scene_rules::create(&pool, switcher, &input(&[s[a], s[b]]))
                    .await
                    .unwrap(),
            );
        }
    }
    assert_eq!(outcomes.len(), 21);
    let saved = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, SaveOutcome::Saved(_)))
        .count();
    assert_eq!(i64::try_from(saved).unwrap(), scene_rules::MAX_SCENE_RULES);
    assert!(matches!(outcomes.last(), Some(SaveOutcome::TooMany)));
}
