#![allow(unknown_lints)]
#![warn(
    clippy::pedantic,
    clippy::cargo,
    clippy::perf,
    clippy::nursery,
    clippy::complexity,
    clippy::correctness,
    clippy::clone_on_ref_ptr,
    clippy::create_dir,
    clippy::decimal_literal_representation,
    clippy::default_numeric_fallback,
    clippy::exit,
    clippy::expect_used,
    clippy::filetype_is_file,
    clippy::if_then_some_else_none,
    clippy::indexing_slicing,
    clippy::inline_asm_x86_att_syntax,
    clippy::let_underscore_must_use,
    clippy::lossy_float_literal,
    clippy::map_err_ignore,
    clippy::mem_forget,
    clippy::multiple_inherent_impl,
    clippy::panic_in_result_fn,
    clippy::rc_buffer,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::semicolon_if_nothing_returned,
    clippy::str_to_string,
    clippy::string_to_string,
    clippy::todo,
    clippy::unimplemented,
    clippy::unneeded_field_pattern,
    clippy::unreachable,
    clippy::unwrap_in_result,
    clippy::unwrap_used,
    clippy::verbose_file_reads,
    clippy::wildcard_enum_match_arm,
    clippy::wrong_pub_self_convention
)]
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::non_ascii_literal,
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions
)]

use futures::stream::StreamExt;
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook_tokio::Signals;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tracing::{debug, error, info, instrument};

use apis::{
    birthday_reminder::BirthdayReminder,
    discord_api::{DiscordApi, DiscordMessageData},
    holo_api::{HoloApi, StreamUpdate},
    /* reminder_notifier::ReminderNotifier, */
    twitter_api::TwitterApi,
};
use bot::DiscordBot;
use utility::{config::Config, logger::Logger};

pub struct HoloBot {}

impl HoloBot {
    #[instrument]
    pub async fn start() -> anyhow::Result<()> {
        let (exit_sender, exit_receiver) = watch::channel(false);

        let signals = Signals::new(&[SIGHUP, SIGTERM, SIGINT, SIGQUIT])?;
        let handle = signals.handle();

        let signals_task = tokio::spawn(async move {
            let mut signals = signals.fuse();

            while let Some(signal) = signals.next().await {
                match signal {
                    SIGHUP => {
                        info!(signal_type = "SIGHUP", signal, "Signal received!");
                    }
                    SIGTERM | SIGINT | SIGQUIT => {
                        info!(
                            signal_type = "Terminate",
                            signal, "Terminate signal received!"
                        );

                        if let Err(e) = exit_sender.send(true) {
                            error!("{:#}", e);
                        }
                    }
                    _ => debug!(
                        signal_type = "Unknown",
                        signal, "Unhandled signal received!"
                    ),
                }
            }
        });

        Logger::initialize()?;

        let config = Config::load_config(Self::get_config_path())?;

        let (discord_message_tx, discord_message_rx): (
            mpsc::Sender<DiscordMessageData>,
            mpsc::Receiver<DiscordMessageData>,
        ) = mpsc::channel(10);

        let (stream_update_tx, stream_update_rx): (
            broadcast::Sender<StreamUpdate>,
            broadcast::Receiver<StreamUpdate>,
        ) = broadcast::channel(16);

        let (_reminder_update_tx, reminder_update_rx) = mpsc::channel(4);

        let (guild_ready_tx, guild_ready_rx) = oneshot::channel();

        let index_receiver = HoloApi::start(
            config.clone(),
            discord_message_tx.clone(),
            stream_update_tx.clone(),
            exit_receiver.clone(),
        )
        .await;

        TwitterApi::start(
            config.clone(),
            discord_message_tx.clone(),
            exit_receiver.clone(),
        )
        .await;

        BirthdayReminder::start(
            config.clone(),
            discord_message_tx.clone(),
            exit_receiver.clone(),
        )
        .await;

        /* ReminderNotifier::start(
            config.clone(),
            discord_message_tx.clone(),
            reminder_update_rx,
            exit_receiver.clone(),
        )
        .await; */

        let (task, cache) = DiscordBot::start(
            config.clone(),
            stream_update_tx.clone(),
            reminder_update_rx,
            index_receiver.clone(),
            guild_ready_tx,
            exit_receiver.clone(),
        )
        .await?;

        DiscordApi::start(
            cache,
            config.clone(),
            discord_message_rx,
            stream_update_rx,
            index_receiver,
            guild_ready_rx,
            exit_receiver,
        )
        .await;

        task.await?;
        info!(task = "Main thread", "Shutting down.");

        handle.close();
        signals_task.await?;

        Ok(())
    }

    #[cfg(target_arch = "arm")]
    const fn get_config_path() -> &'static str {
        "production.json"
    }

    #[cfg(target_arch = "x86_64")]
    const fn get_config_path() -> &'static str {
        "settings/development.json"
    }
}
