use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use murm_cli::{tui::*, *};
fn key(code: KeyCode) -> KeyEvent { KeyEvent::new(code, KeyModifiers::NONE) }
fn ctrl(c:char) -> KeyEvent { KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL) }
async fn setup() -> (tempfile::TempDir, ClientService, ProfileSummary, Draft) {
    let dir=tempfile::tempdir().unwrap(); let app=ClientService::open(dir.path()).unwrap();
    let p=app.create_profile("Writer", ProfileInput::ImportPublic(murm_protocol::derive_public_key(&[1;32]))).await.unwrap();
    let draft=app.create_draft(p.id,"A title","").await.unwrap(); (dir,app,p,draft)
}
async fn state() -> (tempfile::TempDir,ClientService,AppState) {
    let (dir,app,p,draft)=setup().await; let mut state=AppState::default();
    state.switch_profile(p); state.screen=Screen::Drafts;
    let context=state.begin_task(); state.apply_result(context, UiMessage::Editor(Box::new(draft)));
    (dir,app,state)
}
#[tokio::test]
async fn editor_types_shortcut_characters_as_markdown() {
    let (_dir,_app,mut state)=state().await;
    for c in "q12345?esp".chars(){ assert!(handle_key(&mut state,key(KeyCode::Char(c))).is_none()); }
    assert_eq!(state.screen,Screen::Editor);
    assert_eq!(state.editor.as_ref().unwrap().source(),"q12345?esp");
}
#[tokio::test]
async fn unicode_paste_selection_undo_and_redo() {
    let (_dir,_app,mut state)=state().await;
    state.editor.as_mut().unwrap().paste("漢😀\r\n123").unwrap();
    assert_eq!(state.editor.as_ref().unwrap().source(),"漢😀\n123");
    handle_key(&mut state,KeyEvent::new(KeyCode::Left,KeyModifiers::SHIFT));
    handle_key(&mut state,key(KeyCode::Char('x')));
    assert_eq!(state.editor.as_ref().unwrap().source(),"漢😀\n12x");
    handle_key(&mut state,ctrl('z'));
    assert_eq!(state.editor.as_ref().unwrap().source(),"漢😀\n12");
    handle_key(&mut state,ctrl('z'));
    assert_eq!(state.editor.as_ref().unwrap().source(),"漢😀\n123");
    handle_key(&mut state,ctrl('y'));
    handle_key(&mut state,ctrl('y'));
    assert_eq!(state.editor.as_ref().unwrap().source(),"漢😀\n12x");
}
#[tokio::test]
async fn save_is_local_and_updates_draft_version() {
    let (_dir,app,mut state)=state().await;
    state.editor.as_mut().unwrap().paste("# Hello\n\nAn article.").unwrap();
    let Some(UiEffect::Command(command))=handle_key(&mut state,ctrl('s')) else {panic!("save command missing")};
    let context=state.begin_task();
    let message=execute_command(&app,command,context.profile).await.unwrap();
    assert!(state.apply_result(context,message));
    let editor=state.editor.as_ref().unwrap();
    assert!(!editor.dirty);
    let saved=app.load_draft(editor.draft.profile_id,editor.draft.id).await.unwrap();
    assert_eq!(saved.content,"# Hello\n\nAn article."); assert_eq!(saved.version,2);
    assert!(app.outbox(saved.profile_id).await.unwrap().is_empty());
}
#[tokio::test]
async fn conflicting_save_preserves_unsaved_editor_text() {
    let (_dir,app,mut state)=state().await;
    let editor=state.editor.as_mut().unwrap();
    let mut concurrent=editor.draft.clone(); concurrent.content=serde_json::json!("Other edit");
    app.save_draft(concurrent.profile_id,concurrent).await.unwrap();
    editor.paste("My unsaved edit").unwrap();
    let Some(UiEffect::Command(command))=handle_key(&mut state,ctrl('s')) else {panic!("save missing")};
    let context=state.begin_task(); let result=execute_command(&app,command,context.profile).await;
    assert!(matches!(result,Err(ClientError::Conflict)));
    state.finish_task(context,result);
    assert_eq!(state.editor.as_ref().unwrap().source(),"My unsaved edit");
    assert!(state.editor.as_ref().unwrap().dirty); assert_eq!(state.screen,Screen::Editor);
}
#[tokio::test]
async fn escape_and_exit_require_explicit_discard() {
    let (_dir,_app,mut state)=state().await;
    state.editor.as_mut().unwrap().paste("Don't lose me").unwrap();
    handle_key(&mut state,key(KeyCode::Esc)); assert!(state.editor_leave.is_some());
    handle_key(&mut state,key(KeyCode::Char('n')));
    assert!(state.editor_leave.is_none());assert_eq!(state.editor.as_ref().unwrap().source(),"Don't lose me");
    handle_key(&mut state,ctrl('c')); assert!(state.running);
    assert!(matches!(handle_key(&mut state,key(KeyCode::Char('y'))),Some(UiEffect::Exit)));
    assert!(!state.running);
}
#[tokio::test]
async fn editing_is_frozen_until_save_finishes() {
    let (_dir,_app,mut state)=state().await;
    state.editor.as_mut().unwrap().paste("Saved buffer").unwrap();
    state.begin_task(); handle_key(&mut state,key(KeyCode::Char('x')));
    assert_eq!(state.editor.as_ref().unwrap().source(),"Saved buffer");
}
