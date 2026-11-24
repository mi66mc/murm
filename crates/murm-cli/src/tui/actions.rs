use super::Screen;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
#[derive(Debug, Clone, Copy)]
pub enum Action {
    Navigate(Screen),
    Back,
    Next,
    Previous,
    PageDown,
    PageUp,
    Help,
    Quit,
}
pub fn action_from_key(key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Down => Some(Action::Next),
        KeyCode::Up => Some(Action::Previous),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::Char('?') => Some(Action::Help),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('1') => Some(Action::Navigate(Screen::Profiles)),
        KeyCode::Char('2') => Some(Action::Navigate(Screen::Articles)),
        KeyCode::Char('3') => Some(Action::Navigate(Screen::Drafts)),
        KeyCode::Char('4') => Some(Action::Navigate(Screen::Relays)),
        KeyCode::Char('5') => Some(Action::Navigate(Screen::Outbox)),
        _ => None,
    }
}
#[derive(Debug, Clone)]
pub enum UiCommand {
    OpenEditor(crate::ProfileId, crate::DraftId),
    SaveDraft(Box<crate::Draft>),
    SelectProfile(crate::ProfileId),
    Refresh,
    Read(murm_protocol::EventId),
    ReadDraft(crate::ProfileId, crate::DraftId),
    History(murm_protocol::SignerKey, String),
    RelayInfo(crate::ProfileId, String),
    Sync(crate::ProfileId, String),
    Edit(crate::ProfileId, String),
    Publish(crate::ProfileId, crate::DraftId),
    Withdraw(crate::ProfileId, String),
    Retry(crate::ProfileId),
    RemoveProfile(crate::ProfileId),
    RemoveRelay(crate::ProfileId, String),
}
pub fn command_for_key(state: &super::AppState, key: KeyEvent) -> Option<UiCommand> {
    if key.code == KeyCode::F(5) {
        return Some(UiCommand::Refresh);
    }
    let active = state.active.as_ref();
    if key.code == KeyCode::Char('s') {
        let p = active?;
        let index = if state.screen == Screen::Relays {
            state.selected.min(p.relays.len().saturating_sub(1))
        } else {
            0
        };
        return p
            .relays
            .get(index)
            .map(|url| UiCommand::Sync(p.id, url.clone()));
    }
    match state.screen {
        Screen::Profiles if key.code == KeyCode::Enter => state
            .profiles
            .get(state.selected.min(state.profiles.len().saturating_sub(1)))
            .map(|p| UiCommand::SelectProfile(p.id)),
        Screen::Articles => {
            let article = state
                .articles
                .get(state.selected.min(state.articles.len().saturating_sub(1)))?;
            match key.code {
                KeyCode::Enter => Some(UiCommand::Read(article.id)),
                KeyCode::Char('h') => Some(UiCommand::History(
                    article.signer,
                    article.publication.clone(),
                )),
                KeyCode::Char('e')
                    if active.is_some_and(|p| {
                        p.public_key == article.signer && p.mode == crate::ProfileMode::Signing
                    }) =>
                {
                    Some(UiCommand::Edit(active?.id, article.publication.clone()))
                }
                KeyCode::Char('w')
                    if state.can_sign()
                        && active.is_some_and(|p| p.public_key == article.signer) =>
                {
                    Some(UiCommand::Withdraw(active?.id, article.publication.clone()))
                }
                _ => None,
            }
        }
        Screen::Drafts => {
            let p = active?;
            let draft = state
                .drafts
                .get(state.selected.min(state.drafts.len().saturating_sub(1)))?;
            match key.code {
                KeyCode::Enter => Some(UiCommand::ReadDraft(p.id, draft.id)),
                KeyCode::Char('p') if state.can_sign() => Some(UiCommand::Publish(p.id, draft.id)),
                _ => None,
            }
        }
        Screen::Relays if key.code == KeyCode::Enter => {
            let p = active?;
            p.relays
                .get(state.selected.min(p.relays.len().saturating_sub(1)))
                .map(|url| UiCommand::RelayInfo(p.id, url.clone()))
        }
        Screen::History if key.code == KeyCode::Enter => state
            .history
            .get(state.selected.min(state.history.len().saturating_sub(1)))
            .map(|e| UiCommand::Read(e.id)),
        Screen::Outbox if key.code == KeyCode::Char('r') => Some(UiCommand::Retry(active?.id)),
        _ => None,
    }
}
pub async fn execute_command(
    app: &crate::ClientService,
    command: UiCommand,
    scope: Option<crate::ProfileId>,
) -> Result<super::UiMessage, crate::ClientError> {
    use super::{UiMessage, load_snapshot};
    use crate::ClientError;
    match command {
        UiCommand::OpenEditor(profile, id) => Ok(UiMessage::Editor(Box::new(
            app.load_draft(profile, id).await?,
        ))),
        UiCommand::SaveDraft(draft) => Ok(UiMessage::DraftSaved(Box::new(
            app.save_draft(draft.profile_id, *draft).await?,
        ))),
        UiCommand::Read(id) => {
            let event = app.cached_event(id).await?.ok_or(ClientError::NotFound)?;
            let e = event.event();
            Ok(UiMessage::Detail {
                title: format!("Verified event {id}"),
                text: format!(
                    "Signer: {}\nPublication: {}\nRevision: {} · {}\n\n{}",
                    e.signer,
                    e.header
                        .get("publication")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    e.header
                        .get("revision")
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                    e.header
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    e.content.as_str().unwrap_or(
                        "Publication withdrawn in this revision. History remains separate."
                    )
                ),
            })
        }
        UiCommand::ReadDraft(profile, id) => {
            let draft = app.load_draft(profile, id).await?;
            Ok(UiMessage::Detail {
                title: format!("Local draft {id}"),
                text: draft
                    .content
                    .as_str()
                    .unwrap_or("Withdrawal (content null)")
                    .into(),
            })
        }
        UiCommand::History(signer, publication) => Ok(UiMessage::History(
            app.article_history_summaries(signer, &publication).await?,
        )),
        UiCommand::RelayInfo(profile, url) => Ok(UiMessage::Detail {
            title: format!("Policies declared by {url}"),
            text: serde_json::to_string_pretty(&app.relay_info(profile, &url).await?)
                .map_err(|_| ClientError::InvalidInput)?,
        }),
        UiCommand::Sync(profile, url) => {
            let summary = app.sync_profile(profile, &url).await?;
            Ok(UiMessage::FormDone {
                snapshot: Box::new(load_snapshot(app, scope).await?),
                screen: Screen::Articles,
                status: format!(
                    "Sync: {} added to cache; {} rejected. {}",
                    summary.accepted,
                    summary.rejected,
                    if summary.has_more {
                        "More pages are available; sync again."
                    } else {
                        "Retained history checked at this relay."
                    }
                ),
            })
        }
        UiCommand::Edit(profile, publication) => Ok(UiMessage::Editor(Box::new(
            app.edit_publication(profile, &publication).await?,
        ))),
        UiCommand::Publish(profile, draft) => {
            let id = app.publish(profile, draft).await?;
            app.retry_event(profile, id).await?;
            Ok(UiMessage::FormDone {
                snapshot: Box::new(load_snapshot(app, scope).await?),
                screen: Screen::Outbox,
                status: format!("Event {id}. Outbox status means storage at each relay only."),
            })
        }
        UiCommand::Withdraw(profile, publication) => {
            let id = app.withdraw(profile, &publication).await?;
            app.retry_event(profile, id).await?;
            Ok(UiMessage::FormDone {
                snapshot: Box::new(load_snapshot(app, scope).await?),
                screen: Screen::Outbox,
                status: format!("Withdrawal {id} queued. Remote copies are not erased."),
            })
        }
        UiCommand::SelectProfile(profile) => {
            app.select_profile(profile).await?;
            Ok(UiMessage::Snapshot(Box::new(
                load_snapshot(app, Some(profile)).await?,
            )))
        }
        UiCommand::Refresh => Ok(UiMessage::Snapshot(Box::new(
            load_snapshot(app, scope).await?,
        ))),
        UiCommand::Retry(profile) => {
            app.retry_outbox(profile).await?;
            Ok(UiMessage::Snapshot(Box::new(
                load_snapshot(app, scope).await?,
            )))
        }
        UiCommand::RemoveProfile(profile) => {
            app.remove_profile(profile).await?;
            Ok(UiMessage::FormDone {
                snapshot: Box::new(load_snapshot(app, None).await?),
                screen: Screen::Profiles,
                status:
                    "Profile removed from this local database only. Nothing was erased from relays."
                        .into(),
            })
        }
        UiCommand::RemoveRelay(profile, url) => {
            app.remove_relay(profile, &url).await?;
            Ok(UiMessage::Snapshot(Box::new(
                load_snapshot(app, scope).await?,
            )))
        }
    }
}
