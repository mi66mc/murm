# Plano 03 — Cliente, perfis, cofre e SQLite local

> Execução futura: usar superpowers:executing-plans ou superpowers:subagent-driven-development, tarefa por tarefa.

**Objetivo:** tornar murm-cli um cliente persistente com perfis criados/importados,
comandos e serviços reutilizáveis pela TUI.
**Arquitetura:** adicionar lib.rs na crate existente; perfis, cofre, armazenamento,
outbox e transporte são serviços sem dependência de widgets.
**Stack proposta:** Rusqlite bundled, Tokio, Reqwest, Clap, diretórios da plataforma,
Argon2id, XChaCha20-Poly1305, zeroize, RNG do sistema e trava de processo.
**Spec:** [desenho](../design.md), [protocolo](01-protocol.md) e [relay](02-relay.md).

## Restrições globais

Não mudar identidade do protocolo: perfil é cadastro LOCAL de uma chave pública.
SQLite do cliente é separado do SQLite do relay. Seeds entram apenas cifradas.
Não implementar DMs, backup padronizado ou rotação/delegação de identidade.
Publicações usam Pandoc Markdown como fonte. Nenhum segredo é enviado ao relay.
Conservar AGPL-3.0-only e crates existentes; não criar uma quarta crate cliente.

## Foco da revisão

1. Pública importada não corresponde à privada, ou pública duplicada cria outro perfil.
2. Senha/seed aparece em SQLite, WAL, argumentos, logs ou erro.
3. Cofre errado/corrompido sobrescreve dados e perde acesso aos perfis.
4. Erro de rede após POST cria outra assinatura/revisão no retry.
5. Batch ou troca de perfil avança cursor/aplica resposta no contexto errado.

## Arquivos e interfaces

Criar src/lib.rs, app.rs, error.rs, profiles.rs, vault.rs, transport.rs,
articles.rs, sync.rs, outbox.rs, commands.rs, storage/mod.rs,
storage/migrations.rs. main.rs permanece entrada do binário.

Tipos do cliente:
- ProfileId: UUID local, sem significado no protocolo.
- ProfileMode: ReadOnly ou Signing; ProfileSummary contém id, nome e SignerKey,
  modo, estado bloqueado e relays (nunca seed).
- ProfileInput: Generate, ImportSecret(seed, public_key opcional), ImportPublic.
  Material secreto é wrapper zeroizante sem Debug/Serialize.
- DraftId e Draft: autor/ProfileId, publication, revisão, source, metadados,
  versão local para concorrência e evento enfileirado opcional.
- ClientError: VaultNotInitialized, Locked, ReadOnly, InvalidKey, KeyMismatch, WrongPassword,
  CorruptVault, NotFound, Conflict, Protocol, Network, Database.
- VaultActionId: token ligado à geração do cofre, sem segredos.
  VaultStatus informa apenas se o cofre existe e está desbloqueado.
- ClientService: dono dos serviços, com handle do worker de banco e do cofre.
  Operações públicas assíncronas:
  list_profiles() -> Result<Vec<ProfileSummary>, ClientError>;
  create_profile(name, ProfileInput) -> Result<ProfileSummary, ClientError>;
  select_profile(ProfileId) -> Result<(), ClientError>;
  begin_vault_action() -> VaultActionId;
  initialize_vault(password: SecretString, action: VaultActionId) -> Result<VaultStatus, ClientError>;
  unlock(password: SecretString, action: VaultActionId) -> Result<VaultStatus, ClientError>;
  change_password(current: SecretString, new: SecretString, action: VaultActionId) -> Result<(), ClientError>;
  cancel_vault_action(VaultActionId); lock();
  save_draft(ProfileId, Draft) -> Result<DraftId, ClientError>;
  publish(ProfileId, DraftId) -> Result<EventId, ClientError>;
  sync_profile(ProfileId, RelayUrl) -> Result<SyncSummary, ClientError>.
- RelayTransport implementa info, submit, fetch, query e sync com tipos do plano 01.
  Recebe URL e limites de rede; nunca perfil privado ou senha.
- Serviço emite ClientEvent com IDs de tarefa, ProfileId e geração de UI.
  As telas não recebem acesso a ClientStore ou ao VaultKey.

### Task 1: Banco local e perfis somente leitura

