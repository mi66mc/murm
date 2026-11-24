use crate::tui::{Screen, sanitize};
use crate::*;
use zeroize::{Zeroize, Zeroizing};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKind {
    Generate,
    ImportPrivate,
    ImportPublic,
    Unlock,
    ChangePassword,
    Rename,
    AddRelay,
    NewDraft,
}
struct Field {
    key: &'static str,
    label: &'static str,
    secret: bool,
    value: Zeroizing<String>,
    max: usize,
}
pub struct Form {
    pub kind: FormKind,
    pub focused: usize,
    fields: Vec<Field>,
    vault: VaultStatus,
    target: Option<ProfileSummary>,
}
pub struct FormRequest {
    pub(crate) kind: FormKind,
    pub(crate) vault: VaultStatus,
    target: Option<ProfileSummary>,
    values: Vec<(&'static str, Zeroizing<String>)>,
}
impl FormRequest {
    pub(crate) fn take(&mut self, key: &str) -> Zeroizing<String> {
        self.values
            .iter_mut()
            .find(|(k, _)| *k == key)
            .map(|(_, value)| std::mem::take(value))
            .unwrap_or_default()
    }
}
pub struct FormOutcome {
    pub profile: Option<ProfileSummary>,
    pub screen: Screen,
    pub status: String,
}
impl Form {
    pub fn new(kind: FormKind, vault: VaultStatus, target: Option<ProfileSummary>) -> Self {
        let mut specs = match kind {
            FormKind::Generate => vec![("name", "Local name", false, 320)],
            FormKind::ImportPrivate => vec![
                ("name", "Local name", false, 320),
                ("seed", "Private seed (64 hex)", true, 64),
                ("public", "Optional public key (64 hex)", false, 64),
            ],
            FormKind::ImportPublic => vec![
                ("name", "Local name", false, 320),
                ("public", "Public key (64 hex)", false, 64),
            ],
            FormKind::Unlock => vec![],
            FormKind::ChangePassword => vec![
                ("current", "Current password", true, 4096),
                ("new", "New password", true, 4096),
                ("confirm", "Confirm new password", true, 4096),
            ],
            FormKind::Rename => vec![("name", "New local name", false, 320)],
            FormKind::AddRelay => vec![(
                "url",
                "Relay URL (HTTPS; HTTP on loopback only)",
                false,
                2048,
            )],
            FormKind::NewDraft => vec![("title", "Article title", false, 800)],
        };
        if matches!(
            kind,
            FormKind::Generate | FormKind::ImportPrivate | FormKind::Unlock
        ) && !vault.unlocked
        {
            specs.push((
                "password",
                if vault.exists {
                    "Local password"
                } else {
                    "Create a local password (at least 12 characters)"
                },
                true,
                4096,
            ));
            if !vault.exists {
                specs.push(("confirm", "Confirm password", true, 4096));
            }
        }
        let mut fields = specs
            .into_iter()
            .map(|(key, label, secret, max)| Field {
                key,
                label,
                secret,
                value: Zeroizing::new(String::with_capacity(max)),
                max,
            })
            .collect::<Vec<_>>();
        if kind == FormKind::Rename
            && let (Some(field), Some(profile)) = (fields.first_mut(), &target)
        {
            field.value.push_str(&profile.name);
        }
        Self {
            kind,
            focused: 0,
            fields,
            vault,
            target,
        }
    }
    pub fn label(&self) -> &'static str {
        match self.kind {
            FormKind::Generate => "Create identity",
            FormKind::ImportPrivate => "Import private key",
            FormKind::ImportPublic => "Import public key",
            FormKind::Unlock => "Unlock local vault",
            FormKind::ChangePassword => "Change vault password",
            FormKind::Rename => "Rename profile",
            FormKind::AddRelay => "Add relay",
            FormKind::NewDraft => "New article",
        }
    }
    pub fn requires_vault(&self) -> bool {
        matches!(self.kind, FormKind::Unlock | FormKind::ChangePassword)
            || (matches!(self.kind, FormKind::Generate | FormKind::ImportPrivate)
                && !self.vault.unlocked)
    }
    pub fn type_text(&mut self, text: &str) {
        if let Some(field) = self.fields.get_mut(self.focused)
            && !text.chars().any(char::is_control)
            && field.value.len().saturating_add(text.len()) <= field.max
        {
            field.value.push_str(text);
        }
    }
    pub fn backspace(&mut self) {
        if let Some(field) = self.fields.get_mut(self.focused) {
            field.value.pop();
        }
    }
    pub fn next_field(&mut self) {
        if !self.fields.is_empty() {
            self.focused = (self.focused + 1) % self.fields.len();
        }
    }
    pub fn previous_field(&mut self) {
        if !self.fields.is_empty() {
            self.focused = (self.focused + self.fields.len() - 1) % self.fields.len();
        }
    }
    pub fn display_lines(&self) -> Vec<String> {
        self.fields
            .iter()
            .enumerate()
            .map(|(i, f)| {
                format!(
                    "{} {}: {}",
                    if i == self.focused { ">" } else { " " },
                    f.label,
                    if f.secret {
                        "*".repeat(f.value.chars().count().min(64))
                    } else {
                        sanitize(&f.value)
                    }
                )
            })
            .collect()
    }
    pub fn cancel(&mut self) {
        for f in &mut self.fields {
            f.value.zeroize();
        }
    }
    pub fn take_request(&mut self) -> FormRequest {
        let values = self
            .fields
            .iter_mut()
            .map(|f| {
                (
                    f.key,
                    std::mem::replace(&mut f.value, Zeroizing::new(String::with_capacity(f.max))),
                )
            })
            .collect();
        FormRequest {
            kind: self.kind,
            vault: self.vault,
            target: self.target.clone(),
            values,
        }
    }
}
pub async fn execute_form(
    app: &ClientService,
    mut request: FormRequest,
    action: Option<VaultActionId>,
) -> Result<FormOutcome, ClientError> {
    let mut profile = None;
    let screen = match request.kind {
        FormKind::Generate | FormKind::ImportPrivate | FormKind::ImportPublic => {
            let name = request.take("name");
            let input = match request.kind {
                FormKind::ImportPublic => ProfileInput::ImportPublic(
                    request
                        .take("public")
                        .parse()
                        .map_err(|_| ClientError::InvalidKey)?,
                ),
                FormKind::ImportPrivate => {
                    let seed = SecretSeed::from_hex(&request.take("seed"))?;
                    let public = request.take("public");
                    let public_key = if public.is_empty() {
                        None
                    } else {
                        Some(public.parse().map_err(|_| ClientError::InvalidKey)?)
                    };
                    super::unlock::unlock(app, &mut request, action).await?;
                    ProfileInput::ImportSecret { seed, public_key }
                }
                _ => {
                    super::unlock::unlock(app, &mut request, action).await?;
                    ProfileInput::Generate
                }
            };
            let p = app.create_profile(&name, input).await?;
            app.select_profile(p.id).await?;
            profile = Some(p);
            Screen::Profiles
        }
        FormKind::Unlock => {
            super::unlock::unlock(app, &mut request, action).await?;
            Screen::Profiles
        }
        FormKind::ChangePassword => {
            let current = SecretString(request.take("current"));
            let new = request.take("new");
            if new.as_str() != request.take("confirm").as_str() {
                return Err(ClientError::InvalidInput);
            }
            app.change_password(
                current,
                SecretString(new),
                action.unwrap_or_else(|| app.begin_vault_action()),
            )
            .await?;
            Screen::Profiles
        }
        FormKind::Rename => {
            let id = request.target.as_ref().ok_or(ClientError::NotFound)?.id;
            let name = request.take("name");
            app.rename_profile(id, &name).await?;
            Screen::Profiles
        }
        FormKind::AddRelay => {
            let id = request.target.as_ref().ok_or(ClientError::NotFound)?.id;
            app.add_relay(id, &request.take("url")).await?;
            Screen::Relays
        }
        FormKind::NewDraft => {
            let id = request.target.as_ref().ok_or(ClientError::NotFound)?.id;
            app.create_draft(id, &request.take("title"), "").await?;
            Screen::Drafts
        }
    };
    Ok(FormOutcome {
        profile,
        screen,
        status: "Local operation completed.".into(),
    })
}
