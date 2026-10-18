use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use db::chat_commands::ChatCommandRole;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{SwitcherAuthorized, SwitcherManage, SwitcherView},
    routes::{
        ApiError, ApiResult, ErrorBody,
        chat_bots::{ChatPlatform, GrantStatus},
    },
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_chat))
        .routes(routes!(set_chat_settings))
        .routes(routes!(set_command))
        .routes(routes!(add_alias))
        .routes(routes!(remove_alias))
        .routes(routes!(unlink_channel, set_channel_link))
        .routes(routes!(ignore_chatter))
        .routes(routes!(unignore_chatter))
}

/// Languages the chat bot can reply in.
pub const LANGUAGES: &[&str] = db::chat_commands::LANGUAGES;

const MAX_PREFIX_LEN: usize = 3;
const MAX_ALIAS_LEN: usize = 25;

/// Who may use a chat command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    Public,
    Subscriber,
    Vip,
    Moderator,
    Admin,
}

impl From<ChatCommandRole> for ChatRole {
    fn from(role: ChatCommandRole) -> Self {
        match role {
            ChatCommandRole::Public => Self::Public,
            ChatCommandRole::Subscriber => Self::Subscriber,
            ChatCommandRole::Vip => Self::Vip,
            ChatCommandRole::Moderator => Self::Moderator,
            ChatCommandRole::Admin => Self::Admin,
        }
    }
}

impl From<ChatRole> for ChatCommandRole {
    fn from(role: ChatRole) -> Self {
        match role {
            ChatRole::Public => Self::Public,
            ChatRole::Subscriber => Self::Subscriber,
            ChatRole::Vip => Self::Vip,
            ChatRole::Moderator => Self::Moderator,
            ChatRole::Admin => Self::Admin,
        }
    }
}

