use super::*;
use crate::*;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::{sync::Arc, time::Duration};
use futures_util::StreamExt;
pub enum UiEffect {
    Command(UiCommand),
    Submit(FormRequest, bool),
    Cancel(Option<VaultActionId>),
    Lock,
    Editor(ProfileId, DraftId),
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
    if key.kind == KeyEventKind::Release { return None; }
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
            let screens = [Screen::Profiles, Screen::Articles, Screen::Drafts, Screen::Relays, Screen::Outbox];
            let current = screens.iter().position(|s| *s == state.screen).unwrap_or(0);
            let next = if key.code == KeyCode::Tab { (current + 1) % screens.len() } else { (current + screens.len() - 1) % screens.len() };
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
                    "Remover perfil local {}?\n{}\n\nA chave cifrada e os rascunhos deste perfil serão removidos deste banco. Se esta for a única cópia da chave, você perderá o controle da identidade.\nNada será apagado dos relays.",
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
                    format!("Remover contato local {url}? Nenhum dado remoto será apagado."),
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
                return Some(UiEffect::Editor(p.id, draft.id));
            }
        }
    }
    if let Some(command) = command_for_key(state, key) {
        match &command {
            UiCommand::Publish(profile, id) => {
                let p = state.active.as_ref()?;
                let draft = state.drafts.iter().find(|d| d.id == *id)?;
                let text = format!(
                    "Publicar com {}?\nPública: {}\nTítulo: {}\nPublicação: {} · revisão {}\nDestinos: {}\n\nO conteúdo é o rascunho salvo (Enter na lista permite ler antes).\nA confirmação remota será somente de armazenamento.",
                    p.name,
                    p.public_key,
                    draft.title,
                    draft.publication,
                    draft
                        .revision
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "inválida".into()),
                    p.relays.join(", ")
                );
                let cmd = UiCommand::Publish(*profile, *id);
                confirm(state, cmd, text);
                return None;
            }
            UiCommand::Withdraw(profile, publication) => {
                let p = state.active.as_ref()?;
                let text = format!(
                    "Retirar publicação {publication}?\nPerfil: {}\nPública: {}\n\nSerá assinada uma nova revisão com content null. Cópias antigas não são apagadas.",
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
                        if let Some(form)=&mut state.form{form.type_text(text.trim_end_matches(['\r','\n']));}None
                    }
                    _=>None,
                };
                let Some(effect)=effect else{continue};
                match effect{
                    UiEffect::Exit=>break,
                    UiEffect::Cancel(action)=>{
                        if let Some(action)=action{app.cancel_vault_action(action);state.vault.unlocked=false;}
                        state.status="Tela cancelada. Envios já iniciados não podem ser desfeitos.".into();
                    }
                    UiEffect::Lock=>{app.lock();state.status="Cofre bloqueado.".into();}
                    UiEffect::Editor(profile,id)=>{
                        guard.suspend()?;
                        let result=app.edit_draft_external(profile,id).await;
                        guard.resume()?;terminal.clear()?;
                        let context=state.begin_task();let app=app.clone();let scope=context.profile;
                        spawn(&mut jobs,sender.clone(),context,async move{
                            let snapshot=load_snapshot(&app,scope).await?;
                            Ok(UiMessage::FormDone{snapshot:Box::new(snapshot),screen:Screen::Drafts,
                                status:match result{Ok(_)=>"Rascunho salvo; nenhuma assinatura foi criada.".into(),Err(error)=>format!("Editor: {error}. O rascunho anterior foi preservado.")}})
                        });
                    }
                    UiEffect::Command(command)=>{
                        if jobs.len()>=16{state.status="Aguarde as tarefas anteriores.".into();continue;}
                        let context=state.begin_task();let app=app.clone();
                        spawn(&mut jobs,sender.clone(),context,async move{execute_command(&app,command,context.profile).await});
                    }
                    UiEffect::Submit(request,needs_vault)=>{
                        if jobs.len()>=16{state.status="Aguarde as tarefas anteriores; reabra o formulário.".into();state.reduce(Action::Back);continue;}
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
