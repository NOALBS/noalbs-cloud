//! Carrying out a command for one switcher. OBS actions go through
//! [`control::Control`], so the pause rules and the activity feed ("by <chatter>
//! (Twitch chat)") work as for the dashboard.

use std::time::Duration;

use db::{
    chat::ChatPlatform,
    switchers::{ChatFlag, Switcher, Triggers},
};
use live::{
    BelaboxAction, BelaboxLink, BelaboxRefusal, BelaboxStatus, FeedState, MoblinAction,
    MoblinStatus, StatsSample,
};
use uuid::Uuid;

use super::{fuzzy, normalize_alias, on_off};
use crate::{
    app::State,
    messages::{Msg, ServerReading},
};

/// A stats sample older than this doesn't count as "now".
const FRESH_STATS: Duration = Duration::from_secs(10);

pub struct Ctx<'a> {
    pub state: &'a State,
    pub switcher: Switcher,
    pub platform: ChatPlatform,
    /// Who did it, for the activity feed.
    pub by: String,
}

/// belabot's commands; silent for switchers without BELABOX Cloud (it's optional).
const BELABOX_COMMANDS: &[&str] = &["bbstart", "bbstop", "bbb", "bbt", "bbs", "bbsensor"];

/// Moblin's commands; silent for switchers without Moblin (it's optional).
const MOBLIN_COMMANDS: &[&str] = &["mstart", "mstop", "mbitrate", "mscene", "mstats"];

/// Runs `command` (its real name) with `args`; the reply, if any.
pub async fn run(ctx: &Ctx<'_>, command: &str, args: &[String]) -> Option<Msg> {
    if BELABOX_COMMANDS.contains(&command) && ctx.switcher.belabox_remote_key.is_none() {
        return None;
    }
    if MOBLIN_COMMANDS.contains(&command) && ctx.switcher.moblin_password.is_none() {
        return None;
    }
    let reply = match command {
        "mstart" => moblin_stream(ctx, true).await,
        "mstop" => moblin_stream(ctx, false).await,
        "mbitrate" => moblin_bitrate(ctx, args).await,
        "mscene" => moblin_scene(ctx, args).await,
        "mstats" => moblin_stats(ctx).await,
        "bbstart" => belabox_stream(ctx, true).await,
        "bbstop" => belabox_stream(ctx, false).await,
        "bbb" => belabox_bitrate(ctx, args).await,
        "bbt" => belabox_interface(ctx, args).await,
        "bbs" => belabox_stats(ctx).await,
        "bbsensor" => belabox_sensors(ctx).await,
        "bitrate" | "sourceinfo" => bitrate(ctx).await,
        "serverinfo" => server_info(ctx).await,
        "switch" => switch(ctx, args).await,
        "live" => switch_to(ctx, ctx.switcher.scene_live, "LIVE").await,
        "privacy" => switch_to(ctx, ctx.switcher.scene_privacy, "Privacy").await,
        "starting" => switch_to(ctx, ctx.switcher.scene_starting, "Starting").await,
        "ending" => switch_to(ctx, ctx.switcher.scene_ending, "Ending").await,
        "refresh" => refresh(ctx).await,
        "fix" => fix(ctx).await,
        "source" => source(ctx, args).await,
        "marker" => marker(ctx, args).await,
        "start" => stream(ctx, true).await,
        "stop" => stream(ctx, false).await,
        "record" => record(ctx).await,
        "notify" => flag(ctx, args, ChatFlag::AutoSwitchNotification).await,
        "autostop" => flag(ctx, args, ChatFlag::AutoStopOnRaid).await,
        "public" => flag(ctx, args, ChatFlag::PublicCommands).await,
        "mod" => flag(ctx, args, ChatFlag::ModCommands).await,
        "trigger" | "otrigger" | "rtrigger" | "ortrigger" => trigger(ctx, command, args).await,
        "alias" => alias(ctx, args).await,
        "noalbs" => noalbs(ctx, args).await,
        other => Ok(Msg::NotAvailable(format!(
            "{}{other}",
            ctx.switcher.command_prefix
        ))),
    };
    match reply {
        Ok(msg) => Some(msg),
        Err(error) => Some(failure(error)),
    }
}