/// Chat settings of a switcher.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChatSettingsDto {
    /// What commands start with, e.g. `!`.
    pub command_prefix: String,
    /// The bot's reply language, e.g. `en` or `pt-br`.
    pub language: String,
    pub public_commands_enabled: bool,
    pub mod_commands_enabled: bool,
    /// Answer commands as replies to them; off, as plain chat messages.
    pub threaded_replies: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChatCommandDto {
    /// The command without its prefix, e.g. `bitrate`.
    pub command: String,
    pub description: String,
    pub default_role: ChatRole,
    /// Who may use it on this switcher (the default unless overridden).
    pub role: ChatRole,
    pub enabled: bool,
    pub aliases: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChatDto {
    pub settings: ChatSettingsDto,
    /// The Twitch/Kick/YouTube channels whose chat controls this switcher.
    pub channels: Vec<ChatChannelDto>,
    /// Chatters whose commands this switcher ignores (e.g. other bots).
    pub ignored: Vec<IgnoredChatterDto>,
    pub commands: Vec<ChatCommandDto>,
    /// Language codes the bot supports.
    pub languages: Vec<String>,
}

/// A chatter a switcher ignores.
#[derive(Debug, Serialize, ToSchema)]
pub struct IgnoredChatterDto {
    pub platform: ChatPlatform,
    pub user_id: String,
    pub username: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<db::chat::IgnoredChatter> for IgnoredChatterDto {
    fn from(ignored: db::chat::IgnoredChatter) -> Self {
        Self {
            platform: ignored.platform.into(),
            user_id: ignored.platform_user_id,
            username: ignored.platform_username,
            created_at: ignored.created_at,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct IgnoreChatter {
    pub platform: ChatPlatform,
    /// Their name in chat (Twitch login, Kick username, YouTube handle).
    pub username: String,
}

/// A channel linked to a switcher.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatChannelDto {
    pub id: Uuid,
    pub platform: ChatPlatform,
    pub login: String,
    pub display_name: String,
    /// `revoked`: the streamer took the bot's access away; add the channel again.
    pub status: GrantStatus,
    pub linked_at: chrono::DateTime<chrono::Utc>,
    /// Who answers there for this switcher; `null` when no bot is connected.
    pub bot: Option<SpeakingBotDto>,
    /// How many other switchers (in any workspace) the channel controls too.
    pub other_links: i64,
    /// This switcher's automatic switches are posted here (while the switcher's
    /// own `auto_switch_notification` is on).
    pub auto_switch_notification: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SpeakingBotDto {
    pub login: String,
    pub display_name: String,
    /// The workspace's own bot rather than the shared NOALBS one.
    pub custom: bool,
    /// The bot of the organization that manages the workspace.
    pub organization: bool,
}

impl From<db::chat::LinkedChannel> for ChatChannelDto {
    fn from(linked: db::chat::LinkedChannel) -> Self {
        let bot = linked
            .bot_login
            .zip(linked.bot_display_name)
            .map(|(login, display_name)| SpeakingBotDto {
                login,
                display_name,
                custom: linked.bot_is_custom,
                organization: linked.bot_is_organization,
            });
        Self {
            id: linked.channel.id,
            platform: linked.channel.platform.into(),
            login: linked.channel.login,
            display_name: linked.channel.display_name,
            status: linked.channel.status.into(),
            linked_at: linked.linked_at,
            bot,
            other_links: linked.other_links,
            auto_switch_notification: linked.auto_switch_notification,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetCommand {
    pub enabled: bool,
    /// `null` restores the command's default role.
    pub role: Option<ChatRole>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddAlias {
    pub alias: String,
    pub command: String,
}

fn settings_of(switcher: &db::switchers::Switcher) -> ChatSettingsDto {
    ChatSettingsDto {
        command_prefix: switcher.command_prefix.clone(),
        language: switcher.language.clone(),
        public_commands_enabled: switcher.public_commands_enabled,
        mod_commands_enabled: switcher.mod_commands_enabled,
        threaded_replies: switcher.threaded_replies,
    }
}

fn validate_settings(settings: &ChatSettingsDto) -> ApiResult<()> {
    let prefix = &settings.command_prefix;
    if prefix.is_empty()
        || prefix.chars().count() > MAX_PREFIX_LEN
        || prefix.chars().any(char::is_whitespace)
    {
        return Err(ApiError::bad_request(
            "invalid_prefix",
            "The prefix must be 1 to 3 characters without spaces",
        ));
    }
    if !LANGUAGES.contains(&settings.language.as_str()) {
        return Err(ApiError::bad_request(
            "invalid_language",
            "Unsupported language",
        ));
    }

    Ok(())
}

/// Normalises an alias (trimmed, lowercase, no prefix) and checks its characters.
pub fn normalize_alias(alias: &str) -> ApiResult<String> {
    let alias = alias.trim().trim_start_matches('!').to_lowercase();
    let valid = !alias.is_empty()
        && alias.len() <= MAX_ALIAS_LEN
        && alias
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');

    if valid {
        Ok(alias)
    } else {
        Err(ApiError::bad_request(
            "invalid_alias",
            "Aliases are 1 to 25 letters, numbers or underscores",
        ))
    }
}

/// Get a switcher's chat settings and commands.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/chat", tag = "chat",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses((status = OK, body = ChatDto), (status = FORBIDDEN, body = ErrorBody)))]
async fn get_chat(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<Json<ChatDto>> {
    let switcher = authorized.into_access().switcher;
    let commands = db::chat_commands::list_for_switcher(&state.db, switcher.id).await?;
    let channels = db::chat::channels_for_switcher(&state.db, switcher.id).await?;
    let ignored = db::chat::ignored_chatters(&state.db, switcher.id).await?;

    Ok(Json(ChatDto {
        settings: settings_of(&switcher),
        channels: channels.into_iter().map(Into::into).collect(),
        ignored: ignored.into_iter().map(Into::into).collect(),
        commands: commands
            .into_iter()
            .map(|command| ChatCommandDto {
                role: command.min_role.unwrap_or(command.default_role).into(),
                enabled: command.enabled.unwrap_or(true),
                command: command.command,
                description: command.description,
                default_role: command.default_role.into(),
                aliases: command.aliases,
            })
            .collect(),
        languages: LANGUAGES.iter().map(|code| (*code).to_owned()).collect(),
    }))
}

/// Replace a switcher's chat settings.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/settings", tag = "chat",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = ChatSettingsDto,
    responses((status = OK, body = ChatSettingsDto), (status = BAD_REQUEST, body = ErrorBody)))]
async fn set_chat_settings(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<ChatSettingsDto>,
) -> ApiResult<Json<ChatSettingsDto>> {
    validate_settings(&request)?;
    let switcher = db::switchers::set_chat_settings(
        &state.db,
        authorized.into_access().switcher.id,
        &db::switchers::ChatSettings {
            command_prefix: request.command_prefix,
            language: request.language,
            public_commands_enabled: request.public_commands_enabled,
            mod_commands_enabled: request.mod_commands_enabled,
            threaded_replies: request.threaded_replies,
        },
    )
    .await?;

    Ok(Json(settings_of(&switcher)))
}

/// Turn a command on or off and set who may use it.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/commands/{command}", tag = "chat",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("command" = String, Path)),
    request_body = SetCommand,
    responses((status = NO_CONTENT, description = "Saved"), (status = NOT_FOUND, body = ErrorBody)))]
async fn set_command(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, String)>,
    Json(request): Json<SetCommand>,
) -> ApiResult<StatusCode> {
    let (_, _, command) = path;
    if !db::chat_commands::command_exists(&state.db, &command).await? {
        return Err(ApiError::not_found("Unknown command"));
    }

    db::chat_commands::set_override(
        &state.db,
        authorized.into_access().switcher.id,
        &command,
        request.enabled,
        request.role.map(Into::into),
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Add another name for a command, e.g. `!b` for `!bitrate`.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/aliases", tag = "chat",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = AddAlias,
    responses(
        (status = NO_CONTENT, description = "Added"),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = CONFLICT, description = "`alias_taken`", body = ErrorBody),
    ))]
async fn add_alias(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<AddAlias>,
) -> ApiResult<StatusCode> {
    let alias = normalize_alias(&request.alias)?;
    if !db::chat_commands::command_exists(&state.db, &request.command).await? {
        return Err(ApiError::not_found("Unknown command"));
    }
    if db::chat_commands::command_exists(&state.db, &alias).await? {
        return Err(ApiError::conflict(
            "alias_taken",
            "That's already the name of a command",
        ));
    }

    db::chat_commands::add_alias(
        &state.db,
        authorized.into_access().switcher.id,
        &alias,
        &request.command,
    )
    .await
    .map_err(|error| {
        if error.is_unique_violation() {
            ApiError::conflict("alias_taken", "This alias is already used")
        } else {
            error.into()
        }
    })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Remove an alias.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/aliases/{alias}", tag = "chat",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("alias" = String, Path)),
    responses((status = NO_CONTENT, description = "Removed"), (status = NOT_FOUND, body = ErrorBody)))]
async fn remove_alias(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, String)>,
) -> ApiResult<StatusCode> {
    let (_, _, alias) = path;

    if db::chat_commands::remove_alias(&state.db, authorized.into_access().switcher.id, &alias)
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Alias not found"))
    }
}

/// Remove a channel from this switcher. Other switchers using it keep it.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/channels/{channel_id}", tag = "chat",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("channel_id" = Uuid, Path)),
    responses((status = NO_CONTENT, description = "Removed"), (status = NOT_FOUND, body = ErrorBody)))]
