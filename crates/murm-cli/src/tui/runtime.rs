use super::*;
use crate::*;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use std::{sync::Arc, time::Duration};
pub enum UiEffect {
    Command(UiCommand),
    Submit(FormRequest, bool),
    Cancel(Option<VaultActionId>),
    Lock,
    Exit,
}
fn open_form(state: &mut AppState, kind: FormKind, target: Option<ProfileSummary>) {
    state.form = Some(Form::new(kind, state.vault, target));
    state.reduce(Action::Navigate(if kind == FormKind::Unlock {
        Screen::Unlock
    } else {
        Screen::ProfileForm
    }));
}
fn confirm(state: &mut AppState, command: UiCommand, text: String) {
    state.confirm = Some(command);
    state.confirmation_text = text;
    state.reduce(Action::Navigate(Screen::Confirm));
}
pub fn handle_key(state: &mut AppState, key: KeyEvent) -> Option<UiEffect> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if state.help {
        if matches!(key.code, KeyCode::Char('?') | KeyCode::Esc) {
            state.help = false;
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            state.help = false;
        } else {
            return None;
        }
    }
    if state.screen == Screen::Editor && state.editor.is_some() {
        return handle_editor_key(state, key);
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        state.running = false;
        return Some(UiEffect::Exit);
    }
    if key.code == KeyCode::Esc {
        let action = state.vault_action.take();
        state.form = None;
        state.confirm = None;
        state.invalidate_tasks();
        state.reduce(Action::Back);
        return Some(UiEffect::Cancel(action));
    }
    if state.form.is_none() && state.confirm.is_none() {
        if let Some(Action::Navigate(screen)) = action_from_key(key) {
            state.reduce(Action::Navigate(screen));
            return None;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            let screens = [
                Screen::Profiles,
                Screen::Articles,
                Screen::Drafts,
                Screen::Relays,
                Screen::Outbox,
            ];
            let current = screens.iter().position(|s| *s == state.screen).unwrap_or(0);
            let next = if key.code == KeyCode::Tab {
                (current + 1) % screens.len()
            } else {
                (current + screens.len() - 1) % screens.len()
            };
            state.reduce(Action::Navigate(screens[next]));
            return None;
        }
    }
    if state.busy {
        if key.code == KeyCode::Char('?') {
            state.reduce(Action::Help);
        }
        return None;
    }
    if let Some(form) = &mut state.form {
        match key.code {
            KeyCode::Tab => form.next_field(),
            KeyCode::BackTab => form.previous_field(),
            KeyCode::Backspace => form.backspace(),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.type_text(&c.to_string())
            }
            KeyCode::Enter => {
                let needs_vault = form.requires_vault();
                let request = form.take_request();
                state.form = None;
                return Some(UiEffect::Submit(request, needs_vault));
            }
            _ => {}
        }
        return None;
    }
    if state.screen == Screen::Confirm {
        if key.code == KeyCode::Enter {
            let command = state.confirm.take()?;
            state.reduce(Action::Back);
            return Some(UiEffect::Command(command));
        }
        return None;
    }
    match key.code {
        KeyCode::Char('u') if !state.vault.unlocked => {
            open_form(state, FormKind::Unlock, state.active.clone());
            return None;
        }
        KeyCode::Char('l') => {
            state.vault.unlocked = false;
            return Some(UiEffect::Lock);
        }
        KeyCode::Tab | KeyCode::BackTab => {
            let screens = [
                Screen::Profiles,
                Screen::Articles,
                Screen::Drafts,
                Screen::Relays,
                Screen::Outbox,
            ];
            let current = screens.iter().position(|s| *s == state.screen).unwrap_or(0);
            let next = if key.code == KeyCode::Tab {
                (current + 1) % screens.len()
            } else {
                (current + screens.len() - 1) % screens.len()
            };
            state.reduce(Action::Navigate(screens[next]));
            return None;
        }
        _ => {}
    }
    if state.screen == Screen::Profiles {
        let target = state
            .profiles
            .get(state.selected.min(state.profiles.len().saturating_sub(1)))
            .cloned();
        let kind = match key.code {
            KeyCode::Char('n') => Some(FormKind::Generate),
            KeyCode::Char('i') => Some(FormKind::ImportPrivate),
            KeyCode::Char('o') => Some(FormKind::ImportPublic),
            KeyCode::Char('r') if target.is_some() => Some(FormKind::Rename),
            KeyCode::Char('P') if state.vault.exists => Some(FormKind::ChangePassword),
            _ => None,
        };
        if let Some(kind) = kind {
            open_form(state, kind, target);
            return None;
        }
        if key.code == KeyCode::Char('d') {
            let p = target?;
            confirm(
                state,
                UiCommand::RemoveProfile(p.id),
                format!(
                    "Remove local profile {}?\n{}\n\nIts encrypted key and drafts will be removed from this database. If this is the only copy of the key, you will lose control of the identity.\nNothing will be erased from relays.",
                    p.name, p.public_key
                ),
            );
            return None;
        }
    }
    if let Some(p) = state.active.clone() {
        if state.screen == Screen::Relays {
            if key.code == KeyCode::Char('n') {
                open_form(state, FormKind::AddRelay, Some(p));
                return None;
            }
            if key.code == KeyCode::Char('d') {
                let url = p
                    .relays
                    .get(state.selected.min(p.relays.len().saturating_sub(1)))?
                    .clone();
                confirm(
                    state,
                    UiCommand::RemoveRelay(p.id, url.clone()),
                    format!("Remove local contact {url}? No remote data will be erased."),
                );
                return None;
            }
        }
        if state.screen == Screen::Drafts && p.mode == ProfileMode::Signing {
            if key.code == KeyCode::Char('n') {
                open_form(state, FormKind::NewDraft, Some(p));
                return None;
            }
            if key.code == KeyCode::Char('e') {
                let draft = state
                    .drafts
                    .get(state.selected.min(state.drafts.len().saturating_sub(1)))?;
                return Some(UiEffect::Command(UiCommand::OpenEditor(p.id, draft.id)));
            }
        }
    }
    if state.screen == Screen::Outbox && key.code == KeyCode::Enter {
        let entry = state
            .outbox
            .get(state.selected.min(state.outbox.len().saturating_sub(1)))?;
        state.detail_title = "Outbox destination details".into();
        state.detail = super::views::outbox::entry_details(entry);
        state.reduce(Action::Navigate(Screen::Article));
        return None;
    }
    if let Some(command) = command_for_key(state, key) {
        match &command {
            UiCommand::Publish(profile, id) => {
                let p = state.active.as_ref()?;
                let draft = state.drafts.iter().find(|d| d.id == *id)?;
                let text = format!(
                    "Publish as {}?\nPublic key: {}\nTitle: {}\nPublication: {} · revision {}\nDestinations: {}\n\nThis publishes the saved draft (Enter in the list reads it first).\nRemote confirmation means storage only.",
                    p.name,
                    p.public_key,
                    draft.title,
                    draft.publication,
                    draft
                        .revision
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "invalid".into()),
                    p.relays.join(", ")
                );
                let cmd = UiCommand::Publish(*profile, *id);
                confirm(state, cmd, text);
                return None;
            }
            UiCommand::Withdraw(profile, publication) => {
                let p = state.active.as_ref()?;
                let text = format!(
                    "Withdraw publication {publication}?\nProfile: {}\nPublic key: {}\n\nA new revision with content null will be signed. Older copies are not erased.",
                    p.name, p.public_key
                );
                let cmd = UiCommand::Withdraw(*profile, publication.clone());
                confirm(state, cmd, text);
                return None;
            }
            UiCommand::SelectProfile(id) => {
                let p = state.profiles.iter().find(|p| p.id == *id)?.clone();
                state.switch_profile(p);
            }
            _ => {}
        }
        return Some(UiEffect::Command(command));
    }
    if let Some(action) = action_from_key(key) {
        state.reduce(action);
    }
    None
}
pub fn handle_paste(state: &mut AppState, text: &str) {
    if state.help || state.busy || state.editor_leave.is_some() {
        return;
    }
    if state.screen == Screen::Editor {
        if let Some(editor) = &mut state.editor
            && let Err(error) = editor.paste(text)
        {
            state.status = error.to_string();
        }
    } else if let Some(form) = &mut state.form {
        form.type_text(text.trim_end_matches(['\r', '\n']));
    }
}
fn handle_editor_key(state: &mut AppState, key: KeyEvent) -> Option<UiEffect> {
    if let Some(leave) = state.editor_leave {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                state.editor = None;
                state.editor_leave = None;
                if matches!(leave, EditorLeave::Quit) {
                    state.running = false;
                    return Some(UiEffect::Exit);
                }
                state.reduce(Action::Back);
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => state.editor_leave = None,
            _ => {}
        }
        return None;
    }
    if state.busy {
        return None;
    }
    let quit = key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
    if key.code == KeyCode::Esc || quit {
        if state.editor.as_ref().is_some_and(|e| e.dirty) {
            state.editor_leave = Some(if quit {
                EditorLeave::Quit
            } else {
                EditorLeave::Back
            });
            return None;
        }
        state.editor = None;
        if quit {
            state.running = false;
            return Some(UiEffect::Exit);
        }
        state.reduce(Action::Back);
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
        match state.editor.as_ref()?.draft_for_save() {
            Ok(draft) => return Some(UiEffect::Command(UiCommand::SaveDraft(Box::new(draft)))),
            Err(error) => state.status = error.to_string(),
        }
    } else if let Some(editor) = &mut state.editor {
        editor.input(key);
    }
    None
}
fn spawn(
    jobs: &mut tokio::task::JoinSet<()>,
    sender: tokio::sync::mpsc::Sender<TaskResult>,
    context: TaskContext,
    future: impl std::future::Future<Output = Result<UiMessage, ClientError>> + Send + 'static,
) {
    jobs.spawn(async move {
        let result = future.await;
        let _ = sender.send(TaskResult { context, result }).await;
    });
}
struct LockOnExit(ClientService);
impl Drop for LockOnExit {
    fn drop(&mut self) {
        self.0.lock();
    }
}
pub async fn run(app: ClientService) -> Result<(), ClientError> {
    let snapshot = load_snapshot(&app, None).await?;
    let mut state = AppState::default();
    state.apply_snapshot(snapshot);
    if state.active.is_some() {
        state.screen = Screen::Articles;
    }
    let app = Arc::new(app);
    let _lock_on_exit = LockOnExit((*app).clone());
    let mut guard = terminal::TerminalGuard::new(terminal::CrosstermControl)?;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    let (sender, mut receiver) = task_channel();
    let mut jobs = tokio::task::JoinSet::new();
    let mut events = event::EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(16));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut dirty = true;
    while state.running {
        tokio::select! {
            _=tick.tick(), if dirty=>{
                terminal.draw(|frame| render(frame, &state))?;
                dirty = false;
            }
            next=events.next()=>{
                let Some(next) = next else { break; };
                dirty = true;
                let effect=match next?{
                    Event::Key(key) if key.kind!=KeyEventKind::Release=>handle_key(&mut state,key),
                    Event::Paste(text)=>{
                        handle_paste(&mut state,&text);
                        None
                    }
                    _=>None,
                };
                let Some(effect)=effect else{continue};
                match effect{
                    UiEffect::Exit=>break,
                    UiEffect::Cancel(action)=>{
                        if let Some(action)=action{app.cancel_vault_action(action);state.vault.unlocked=false;}
                        state.status="View cancelled. Already-started sends cannot be undone.".into();
                    }
                    UiEffect::Lock=>{app.lock();state.status="Vault locked.".into();}
                    UiEffect::Command(command)=>{
                        if jobs.len()>=16{state.status="Previous tasks are still running.".into();continue;}
                        let context=state.begin_task();let app=app.clone();
                        spawn(&mut jobs,sender.clone(),context,async move{execute_command(&app,command,context.profile).await});
                    }
                    UiEffect::Submit(request,needs_vault)=>{
                        if jobs.len()>=16{state.status="Wait for earlier tasks, then reopen the form.".into();state.reduce(Action::Back);continue;}
                        let action=needs_vault.then(||app.begin_vault_action());state.vault_action=action;
                        let context=state.begin_task();let app=app.clone();
                        spawn(&mut jobs,sender.clone(),context,async move{
                            let outcome=execute_form(&app,request,action).await?;
                            let profile=outcome.profile.map(|p|p.id).or(context.profile);
                            Ok(UiMessage::FormDone{snapshot:Box::new(load_snapshot(&app,profile).await?),screen:outcome.screen,status:outcome.status})
                        });
                    }
                }
            }
            result=receiver.recv()=>{
                dirty = true;
                if let Some(TaskResult{context,result})=result
                    && state.finish_task(context,result){
                        let context=state.begin_task();let app=app.clone();
                        spawn(&mut jobs,sender.clone(),context,async move{Ok(UiMessage::Snapshot(Box::new(load_snapshot(&app,context.profile).await?)))});
                    }
            }
            _=jobs.join_next(),if !jobs.is_empty()=>{}
        }
    }
    if let Some(action) = state.vault_action {
        app.cancel_vault_action(action);
    }
    app.lock();
    guard.suspend()?;
    jobs.abort_all();
    while jobs.join_next().await.is_some() {}
    Ok(())
}