type Reply = Result<Msg, control::Error>;

fn failure(error: control::Error) -> Msg {
    match error {
        control::Error::Paused => Msg::Paused,
        control::Error::NotStreaming => Msg::NotStreaming,
        // `marker` cuts the description to fit.
        control::Error::InvalidMarker => Msg::Failed,
        control::Error::ObsOffline => Msg::ObsOffline,
        control::Error::ObsRefused { code, comment } => {
            Msg::ObsRefused(comment.unwrap_or_else(|| format!("code {code}")))
        }
        control::Error::ObsUnreachable(_) => Msg::ObsTimeout,
        control::Error::NoRefreshScene => Msg::NoScene("Refresh"),
        control::Error::AlreadyRefreshing => Msg::AlreadyRefreshing,
        control::Error::NoSceneIds => Msg::NoSceneIds,
        control::Error::SourceNotFound
        | control::Error::InputNotFound
        | control::Error::InvalidVolume => Msg::Failed,
        // Checked before running BELABOX commands; the key was removed meanwhile.
        control::Error::BelaboxNotSetUp => Msg::Failed,
        control::Error::BelaboxUnreachable => Msg::BelaboxUnreachable,
        control::Error::BelaboxRefused(refusal) => Msg::BelaboxRefused(refusal),
        // Checked before running Moblin commands; the password was removed meanwhile.
        control::Error::MoblinNotSetUp => Msg::Failed,
        control::Error::MoblinUnreachable => Msg::MoblinUnreachable,
        control::Error::MoblinRefused(refusal) => Msg::MoblinRefused(refusal),
        control::Error::Db(error) => {
            tracing::warn!(%error, "A chat command failed");
            Msg::Failed
        }
    }
}

/// The switcher's latest stats sample, if it's recent.
async fn last_sample(state: &State, switcher_id: Uuid) -> Option<StatsSample> {
    let message = state
        .stats
        .get_last_raw_message_by_subject(&live::stats_subject(switcher_id))
        .await
        .ok()?;
    let sample: StatsSample = serde_json::from_slice(&message.payload).ok()?;
    let age = chrono::Utc::now() - sample.at;
    (age.to_std().unwrap_or_default() <= FRESH_STATS).then_some(sample)
}

async fn bitrate(ctx: &Ctx<'_>) -> Reply {
    let Some(sample) = last_sample(ctx.state, ctx.switcher.id).await else {
        return Ok(Msg::NoStats);
    };
    let servers = db::stream_servers::by_switcher_id(&ctx.state.db, ctx.switcher.id).await?;
    let readings = sample
        .servers
        .iter()
        .filter_map(|reading| {
            let server = servers
                .iter()
                .find(|server| server.id == reading.server_id)?;
            let value = match reading.state {
                FeedState::Live => ServerReading::Live {
                    bitrate: reading.bitrate,
                    rtt: reading.rtt,
                },
                FeedState::Low => ServerReading::Low {
                    bitrate: reading.bitrate,
                    rtt: reading.rtt,
                },
                FeedState::Offline | FeedState::Idle => ServerReading::Offline,
            };
            Some((server.name.clone(), value))
        })
        .collect::<Vec<_>>();
    Ok(if readings.is_empty() {
        Msg::NoStats
    } else {
        Msg::Bitrate(readings)
    })
}

async fn server_info(ctx: &Ctx<'_>) -> Reply {
    let (stats, bitrate) = ctx
        .state
        .control
        .obs_stats_with_bitrate(ctx.switcher.id)
        .await?;
    let scene = ctx
        .state
        .control
        .status(ctx.switcher.id)
        .await
        .and_then(|status| status.current_scene)
        .map(|scene| scene.name);
    Ok(Msg::ServerInfo {
        scene,
        bitrate,
        stats: Box::new(stats),
    })
}

