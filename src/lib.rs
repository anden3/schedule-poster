#[path = "config.rs"]
mod config;
#[path = "discord_api.rs"]
mod discord_api;
#[path = "holo_api.rs"]
mod holo_api;
#[path = "serializers.rs"]
mod serializers;
#[path = "twitter_api.rs"]
mod twitter_api;

use tokio::sync::mpsc::{self, Receiver, Sender};

use config::Config;
use discord_api::{DiscordAPI, DiscordMessageData};
use holo_api::HoloAPI;
use twitter_api::TwitterAPI;

pub struct HoloBot {}

impl HoloBot {
    pub async fn start() {
        let config = Config::load_config("settings.json");
        let discord = DiscordAPI::new(&config.discord_token).await;

        let config_clone = config.clone();
        let (tx, rx): (Sender<DiscordMessageData>, Receiver<DiscordMessageData>) =
            mpsc::channel(10);

        HoloAPI::start(tx.clone()).await;

        tokio::spawn(async move {
            TwitterAPI::start(config.clone(), tx.clone()).await.unwrap();
        });

        tokio::spawn(async move {
            DiscordAPI::posting_thread(discord, rx, config_clone).await;
        });

        loop {}
    }
}