**Arquivos:** storage/*, profiles.rs, error.rs, tests/profiles.rs.

- [ ] Testar create_read_only_and_restart, public_key_unique,
  rename_keeps_identity, independent_client_databases,
  migration_failure_preserves_database e second_instance_is_refused.
- [ ] Rodar cargo test -p murm-cli --test profiles; esperar falhas.
- [ ] Criar tabelas client_meta, profiles, vault, profile_secrets,
  profile_relays, drafts, verified_events, publication_heads, sync_cursors,
  outbox e outbox_destinations.
  profiles.public_key é UNIQUE; secret só existe para Signing.
  Eventos públicos podem ser compartilhados no cache, mas rascunhos, cursor
  de acompanhamento, preferências e fila possuem contexto explícito de perfil.
- [ ] Aplicar migrações/WAL/foreign keys e worker dedicado do desenho.
  Resolução de data-dir é única para CLI e TUI; --data-dir sobrescreve o padrão.
  Trava de processo cobre operações de escrita, com erro claro em segunda instância.
- [ ] Rodar testes e commit feat(client): persist local profiles.

### Task 2: Cofre e importação de identidade

**Arquivos:** vault.rs, profiles.rs, tests/vault.rs.
**Consome:** derive_public_key, SignerKey e formato de seed do plano 01.

- [ ] Testar generated_profile_can_sign_after_restart, import_secret_derives_public,
  mismatched_pair_rolls_back, upgrade_read_only_preserves_state,
  duplicate_import_is_idempotent, wrong_password_does_not_mutate,
  ciphertext_tampering_fails, changing_password_is_atomic e lock_blocks_signing.
- [ ] Rodar cargo test -p murm-cli --test vault; esperar falhas.
- [ ] Implementar o cofre exatamente como no desenho: master key 32 bytes,
  Argon2id com 65536 KiB/3/1, salt 16 bytes e XChaCha20-Poly1305 com nonce 24.
  Dados associados identificam versão/contexto/pública; copiar ciphertext para
  outro perfil não pode produzir uma seed aceita.
- [ ] Exigir uma transação para criar/promover perfil e salvar a seed cifrada.
  Validar/derivar a pública antes de gravar; promoção requer desbloqueio.
  O worker do cofre entrega assinaturas, não seeds aos widgets/transportes.
- [ ] Conferir parâmetros do cofre antes do KDF. Limpar buffers transitórios
  de senha/seed, não derivar Debug para segredos. Não alegar apagamento físico
  garantido de memória, WAL antigo ou backups. Rascunhos públicos não são cifrados.
- [ ] Adicionar plaintext_seed_absent_from_db_and_wal usando seed de teste
  conhecida e varredura dos bytes do arquivo e WAL; adicionar
  secret_not_in_error_or_debug e malformed_vault_parameters_rejected.
- [ ] Acrescentar cancel_during_kdf_never_unlocks_later e
  cancel_after_initialization_commit_leaves_locked_vault. O serviço coordena
  geração/token e instalação do segredo sob exclusão mútua; KDF fica fora dela.
  lock/cancel invalidam a tentativa e removem um segredo instalado por ela.
  create_profile de Signing exige cofre desbloqueado; sem cofre retorna
  VaultNotInitialized, novo caso explícito de ClientError.
  A aplicação consulta VaultStatus e chama initialize_vault antes de criar.
- [ ] Rodar testes e commit feat(client): encrypt signing profiles locally.

### Task 3: Transporte, cache e sincronização

**Arquivos:** transport.rs, sync.rs, tests/client_sync.rs.
**Consome:** APIs comuns, ClientStore e relay do plano 02.

- [ ] Testar verify_before_cache, sync_cursor_commits_with_events,
  invalid_batch_leaves_cursor_unchanged, replay_deduplicates,
  late_event_is_visible, expired_cursor_rescans, retry_after_respected,
  http_alias_fallback_only_for_method_failure e network_timeout_is_bounded.
- [ ] Rodar cargo test -p murm-cli --test client_sync; esperar falhas.
- [ ] Configurar URLs por perfil com HTTP loopback/HTTPS, sem credenciais na URL.
  Limitar conexão a 5 s e operação a 20 s por padrão, configuráveis. Não seguir
  redirects automaticamente. Obter /info e validar os anúncios sem tratá-los
  como prova de honestidade. Negociar QUERY e fallback segundo a MIP-03.
- [ ] Validar cada evento com a biblioteca antes de armazenar; aplicar preferência
  local. Eventos aceitos, heads e marcador final ficam na mesma transação.
  O decoder mantém os bytes dos eventos ou usa o visitor estrito compartilhado
  antes de criar Value; não pode descartar campos repetidos da resposta e só
  depois verificar um evento já modificado.
  Falhas semânticas de kind anunciado como validado obedecem a regra de batch
  inválido; persistência falha não pode avançar cursor.
- [ ] Para cursor expirado, reiniciar histórico retido e deduplicar por ID.
  Usar filtros estáveis sem excluir retiradas. Cada assinatura, peer, filtro
  e perfil tem contexto explícito; não reutilizar marcador entre assinaturas
  diferentes de filtros.
- [ ] Rodar testes e commit feat(client): add verified relay synchronization.

### Task 4: Artigos e fila de envio

**Arquivos:** articles.rs, outbox.rs, tests/publishing.rs.
**Produz:** rascunhos e operações de publicação consumidas pela TUI.

- [ ] Testar read_only_cannot_publish, locked_cannot_sign,
  draft_survives_restart, publish_uses_selected_profile,
  duplicate_publish_reuses_queued_event, timeout_retries_same_id,
  editing_uses_max_known_revision, withdraw_and_restore,
  stale_revision_conflict_is_visible e switching_profile_keeps_outbox_owner.
- [ ] Rodar cargo test -p murm-cli --test publishing; esperar falhas.
- [ ] Criar publication estável aleatória no primeiro rascunho; nome/título
  não é sua identidade. Edição usa maior revisão conhecida + 1, sem overflow.
  Nunca afirmar que ela é globalmente a maior. Retirada é evento com content null.
- [ ] Confirmar o conteúdo e ProfileId antes de assinar. Persistir o evento
  assinado e destinos antes da rede; a outbox guarda os mesmos bytes/ID nas
  tentativas. Pressionar publicar novamente no mesmo snapshot já enfileirado
  devolve o mesmo ID, não cria outra revisão por mudar o relógio.
- [ ] Guardar resultado por destino: pending, stored com retain_until,
  refused ou retry_at. Não descrever stored como entregue/lido/replicado.
  Um sucesso de rede seguido de falha local é repetido com o mesmo evento.
- [ ] Cliente que já sabe de retirada não apresenta revisão antiga como atual,
  mesmo se outro relay a retornar. Cache histórico e visão atual são separados.
- [ ] Rodar testes e commit feat(client): publish revision events from durable outbox.

### Task 5: Comandos e API utilizada pela TUI

**Arquivos:** app.rs, commands.rs, main.rs, tests/commands.rs.
**Consome:** todos os serviços anteriores.

- [ ] Testar profile_commands_match_service, public_import_never_unlocks,
  secret_input_not_in_argv, cli_uses_same_database_as_tui e headless_never_hangs.
- [ ] Rodar cargo test -p murm-cli --test commands; esperar falhas.
- [ ] Implementar comandos profile create/import/list/use/rename/remove,
  vault init/passwd, relay add/list/remove, article draft/edit/publish/history/withdraw/restore,
  fetch, query, sync e outbox retry.
  --public-key permite pública por argumento; privada só por --secret-file,
  --secret-stdin ou formulário oculto, com chave derivada/conferida.
- [ ] Senha vem de prompt oculto num terminal ou --password-file para automação,
  nunca --password valor nem variável de ambiente. Quando stdin contém a seed,
  o prompt de senha usa o terminal de controle; sem terminal exigir password-file
  ou dar erro imediatamente. Não ler dois segredos ambiguamente do mesmo stream.
- [ ] Exportar ClientService/ClientEvent/DTOs sem VaultKey. Publicação e mudanças
  locais passam pelos mesmos serviços, independentemente de comando ou TUI.
  Remover perfil requer confirmação explícita ou --yes; não muda dados remotos
  e não promete destruir cópias já existentes da chave.
- [ ] Rodar testes e commit feat(client): expose profile and article commands.

## Saída deste plano

cargo test -p murm-cli --lib e testes profiles/vault/client_sync/publishing/commands.
Conferir cargo tree -p murm-cli: não há dependência de murm-relay.
Perfis e rascunhos sobrevivem a restart; senha não é cadastro no protocolo.
Ainda não implementar telas neste plano. A execução sem subcomando será ligada
à TUI no plano 04; até lá pode mostrar ajuda.