async fn switch(ctx: &Ctx<'_>, args: &[String]) -> Reply {
    let query = args.join(" ");
    if query.is_empty() {
        return Ok(Msg::Usage(format!(
            "{}switch <scene name>",
            ctx.switcher.command_prefix
        )));
    }
    let obs = ctx.state.control.connected_obs(ctx.switcher.id).await?;
    let list = obs.scene_list().await?;
    let names: Vec<String> = list
        .scenes
        .iter()
        .map(|scene| scene.scene_name.clone())
        .collect();
    let Some(index) = fuzzy::best_match(&query, &names) else {
        return Ok(Msg::NoSceneMatch(query));
    };
    let scene = &list.scenes[index];
    let uuid = scene.scene_uuid.ok_or(control::Error::NoSceneIds)?;
    ctx.state
        .control
        .switch_scene(ctx.switcher.id, uuid, &ctx.by)
        .await?;
    Ok(Msg::SwitchedTo(scene.scene_name.clone()))
}

async fn switch_to(ctx: &Ctx<'_>, scene: Option<Uuid>, kind: &'static str) -> Reply {
    let Some(uuid) = scene else {
        return Ok(Msg::NoScene(kind));
    };
    ctx.state
        .control
        .switch_scene(ctx.switcher.id, uuid, &ctx.by)
        .await?;
    Ok(Msg::SwitchedTo(format!("the {kind} scene")))
}

/// Without a Refresh scene it fixes instead, like NOALBS.
async fn refresh(ctx: &Ctx<'_>) -> Reply {
    match ctx.state.control.refresh(&ctx.switcher, &ctx.by).await {
        Ok(()) => Ok(Msg::Refreshing),
        Err(control::Error::NoRefreshScene) => Ok(Msg::FixInstead(Box::new(fix(ctx).await?))),
        Err(error) => Err(error),
    }
}

async fn fix(ctx: &Ctx<'_>) -> Reply {
    let (scene, fixed) = ctx.state.control.fix(ctx.switcher.id, &ctx.by).await?;
    Ok(if fixed.is_empty() {
        Msg::NothingToFix(scene)
    } else {
        Msg::Fixed(fixed)
    })
}

/// Shows or hides the source on the scene on air (nested scenes and groups
/// included) whose name best matches, like NOALBS's `!source`.
async fn source(ctx: &Ctx<'_>, args: &[String]) -> Reply {
    let query = args.join(" ");
    if query.is_empty() {
        return Ok(Msg::Usage(format!(
            "{}source <source name>",
            ctx.switcher.command_prefix
        )));
    }
    let (_, items) = ctx.state.control.sources(ctx.switcher.id).await?;
    let names: Vec<String> = items.iter().map(|item| item.name.clone()).collect();
    let Some(index) = fuzzy::best_match(&query, &names) else {
        return Ok(Msg::NoSourceMatch(query));
    };
    let item = &items[index];
    let enabled = !item.enabled;
    let name = ctx
        .state
        .control
        .set_source_enabled(
            ctx.switcher.id,
            &item.parent,
            item.parent_is_group,
            item.item_id,
            enabled,
            &ctx.by,
        )
        .await?;
    Ok(Msg::SourceToggled { name, enabled })
}

/// `!marker [description]`: marks this moment on the running stream.
async fn marker(ctx: &Ctx<'_>, args: &[String]) -> Reply {
    let description: String = args
        .join(" ")
        .chars()
        .take(control::MAX_MARKER_CHARS)
        .collect();
    let created = ctx
        .state
        .control
        .create_marker(ctx.switcher.id, Some(&description), &ctx.by)
        .await?;
    Ok(Msg::MarkerAdded {
        offset_seconds: created.offset_seconds,
    })
}

/// The BELABOX's status, if it's online.
async fn online_belabox(ctx: &Ctx<'_>) -> Result<BelaboxStatus, control::Error> {
    let status = ctx
        .state
        .control
        .belabox_status(ctx.switcher.id)
        .await
        .ok_or(control::Error::BelaboxUnreachable)?;
    let refusal = match status.link {
        BelaboxLink::Online => return Ok(status),
        BelaboxLink::Connecting => BelaboxRefusal::Connecting,
        BelaboxLink::EncoderOffline => BelaboxRefusal::EncoderOffline,
        BelaboxLink::InvalidKey => BelaboxRefusal::InvalidKey,
    };
    Err(control::Error::BelaboxRefused(refusal))
}

