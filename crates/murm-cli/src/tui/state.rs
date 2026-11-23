use super::Action;
use super::{Form, Snapshot, TaskContext, UiMessage};
use crate::{ArticleSummary, OutboxEntry, ProfileMode, ProfileSummary, VaultActionId, VaultStatus};
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
    Editor,
}
pub struct AppState {
    pub screen: Screen,
    pub running: bool,
    pub selected: usize,
    pub scroll: usize,
    pub help: bool,
    pub generation: u64,
    navigation: u64,
    pub status: String,
    previous: Vec<Screen>,
    pub active: Option<ProfileSummary>,
    pub profiles: Vec<ProfileSummary>,
    pub vault: VaultStatus,
    pub form: Option<Form>,
    pub busy: bool,
    pub vault_action: Option<VaultActionId>,
    pub articles: Vec<ArticleSummary>,
    pub drafts: Vec<crate::DraftSummary>,
    pub outbox: Vec<OutboxEntry>,
    pub detail_title: String,
    pub detail: String,
    pub history: Vec<ArticleSummary>,
    pub task: Option<uuid::Uuid>,
    pub confirm: Option<super::UiCommand>,
    pub confirmation_text: String,
    pub editor: Option<super::DraftEditor>,
    pub editor_leave: Option<super::EditorLeave>,
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
            navigation: 0,
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
            editor: None,
            editor_leave: None,
        }
    }
}
impl AppState {
    fn list_len(&self) -> Option<usize> {
        match self.screen {
            Screen::Profiles => Some(self.profiles.len()),
            Screen::Articles => Some(self.articles.len()),
            Screen::Drafts => Some(self.drafts.len()),
            Screen::Relays => Some(self.active.as_ref().map_or(0, |p| p.relays.len())),
            Screen::Outbox => Some(self.outbox.len()),
            Screen::History => Some(self.history.len()),
            _ => None,
        }
    }
    fn move_selection(&mut self, down: bool, amount: usize) {
        if let Some(len) = self.list_len() {
            let current = self.selected.min(len.saturating_sub(1));
            self.selected = if down {
                current.saturating_add(amount).min(len.saturating_sub(1))
            } else {
                current.saturating_sub(amount)
            };
        }
        self.scroll = if down {
            self.scroll.saturating_add(amount)
        } else {
            self.scroll.saturating_sub(amount)
        };
    }
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
            navigation: self.navigation,
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
        let same_screen = context.navigation == self.navigation;
        match message {
            UiMessage::Editor(draft) => {
                if !same_screen { return true; }
                match super::DraftEditor::new(*draft) {
                    Ok(editor) => { self.editor=Some(editor); self.reduce(Action::Navigate(Screen::Editor)); }
                    Err(error) => self.status=error.to_string(),
                }
            }
            UiMessage::DraftSaved(draft) => {
                if let Some(editor)=&mut self.editor {
                    if editor.draft.id==draft.id { editor.saved(*draft); }
                }
                self.status="Draft saved locally. Nothing was published.".into();
            }
            UiMessage::Status(s) => self.status = s,
            UiMessage::Snapshot(s) => self.apply_snapshot(*s),
            UiMessage::Detail { title, text } => {
                if !same_screen { return true; }
                self.detail_title = title;
                self.detail = text;
                self.reduce(Action::Navigate(Screen::Article));
            }
            UiMessage::History(events) => {
                if !same_screen { return true; }
                self.history = events;
                self.reduce(Action::Navigate(Screen::History));
            }
            UiMessage::FormDone {
                snapshot,
                screen,
                status,
            } => {
                self.apply_snapshot(*snapshot);
                if same_screen {
                    self.form = None;
                    self.reduce(Action::Navigate(screen));
                }
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
                    self.navigation = self.navigation.wrapping_add(1);
                    if self.previous.len() >= 16 { self.previous.remove(0); }
                    self.previous.push(self.screen);
                    self.screen = screen;
                }
                self.selected = 0;
                self.scroll = 0;
            }
            Action::Back => {
                self.navigation = self.navigation.wrapping_add(1);
                self.form = None;
                self.screen = self.previous.pop().unwrap_or(Screen::Profiles);
                self.selected = 0;
                self.scroll = 0;
            }
            Action::Next => {
                self.move_selection(true, 1);
            }
            Action::Previous => {
                self.move_selection(false, 1);
            }
            Action::PageDown => {
                self.move_selection(true, 10);
            }
            Action::PageUp => {
                self.move_selection(false, 10);
            }
            Action::Help => self.help = !self.help,
            Action::Quit => self.running = false,
        }
    }
}