async fn unlink_channel(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let (_, _, channel_id) = path;
    let switcher_id = authorized.into_access().switcher.id;
    if !db::chat::unlink_channel(&state.db, switcher_id, channel_id).await? {
        return Err(ApiError::not_found("Channel not linked"));
    }
    state.notify_chat().await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChannelLinkSettings {
    /// Post this switcher's automatic switches in the channel.
    pub auto_switch_notification: bool,
}

/// Change how a channel works for this switcher (other switchers using it keep
/// their own settings).
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/channels/{channel_id}", tag = "chat",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("channel_id" = Uuid, Path)),
    request_body = ChannelLinkSettings,
    responses((status = NO_CONTENT, description = "Saved"), (status = NOT_FOUND, body = ErrorBody)))]
async fn set_channel_link(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<ChannelLinkSettings>,
) -> ApiResult<StatusCode> {
    let (_, _, channel_id) = path;
    let switcher_id = authorized.into_access().switcher.id;
    if !db::chat::set_link_notification(
        &state.db,
        switcher_id,
        channel_id,
        request.auto_switch_notification,
    )
    .await?
    {
        return Err(ApiError::not_found("Channel not linked"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Ignore a chatter's commands on this switcher. The name is looked up on the
/// platform, so they stay ignored if they rename.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/ignored", tag = "chat",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = IgnoreChatter,
    responses(
        (status = CREATED, body = IgnoredChatterDto),
        (status = NOT_FOUND, description = "`user_not_found`", body = ErrorBody),
    ))]
async fn ignore_chatter(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<IgnoreChatter>,
) -> ApiResult<(StatusCode, Json<IgnoredChatterDto>)> {
    let access = authorized.into_access();
    state.limits.outbound(access.workspace.user.id())?;
    let switcher_id = access.switcher.id;
    let name = request.username.trim().trim_start_matches('@');
    // YouTube handles are at most 30 characters and may hold dots.
    let youtube = request.platform == ChatPlatform::YouTube;
    let valid = !name.is_empty()
        && name.len() <= if youtube { 30 } else { 50 }
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || (youtube && c == '.'));
    if !valid {
        return Err(ApiError::bad_request(
            "invalid_username",
            "That isn't a username",
        ));
    }
    let (platform, login_platform) = (request.platform.db(), request.platform.login());
    let (user_id, username) = state
        .oauth
        .find_user(&state.http, login_platform, name)
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "user_not_found",
                "No user has that name",
            )
        })?;
    db::chat::ignore_chatter(&state.db, switcher_id, platform, &user_id, &username).await?;

    Ok((
        StatusCode::CREATED,
        Json(IgnoredChatterDto {
            platform: request.platform,
            user_id,
            username,
            created_at: chrono::Utc::now(),
        }),
    ))
}