async fn belabox_stream(ctx: &Ctx<'_>, start: bool) -> Reply {
    let action = if start {
        BelaboxAction::Start
    } else {
        BelaboxAction::Stop
    };
    ctx.state
        .control
        .belabox(ctx.switcher.id, action, &ctx.by)
        .await?;
    Ok(if start {
        Msg::BelaboxStarting
    } else {
        Msg::BelaboxStopping
    })
}

/// `!bbb` says the max bitrate, `!bbb <kbps>` sets it (while BELABOX isn't
/// streaming, for its next start).
async fn belabox_bitrate(ctx: &Ctx<'_>, args: &[String]) -> Reply {
    let Some(arg) = args.first() else {
        return Ok(Msg::BelaboxBitrate(online_belabox(ctx).await?.max_br));
    };
    let Ok(max_br) = arg.trim_end_matches("kbps").parse::<u32>() else {
        return Ok(Msg::Usage(format!(
            "{}bbb <{}-{} kbps>",
            ctx.switcher.command_prefix,
            live::BELABOX_MIN_BITRATE,
            live::BELABOX_MAX_BITRATE
        )));
    };
    // On BELABOX's 250 kbps grid, like belabot.
    let max_br = max_br.saturating_add(125) / 250 * 250;
    ctx.state
        .control
        .belabox(ctx.switcher.id, BelaboxAction::Bitrate { max_br }, &ctx.by)
        .await?;
    Ok(Msg::BelaboxBitrateSet(max_br))
}

/// `!bbt <interface>` turns a network interface off, or back on. It finds the
/// interface by the streamer's name for it or by its own (`usb0`).
async fn belabox_interface(ctx: &Ctx<'_>, args: &[String]) -> Reply {
    let query = args.join(" ");
    if query.is_empty() {
        return Ok(Msg::Usage(format!(
            "{}bbt <interface, e.g. usb0>",
            ctx.switcher.command_prefix
        )));
    }
    let status = online_belabox(ctx).await?;
    let labels = ctx.switcher.interface_labels();
    // Every name an interface answers to, and which interface it is.
    let mut names = Vec::new();
    let mut owners = Vec::new();
    for (index, interface) in status.interfaces.iter().enumerate() {
        if let Some(label) = labels.get(&interface.key) {
            names.push(label.clone());
            owners.push(index);
        }
        names.push(interface.name.clone());
        owners.push(index);
    }
    let Some(index) = fuzzy::best_match(&query, &names).map(|found| owners[found]) else {
        return Ok(Msg::NoInterfaceMatch(query));
    };
    let interface = &status.interfaces[index];
    let enabled = !interface.enabled;
    ctx.state
        .control
        .belabox(
            ctx.switcher.id,
            BelaboxAction::Interface {
                name: interface.name.clone(),
                enabled,
            },
            &ctx.by,
        )
        .await?;
    Ok(Msg::BelaboxInterfaceToggled {
        name: interface.display_name(&labels).to_owned(),
        enabled,
    })
}

async fn belabox_stats(ctx: &Ctx<'_>) -> Reply {
    let status = online_belabox(ctx).await?;
    Ok(Msg::BelaboxStats {
        streaming: status.streaming,
        max_br: status.max_br,
        total_kbps: status.total_kbps,
        interfaces: status.interfaces,
        labels: ctx.switcher.interface_labels(),
    })
}

async fn belabox_sensors(ctx: &Ctx<'_>) -> Reply {
    let status = online_belabox(ctx).await?;
    Ok(Msg::BelaboxSensors(
        status
            .sensors
            .into_iter()
            .map(|sensor| (sensor.name, sensor.value))
            .collect(),
    ))
}

/// The connected Moblin's status.
async fn connected_moblin(ctx: &Ctx<'_>) -> Result<MoblinStatus, control::Error> {
    ctx.state
        .control
        .moblin_status(ctx.switcher.id)
        .await
        .ok_or(control::Error::MoblinUnreachable)
}

