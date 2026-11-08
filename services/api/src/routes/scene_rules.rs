//! Server combinations ("scene rules"): scenes to use while all of a set of
//! stream servers have a feed. The first matching rule wins.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use db::scene_rules::{MAX_SCENE_RULES, SaveOutcome, SceneRule, SceneRuleInput};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{SwitcherAuthorized, SwitcherManage, SwitcherView},
    routes::{ApiError, ApiResult, ErrorBody, stream_servers::OverridesDto},
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_scene_rules, create_scene_rule))
        .routes(routes!(reorder_scene_rules))
        .routes(routes!(update_scene_rule, delete_scene_rule))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SceneRuleDto {
    pub id: Uuid,
    /// 0 is tried first.
    pub priority: i32,
    pub enabled: bool,
    /// The servers that must all have a feed, in their priority order.
    pub server_ids: Vec<Uuid>,
    /// `null` falls back to the in-use server's override, then the switcher's scene.
    pub scenes: OverridesDto,
}

impl From<SceneRule> for SceneRuleDto {
    fn from(rule: SceneRule) -> Self {
        Self {
            id: rule.id,
            priority: rule.priority,
            enabled: rule.enabled,
            server_ids: rule.server_ids,
            scenes: OverridesDto {
                live: rule.scene_live,
                low: rule.scene_low,
                offline: rule.scene_offline,
            },
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SaveSceneRule {
    /// Two or more of the switcher's stream servers.
    pub server_ids: Vec<Uuid>,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub scenes: OverridesDto,
}

fn enabled_by_default() -> bool {
    true
}

impl SaveSceneRule {
    fn into_input(self) -> ApiResult<SceneRuleInput> {
        let mut servers = self.server_ids.clone();
        servers.sort();
        servers.dedup();
        if servers.len() < 2 {
            return Err(ApiError::bad_request(
                "too_few_servers",
                "A combination needs at least two different stream servers",
            ));
        }
        Ok(SceneRuleInput {
            server_ids: servers,
            enabled: self.enabled,
            scene_live: self.scenes.live,
            scene_low: self.scenes.low,
            scene_offline: self.scenes.offline,
        })
    }
}

fn saved(outcome: SaveOutcome) -> ApiResult<SceneRuleDto> {
    match outcome {
        SaveOutcome::Saved(rule) => Ok(rule.into()),
        SaveOutcome::NotFound => Err(ApiError::not_found("Combination not found")),
        SaveOutcome::UnknownServer => Err(ApiError::bad_request(
            "unknown_server",
            "Every server of a combination must be one of this switcher's stream servers",
        )),
        SaveOutcome::Duplicate => Err(ApiError::conflict(
            "duplicate_combination",
            "There is already a combination of these servers",
        )),
        SaveOutcome::TooMany => Err(ApiError::bad_request(
            "too_many_combinations",
            format!("A switcher can have at most {MAX_SCENE_RULES} combinations"),
        )),
    }
}

/// List a switcher's server combinations, first tried first.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/scene-rules", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses((status = OK, body = Vec<SceneRuleDto>), (status = FORBIDDEN, body = ErrorBody)))]
async fn list_scene_rules(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<Json<Vec<SceneRuleDto>>> {
    let switcher_id = authorized.into_access().switcher.id;
    let rules = db::scene_rules::by_switcher_id(&state.db, switcher_id).await?;
    Ok(Json(rules.into_iter().map(Into::into).collect()))
}

/// Add a server combination at the end of the order.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/scene-rules", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = SaveSceneRule,
    responses(
        (status = CREATED, body = SceneRuleDto),
        (status = BAD_REQUEST, description = "`too_few_servers`, `unknown_server` or `too_many_combinations`", body = ErrorBody),
        (status = CONFLICT, description = "`duplicate_combination` when another combination has the same servers", body = ErrorBody),
    ))]
async fn create_scene_rule(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<SaveSceneRule>,
) -> ApiResult<(StatusCode, Json<SceneRuleDto>)> {
    let switcher_id = authorized.into_access().switcher.id;
    let input = request.into_input()?;
    let rule = saved(db::scene_rules::create(&state.db, switcher_id, &input).await?)?;
    state.notify_config(switcher_id).await;
    Ok((StatusCode::CREATED, Json(rule)))
}

/// Replace a server combination's servers, scenes and whether it is used.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/scene-rules/{scene_rule_id}", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("scene_rule_id" = Uuid, Path)),
    request_body = SaveSceneRule,
    responses(
        (status = OK, body = SceneRuleDto),
        (status = BAD_REQUEST, description = "`too_few_servers` or `unknown_server`", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
        (status = CONFLICT, description = "`duplicate_combination` when another combination has the same servers", body = ErrorBody),
    ))]
async fn update_scene_rule(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<SaveSceneRule>,
) -> ApiResult<Json<SceneRuleDto>> {
    let (_, _, rule_id) = path;
    let switcher_id = authorized.into_access().switcher.id;
    let input = request.into_input()?;
    let rule = saved(db::scene_rules::update(&state.db, switcher_id, rule_id, &input).await?)?;
    state.notify_config(switcher_id).await;
    Ok(Json(rule))
}

/// Remove a server combination.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/scene-rules/{scene_rule_id}", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("scene_rule_id" = Uuid, Path)),
    responses((status = NO_CONTENT, description = "Removed"), (status = NOT_FOUND, body = ErrorBody)))]
async fn delete_scene_rule(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let (_, _, rule_id) = path;
    let switcher_id = authorized.into_access().switcher.id;
    if db::scene_rules::delete(&state.db, switcher_id, rule_id).await? {
        state.notify_config(switcher_id).await;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Combination not found"))
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReorderSceneRules {
    /// Every combination of the switcher, first tried first.
    pub ids: Vec<Uuid>,
}

/// Set the order server combinations are tried in.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/scene-rules/order", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = ReorderSceneRules,
    responses(
        (status = OK, body = Vec<SceneRuleDto>),
        (status = BAD_REQUEST, description = "`invalid_order` unless `ids` is every combination once", body = ErrorBody),
    ))]
async fn reorder_scene_rules(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<ReorderSceneRules>,
) -> ApiResult<Json<Vec<SceneRuleDto>>> {
    let switcher_id = authorized.into_access().switcher.id;
    if !db::scene_rules::reorder(&state.db, switcher_id, &request.ids).await? {
        return Err(ApiError::bad_request(
            "invalid_order",
            "The order must list every combination of this switcher exactly once",
        ));
    }
    state.notify_config(switcher_id).await;
    let rules = db::scene_rules::by_switcher_id(&state.db, switcher_id).await?;
    Ok(Json(rules.into_iter().map(Into::into).collect()))
}
