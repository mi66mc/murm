use crate::*;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorConfig {
    pub program: String,
    pub args: Vec<String>,
}
impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            program: if cfg!(windows) { "notepad.exe" } else { "vi" }.into(),
            args: vec![],
        }
    }
}
impl ClientService {
    pub async fn editor(&self) -> Result<EditorConfig, ClientError> {
        self.db
            .call(|c| {
                let value: Option<String> = c
                    .query_row(
                        "SELECT value FROM client_meta WHERE key='editor'",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?;
                value
                    .map(|s| serde_json::from_str(&s).map_err(|_| ClientError::InvalidInput))
                    .unwrap_or_else(|| Ok(EditorConfig::default()))
            })
            .await
    }
    pub async fn set_editor(&self, editor: EditorConfig) -> Result<(), ClientError> {
        if editor.program.is_empty()
            || editor.program.len() > 1024
            || editor.args.len() > 32
            || editor.args.iter().any(|s| s.len() > 4096)
        {
            return Err(ClientError::InvalidInput);
        }
        let value = serde_json::to_string(&editor).map_err(|_| ClientError::InvalidInput)?;
        self.db.call(move|c|{c.execute("INSERT INTO client_meta(key,value) VALUES('editor',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[value])?;Ok(())}).await
    }
    /// Caller must release its terminal before invoking a terminal editor.
    pub async fn edit_draft_external(
        &self,
        profile: ProfileId,
        id: DraftId,
    ) -> Result<Draft, ClientError> {
        let mut draft = self.load_draft(profile, id).await?;
        let editor = self.editor().await?;
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("article.md");
        tokio::fs::write(&path, draft.content.as_str().unwrap_or("")).await?;
        let status = tokio::process::Command::new(editor.program)
            .args(editor.args)
            .arg(&path)
            .status()
            .await?;
        if !status.success() {
            return Err(ClientError::Io);
        }
        if tokio::fs::metadata(&path).await?.len() > 4194304 {
            return Err(ClientError::InvalidInput);
        }
        let source = tokio::fs::read_to_string(&path).await?;
        draft.content = serde_json::json!(source);
        self.save_draft(profile, draft).await
    }
}
