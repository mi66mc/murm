# Plano 02 — Relay HTTP persistente

> Execução futura: usar superpowers:executing-plans ou superpowers:subagent-driven-development, tarefa por tarefa.

**Objetivo:** servir publications-test-v1 com SQLite e propagação limitada.
**Arquitetura:** HTTP -> validação compartilhada -> serviço -> worker SQLite.
Retenção, cursores e filas são estado do relay, fora do evento assinado.
**Stack proposta:** Tokio, Axum, Rusqlite com SQLite bundled, Reqwest, HMAC/SHA-256,
base64url, configuração TOML e logs estruturados sem dados secretos.
**Spec:** [desenho](../design.md), [plano 01](01-protocol.md) e MIPs 03–06 no
[commit de referência](https://github.com/mi66mc/mips/tree/c3d601848cd05c41b3726a9dc2631239656da8e7).

## Restrições globais

AGPL-3.0-only; eventos verificados por murm-protocol; kind 1 somente.
Sem login; /info anuncia configuração efetiva; lookup e anúncios desativados.
HTTP simples somente em loopback no teste. Para uso remoto, TLS direto ou
reverse proxy HTTPS com conexão local. Não confiar em X-Forwarded-For arbitrário.

## Foco da revisão

1. ACK antes do commit ou evento sem registro de admissão.
2. Reinício que perde cursores, filas ou a memória de retirada.
3. Query/current que seleciona versão antiga após filtrar metadados.
4. Paginação por bytes pulando evento ou repetindo página vazia.
5. Filas/retries que amplificam cópias indefinidamente entre dois relays.

## Arquivos e interfaces

Criar src/lib.rs, config.rs, service.rs, http.rs, cursor.rs, replication.rs,
storage/mod.rs, storage/migrations.rs, storage/events.rs, storage/query.rs.
Modificar main.rs e Cargo.toml.

RelayConfig contém a forma completa de /info e campos locais: database_path,
listen, peers, trusted_proxy (desativado por padrão), backoff/timeout e limite
agregado de snapshots/filas. Config inválida impede iniciar, não anuncia limites falsos.

Clock::now_unix() -> u64 é injetável nos serviços/testes, sem sleeps para expiração.
RelayService recebe comandos do HTTP e coordena validação. RelayStore, no worker,
oferece operações síncronas:
- admit(&VerifiedEvent, &AdmissionPolicy, now: u64) -> Result<StoreReceipt, StoreError>.
- fetch(EventId, now: u64) -> Result<Option<VerifiedEvent>, StoreError>.
- query(&EventQuery, now: u64) -> Result<QueryResponse, StoreError>.
- sync(&SyncRequest, now: u64) -> Result<SyncResponse, StoreError>.
- import_sync_batch(peer: &PeerId, filters: &FilterSet, batch: ValidatedSyncBatch,
  now: u64) -> Result<SyncImportReport, StoreError>. ValidatedSyncBatch é
  tipo interno do relay: eventos verificados, decisões semânticas/políticas
  registráveis e marcador da origem, produzido somente após validar o lote.

Nomes acima são o contrato deste plano; usar os tipos da biblioteca, sem recriar
Event, filtros ou códigos de erro no servidor.

### Task 1: Configuração, migrações e worker

**Arquivos:** config.rs, storage/mod.rs, storage/migrations.rs,
tests/storage.rs, main.rs.
**Produz:** RelayConfig::load(path), RelayStore::open(path), worker de DB.

- [ ] Testar rejects_inconsistent_info_policy, migrations_are_repeatable,
  corrupt_database_not_recreated e sqlite_worker_does_not_block_runtime.
- [ ] Rodar cargo test -p murm-relay --test storage; esperar falhas.
- [ ] Criar tabelas: relay_meta(epoch e segredo HMAC), events(id único, JSON
  completo verificado, signer/kind/time, admission_seq, admitted_at, prazos),
  publication_heads(chave lógica, maior revision/id/status),
  propagation_jobs(event_id, peer, attempts, next_try, deadline),
  seen_ids(id, expires_at), peer_sync(peer, filter_hash, cursor),
  query_snapshots(id, query_hash, IDs ordenados, posição, validade).
  admission_seq é monotônica, não reutiliza números após remoção.
- [ ] Aplicar foreign keys, WAL e timeout do desenho. Manter estado HMAC/epoch.
  Migrações usam user_version; configurações não aceitam bytes/prazos impossíveis.
- [ ] Rodar testes e commit feat(relay): add persistent relay storage.

### Task 2: Admissão, retenção e estado atual

**Arquivos:** storage/events.rs, service.rs, tests/admission.rs.
**Consome:** protocolo validado, política efetiva por kind e Clock.

- [ ] Testar commit_before_receipt, duplicate_does_not_renew,
  duplicate_has_no_new_sequence, reaccept_evicted_gets_new_sequence,
  future_timestamp_policy, minimum_and_maximum_retention,
  removal_exception_is_explicit, head_survives_body_expiry,
  higher_revision_restores e full_head_budget_refuses_new_work.
- [ ] Rodar cargo test -p murm-relay --test admission; esperar falhas.
- [ ] Implementar transação que grava evento/admissão/preferência e prepara
  trabalho de propagação quando houver capacidade. Só depois retornar stored.
  A fila cheia não desfaz armazenamento: apenas não agenda novo trabalho.
  A admissão valida também os metadados de kind 1 antes de atualizar heads,
  inclusive quando chamada pelo importador de sync. Duplicatas passam por
  validação, mas preservam conteúdo/deadline/sequence.
- [ ] Expirar conteúdo pelos prazos da admissão; preservar publication_heads.
  Sem corpo do vencedor, current omite a publicação e não promove uma antiga.
  Preferência não usa created_at. Falha de gravação não emite 201.
- [ ] Rodar testes, reiniciar conexão no teste e commit feat(relay): enforce
  durable admission and retention.

### Task 3: Consultas, snapshots e sincronização

**Arquivos:** storage/query.rs, cursor.rs, tests/query_sync.rs.
**Produz:** query/sync e tokens opacos autenticados.

- [ ] Testar filters_match_reference_implementation, raw_keyset_ties,
  current_selected_before_filters, current_snapshot_under_updates,
  late_old_event_appears_in_sync, wrong_filter_cursor_rejected,
  cursor_expires_explicitly, sync_survives_restart,
  response_byte_limit_does_not_skip e expired_rows_do_not_loop.
- [ ] Rodar cargo test -p murm-relay --test query_sync; esperar falhas.
- [ ] Traduzir apenas filtros validados para SQL com parâmetros, sem interpolar
  paths arbitrários. Avaliar igualdade estruturada com semântica JCS da crate.
  Raw ordena created_at e ID decrescentes, com keyset.
- [ ] Current usa heads antes de filtros e fixa os IDs do resultado num snapshot.
  Limitar bytes e quantidade agregados de snapshots além do teto por snapshot;
  recusar por capacidade em vez de crescer memória indefinidamente.
- [ ] Tokens contêm versão interna, tipo, epoch, hash do pedido/filtros, limite
  quando aplicável, posição e expiry; autenticar com HMAC. Query vincula limite
  e view; sync permite variar só o tamanho do lote. Token forjado -> invalid_cursor;
  expirado/perda de história -> cursor_expired. Não expor SQL ou segredos.
- [ ] Sync usa admission_seq, persiste estado e nunca avança além de um evento
  que não coube na página. O próximo cursor pode avançar por registros expirados.
- [ ] Rodar testes e commit feat(relay): add query and resumable synchronization.

### Task 4: API, erros e limites

**Arquivos:** http.rs, service.rs, tests/http_api.rs.
**Produz:** servidor com todos os endpoints do perfil.

- [ ] Testar single_event_post, local_get_404, query_and_post_equivalent,
  query_sync_and_post_equivalent, fixed_window_shared_between_aliases,
  invalid_event_problem_details, unsupported_feature_explicit,
  source_ip_not_untrusted_forwarded_header e bounded_body_before_parse.
- [ ] Rodar cargo test -p murm-relay --test http_api; esperar falhas.
- [ ] Implementar QUERY como método HTTP customizado aceito pela camada http;
  não depender de um MethodFilter do framework reconhecer o token.
  Expor /info, /events, /events/{id}, /events/query e /events/sync.
  /relays e lookup=true respondem unsupported_feature.
- [ ] 201/200 e Location/retain_until seguem MIP-03. Problem Details mantém
  code e status corretos; JSON inválido, ID errado e assinatura inválida
  não viram policy_rejected. Aplicar limites de entrada descomprimida, depth,
  saída, filtros, snapshot e admissão antes do trabalho correspondente.
- [ ] Rate windows seguem UTC e compartilham orçamento entre aliases.
  Usar Retry-After no 429; sem credenciais de protocolo.
- [ ] Rodar testes e commit feat(relay): expose the publication test API.

### Task 5: Propagação e recuperação

**Arquivos:** replication.rs, tests/replication.rs.
**Consome:** peers configurados, fila persistente e API comum.

- [ ] Testar two_relays_converge_without_loop, retries_are_globally_bounded,
  duplicate_does_not_reopen_job, queue_full_keeps_receipt,
  restart_resumes_job, retry_after_respected, peer_sync_restores_outage,
  invalid_last_event_leaves_batch_uncommitted e failed_batch_commit_keeps_cursor.
- [ ] Rodar cargo test -p murm-relay --test replication; esperar falhas.
- [ ] Enviar o mesmo evento a peers configurados; registrar tentativas por evento
  entre destinos, timeout dentro do deadline e backoff crescente.
  Duplicata ou 201 confirma destino; outros 4xx não são repetidos sem mudança,
  exceto 408/429. 5xx/rede são tentados dentro do orçamento.
- [ ] Recuperar por sync com filtro estável kind 1 e cursor próprio por peer.
  Aplicar as regras de batch inválido/política da MIP-03. Persistir eventos e
  marcador juntos; não fazer busca recursiva ou descoberta implícita.
  Usar import_sync_batch, não várias chamadas a admit com commit próprio.
  Compartilhar um helper admit_in_transaction entre as operações.
  Validar o lote inteiro antes de iniciar alterações; evento inválido de uma
  origem que afirma validá-lo rejeita tudo. Falha de transação reverte eventos,
  heads, filas e cursor daquele lote. Rejeições locais permitidas pela MIP-03
  são registradas sem armazenar o evento como válido.
- [ ] Rodar testes e commit feat(relay): add bounded propagation and recovery.

## Saída deste plano

cargo test -p murm-relay; cargo fmt --all -- --check.
Processo inicia com murm-relay --config relay-a.toml; instâncias usam bancos
separados e peers explícitos. Config de teste encurta TTL sem falsificar /info.
Não colocar seeds de usuário no servidor nem chamar TUI a partir dele.
