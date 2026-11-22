use super::profiles::FormRequest;
use crate::*;
pub async fn unlock(
    app: &ClientService,
    request: &mut FormRequest,
    action: Option<VaultActionId>,
) -> Result<(), ClientError> {
    if request.vault.unlocked {
        return Ok(());
    }
    let secret = request.take("password");
    if !request.vault.exists && secret.as_str() != request.take("confirm").as_str() {
        return Err(ClientError::InvalidInput);
    }
    let action = action.unwrap_or_else(|| app.begin_vault_action());
    if request.vault.exists {
        app.unlock(SecretString(secret), action).await?;
    } else {
        app.initialize_vault(SecretString(secret), action).await?;
    }
    Ok(())
}