async fn moblin_stream(ctx: &Ctx<'_>, start: bool) -> Reply {
    ctx.state
        .control
        .moblin(
            ctx.switcher.id,
            MoblinAction::Stream { active: start },
            &ctx.by,
        )
        .await?;
    Ok(if start {
        Msg::MoblinStarting
    } else {
        Msg::MoblinStopping
    })
}

/// `!mbitrate` says the preset in use and the presets, `!mbitrate <kbps>` picks
/// the preset with that bitrate.
async fn moblin_bitrate(ctx: &Ctx<'_>, args: &[String]) -> Reply {
    let status = connected_moblin(ctx).await?;
    let presets: Vec<u32> = status.bitrate_presets.iter().map(|p| p.kbps).collect();
    let Some(arg) = args.first() else {
        return Ok(Msg::MoblinBitrate {
            current: status.bitrate_preset().map(|preset| preset.kbps),
            presets,
        });
    };
    let Ok(wanted) = arg.trim_end_matches("kbps").replace(',', "").parse::<u32>() else {
        return Ok(Msg::Usage(format!(
            "{}mbitrate <kbps of a preset>",
            ctx.switcher.command_prefix
        )));
    };
    let Some(preset) = status.bitrate_presets.iter().find(|p| p.kbps == wanted) else {
        return Ok(Msg::NoMoblinPreset {
            kbps: wanted,
            presets,
        });
    };
    ctx.state
        .control
        .moblin(
            ctx.switcher.id,
            MoblinAction::BitratePreset {
                id: preset.id.clone(),
            },
            &ctx.by,
        )
        .await?;
    Ok(Msg::MoblinBitrateSet(preset.kbps))
}

/// `!mscene` says the scene on air and the scenes, `!mscene <name>` switches.
async fn moblin_scene(ctx: &Ctx<'_>, args: &[String]) -> Reply {
    let status = connected_moblin(ctx).await?;
    let names: Vec<String> = status.scenes.iter().map(|s| s.name.clone()).collect();
    let query = args.join(" ");
    if query.is_empty() {
        return Ok(Msg::MoblinScenes {
            current: status.scene_name().map(str::to_owned),
            scenes: names,
        });
    }
    let Some(index) = fuzzy::best_match(&query, &names) else {
        return Ok(Msg::NoMoblinSceneMatch(query));
    };
    let scene = &status.scenes[index];
    ctx.state
        .control
        .moblin(
            ctx.switcher.id,
            MoblinAction::Scene {
                id: scene.id.clone(),
            },
            &ctx.by,
        )
        .await?;
    Ok(Msg::MoblinSceneSet(scene.name.clone()))
}

async fn moblin_stats(ctx: &Ctx<'_>) -> Reply {
    Ok(Msg::MoblinStats(Box::new(connected_moblin(ctx).await?)))
}

async fn stream(ctx: &Ctx<'_>, start: bool) -> Reply {
    let status = ctx
        .state
        .control
        .status(ctx.switcher.id)
        .await
        .filter(|status| status.obs_connected)
        .ok_or(control::Error::ObsOffline)?;
    if status.streaming == start {
        return Ok(if start {
            Msg::AlreadyStreaming
        } else {
            Msg::NotStreaming
        });
    }
    ctx.state
        .control
        .stream(ctx.switcher.id, start, &ctx.by)
        .await?;
    Ok(if start {
        Msg::StreamStarting
    } else {
        Msg::StreamStopping
    })
}

async fn record(ctx: &Ctx<'_>) -> Reply {
    let status = ctx
        .state
        .control
        .status(ctx.switcher.id)
        .await
        .filter(|status| status.obs_connected)
        .ok_or(control::Error::ObsOffline)?;
    let start = !status.recording;
    ctx.state
        .control
        .record(ctx.switcher.id, start, &ctx.by)
        .await?;
    Ok(if start {
        Msg::RecordingStarting
    } else {
        Msg::RecordingStopping
    })
}

