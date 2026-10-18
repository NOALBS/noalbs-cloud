//! Chat commands: parsing a message, finding the command (by name or one of the
//! switcher's aliases), and who may use it. [`run`] carries them out.

pub mod fuzzy;
pub mod run;

use db::chat_commands::{ChatCommandRole, SwitcherCommand};

use crate::inbound::Chatter;

/// How much a chatter may do. Ranked: each role may use what the ones before
/// it may.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Public,
    /// Has the channel's subscriber (or founder) badge.
    Subscriber,
    /// Has the channel's VIP badge.
    Vip,
    /// Has the channel's moderator badge.
    Moderator,
    /// The broadcaster, or a member of the switcher's workspace (by the chat
    /// account they linked to their NOALBS login).
    Admin,
}

impl From<ChatCommandRole> for Role {
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

/// A message that starts with the prefix: the command word (lowercase, as
/// typed, maybe an alias) and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub word: String,
    pub args: Vec<String>,
}

pub fn parse(text: &str, prefix: &str) -> Option<Invocation> {
    let rest = text.trim().strip_prefix(prefix)?;
    let mut words = rest.split_whitespace();
    let word = words.next()?.to_lowercase();
    Some(Invocation {
        word,
        args: words.map(str::to_owned).collect(),
    })
}

/// The command a word names, by name or alias.
pub fn resolve<'a>(word: &str, commands: &'a [SwitcherCommand]) -> Option<&'a SwitcherCommand> {
    commands
        .iter()
        .find(|command| command.command == word)
        .or_else(|| {
            commands
                .iter()
                .find(|command| command.aliases.iter().any(|alias| alias == word))
        })
}

/// The highest role a chatter's badges give them (admins are found by
/// account, not badge). With the switcher's mod commands off, the moderator
/// badge counts for nothing, so a moderator is whatever else they are.
pub fn badge_role(chatter: &Chatter, mod_enabled: bool) -> Role {
    if chatter.moderator && mod_enabled {
        Role::Moderator
    } else if chatter.vip {
        Role::Vip
    } else if chatter.subscriber {
        Role::Subscriber
    } else {
        Role::Public
    }
}

/// Whether `role` may use `command`, given the switcher's public switch: with
/// public commands off, those need a moderator.
pub fn allowed(role: Role, command: &SwitcherCommand, public_enabled: bool) -> bool {
    if command.enabled == Some(false) {
        return false;
    }
    let mut required = Role::from(command.min_role.unwrap_or(command.default_role));
    if required == Role::Public && !public_enabled {
        required = Role::Moderator;
    }
    role >= required
}

/// Normalises an alias the way the API does: trimmed, lowercase, no `!`, 1 to
/// 25 letters, numbers or underscores.
pub fn normalize_alias(alias: &str) -> Option<String> {
    let alias = alias.trim().trim_start_matches('!').to_lowercase();
    let valid = !alias.is_empty()
        && alias.len() <= 25
        && alias
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    valid.then_some(alias)
}

