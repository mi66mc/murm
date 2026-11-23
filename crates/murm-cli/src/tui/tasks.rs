use super::Screen;
use crate::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskContext {
    pub id: uuid::Uuid,
    pub profile: Option<ProfileId>,
    pub generation: u64,
    pub navigation: u64,
}
pub struct Snapshot {
    pub profiles: Vec<ProfileSummary>,
    pub active: Option<ProfileSummary>,
    pub vault: VaultStatus,
    pub articles: Vec<ArticleSummary>,
    pub drafts: Vec<DraftSummary>,
    pub outbox: Vec<OutboxEntry>,
}
pub enum UiMessage {
    Editor(Box<crate::Draft>),
    DraftSaved(Box<crate::Draft>),
    Status(String),
    Snapshot(Box<Snapshot>),
    Detail {
        title: String,
        text: String,
    },
    History(Vec<ArticleSummary>),
    FormDone {
        snapshot: Box<Snapshot>,
        screen: Screen,
        status: String,
    },
}
pub struct TaskResult {
    pub context: TaskContext,
    pub result: Result<UiMessage, ClientError>,
}
pub fn task_channel() -> (
    tokio::sync::mpsc::Sender<TaskResult>,
    tokio::sync::mpsc::Receiver<TaskResult>,
) {
    tokio::sync::mpsc::channel(64)
}
pub async fn load_snapshot(
    app: &ClientService,
    profile: Option<ProfileId>,
) -> Result<Snapshot, ClientError> {
    let profiles = app.list_profiles().await?;
    let vault = app.vault_status().await?;
    let active = if let Some(id) = profile {
        match app.profile(id).await {
            Ok(p) => Some(p),
            Err(ClientError::NotFound) => None,
            Err(e) => return Err(e),
        }
    } else {
        app.active_profile().await?
    };
    let (drafts, outbox) = if let Some(p) = &active {
        (
            app.list_draft_summaries(p.id).await?,
            app.outbox(p.id).await?,
        )
    } else {
        (vec![], vec![])
    };
    Ok(Snapshot {
        profiles,
        active,
        vault,
        articles: app.list_articles(true).await?,
        drafts,
        outbox,
    })
}
