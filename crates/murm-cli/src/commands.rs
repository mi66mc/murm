use crate::*;
use clap::{Parser, Subcommand};
use murm_protocol::*;
use serde_json::{Value, json};
use std::{
    io::{IsTerminal, Read},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(
    version,
    about = "Experimental Murm client: local signing profiles, articles and terminal UI"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub max_db_bytes: Option<u64>,
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    #[arg(long, global = true, conflicts_with = "data_dir")]
    pub db: Option<PathBuf>,
    #[arg(long, global = true)]
    pub profile: Option<ProfileId>,
    #[arg(long, global = true)]
    pub password_file: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Subcommand)]
pub enum Command {
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    Vault {
        #[command(subcommand)]
        command: VaultCommand,
    },
    Relay {
        #[command(subcommand)]
        command: RelayCommand,
    },
    Article {
        #[command(subcommand)]
        command: ArticleCommand,
    },
    Fetch {
        id: EventId,
        #[arg(long)]
        relay: Option<String>,
    },
    Query {
        #[arg(long)]
        relay: Option<String>,
        #[arg(long)]
        request_file: Option<PathBuf>,
        #[arg(long)]
        current: bool,
    },
    Sync {
        #[arg(long)]
        relay: Option<String>,
        #[arg(long)]
        filters_file: Option<PathBuf>,
    },
    Outbox {
        #[command(subcommand)]
        command: OutboxCommand,
    },
    Editor {
        program: String,
        #[arg(long = "arg", allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
#[derive(Subcommand)]
pub enum ProfileCommand {
    Create {
        name: String,
    },
    Import {
        name: String,
        #[arg(long)]
        public_key: Option<SignerKey>,
        #[arg(long, conflicts_with = "secret_stdin")]
        secret_file: Option<PathBuf>,
        #[arg(long)]
        secret_stdin: bool,
    },
    List,
    Use {
        id: ProfileId,
    },
    Rename {
        id: ProfileId,
        name: String,
    },
    Remove {
        id: ProfileId,
        #[arg(long)]
        yes: bool,
    },
}
#[derive(Subcommand)]
pub enum VaultCommand {
    Init,
    Passwd {
        #[arg(long)]
        new_password_file: Option<PathBuf>,
    },
}
#[derive(Subcommand)]
pub enum RelayCommand {
    Add {
        url: String,
    },
    List,
    Remove {
        url: String,
    },
    Info {
        #[arg(long)]
        relay: Option<String>,
    },
}
#[derive(Subcommand)]
pub enum ArticleCommand {
    Draft {
        #[arg(long, default_value = "")]
        title: String,
        #[arg(long)]
        source_file: Option<PathBuf>,
    },
    Edit {
        #[arg(
            long,
            required_unless_present = "publication",
            conflicts_with = "publication"
        )]
        draft: Option<DraftId>,
        #[arg(long)]
        publication: Option<String>,
        #[arg(long)]
        source_file: Option<PathBuf>,
    },
    List {
        #[arg(long)]
        withdrawn: bool,
    },
    Publish {
        draft: DraftId,
    },
    History {
        publication: String,
        #[arg(long)]
        signer: Option<SignerKey>,
    },
    Withdraw {
        publication: String,
    },
    Restore {
        publication: String,
        #[arg(long)]
        source_file: PathBuf,
    },
}
#[derive(Subcommand)]
pub enum OutboxCommand {
    List,
    Retry {
        #[arg(long)]
        event: Option<EventId>,
    },
}
pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}
fn secret_prompt_allowed() -> bool {
    std::io::stderr().is_terminal() || std::io::stdout().is_terminal()
}
fn read_limited(path: &Path, max: u64) -> Result<Zeroizing<String>, ClientError> {
    let mut value = Zeroizing::new(String::new());
    std::fs::File::open(path)?
        .take(max + 1)
        .read_to_string(&mut value)?;
    if value.len() as u64 > max {
        return Err(ClientError::InvalidInput);
    }
    Ok(value)
}
fn strip_line(value: &mut String) {
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
}
fn password(path: Option<&Path>, confirm: bool) -> Result<SecretString, ClientError> {
    if let Some(path) = path {
        let mut s = read_limited(path, 4096)?;
        strip_line(&mut s);
        return Ok(SecretString(s));
    }
    if !secret_prompt_allowed() {
        return Err(ClientError::InvalidInput);
    }
    let s = SecretString::new(rpassword::prompt_password("Local vault password: ")?);
    if confirm {
        let second = SecretString::new(rpassword::prompt_password("Confirm password: ")?);
        if s.0.as_str() != second.0.as_str() {
            return Err(ClientError::InvalidInput);
        }
    }
    Ok(s)
}
async fn unlock(app: &ClientService, path: Option<&Path>) -> Result<(), ClientError> {
    let status = app.vault_status().await?;
    if status.unlocked {
        return Ok(());
    }
    let action = app.begin_vault_action();
    let secret = password(path, !status.exists)?;
    if status.exists {
        app.unlock(secret, action).await?;
    } else {
        app.initialize_vault(secret, action).await?;
    }
    Ok(())
}
async fn profile(
    app: &ClientService,
    id: Option<ProfileId>,
) -> Result<ProfileSummary, ClientError> {
    if let Some(id) = id {
        app.profile(id).await
    } else {
        app.active_profile().await?.ok_or(ClientError::NotFound)
    }
}
async fn relay(
    app: &ClientService,
    id: ProfileId,
    selected: Option<String>,
) -> Result<String, ClientError> {
    selected
        .or(app.profile(id).await?.relays.into_iter().next())
        .ok_or(ClientError::NotFound)
}
pub async fn run(app: &ClientService, cli: Cli) -> Result<Value, ClientError> {
    let password_file = cli.password_file.as_deref();
    let result = match cli.command.ok_or(ClientError::InvalidInput)? {
        Command::Profile { command } => match command {
            ProfileCommand::List => serde_json::to_value(app.list_profiles().await?),
            ProfileCommand::Create { name } => {
                unlock(app, password_file).await?;
                let p = app.create_profile(&name, ProfileInput::Generate).await?;
                if app.active_profile().await?.is_none() {
                    app.select_profile(p.id).await?;
                }
                serde_json::to_value(p)
            }
            ProfileCommand::Import {
                name,
                public_key,
                secret_file,
                secret_stdin,
            } => {
                let secret = if let Some(path) = secret_file {
                    Some(read_limited(&path, 256)?)
                } else if secret_stdin {
                    let mut value = Zeroizing::new(String::new());
                    std::io::stdin().take(257).read_to_string(&mut value)?;
                    if value.len() > 256 {
                        return Err(ClientError::InvalidKey);
                    }
                    Some(value)
                } else {
                    None
                };
                let input = if let Some(mut secret) = secret {
                    strip_line(&mut secret);
                    let seed = SecretSeed::from_hex(&secret)?;
                    unlock(app, password_file).await?;
                    ProfileInput::ImportSecret { seed, public_key }
                } else if let Some(public) = public_key {
                    ProfileInput::ImportPublic(public)
                } else {
                    if !secret_prompt_allowed() {
                        return Err(ClientError::InvalidInput);
                    }
                    let secret =
                        Zeroizing::new(rpassword::prompt_password("Seed privada Ed25519 (hex): ")?);
                    let seed = SecretSeed::from_hex(&secret)?;
                    unlock(app, password_file).await?;
                    ProfileInput::ImportSecret {
                        seed,
                        public_key: None,
                    }
                };
                let p = app.create_profile(&name, input).await?;
                if app.active_profile().await?.is_none() {
                    app.select_profile(p.id).await?;
                }
                serde_json::to_value(p)
            }
            ProfileCommand::Use { id } => {
                app.select_profile(id).await?;
                serde_json::to_value(app.profile(id).await?)
            }
            ProfileCommand::Rename { id, name } => {
                app.rename_profile(id, &name).await?;
                serde_json::to_value(app.profile(id).await?)
            }
            ProfileCommand::Remove { id, yes } => {
                if !yes {
                    return Err(ClientError::InvalidInput);
                }
                app.remove_profile(id).await?;
                Ok(json!({"removed_locally":id}))
            }
        },
        Command::Vault { command } => match command {
            VaultCommand::Init => {
                let secret = password(password_file, true)?;
                serde_json::to_value(
                    app.initialize_vault(secret, app.begin_vault_action())
                        .await?,
                )
            }
            VaultCommand::Passwd { new_password_file } => {
                let current = password(password_file, false)?;
                let new = password(new_password_file.as_deref(), true)?;
                app.change_password(current, new, app.begin_vault_action())
                    .await?;
                Ok(json!({"password_changed":true}))
            }
        },
        Command::Editor { program, args } => {
            app.set_editor(EditorConfig { program, args }).await?;
            serde_json::to_value(app.editor().await?)
        }
        Command::Relay { command } => {
            let p = profile(app, cli.profile).await?;
            match command {
                RelayCommand::Add { url } => {
                    app.add_relay(p.id, &url).await?;
                    serde_json::to_value(app.profile(p.id).await?.relays)
                }
                RelayCommand::List => serde_json::to_value(p.relays),
                RelayCommand::Remove { url } => {
                    app.remove_relay(p.id, &url).await?;
                    serde_json::to_value(app.profile(p.id).await?.relays)
                }
                RelayCommand::Info { relay: selected } => {
                    let url = relay(app, p.id, selected).await?;
                    serde_json::to_value(app.relay_info(p.id, &url).await?)
                }
            }
        }
        Command::Article { command } => {
            let p = profile(app, cli.profile).await?;
            match command {
                ArticleCommand::Draft { title, source_file } => {
                    let source = source_file
                        .map(|p| read_limited(&p, 4194304))
                        .transpose()?
                        .unwrap_or_default();
                    serde_json::to_value(app.create_draft(p.id, &title, &source).await?)
                }
                ArticleCommand::Edit {
                    draft,
                    publication,
                    source_file,
                } => {
                    let mut d = if let Some(id) = draft {
                        app.load_draft(p.id, id).await?
                    } else {
                        app.edit_publication(p.id, &publication.ok_or(ClientError::InvalidInput)?)
                            .await?
                    };
                    if let Some(path) = source_file {
                        d.content = json!(read_limited(&path, 4194304)?.as_str());
                        d = app.save_draft(p.id, d).await?;
                    } else {
                        if !interactive() {
                            return Err(ClientError::InvalidInput);
                        }
                        d = app.edit_draft_external(p.id, d.id).await?;
                    }
                    serde_json::to_value(d)
                }
                ArticleCommand::List { withdrawn } => {
                    serde_json::to_value(app.list_articles(withdrawn).await?)
                }
                ArticleCommand::Publish { draft } => {
                    if app.load_draft(p.id, draft).await?.queued_event_id.is_none() {
                        unlock(app, password_file).await?;
                    }
                    let id = app.publish(p.id, draft).await?;
                    app.retry_event(p.id, id).await?;
                    Ok(json!({"id":id,"outbox":app.outbox(p.id).await?}))
                }
                ArticleCommand::History {
                    publication,
                    signer,
                } => serde_json::to_value(
                    app.article_history(signer.unwrap_or(p.public_key), &publication)
                        .await?,
                ),
                ArticleCommand::Withdraw { publication } => {
                    unlock(app, password_file).await?;
                    let id = app.withdraw(p.id, &publication).await?;
                    app.retry_event(p.id, id).await?;
                    Ok(json!({"id":id,"outbox":app.outbox(p.id).await?}))
                }
                ArticleCommand::Restore {
                    publication,
                    source_file,
                } => {
                    let source = read_limited(&source_file, 4194304)?;
                    unlock(app, password_file).await?;
                    let id = app.restore(p.id, &publication, &source).await?;
                    app.retry_event(p.id, id).await?;
                    Ok(json!({"id":id,"outbox":app.outbox(p.id).await?}))
                }
            }
        }
        Command::Fetch {
            id,
            relay: selected,
        } => {
            let p = profile(app, cli.profile).await?;
            let url = relay(app, p.id, selected).await?;
            serde_json::to_value(app.fetch_event(p.id, &url, id).await?)
        }
        Command::Query {
            relay: selected,
            request_file,
            current,
        } => {
            let p = profile(app, cli.profile).await?;
            let url = relay(app, p.id, selected).await?;
            let q = if let Some(path) = request_file {
                serde_json::from_value(parse_json(
                    read_limited(&path, 65536)?.as_bytes(),
                    ParseLimits {
                        max_bytes: 65536,
                        max_depth: 32,
                    },
                )?)
                .map_err(|_| ClientError::InvalidInput)?
            } else {
                EventQuery {
                    filters: vec![Filter {
                        kinds: Some(vec![1]),
                        ..Default::default()
                    }],
                    limit: 20,
                    cursor: None,
                    view: current.then_some(QueryView::Current),
                }
            };
            let result = app.query_relay(p.id, &url, q).await?;
            Ok(
                json!({"events":result.events,"next_cursor":result.next_cursor,"local_rejections":result.rejections}),
            )
        }
        Command::Sync {
            relay: selected,
            filters_file,
        } => {
            let p = profile(app, cli.profile).await?;
            let urls = if let Some(url) = selected {
                vec![url]
            } else {
                p.relays
            };
            if urls.is_empty() {
                return Err(ClientError::NotFound);
            }
            let filters: FilterSet = if let Some(path) = filters_file {
                serde_json::from_value(parse_json(
                    read_limited(&path, 65536)?.as_bytes(),
                    ParseLimits {
                        max_bytes: 65536,
                        max_depth: 32,
                    },
                )?)
                .map_err(|_| ClientError::InvalidInput)?
            } else {
                vec![Filter {
                    kinds: Some(vec![1]),
                    ..Default::default()
                }]
            };
            let mut results = Vec::new();
            for url in urls {
                results.push(
                    json!({"relay":url,"sync":app.sync_filtered(p.id,&url,filters.clone()).await?}),
                );
            }
            Ok(json!(results))
        }
        Command::Outbox { command } => {
            let p = profile(app, cli.profile).await?;
            if let OutboxCommand::Retry { event } = command {
                if let Some(id) = event {
                    app.retry_event(p.id, id).await?;
                } else {
                    app.retry_outbox(p.id).await?;
                }
            }
            serde_json::to_value(app.outbox(p.id).await?)
        }
    };
    result.map_err(|_| ClientError::InvalidInput)
}
