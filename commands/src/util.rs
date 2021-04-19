use std::ops::Deref;

use serenity::{
    builder::CreateEmbed,
    framework::standard::{Configuration, DispatchError},
    prelude::TypeMapKey,
};
use tokio::{
    sync::{broadcast, oneshot},
    time::{sleep_until, Duration, Instant},
};

use super::{
    prelude::*,
    slash_types::{SlashCommandOptions, SlashGroupOptions},
};

use utility::{client_data_types, wrap_type_aliases};

pub use tokio_util::sync::CancellationToken;

wrap_type_aliases!(
    StreamIndex | apis::holo_api::StreamIndex,
    ReactionSender | broadcast::Sender<ReactionUpdate>,
    MessageSender | broadcast::Sender<MessageUpdate>);

client_data_types!(StreamIndex, ReactionSender, MessageSender);

pub type ElementFormatter<'a, D> = Box<dyn Fn(&D) -> String + Send + Sync>;
pub type EmbedMutator<'a, D> =
    Box<dyn Fn(&'a mut CreateEmbed, &[D], usize) -> &'a mut CreateEmbed + Send + Sync>;

pub struct PaginatedList<'a, D> {
    title: Option<String>,
    layout: PageLayout,

    data: &'a [D],
    format_func: Option<ElementFormatter<'a, D>>,
    embed_func: Option<EmbedMutator<'a, D>>,

    show_page_count: ShowPageCount,
    page_change_perm: PageChangePermission,

    timeout: Duration,
    token: Option<CancellationToken>,
    message_sender: Option<oneshot::Sender<Message>>,

    delete_when_dropped: bool,
}

pub enum PageLayout {
    Standard {
        items_per_page: usize,
    },
    Chunked {
        chunk_size: usize,
        chunks_per_page: usize,
    },
}

pub enum ShowPageCount {
    Always,
    WhenSeveralPages,
    Never,
}

pub enum PageChangePermission {
    Interactor,
    Everyone,
}

