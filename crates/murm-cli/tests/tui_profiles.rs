use murm_cli::*;
use murm_cli::tui::*;
use murm_protocol::derive_public_key;
fn fill(form:&mut Form,values:&[&str]){for value in values{form.type_text(value);form.next_field();}}
#[tokio::test]
async fn generate_profile_flow(){
    let d=tempfile::tempdir().unwrap();let app=ClientService::open(d.path()).unwrap();
    let mut form=Form::new(FormKind::Generate,app.vault_status().await.unwrap(),None);
    fill(&mut form,&["Ana","uma senha longa de teste","uma senha longa de teste"]);
    let outcome=execute_form(&app,form.take_request(),None).await.unwrap();
    let p=outcome.profile.unwrap();assert_eq!(p.mode,ProfileMode::Signing);assert_eq!(p.name,"Ana");assert!(app.is_unlocked());
}
#[tokio::test]
async fn import_pair_rejects_mismatch(){
    let d=tempfile::tempdir().unwrap();let app=ClientService::open(d.path()).unwrap();
    app.initialize_vault(SecretString::new("uma senha longa de teste".into()),app.begin_vault_action()).await.unwrap();
    let mut form=Form::new(FormKind::ImportPrivate,app.vault_status().await.unwrap(),None);
    fill(&mut form,&["Ana",&"03".repeat(32),&derive_public_key(&[4;32]).to_string()]);
    assert!(matches!(execute_form(&app,form.take_request(),None).await,Err(ClientError::KeyMismatch)));assert!(app.list_profiles().await.unwrap().is_empty());
}
#[tokio::test]
async fn import_public_is_read_only(){
    let d=tempfile::tempdir().unwrap();let app=ClientService::open(d.path()).unwrap();
    let mut form=Form::new(FormKind::ImportPublic,app.vault_status().await.unwrap(),None);
    fill(&mut form,&["Ana",&derive_public_key(&[3;32]).to_string()]);
    let p=execute_form(&app,form.take_request(),None).await.unwrap().profile.unwrap();
    assert_eq!(p.mode,ProfileMode::ReadOnly);assert!(!app.vault_status().await.unwrap().exists);
}
#[tokio::test]
async fn duplicate_public_promotes_existing_profile(){
    let d=tempfile::tempdir().unwrap();let app=ClientService::open(d.path()).unwrap();
    let original=app.create_profile("Ana",ProfileInput::ImportPublic(derive_public_key(&[3;32]))).await.unwrap();
    let mut form=Form::new(FormKind::ImportPrivate,app.vault_status().await.unwrap(),Some(original.clone()));
    fill(&mut form,&["Ana",&"03".repeat(32),"","uma senha longa de teste","uma senha longa de teste"]);
    let p=execute_form(&app,form.take_request(),None).await.unwrap().profile.unwrap();
    assert_eq!(p.id,original.id);assert_eq!(p.mode,ProfileMode::Signing);
}
#[test]
fn secrets_masked_and_cleared(){
    let mut form=Form::new(FormKind::ImportPrivate,VaultStatus{exists:false,unlocked:false},None);
    let seed="03".repeat(32);fill(&mut form,&["Ana",&seed]);
    let mut state=AppState::default();state.form=Some(form);state.screen=Screen::ProfileForm;
    let backend=ratatui::backend::TestBackend::new(100,30);let mut terminal=ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame|render(frame,&state)).unwrap();
    let text=terminal.backend().buffer().content.iter().map(|cell|cell.symbol()).collect::<String>();
    assert!(!text.contains(&seed));assert!(text.contains("********"));
    let request=state.form.as_mut().unwrap().take_request();drop(request);
    assert!(state.form.as_ref().unwrap().display_lines().iter().all(|s|!s.contains('*')));
}
#[tokio::test]
async fn unlock_wrong_password_keeps_profiles(){
    let d=tempfile::tempdir().unwrap();let app=ClientService::open(d.path()).unwrap();
    app.initialize_vault(SecretString::new("uma senha longa de teste".into()),app.begin_vault_action()).await.unwrap();
    app.create_profile("Ana",ProfileInput::Generate).await.unwrap();app.lock();
    let mut form=Form::new(FormKind::Unlock,app.vault_status().await.unwrap(),None);form.type_text("errada");
    assert!(execute_form(&app,form.take_request(),None).await.is_err());assert!(!app.is_unlocked());assert_eq!(app.list_profiles().await.unwrap().len(),1);
}
#[tokio::test]
async fn changing_profile_discards_stale_ui_results(){
    let d=tempfile::tempdir().unwrap();let app=ClientService::open(d.path()).unwrap();
    let a=app.create_profile("Ana",ProfileInput::ImportPublic(derive_public_key(&[3;32]))).await.unwrap();
    let b=app.create_profile("Bruno",ProfileInput::ImportPublic(derive_public_key(&[4;32]))).await.unwrap();
    let mut state=AppState::default();state.switch_profile(a);let context=state.begin_task();state.switch_profile(b.clone());
    assert!(!state.apply_result(context,UiMessage::Status("old response".into())));assert_eq!(state.active.unwrap().id,b.id);
}
#[test]
fn lock_removes_signing_actions(){
    let mut state=AppState::default();state.active=Some(ProfileSummary{id:ProfileId::new_v4(),name:"Ana".into(),public_key:derive_public_key(&[1;32]),mode:ProfileMode::Signing,locked:false,relays:vec![]});
    state.vault=VaultStatus{exists:true,unlocked:true};assert!(state.can_sign());state.vault.unlocked=false;assert!(!state.can_sign());
}
