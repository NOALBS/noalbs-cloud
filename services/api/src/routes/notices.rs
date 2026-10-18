//! Notices: messages staff post for every signed-in user (maintenance, an
//! outage, a changed policy), shown as a banner in the dashboard. Everyone
//! signed in reads the active ones; staff manage them in Admin.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{CurrentUser, Staff, user_display_name},
    routes::{
        ApiError, ApiResult, ErrorBody,
        admin_actions::{Action, record},
    },
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_notices))
        .routes(routes!(admin_list_notices, admin_create_notice))
        .routes(routes!(admin_update_notice, admin_delete_notice))
        .routes(routes!(admin_end_notice))
}

const MAX_MESSAGE_LEN: usize = 500;
/// More than anyone reads in Admin.
const MAX_LISTED: i64 = 200;
const SEVERITIES: &[&str] = &["info", "warning", "critical"];

/// A notice in force now.
#[derive(Debug, Serialize, ToSchema)]
pub struct NoticeDto {
    pub id: Uuid,
    /// Plain text.
    pub message: String,
    /// `info`, `warning` or `critical`.
    pub severity: String,
    pub ends_at: Option<DateTime<Utc>>,
    /// Whether the viewer may hide it (never for `critical`).
    pub dismissible: bool,
}

impl From<db::notices::Notice> for NoticeDto {
    fn from(notice: db::notices::Notice) -> Self {
        Self {
            id: notice.id,
            message: notice.message,
            severity: notice.severity,
            ends_at: notice.ends_at,
            dismissible: notice.dismissible,
        }
    }
}

/// A notice as staff see it.
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminNoticeDto {
    pub id: Uuid,
    pub message: String,
    pub severity: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub dismissible: bool,
    /// Who posted it, as they were named then.
    pub created_by_name: Option<String>,
    pub created_at: DateTime<Utc>,
    /// `upcoming`, `active` or `ended`.
    pub state: String,
}

impl AdminNoticeDto {
    fn new(notice: db::notices::Notice, now: DateTime<Utc>) -> Self {
        let state = notice_state(notice.starts_at, notice.ends_at, now);
        Self {
            id: notice.id,
            message: notice.message,
            severity: notice.severity,
            starts_at: notice.starts_at,
            ends_at: notice.ends_at,
            dismissible: notice.dismissible,
            created_by_name: notice.created_by_name,
            created_at: notice.created_at,
            state: state.to_owned(),
        }
    }
}

fn notice_state(
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> &'static str {
    if starts_at > now {
        "upcoming"
    } else if ends_at.is_some_and(|end| end <= now) {
        "ended"
    } else {
        "active"
    }
}

/// What staff set on a notice.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NoticeInput {
    /// Plain text, 1–500 characters.
    pub message: String,
    /// `info`, `warning` or `critical`.
    pub severity: String,
    /// When it starts showing; `null` = now.
    pub starts_at: Option<DateTime<Utc>>,
    /// When it stops; `null` = until someone ends it.
    pub ends_at: Option<DateTime<Utc>>,
    /// Whether viewers can hide it. Ignored (off) for `critical`.
    pub dismissible: bool,
}

/// A validated [`NoticeInput`].
#[derive(Debug, PartialEq)]
struct Checked {
    message: String,
    severity: &'static str,
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
    dismissible: bool,
}

impl Checked {
    fn fields(&self) -> db::notices::NoticeFields<'_> {
        db::notices::NoticeFields {
            message: &self.message,
            severity: self.severity,
            starts_at: self.starts_at,
            ends_at: self.ends_at,
            dismissible: self.dismissible,
        }
    }
}

fn check(input: NoticeInput, now: DateTime<Utc>) -> ApiResult<Checked> {
    let message = input.message.trim();
    if message.is_empty() || message.chars().count() > MAX_MESSAGE_LEN {
        return Err(ApiError::bad_request(
            "invalid_message",
            format!("The message is 1 to {MAX_MESSAGE_LEN} characters"),
        ));
    }
    let Some(severity) = SEVERITIES
        .iter()
        .copied()
        .find(|severity| *severity == input.severity)
    else {
        return Err(ApiError::bad_request(
            "invalid_severity",
            "Severity is info, warning or critical",
        ));
    };
    let starts_at = input.starts_at.unwrap_or(now);
    if input.ends_at.is_some_and(|end| end <= starts_at) {
        return Err(ApiError::bad_request(
            "invalid_schedule",
            "A notice has to end after it starts",
        ));
    }
    Ok(Checked {
        message: message.to_owned(),
        severity,
        starts_at,
        ends_at: input.ends_at,
        dismissible: input.dismissible && severity != "critical",
    })
}

fn details(notice: &db::notices::Notice) -> serde_json::Value {
    serde_json::json!({ "message": notice.message, "severity": notice.severity })
}

/// The notices in force now, oldest first.
#[utoipa::path(get, path = "/notices", tag = "notices",
    responses((status = OK, body = Vec<NoticeDto>)))]
async fn list_notices(
    State(state): State<ApiState>,
    _user: CurrentUser,
) -> ApiResult<Json<Vec<NoticeDto>>> {
    let notices = db::notices::active(&state.db).await?;
    Ok(Json(notices.into_iter().map(NoticeDto::from).collect()))
}

/// Every notice, newest first.
#[utoipa::path(get, path = "/admin/notices", tag = "admin",
    responses((status = OK, body = Vec<AdminNoticeDto>)))]
async fn admin_list_notices(
    State(state): State<ApiState>,
    Staff(_staff): Staff,
) -> ApiResult<Json<Vec<AdminNoticeDto>>> {
    let now = Utc::now();
    let notices = db::notices::list(&state.db, MAX_LISTED).await?;
    Ok(Json(
        notices
            .into_iter()
            .map(|notice| AdminNoticeDto::new(notice, now))
            .collect(),
    ))
}