enum FormattedData<'a, D> {
    Standard(&'a [D]),
    Chunked(Vec<(usize, &'a [D])>),
}

impl<'a, D> PaginatedList<'a, D> {
    pub fn new() -> PaginatedList<'a, D> {
        Self::default()
    }

    pub fn title(&'_ mut self, title: &str) -> &'_ mut Self {
        self.title = Some(title.to_string());
        self
    }

    pub fn layout(&'_ mut self, layout: PageLayout) -> &'_ mut Self {
        self.layout = layout;
        self
    }

    pub fn data(&'_ mut self, data: &'a [D]) -> &'_ mut Self {
        self.data = data;
        self
    }

    pub fn format(&'_ mut self, format: ElementFormatter<'a, D>) -> &'_ mut Self {
        self.format_func = Some(format);
        self
    }

    pub fn embed_func(&'_ mut self, func: EmbedMutator<'a, D>) -> &'_ mut Self {
        self.embed_func = Some(func);
        self
    }

    pub fn show_page_count(&'_ mut self, show_page_count: ShowPageCount) -> &'_ mut Self {
        self.show_page_count = show_page_count;
        self
    }

    pub fn page_change_permission(&'_ mut self, permission: PageChangePermission) -> &'_ mut Self {
        self.page_change_perm = permission;
        self
    }

    pub fn timeout(&'_ mut self, timeout: Duration) -> &'_ mut Self {
        self.timeout = timeout;
        self
    }

    pub fn token(&'_ mut self, token: CancellationToken) -> &'_ mut Self {
        self.token = Some(token);
        self
    }

    pub fn get_message(&'_ mut self, channel: oneshot::Sender<Message>) -> &'_ mut Self {
        self.message_sender = Some(channel);
        self
    }

    pub fn delete_when_dropped(&'_ mut self, delete: bool) -> &'_ mut Self {
        self.delete_when_dropped = delete;
        self
    }

    pub async fn display(
        &'_ mut self,
        interaction: &'a Interaction,
        ctx: &'a Ctx,
        app_id: u64,
    ) -> anyhow::Result<()> {
        let mut current_page: i32 = 1;

        let (data, required_pages) = match self.layout {
            PageLayout::Standard { items_per_page } => (
                FormattedData::Standard(self.data),
                ((self.data.len() as f32) / items_per_page as f32).ceil() as usize,
            ),
            PageLayout::Chunked {
                chunk_size,
                chunks_per_page,
            } => (
                FormattedData::Chunked(
                    self.data.chunks(chunk_size).enumerate().collect::<Vec<_>>(),
                ),
                ((self.data.len() as f32) / (chunk_size * chunks_per_page) as f32).ceil() as usize,
            ),
        };

        let message = self
            .create_page(
                &data,
                current_page as usize,
                required_pages,
                interaction,
                ctx,
                app_id,
            )
            .await?;

        if let Some(channel) = self.message_sender.take() {
            channel
                .send(message.clone())
                .map_err(|m| anyhow!("Could not send message: {}.", m.id))
                .context(here!())?;
        }

        if required_pages == 1 {
            return Ok(());
        }

        let left = message.react(&ctx, '⬅').await.context(here!())?;
        let right = message.react(&ctx, '➡').await.context(here!())?;

        let bot_data = ctx.data.read().await;
        let mut reaction_recv = bot_data.get::<ReactionSender>().unwrap().subscribe();

        let deadline = Instant::now() + self.timeout;

        loop {
            tokio::select! {
                _ = self.token.as_ref().unwrap().cancelled(), if self.token.is_some() => {
                    if self.delete_when_dropped {
                        interaction.delete_original_interaction_response(&ctx.http, app_id).await.context(here!())?;
                    }

                    return Ok(());
                }
                _ = sleep_until(deadline) => {
                    if self.delete_when_dropped {
                        interaction.delete_original_interaction_response(&ctx.http, app_id).await.context(here!())?;
                    }

                    return Ok(());
                }
                Ok(ReactionUpdate::Added(reaction)) = reaction_recv.recv() => {
                    if reaction.message_id != message.id {
                        continue;
                    }

                    if let Some(user) = reaction.user_id {
                        if user == app_id {
                            continue;
                        }

                        match self.page_change_perm {
                            PageChangePermission::Interactor if user != interaction.member.user.id => {
                                reaction.delete(&ctx).await.context(here!())?;
                                continue;
                            }
                            _ => (),
                        }
                    }

                    if reaction.emoji == left.emoji {
                        reaction.delete(&ctx).await.context(here!())?;
                        current_page -= 1;

                        if current_page < 1 {
                            current_page = required_pages as i32;
                        }
                    } else if reaction.emoji == right.emoji {
                        reaction.delete(&ctx).await.context(here!())?;
                        current_page += 1;

                        if current_page > required_pages as i32 {
                            current_page = 1;
                        }
                    } else {
                        continue;
                    }

                    self.create_page(
                        &data, current_page as usize,
                        required_pages,
                        interaction,
                        ctx,
                        app_id,
                    )
                    .await?;
                }
            }
        }
    }

    async fn create_page(
        &self,
        data: &FormattedData<'_, D>,
        page: usize,
        required_pages: usize,
        interaction: &Interaction,
        ctx: &Ctx,
        app_id: u64,
    ) -> anyhow::Result<Message> {
        interaction
            .edit_original_interaction_response(&ctx.http, app_id, |r| {
                r.embed(|e| {
                    e.colour(Colour::new(6_282_735));

                    if let Some(title) = &self.title {
                        e.title(title);
                    }

                    match (&self.layout, data) {
                        (PageLayout::Standard { items_per_page }, FormattedData::Standard(d)) => {
                            e.description(
                                d.iter()
                                    .skip(((page - 1) as usize) * *items_per_page)
                                    .take(*items_per_page)
                                    .fold(String::new(), |mut acc, element| {
                                        acc += match &self.format_func {
                                            Some(func) => func(element),
                                            None => String::new(),
                                        }
                                        .as_str();
                                        acc
                                    }),
                            );
                        }
                        (
                            PageLayout::Chunked {
                                chunk_size,
                                chunks_per_page,
                            },
                            FormattedData::Chunked(d),
                        ) => {
                            e.fields(
                                d.iter()
                                    .skip((page - 1) * chunks_per_page)
                                    .take(*chunks_per_page)
                                    .map(|(i, chunk)| {
                                        (
                                            format!(
                                                "{}-{}",
                                                i * chunk_size + 1,
                                                i * chunk_size + chunk.len()
                                            ),
                                            chunk.iter().fold(String::new(), |mut acc, element| {
                                                acc += match &self.format_func {
                                                    Some(func) => func(element),
                                                    None => String::new(),
                                                }
                                                .as_str();
                                                acc
                                            }),
                                            true,
                                        )
                                    }),
                            );
                        }
                        _ => error!("Invalid layout and data format found!"),
                    }

                    e.footer(|f| {
                        match self.show_page_count {
                            ShowPageCount::Always => {
                                f.text(format!("Page {} of {}", page, required_pages));
                            }
                            ShowPageCount::WhenSeveralPages if required_pages > 1 => {
                                f.text(format!("Page {} of {}", page, required_pages));
                            }
                            _ => (),
                        };
                        f
                    })
                })
            })
            .await
            .context(here!())
    }
}

impl<'a, D> Default for PaginatedList<'a, D> {
    fn default() -> Self {
        Self {
            title: None,
            layout: PageLayout::Standard { items_per_page: 5 },
            data: &[],
            format_func: None,
            embed_func: None,
            show_page_count: ShowPageCount::WhenSeveralPages,
            page_change_perm: PageChangePermission::Everyone,
            timeout: Duration::from_secs(15 * 60),
            token: None,
            message_sender: None,
            delete_when_dropped: false,
        }
    }
}

pub async fn should_fail<'a>(
    cfg: &'a Configuration,
    ctx: &'a Ctx,
    inter: &'a Interaction,
    command: &'static SlashCommandOptions,
    group: &'static SlashGroupOptions,
) -> Option<DispatchError> {
    if (command.owner_privilege && group.owner_privilege)
        && cfg.owners.contains(&inter.member.user.id)
    {
        return None;
    }

    if cfg.blocked_users.contains(&inter.member.user.id) {
        return Some(DispatchError::BlockedUser);
    }

    {
        if let Some(Channel::Guild(channel)) = inter.channel_id.to_channel_cached(&ctx).await {
            let guild_id = channel.guild_id;

            if cfg.blocked_guilds.contains(&guild_id) {
                return Some(DispatchError::BlockedGuild);
            }

            if let Some(guild) = guild_id.to_guild_cached(&ctx.cache).await {
                if cfg.blocked_users.contains(&guild.owner_id) {
                    return Some(DispatchError::BlockedGuild);
                }
            }
        }
    }

    if !cfg.allowed_channels.is_empty() && !cfg.allowed_channels.contains(&inter.channel_id) {
        return Some(DispatchError::BlockedChannel);
    }

    for check in group.checks.iter().chain(command.checks.iter()) {
        let res = (check.function)(ctx, inter, command).await;

        if let Result::Err(reason) = res {
            return Some(DispatchError::CheckFailed(check.name, reason));
        }
    }

    None
}

#[derive(Debug, Clone)]
pub enum ReactionUpdate {
    Added(Reaction),
    Removed(Reaction),
    Wiped(ChannelId, MessageId),
}

#[derive(Debug, Clone)]
pub enum MessageUpdate {
    Sent(Message),
    Edited(Message),
    Deleted(Message),
}