async fn flag(ctx: &Ctx<'_>, args: &[String], flag: ChatFlag) -> Reply {
    let (label, current) = match flag {
        ChatFlag::AutoSwitchNotification => {
            ("Chat notifications", ctx.switcher.auto_switch_notification)
        }
        ChatFlag::AutoStopOnRaid => ("Auto stop on raid", ctx.switcher.auto_stop_on_raid),
        ChatFlag::PublicCommands => ("Public commands", ctx.switcher.public_commands_enabled),
        ChatFlag::ModCommands => ("Mod commands", ctx.switcher.mod_commands_enabled),
    };
    let value = match args.first() {
        None => !current,
        Some(arg) => match on_off(arg) {
            Some(value) => value,
            None => {
                return Ok(Msg::Usage(format!(
                    "{}{} [on|off]",
                    ctx.switcher.command_prefix,
                    match flag {
                        ChatFlag::AutoSwitchNotification => "notify",
                        ChatFlag::AutoStopOnRaid => "autostop",
                        ChatFlag::PublicCommands => "public",
                        ChatFlag::ModCommands => "mod",
                    }
                )));
            }
        },
    };
    db::switchers::set_chat_flag(&ctx.state.db, ctx.switcher.id, flag, value).await?;
    if flag == ChatFlag::AutoStopOnRaid {
        // Raids out are only subscribed to where something stops on them.
        ctx.state.request_reconcile().await;
    }
    Ok(Msg::Toggled(label, value))
}

async fn trigger(ctx: &Ctx<'_>, command: &str, args: &[String]) -> Reply {
    let switcher = &ctx.switcher;
    let mut triggers = Triggers {
        low: switcher.trigger_low,
        offline: switcher.trigger_offline,
        rtt: switcher.trigger_rtt,
        rtt_offline: switcher.trigger_rtt_offline,
    };
    let (what, unit, slot) = match command {
        "trigger" => ("The LOW trigger", "kbps", &mut triggers.low),
        "otrigger" => ("The offline trigger", "kbps", &mut triggers.offline),
        "rtrigger" => ("The RTT trigger", "ms", &mut triggers.rtt),
        _ => ("The offline RTT trigger", "ms", &mut triggers.rtt_offline),
    };
    let Some(arg) = args.first() else {
        return Ok(Msg::Trigger {
            what,
            unit,
            value: *slot,
        });
    };
    let value = if on_off(arg) == Some(false) {
        None
    } else {
        match arg.replace([',', '.'], "").parse::<i32>() {
            Ok(value) => Some(value),
            Err(_) => {
                return Ok(Msg::Usage(format!(
                    "{}{command} <{unit}|off>",
                    switcher.command_prefix
                )));
            }
        }
    };
    *slot = value;
    if let Err(reason) = control::check_triggers(&triggers) {
        return Ok(Msg::Invalid(reason.to_owned()));
    }
    db::switchers::set_triggers(&ctx.state.db, switcher.id, triggers).await?;
    notify_config(ctx.state, switcher.id).await;
    Ok(Msg::TriggerSet { what, unit, value })
}

/// Tells obs-link that the switcher's settings changed.
async fn notify_config(state: &State, switcher_id: Uuid) {
    if let Err(error) = state
        .nats
        .publish(live::config_subject(switcher_id), "{}".into())
        .await
    {
        tracing::warn!(%error, %switcher_id, "Failed to notify obs-link of a config change");
    }
}

async fn alias(ctx: &Ctx<'_>, args: &[String]) -> Reply {
    let prefix = &ctx.switcher.command_prefix;
    let usage = || {
        Msg::Usage(format!(
            "{prefix}alias <alias> <command> | {prefix}alias rem <alias>"
        ))
    };
    let pool = &ctx.state.db;
    match args {
        [action, alias] if matches!(action.as_str(), "rem" | "remove" | "del" | "delete") => {
            let Some(alias) = normalize_alias(alias) else {
                return Ok(Msg::AliasNotFound(alias.clone()));
            };
            Ok(
                if db::chat_commands::remove_alias(pool, ctx.switcher.id, &alias).await? {
                    Msg::AliasRemoved(alias)
                } else {
                    Msg::AliasNotFound(alias)
                },
            )
        }
        [alias, command] => {
            let Some(alias) = normalize_alias(alias) else {
                return Ok(usage());
            };
            let command = command
                .trim()
                .trim_start_matches(prefix.as_str())
                .to_lowercase();
            if !db::chat_commands::command_exists(pool, &command).await? {
                return Ok(Msg::UnknownCommand(command));
            }
            if db::chat_commands::command_exists(pool, &alias).await? {
                return Ok(Msg::AliasTaken(alias));
            }
            match db::chat_commands::add_alias(pool, ctx.switcher.id, &alias, &command).await {
                Ok(()) => Ok(Msg::AliasAdded {
                    alias,
                    command,
                    prefix: prefix.clone(),
                }),
                Err(error) if error.is_unique_violation() => Ok(Msg::AliasTaken(alias)),
                Err(error) => Err(error.into()),
            }
        }
        _ => Ok(usage()),
    }
}