/// "on"/"off" and friends; `None` for anything else.
pub fn on_off(arg: &str) -> Option<bool> {
    match arg.to_lowercase().as_str() {
        "on" | "true" | "yes" | "enable" | "enabled" | "1" => Some(true),
        "off" | "false" | "no" | "disable" | "disabled" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use db::chat_commands::{ChatCommandRole, SwitcherCommand};

    use super::{Invocation, Role, allowed, badge_role, normalize_alias, parse, resolve};
    use crate::inbound::Chatter;

    fn command(name: &str, role: ChatCommandRole, aliases: &[&str]) -> SwitcherCommand {
        SwitcherCommand {
            command: name.into(),
            description: String::new(),
            default_role: role,
            enabled: None,
            min_role: None,
            aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        }
    }

    #[test]
    fn messages_with_the_prefix_are_commands() {
        assert_eq!(
            parse("  !Switch  Main Cam ", "!"),
            Some(Invocation {
                word: "switch".into(),
                args: vec!["Main".into(), "Cam".into()]
            })
        );
        assert_eq!(parse("?? bitrate", "??").unwrap().word, "bitrate");
        assert_eq!(parse("hello !bitrate", "!"), None);
        assert_eq!(parse("!", "!"), None);
    }

    #[test]
    fn aliases_resolve_to_their_command() {
        let commands = [
            command("bitrate", ChatCommandRole::Public, &["b"]),
            command("switch", ChatCommandRole::Admin, &["ss"]),
        ];
        assert_eq!(resolve("ss", &commands).unwrap().command, "switch");
        assert_eq!(resolve("bitrate", &commands).unwrap().command, "bitrate");
        assert!(resolve("nope", &commands).is_none());
    }

    fn chatter(moderator: bool, vip: bool, subscriber: bool) -> Chatter {
        Chatter {
            id: "5".into(),
            login: "viewer".into(),
            display_name: "Viewer".into(),
            moderator,
            vip,
            subscriber,
        }
    }

    #[test]
    fn roles_overrides_and_the_public_switch() {
        let bitrate = command("bitrate", ChatCommandRole::Public, &[]);
        let trigger = command("trigger", ChatCommandRole::Moderator, &[]);
        let start = command("start", ChatCommandRole::Admin, &[]);

        assert!(allowed(Role::Public, &bitrate, true));
        assert!(!allowed(Role::Public, &trigger, true));
        assert!(allowed(Role::Moderator, &trigger, true));
        assert!(!allowed(Role::Moderator, &start, true));
        assert!(allowed(Role::Admin, &start, true));

        // Public commands off: moderators still may, subscribers and VIPs not.
        assert!(!allowed(Role::Public, &bitrate, false));
        assert!(!allowed(Role::Vip, &bitrate, false));
        assert!(allowed(Role::Moderator, &bitrate, false));

        // Per-switcher overrides.
        let opened = SwitcherCommand {
            min_role: Some(ChatCommandRole::Public),
            ..start.clone()
        };
        assert!(allowed(Role::Public, &opened, true));
        let disabled = SwitcherCommand {
            enabled: Some(false),
            ..bitrate
        };
        assert!(!allowed(Role::Admin, &disabled, true));
    }

    #[test]
    fn subscribers_rank_below_vips_below_moderators() {
        let for_subs = SwitcherCommand {
            min_role: Some(ChatCommandRole::Subscriber),
            ..command("bitrate", ChatCommandRole::Public, &[])
        };
        let for_vips = SwitcherCommand {
            min_role: Some(ChatCommandRole::Vip),
            ..command("switch", ChatCommandRole::Admin, &[])
        };
        assert!(!allowed(Role::Public, &for_subs, true));
        assert!(allowed(Role::Subscriber, &for_subs, true));
        assert!(allowed(Role::Vip, &for_subs, true));
        assert!(!allowed(Role::Subscriber, &for_vips, true));
        assert!(allowed(Role::Vip, &for_vips, true));
        assert!(allowed(Role::Moderator, &for_vips, true));
        // The public switch is about "everyone" commands only.
        assert!(allowed(Role::Subscriber, &for_subs, false));
    }

    #[test]
    fn badges_give_the_highest_role_and_mod_commands_off_ignores_the_mod_badge() {
        assert_eq!(
            badge_role(&chatter(false, false, false), true),
            Role::Public
        );
        assert_eq!(
            badge_role(&chatter(false, false, true), true),
            Role::Subscriber
        );
        assert_eq!(badge_role(&chatter(false, true, true), true), Role::Vip);
        assert_eq!(
            badge_role(&chatter(true, false, true), true),
            Role::Moderator
        );
        // Mod commands off: a moderator is whatever else they are.
        assert_eq!(
            badge_role(&chatter(true, false, false), false),
            Role::Public
        );
        assert_eq!(
            badge_role(&chatter(true, false, true), false),
            Role::Subscriber
        );
    }

    #[test]
    fn aliases_are_normalised_like_the_dashboard_does() {
        assert_eq!(normalize_alias("!BR"), Some("br".into()));
        assert_eq!(normalize_alias("two words"), None);
        assert_eq!(normalize_alias(""), None);
    }
}
