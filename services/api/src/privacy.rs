//! Privacy (GDPR): deleting workspaces and accounts, and keeping personal data
//! no longer than needed.

use std::time::Duration;

use axum::http::StatusCode;
use common::shutdown::ShutdownHandler;
use uuid::Uuid;

use crate::{
    app::ApiState,
    routes::{ApiError, ApiResult},
};

const UPKEEP_EVERY: Duration = Duration::from_secs(60 * 60);

/// Deletes a workspace and everything in it. Its Stripe customer goes first,
/// which ends any subscription at once (no refund); if Stripe can't be reached,
/// nothing is deleted. Then obs-link drops its switchers and the chat service
/// their channels.
pub async fn delete_workspace(
    state: &ApiState,
    workspace_id: Uuid,
    stripe_customer: Option<&str>,
) -> ApiResult<()> {
    if let Some(customer) = stripe_customer {
        let Some(billing) = &state.billing else {
            // Its subscription would keep charging with no way to stop it from here.
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "billing_disabled",
                "This workspace has billing with Stripe, which isn't set up on this server, so its subscription can't be cancelled.",
            ));
        };
        billing
            .stripe
            .delete_customer(customer)
            .await
            .map_err(|error| {
                tracing::warn!(%error, %workspace_id, "Couldn't delete a Stripe customer");
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "stripe_error",
                    "Couldn't cancel the subscription with the payment provider, so nothing was deleted. Try again in a moment.",
                )
            })?;
    }

    let switchers = db::switchers::list_summaries(&state.db, workspace_id).await?;
    db::workspaces::delete(&state.db, workspace_id).await?;
    for switcher in switchers {
        state.notify_config(switcher.id).await;
    }
    state.notify_chat().await;
    tracing::info!(%workspace_id, "Deleted a workspace");
    Ok(())
}

/// Deletes expired sessions and old activity every hour. Stream history has its
/// own upkeep (`routes::history`). Safe to run on every API replica.
pub fn spawn_upkeep(db: db::DbPool, mut shutdown: ShutdownHandler) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(UPKEEP_EVERY);
        loop {
            tokio::select! {
                _ = shutdown.changed() => return,
                _ = every.tick() => upkeep(&db).await,
            }
        }
    });
}

async fn upkeep(db: &db::DbPool) {
    match db::sessions::delete_expired(db).await {
        Ok(0) => {}
        Ok(deleted) => tracing::info!(deleted, "Deleted expired sessions"),
        Err(error) => tracing::warn!(%error, "Couldn't delete expired sessions"),
    }
    match db::switcher_events::delete_expired(db).await {
        Ok(0) => {}
        Ok(deleted) => tracing::info!(deleted, "Deleted old activity"),
        Err(error) => tracing::warn!(%error, "Couldn't delete old activity"),
    }
    match db::support::delete_expired(db).await {
        Ok(0) => {}
        Ok(deleted) => tracing::info!(deleted, "Deleted old support access records"),
        Err(error) => tracing::warn!(%error, "Couldn't delete old support access records"),
    }
    match db::organization_audit::delete_expired(db).await {
        Ok(0) => {}
        Ok(deleted) => tracing::info!(deleted, "Deleted old organization audit entries"),
        Err(error) => tracing::warn!(%error, "Couldn't delete old organization audit entries"),
    }
    match db::staff_actions::delete_expired(db).await {
        Ok(0) => {}
        Ok(deleted) => tracing::info!(deleted, "Deleted old staff actions"),
        Err(error) => tracing::warn!(%error, "Couldn't delete old staff actions"),
    }
}