/// Post a notice.
#[utoipa::path(post, path = "/admin/notices", tag = "admin",
    request_body = NoticeInput,
    responses(
        (status = CREATED, body = AdminNoticeDto),
        (status = BAD_REQUEST, description = "`invalid_message`, `invalid_severity`, `invalid_schedule`", body = ErrorBody),
    ))]
async fn admin_create_notice(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Json(input): Json<NoticeInput>,
) -> ApiResult<(StatusCode, Json<AdminNoticeDto>)> {
    let now = Utc::now();
    let checked = check(input, now)?;
    let name = user_display_name(&state, staff.id()).await?;
    let notice = db::notices::create(&state.db, &checked.fields(), staff.id(), &name).await?;
    record(
        &state,
        &staff,
        Action {
            action: "create_notice",
            route: "/admin/notices",
            method: "POST",
            details: details(&notice),
            ..Action::default()
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(AdminNoticeDto::new(notice, now))))
}

/// Change a notice.
#[utoipa::path(put, path = "/admin/notices/{notice_id}", tag = "admin",
    params(("notice_id" = Uuid, Path)),
    request_body = NoticeInput,
    responses(
        (status = OK, body = AdminNoticeDto),
        (status = BAD_REQUEST, description = "`invalid_message`, `invalid_severity`, `invalid_schedule`", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn admin_update_notice(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(notice_id): Path<Uuid>,
    Json(input): Json<NoticeInput>,
) -> ApiResult<Json<AdminNoticeDto>> {
    let now = Utc::now();
    let checked = check(input, now)?;
    let notice = db::notices::update(&state.db, notice_id, &checked.fields())
        .await?
        .ok_or_else(|| ApiError::not_found("Notice not found"))?;
    record(
        &state,
        &staff,
        Action {
            action: "update_notice",
            route: "/admin/notices/{notice_id}",
            method: "PUT",
            details: details(&notice),
            ..Action::default()
        },
    )
    .await?;
    Ok(Json(AdminNoticeDto::new(notice, now)))
}

/// Stop showing a notice now. One that hasn't started is deleted.
#[utoipa::path(post, path = "/admin/notices/{notice_id}/end", tag = "admin",
    params(("notice_id" = Uuid, Path)),
    responses((status = NO_CONTENT), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_end_notice(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(notice_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let notice = db::notices::get(&state.db, notice_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Notice not found"))?;
    match notice_state(notice.starts_at, notice.ends_at, Utc::now()) {
        "upcoming" => {
            db::notices::delete(&state.db, notice_id).await?;
        }
        // Already over (or ended in the meantime): nothing to do.
        _ => {
            db::notices::end_now(&state.db, notice_id).await?;
        }
    }
    record(
        &state,
        &staff,
        Action {
            action: "end_notice",
            route: "/admin/notices/{notice_id}/end",
            method: "POST",
            details: details(&notice),
            ..Action::default()
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete a notice.
#[utoipa::path(delete, path = "/admin/notices/{notice_id}", tag = "admin",
    params(("notice_id" = Uuid, Path)),
    responses((status = NO_CONTENT), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_delete_notice(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(notice_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let notice = db::notices::get(&state.db, notice_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Notice not found"))?;
    db::notices::delete(&state.db, notice_id).await?;
    record(
        &state,
        &staff,
        Action {
            action: "delete_notice",
            route: "/admin/notices/{notice_id}",
            method: "DELETE",
            details: details(&notice),
            ..Action::default()
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    fn input() -> NoticeInput {
        NoticeInput {
            message: "  Maintenance at noon  ".to_owned(),
            severity: "warning".to_owned(),
            starts_at: None,
            ends_at: None,
            dismissible: true,
        }
    }

    fn code(result: ApiResult<Checked>) -> &'static str {
        result.unwrap_err().code
    }

    #[test]
    fn trims_the_message_and_starts_now_by_default() {
        let now = Utc::now();
        let checked = check(input(), now).unwrap();
        assert_eq!(checked.message, "Maintenance at noon");
        assert_eq!(checked.starts_at, now);
        assert_eq!(checked.severity, "warning");
        assert!(checked.dismissible);
    }

    #[test]
    fn critical_notices_cannot_be_dismissed() {
        let checked = check(
            NoticeInput {
                severity: "critical".to_owned(),
                ..input()
            },
            Utc::now(),
        )
        .unwrap();
        assert!(!checked.dismissible);
    }

    #[test]
    fn refuses_bad_messages_severities_and_schedules() {
        let now = Utc::now();
        let empty = NoticeInput {
            message: "   ".to_owned(),
            ..input()
        };
        assert_eq!(code(check(empty, now)), "invalid_message");
        let long = NoticeInput {
            message: "x".repeat(MAX_MESSAGE_LEN + 1),
            ..input()
        };
        assert_eq!(code(check(long, now)), "invalid_message");
        let severity = NoticeInput {
            severity: "loud".to_owned(),
            ..input()
        };
        assert_eq!(code(check(severity, now)), "invalid_severity");
        let backwards = NoticeInput {
            ends_at: Some(now),
            ..input()
        };
        assert_eq!(code(check(backwards, now)), "invalid_schedule");
    }

    #[test]
    fn states() {
        let now = Utc::now();
        let hour = TimeDelta::hours(1);
        assert_eq!(notice_state(now + hour, None, now), "upcoming");
        assert_eq!(notice_state(now - hour, None, now), "active");
        assert_eq!(notice_state(now - hour, Some(now + hour), now), "active");
        assert_eq!(notice_state(now - hour, Some(now), now), "ended");
    }
}
