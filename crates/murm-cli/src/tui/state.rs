use super::Action;
use super::{Form, Snapshot, TaskContext, UiMessage};
use crate::{
    ArticleSummary, Draft, OutboxEntry, ProfileMode, ProfileSummary, VaultActionId, VaultStatus,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Profiles,
    ProfileForm,
    Unlock,
    Relays,
    Articles,
    Article,
    History,
    Drafts,
    Outbox,
    Confirm,
}
pub struct AppState {
    pub screen: Screen,
    pub running: bool,
    pub selected: usize,
    pub scroll: usize,
    pub help: bool,
    pub generation: u64,
    pub status: String,
    previous: Vec<Screen>,
    pub active: Option<ProfileSummary>,
    pub profiles: Vec<ProfileSummary>,
    pub vault: VaultStatus,
    pub form: Option<Form>,
    pub busy: bool,
    pub vault_action: Option<VaultActionId>,
    pub articles: Vec<ArticleSummary>,
    pub drafts: Vec<Draft>,
    pub outbox: Vec<OutboxEntry>,
    pub detail_title: String,
    pub detail: String,
    pub history: Vec<murm_protocol::VerifiedEvent>,
    pub task: Option<uuid::Uuid>,
    pub confirm: Option<super::UiCommand>,
    pub confirmation_text: String,
}
impl Default for AppState {
    fn default() -> Self {
        Self {
            screen: Screen::Profiles,
            running: true,
            selected: 0,
            scroll: 0,
            help: false,
            generation: 0,
            status: String::new(),
            previous: vec![],
            active: None,
            profiles: vec![],
            vault: VaultStatus {
                exists: false,
                unlocked: false,
            },
            form: None,
            busy: false,
            vault_action: None,
            articles: vec![],
            drafts: vec![],
            outbox: vec![],
            detail_title: String::new(),
            detail: String::new(),
            history: vec![],
            task: None,
            confirm: None,
            confirmation_text: String::new(),
        }
    }
}
impl AppState {
    pub fn can_sign(&self) -> bool {
        self.vault.unlocked
            && self
                .active
                .as_ref()
                .is_some_and(|p| p.mode == ProfileMode::Signing)
    }
    pub fn invalidate_tasks(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.busy = false;
        self.task = None;
    }
    pub fn switch_profile(&mut self, profile: ProfileSummary) {
        self.invalidate_tasks();
        self.active = Some(profile);
        self.selected = 0;
        self.scroll = 0;
    }
    pub fn begin_task(&mut self) -> TaskContext {
        let context = TaskContext {
            id: uuid::Uuid::new_v4(),
            profile: self.active.as_ref().map(|p| p.id),
            generation: self.generation,
        };
        self.busy = true;
        self.task = Some(context.id);
        context
    }
    pub fn apply_snapshot(&mut self, snapshot: Snapshot) {
        self.profiles = snapshot.profiles;
        self.active = snapshot.active;
        self.vault = snapshot.vault;
        self.articles = snapshot.articles;
        self.drafts = snapshot.drafts;
        self.outbox = snapshot.outbox;
    }
    pub fn apply_result(&mut self, context: TaskContext, message: UiMessage) -> bool {
        if context.generation != self.generation
            || context.profile != self.active.as_ref().map(|p| p.id)
            || self.task != Some(context.id)
        {
            return false;
        }
        self.busy = false;
        self.task = None;
        self.vault_action = None;
        match message {
            UiMessage::Status(s) => self.status = s,
            UiMessage::Snapshot(s) => self.apply_snapshot(*s),
            UiMessage::Detail { title, text } => {
                self.detail_title = title;
                self.detail = text;
                self.reduce(Action::Navigate(Screen::Article));
            }
            UiMessage::History(events) => {
                self.history = events;
                self.reduce(Action::Navigate(Screen::History));
            }
            UiMessage::FormDone {
                snapshot,
                screen,
                status,
            } => {
                self.apply_snapshot(*snapshot);
                self.form = None;
                self.reduce(Action::Navigate(screen));
                self.status = status;
            }
        }
        true
    }
    /// Refresh public vault state once after a failed form, never retry a failed refresh.
    pub fn finish_task(
        &mut self,
        context: TaskContext,
        result: Result<UiMessage, crate::ClientError>,
    ) -> bool {
        match result {
            Ok(message) => {
                self.apply_result(context, message);
                false
            }
            Err(error) => {
                let form_screen = matches!(self.screen, Screen::ProfileForm | Screen::Unlock);
                let refresh = form_screen || self.vault_action.is_some();
                if !self.apply_result(context, UiMessage::Status(error.to_string())) {
                    return false;
                }
                self.form = None;
                if form_screen {
                    self.reduce(Action::Back);
                }
                refresh
            }
        }
    }
    pub fn reduce(&mut self, action: Action) {
        match action {
            Action::Navigate(screen) => {
                if self.screen != screen {
                    self.previous.push(self.screen);
                    self.screen = screen;
                }
                self.selected = 0;
                self.scroll = 0;
            }
            Action::Back => {
                self.form = None;
                self.screen = self.previous.pop().unwrap_or(Screen::Profiles);
                self.selected = 0;
                self.scroll = 0;
            }
            Action::Next => {
                self.selected = self.selected.saturating_add(1);
                self.scroll = self.scroll.saturating_add(1);
            }
            Action::Previous => {
                self.selected = self.selected.saturating_sub(1);
                self.scroll = self.scroll.saturating_sub(1);
            }
            Action::PageDown => {
                self.scroll = self.scroll.saturating_add(10);
            }
            Action::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
            }
            Action::Help => self.help = !self.help,
            Action::Quit => self.running = false,
        }
    }
}
