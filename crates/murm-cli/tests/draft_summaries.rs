use murm_cli::*;
use murm_protocol::derive_public_key;
#[tokio::test]
async fn draft_list_is_metadata_only() {
    let d = tempfile::tempdir().unwrap();
    let app = ClientService::open(d.path()).unwrap();
    let p = app
        .create_profile(
            "Leitor",
            ProfileInput::ImportPublic(derive_public_key(&[1; 32])),
        )
        .await
        .unwrap();
    let draft = app
        .create_draft(p.id, "Title", &"big-source-marker".repeat(10000))
        .await
        .unwrap();
    let summaries = app.list_draft_summaries(p.id).await.unwrap();
    let bytes = serde_json::to_vec(&summaries).unwrap();
    assert!(bytes.len() < 2000);
    assert_eq!(summaries[0].id, draft.id);
    assert!(!String::from_utf8_lossy(&bytes).contains("big-source-marker"));
}
#[tokio::test]
async fn sqlite_capacity_covers_drafts_without_destroying_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let app = ClientService::open_with_capacity(dir.path(), 262144).unwrap();
    let p = app
        .create_profile(
            "Leitor",
            ProfileInput::ImportPublic(derive_public_key(&[1; 32])),
        )
        .await
        .unwrap();
    assert!(
        app.create_draft(p.id, "Large", &"x".repeat(600000))
            .await
            .is_err()
    );
    assert_eq!(app.profile(p.id).await.unwrap().public_key, p.public_key);
    assert!(app.list_draft_summaries(p.id).await.unwrap().is_empty());
}
#[tokio::test]
async fn history_list_is_metadata_only() {
    let dir = tempfile::tempdir().unwrap();
    let app = ClientService::open(dir.path()).unwrap();
    app.initialize_vault(
        SecretString::new("senha longa para teste".into()),
        app.begin_vault_action(),
    )
    .await
    .unwrap();
    let p = app
        .create_profile("Autor", ProfileInput::Generate)
        .await
        .unwrap();
    app.add_relay(p.id, "http://127.0.0.1:9").await.unwrap();
    let draft = app
        .create_draft(p.id, "Title", &"history-source-marker".repeat(10000))
        .await
        .unwrap();
    app.publish(p.id, draft.id).await.unwrap();
    let summaries = app
        .article_history_summaries(p.public_key, &draft.publication)
        .await
        .unwrap();
    let bytes = serde_json::to_vec(&summaries).unwrap();
    assert!(bytes.len() < 3000);
    assert!(!String::from_utf8_lossy(&bytes).contains("history-source-marker"));
}
