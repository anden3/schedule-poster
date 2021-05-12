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
use log::{error, info};
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook_tokio::Signals;
use tokio::sync::{
    mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender},
    watch,
};

use apis::{
    birthday_reminder::BirthdayReminder,
    discord_api::{DiscordApi, DiscordMessageData},
    holo_api::{HoloApi, StreamUpdate},
    twitter_api::TwitterApi,
};
use bot::DiscordBot;
use utility::{config::Config, logger::Logger};

pub struct HoloBot {}

impl HoloBot {
    pub async fn start() -> anyhow::Result<()> {
        let (exit_sender, exit_receiver) = watch::channel(false);

        let signals = Signals::new(&[SIGHUP, SIGTERM, SIGINT, SIGQUIT])?;
        let handle = signals.handle();

        let signals_task = tokio::spawn(async move {
            let mut signals = signals.fuse();

            while let Some(signal) = signals.next().await {
                match signal {
                    SIGHUP => {
                        info!("SIGHUP signal received!");
                    }
                    SIGTERM | SIGINT | SIGQUIT => {
                        info!("Terminate signal received!");

                        if let Err(e) = exit_sender.send(true) {
                            error!("{:#}", e);
                        }
                    }
                    _ => (),
                }
            }
        });

        Logger::initialize()?;

        let config = Config::load_config(Self::get_config_path())?;

        let (discord_message_tx, discord_message_rx): (
            Sender<DiscordMessageData>,
            Receiver<DiscordMessageData>,
        ) = mpsc::channel(10);

        let (stream_update_tx, stream_update_rx): (
            UnboundedSender<StreamUpdate>,
            UnboundedReceiver<StreamUpdate>,
        ) = mpsc::unbounded_channel();

        HoloApi::start(
            config.clone(),
            discord_message_tx.clone(),
            stream_update_tx,
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

        let (task, cache) = DiscordBot::start(config.clone(), exit_receiver.clone()).await?;

        DiscordApi::start(
            cache,
            discord_message_rx,
            stream_update_rx,
            config.clone(),
            exit_receiver,
        )
        .await;

        task.await?;
        info!("Shutting down main thread...");

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