/// Stop ignoring a chatter.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/ignored/{platform}/{user_id}", tag = "chat",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("platform" = ChatPlatform, Path), ("user_id" = String, Path)),
    responses((status = NO_CONTENT, description = "Removed"), (status = NOT_FOUND, body = ErrorBody)))]
async fn unignore_chatter(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, ChatPlatform, String)>,
) -> ApiResult<StatusCode> {
    let (_, _, platform, user_id) = path;
    let platform = platform.db();
    if !db::chat::unignore_chatter(
        &state.db,
        authorized.into_access().switcher.id,
        platform,
        &user_id,
    )
    .await?
    {
        return Err(ApiError::not_found("Not ignored"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::{ChatSettingsDto, normalize_alias, validate_settings};

    fn settings(prefix: &str, language: &str) -> ChatSettingsDto {
        ChatSettingsDto {
            command_prefix: prefix.into(),
            language: language.into(),
            public_commands_enabled: true,
            mod_commands_enabled: true,
            threaded_replies: true,
        }
    }

    #[test]
    fn validates_prefix_and_language() {
        assert!(validate_settings(&settings("!", "en")).is_ok());
        assert!(validate_settings(&settings("?!", "pt-br")).is_ok());
        assert!(validate_settings(&settings("", "en")).is_err());
        assert!(validate_settings(&settings("! ", "en")).is_err());
        assert!(validate_settings(&settings("!!!!", "en")).is_err());
        assert!(validate_settings(&settings("!", "klingon")).is_err());
    }

    #[test]
    fn normalizes_aliases() {
        assert_eq!(normalize_alias(" !BR ").unwrap(), "br");
        assert_eq!(normalize_alias("stream_info2").unwrap(), "stream_info2");
        assert!(normalize_alias("").is_err());
        assert!(normalize_alias("two words").is_err());
        assert!(normalize_alias("émoji").is_err());
    }
}