async fn noalbs(ctx: &Ctx<'_>, args: &[String]) -> Reply {
    let prefix = &ctx.switcher.command_prefix;
    let usage = || {
        Msg::Usage(format!(
            "{prefix}noalbs version | prefix <new> | lang <code>"
        ))
    };
    let mut settings = db::switchers::ChatSettings {
        command_prefix: ctx.switcher.command_prefix.clone(),
        language: ctx.switcher.language.clone(),
        public_commands_enabled: ctx.switcher.public_commands_enabled,
        mod_commands_enabled: ctx.switcher.mod_commands_enabled,
        threaded_replies: ctx.switcher.threaded_replies,
    };
    match args {
        [action] if action == "version" => Ok(Msg::Version),
        [action, new] if action == "prefix" => {
            if new.chars().count() > 3 {
                return Ok(Msg::Invalid(
                    "The prefix must be 1 to 3 characters without spaces".into(),
                ));
            }
            settings.command_prefix = new.clone();
            db::switchers::set_chat_settings(&ctx.state.db, ctx.switcher.id, &settings).await?;
            Ok(Msg::PrefixSet(new.clone()))
        }
        [action, code] if action == "lang" => {
            let code = code.to_lowercase();
            if !db::chat_commands::LANGUAGES.contains(&code.as_str()) {
                return Ok(Msg::Invalid(format!(
                    "Languages: {}",
                    db::chat_commands::LANGUAGES.join(", ")
                )));
            }
            settings.language = code.clone();
            db::switchers::set_chat_settings(&ctx.state.db, ctx.switcher.id, &settings).await?;
            Ok(Msg::LanguageSet(code))
        }
        [action, name] if action == "ignore" => ignore(ctx, name).await,
        [action, name] if action == "unignore" => {
            let name = name.trim_start_matches('@');
            Ok(
                if db::chat::unignore_chatter_named(
                    &ctx.state.db,
                    ctx.switcher.id,
                    ctx.platform,
                    name,
                )
                .await?
                {
                    Msg::Unignored(name.to_owned())
                } else {
                    Msg::NotIgnored(name.to_owned())
                },
            )
        }
        _ => Ok(usage()),
    }
}

/// Looks the name up on the platform, so the chatter stays ignored if they rename.
async fn ignore(ctx: &Ctx<'_>, name: &str) -> Reply {
    let name = name.trim_start_matches('@');
    let found = match ctx.platform {
        ChatPlatform::Twitch => match &ctx.state.twitch {
            Some(twitch) => twitch.helix.user_by_login(&name.to_lowercase()).await,
            None => Ok(None),
        },
        ChatPlatform::Kick => match &ctx.state.kick {
            Some(kick) => kick.user_by_slug(name).await,
            None => Ok(None),
        },
        ChatPlatform::YouTube => match &ctx.state.youtube {
            Some(youtube) => youtube.channel_by_handle(name).await,
            None => Ok(None),
        },
    };
    let (id, login) = match found {
        Ok(Some(found)) => found,
        Ok(None) => return Ok(Msg::UserNotFound(name.to_owned())),
        Err(error) => {
            tracing::warn!(%error, "Couldn't look a chatter up");
            return Ok(Msg::Failed);
        }
    };
    db::chat::ignore_chatter(&ctx.state.db, ctx.switcher.id, ctx.platform, &id, &login).await?;
    Ok(Msg::Ignoring(login))
}
