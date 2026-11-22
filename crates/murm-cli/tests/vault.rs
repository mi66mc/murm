use murm_cli::*;
use murm_protocol::*;
use serde_json::json;
fn password()->SecretString{SecretString::new("uma senha de teste longa".into())}
async fn initialize(app:&ClientService){app.initialize_vault(password(),app.begin_vault_action()).await.unwrap();}
fn private()->ProfileInput{ProfileInput::ImportSecret{seed:SecretSeed::from_hex(&"03".repeat(32)).unwrap(),public_key:None}}
fn draft(key:SignerKey)->UnsignedEvent{UnsignedEvent{version:1,signer:key,created_at:1,kind:1,
    header:json!({"publication":"test","revision":1,"status":"published"}).as_object().unwrap().clone(),content:json!("body")}}
#[tokio::test]
async fn generated_profile_can_sign_after_restart(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;
    let p=app.create_profile("Ana",ProfileInput::Generate).await.unwrap();drop(app);
    let app=ClientService::open(dir.path()).unwrap();app.unlock(password(),app.begin_vault_action()).await.unwrap();
    let event=app.sign(p.id,draft(p.public_key)).await.unwrap();
    parse_and_verify(&serde_json::to_vec(&event).unwrap(),ParseLimits::default()).unwrap();
}
#[tokio::test]
async fn import_secret_derives_public(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;
    let p=app.create_profile("Ana",private()).await.unwrap();assert_eq!(p.public_key,derive_public_key(&[3;32]));assert_eq!(p.mode,ProfileMode::Signing);
}
#[tokio::test]
async fn mismatched_pair_rolls_back(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;
    let input=ProfileInput::ImportSecret{seed:SecretSeed::from_hex(&"03".repeat(32)).unwrap(),public_key:Some(derive_public_key(&[4;32]))};
    assert!(matches!(app.create_profile("Ana",input).await,Err(ClientError::KeyMismatch)));assert!(app.list_profiles().await.unwrap().is_empty());
}
#[tokio::test]
async fn upgrade_read_only_preserves_state(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();
    let p=app.create_profile("Ana",ProfileInput::ImportPublic(derive_public_key(&[3;32]))).await.unwrap();app.select_profile(p.id).await.unwrap();
    initialize(&app).await;let promoted=app.create_profile("different",private()).await.unwrap();
    assert_eq!(promoted.id,p.id);assert_eq!(promoted.name,"Ana");assert_eq!(promoted.mode,ProfileMode::Signing);
    assert_eq!(app.active_profile().await.unwrap().unwrap().id,p.id);
}
#[tokio::test]
async fn duplicate_import_is_idempotent(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;
    let a=app.create_profile("Ana",private()).await.unwrap();let b=app.create_profile("Ana",private()).await.unwrap();
    assert_eq!(a.id,b.id);assert_eq!(app.list_profiles().await.unwrap().len(),1);
}
#[tokio::test]
async fn wrong_password_does_not_mutate(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;
    app.create_profile("Ana",private()).await.unwrap();app.lock();
    assert!(matches!(app.unlock(SecretString::new("errada".into()),app.begin_vault_action()).await,Err(ClientError::WrongPassword)));
    app.unlock(password(),app.begin_vault_action()).await.unwrap();assert_eq!(app.list_profiles().await.unwrap().len(),1);
}
#[tokio::test]
async fn ciphertext_tampering_fails(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;
    let p=app.create_profile("Ana",private()).await.unwrap();
    rusqlite::Connection::open(app.database_path()).unwrap().execute("UPDATE profile_secrets SET ciphertext=zeroblob(48)",[]).unwrap();
    assert!(app.sign(p.id,draft(p.public_key)).await.is_err());
}
#[tokio::test]
async fn changing_password_is_atomic(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;
    app.create_profile("Ana",private()).await.unwrap();let connection=rusqlite::Connection::open(app.database_path()).unwrap();
    connection.execute_batch("CREATE TRIGGER refuse_change BEFORE UPDATE ON vault BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(app.change_password(password(),SecretString::new("outra senha longa aqui".into()),app.begin_vault_action()).await.is_err());
    connection.execute_batch("DROP TRIGGER refuse_change;").unwrap();app.lock();app.unlock(password(),app.begin_vault_action()).await.unwrap();
    app.change_password(password(),SecretString::new("outra senha longa aqui".into()),app.begin_vault_action()).await.unwrap();app.lock();
    assert!(app.unlock(password(),app.begin_vault_action()).await.is_err());
    app.unlock(SecretString::new("outra senha longa aqui".into()),app.begin_vault_action()).await.unwrap();
}
#[tokio::test]
async fn lock_blocks_signing(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;
    let p=app.create_profile("Ana",private()).await.unwrap();app.lock();
    assert!(matches!(app.sign(p.id,draft(p.public_key)).await,Err(ClientError::Locked)));
}
#[tokio::test]
async fn plaintext_seed_absent_from_db_and_wal(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;app.create_profile("Ana",private()).await.unwrap();
    let seed=[3u8;32];let text="03".repeat(32);
    for p in [app.database_path().to_owned(),app.database_path().with_extension("sqlite-wal")] {
        if let Ok(bytes)=std::fs::read(p) {
            assert!(!bytes.windows(seed.len()).any(|w|w==seed));
            assert!(!bytes.windows(text.len()).any(|w|w==text.as_bytes()));
        }
    }
}
#[test]
fn secret_not_in_error_or_debug(){
    let secret="private-confidential";
    let err=SecretSeed::from_hex(secret).err().unwrap();
    assert!(!format!("{err:?} {err}").contains(secret));
    // SecretSeed, SecretString and ProfileInput intentionally implement no Debug/Serialize.
}
#[tokio::test]
async fn profile_summary_reflects_vault_lock(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;
    let profile=app.create_profile("Ana",private()).await.unwrap();app.select_profile(profile.id).await.unwrap();
    assert!(!app.list_profiles().await.unwrap()[0].locked);
    assert!(!app.profile(profile.id).await.unwrap().locked);
    assert!(!app.active_profile().await.unwrap().unwrap().locked);
    app.lock();assert!(app.list_profiles().await.unwrap()[0].locked);
}
#[tokio::test]
async fn malformed_vault_parameters_rejected(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;app.lock();
    rusqlite::Connection::open(app.database_path()).unwrap().execute("UPDATE vault SET params='{\"memory\":4294967295}'",[]).unwrap();
    assert!(matches!(app.unlock(password(),app.begin_vault_action()).await,Err(ClientError::CorruptVault)));
}
#[tokio::test]
async fn cancel_during_kdf_never_unlocks_later(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();initialize(&app).await;app.lock();
    let mut events=app.subscribe_events();let action=app.begin_vault_action();let other=app.clone();
    let task=tokio::spawn(async move{other.unlock(password(),action).await});
    loop{if matches!(events.recv().await.unwrap(),ClientEvent::VaultDerivationStarted(a) if a==action){break;}}
    app.cancel_vault_action(action);
    assert!(matches!(task.await.unwrap(),Err(ClientError::Cancelled)));assert!(!app.vault_status().await.unwrap().unlocked);
}
#[tokio::test]
async fn cancel_after_initialization_commit_leaves_locked_vault(){
    let dir=tempfile::tempdir().unwrap();let app=ClientService::open(dir.path()).unwrap();let action=app.begin_vault_action();
    app.initialize_vault(password(),action).await.unwrap();app.cancel_vault_action(action);
    let status=app.vault_status().await.unwrap();assert!(status.exists);assert!(!status.unlocked);
    assert!(app.list_profiles().await.unwrap().is_empty());
}
