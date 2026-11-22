use murm_cli::*;
use murm_protocol::derive_public_key;
fn input() -> ProfileInput {
    ProfileInput::ImportPublic(derive_public_key(&[1; 32]))
}
#[tokio::test]
async fn create_read_only_and_restart() {
    let d = tempfile::tempdir().unwrap();
    let app = ClientService::open(d.path()).unwrap();
    let p = app.create_profile("Ana", input()).await.unwrap();
    assert_eq!(p.mode, ProfileMode::ReadOnly);
    app.select_profile(p.id).await.unwrap();
    drop(app);
    let app = ClientService::open(d.path()).unwrap();
    assert_eq!(app.list_profiles().await.unwrap()[0].id, p.id);
    assert_eq!(app.active_profile().await.unwrap().unwrap().id, p.id);
}
#[tokio::test]
async fn public_key_unique() {
    let d = tempfile::tempdir().unwrap();
    let app = ClientService::open(d.path()).unwrap();
    let a = app.create_profile("Ana", input()).await.unwrap();
    let b = app.create_profile("Outra", input()).await.unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(app.list_profiles().await.unwrap().len(), 1);
}
#[tokio::test]
async fn rename_keeps_identity() {
    let d = tempfile::tempdir().unwrap();
    let app = ClientService::open(d.path()).unwrap();
    let a = app.create_profile("Ana", input()).await.unwrap();
    app.rename_profile(a.id, "Novo").await.unwrap();
    let p = app.list_profiles().await.unwrap().remove(0);
    assert_eq!(p.public_key, a.public_key);
    assert_eq!(p.name, "Novo");
}
#[tokio::test]
async fn independent_client_databases() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let x = ClientService::open(a.path()).unwrap();
    let y = ClientService::open(b.path()).unwrap();
    x.create_profile("Ana", input()).await.unwrap();
    assert!(y.list_profiles().await.unwrap().is_empty());
}
#[test]
fn migration_failure_preserves_database() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("client.sqlite");
    std::fs::write(&p, b"not sqlite").unwrap();
    assert!(ClientService::open(d.path()).is_err());
    assert_eq!(std::fs::read(p).unwrap(), b"not sqlite");
}
#[test]
fn second_instance_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let first = ClientService::open(d.path()).unwrap();
    assert!(matches!(
        ClientService::open(d.path()),
        Err(ClientError::AlreadyRunning)
    ));
    drop(first);
    assert!(ClientService::open(d.path()).is_ok());
}
