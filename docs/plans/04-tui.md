# Plano 04 — TUI, fluxos completos e testes de falha

> Execução futura: usar superpowers:executing-plans ou superpowers:subagent-driven-development, tarefa por tarefa.

**Objetivo:** interface de terminal para perfis, artigos, relays e sincronização.
**Arquitetura:** eventos de teclado e respostas de tarefas atualizam AppState;
renderização apenas lê estado e chama ClientService para executar ações.
**Stack proposta:** Ratatui/Crossterm, Tokio e testes com TestBackend.
**Spec:** [desenho](../design.md), [cliente](03-client.md), [relay](02-relay.md).

## Restrições globais

Usar a crate murm-cli existente; TUI e comandos partilham dados/serviços.
Sem SQL, KDF ou assinatura dentro do draw. Sem logs imprimindo no terminal ativo.
Sem widgets que exponham senha/seed ou snapshots com segredos reais.
Não instalar/compilar o relay para instalar apenas o CLI.

## Foco da revisão

1. Crash/cancelamento deixa o terminal em raw mode ou esconde o cursor.
2. Troca de perfil aplica resultado de rede no perfil errado.
3. UI congela durante KDF, SQLite ou rede lenta.
4. Perfil somente leitura oferece publicação ou ações de assinatura.
5. Repetição de tecla ou timeout duplica revisão/publicação.

## Arquivos e interfaces

Criar src/tui/mod.rs, state.rs, actions.rs, render.rs, terminal.rs, tasks.rs,
views/profiles.rs, views/unlock.rs, views/relays.rs, views/articles.rs,
views/drafts.rs e views/outbox.rs. Esses módulos usam ClientService do plano 03.

AppState guarda Screen, ProfileSummary ativo, estado público do cofre,
UiGeneration, itens, seleção/rolagem e tarefas. Nunca segredos persistentes.
Campos sensíveis do formulário são buffers zeroizantes descartados após
confirmar/cancelar. Action descreve intenção; tarefas têm TaskId/ProfileId/generation.
Screen inclui Profiles, ProfileForm, Unlock, Relays, Articles, Article,
Drafts, Outbox e confirmação de remoção. Enter confirma, Esc volta/cancela,
setas e PageUp/PageDown navegam, Tab muda foco, ? mostra ajuda e Ctrl-C sai.

A biblioteca Ratatui é uma escolha proposta, não formato de conteúdo Murm.

### Task 1: Estado, navegação e recuperação do terminal

**Arquivos:** tui/state.rs, actions.rs, terminal.rs, tests/tui_navigation.rs.

- [ ] Testar starts_at_profiles_when_empty, escape_restores_previous_screen,
  noninteractive_prints_help, ctrl_c_exits_cleanly e terminal_restored_after_error.
- [ ] Rodar cargo test -p murm-cli --test tui_navigation; esperar falhas.
- [ ] Implementar AppState + reducer; terminal guard restaura raw mode,
  alternate screen e cursor em saída normal, erro e panic recuperável.
  Não prometer restauração após kill forçado pelo sistema.
- [ ] Nenhuma chamada de rede/DB/KDF no draw. Canal limitado para eventos de
  tarefas, cancelamento e encerramento ordenado dos workers.
- [ ] Rodar testes e commit feat(tui): add terminal lifecycle and navigation.

### Task 2: Perfis, importação e desbloqueio

**Arquivos:** views/profiles.rs, views/unlock.rs, tasks.rs, tests/tui_profiles.rs.

- [ ] Testar generate_profile_flow, import_pair_rejects_mismatch,
  import_public_is_read_only, duplicate_public_promotes_existing_profile,
  secrets_masked_and_cleared, unlock_wrong_password_keeps_profiles,
  changing_profile_discards_stale_ui_results e lock_removes_signing_actions.
- [ ] Rodar cargo test -p murm-cli --test tui_profiles; esperar falhas.
- [ ] Mostrar lista de perfis com nome, pública abreviada, modo e bloqueio.
  Fluxos distintos: gerar identidade, importar privada com pública opcional,
  importar pública somente leitura. Pública completa disponível para conferir.
- [ ] Formulários de senha/privada são ocultos; copiar/mostrar privada não é uma
  ação implícita. Limitar tamanho antes do decoding. Erro não repete o segredo.
  Confirmar remoção local e explicar que nada foi removido dos relays.
- [ ] Disparar KDF em worker sem bloquear interface; no cancelamento, descartar
  resultado/segredos e não deixar a tela anterior desbloqueada por resposta tardia.
  Usar VaultActionId do serviço, não apenas UiGeneration. Esc chama
  cancel_vault_action; antes do primeiro Signing mostrar criação/confirmação
  de senha e initialize_vault. Caso um cofre vazio já tenha sido gravado quando
  houve cancelamento, mantê-lo bloqueado e refletir VaultStatus na tela.
- [ ] Rodar testes e commit feat(tui): add local profile and vault flows.

### Task 3: Relays, artigos, rascunhos e outbox

**Arquivos:** views/relays.rs, articles.rs, drafts.rs, outbox.rs,
tests/tui_articles.rs.

- [ ] Testar offline_cache_is_readable, relay_settings_are_profile_scoped,
  history_is_distinct_from_current, withdraw_requires_owner,
  read_only_can_sync_but_not_publish, repeated_confirm_queues_once e
  status_shows_local_storage_not_delivery.
  Incluir terminal_escape_sequences_render_as_text e unicode_navigation_does_not_panic.
- [ ] Rodar cargo test -p murm-cli --test tui_articles; esperar falhas.
- [ ] Mostrar relays configurados, capacidades/políticas obtidas por /info,
  sincronização em andamento e último erro. Não iniciar consulta em outros
  relays por resultado vazio sem ação/configuração explícita do usuário.
- [ ] Listar artigos por visão atual e oferecer histórico separado, edição,
  retirada/restauração, rascunhos e fila. Renderizar fonte Pandoc como texto
  com rolagem, sem executar HTML ou filtros.
  Escapar caracteres de controle vindos de artigos, metadados e respostas:
  texto remoto não pode emitir sequências ANSI/OSC para o terminal.
  Desenhar só a região visível e tratar posições em Unicode, sem cortar bytes
  no meio de um caractere.
- [ ] Proposta de edição: executar editor configurado como executável+argv,
  sem shell; inserir o caminho de arquivo temporário como argumento separado.
  Liberar terminal antes de abrir o editor, retomá-lo em qualquer retorno.
  Falha/cancelamento não assina nem descarta o rascunho salvo.
- [ ] A confirmação de envio mostra perfil/pública, título/revisão e destinos.
  A tarefa usa o ProfileId capturado, não lê o perfil ativo depois de esperar rede.
- [ ] Rodar testes e commit feat(tui): add relay and publication screens.

### Task 4: Ligação do binário e testes completos

**Arquivos:** main.rs, tests/end_to_end.rs, README.md do projeto.
**Consome:** relay completo dos testes e serviços/TUI anteriores.

- [ ] Escrever testes two_relays_two_clients, create_profile_publish_read,
  stop_origin_read_replica, restart_retains_profiles_and_events,
  offline_edit_then_same_event_retry, withdrawal_and_body_expiry,
  stale_cursor_recovers e storage_failure_does_not_ack.
- [ ] Rodar cargo test --workspace --test end_to_end; inicialmente falha.
  Antes desse comando, rodar cargo build --workspace. O teste mora em murm-cli;
  localizar o relay junto ao binário CARGO_BIN_EXE_murm-cli, com o sufixo da
  plataforma, ou aceitar MURM_RELAY_BIN explícito. Não executar cargo de dentro
  do teste. O executável do CLI não depende da crate murm-relay.
- [ ] Executar murm-cli sem argumentos na TUI interativa; subcomandos continuam
  utilizáveis sem desenhar tela. Tratar erros de inicialização antes de raw mode.
- [ ] Ajustar somente integração necessária, usando portas loopback efêmeras,
  diretórios temporários separados e relógio injetado. Não usar chaves reais.
  Não lançar instâncias persistentes no computador durante a execução dos testes.
- [ ] Rodar cargo test --workspace, cargo fmt --all -- --check,
  cargo clippy --workspace --all-targets -- -D warnings.
  Rodar cargo build -p murm-cli isoladamente e confirmar que o relay não é instalado.
- [ ] Documentar execução de dois relays/cliente, caminhos de dados, geração e
  importação de perfis, cofre, modo somente leitura e limites do protótipo.
- [ ] Commit: test: validate the relay and TUI publication workflow.

### Task 5: Medição de gargalos depois da correção

**Arquivos:** docs/prototype-results.md (relatório futuro, não criar agora).

- [ ] Medir lotes de 100, 1000 e 10000 eventos, fontes de 1 KiB e 100 KiB,
  respeitando /info e rate limits; registrar política usada.
- [ ] Medir p50/p95 de publicação/consulta/sync, custo da verificação, fila
  de replicação, uso de memória e crescimento do banco.
- [ ] Repetir com um peer offline e com retenção curta; confirmar orçamento
  finito e que a TUI continua respondendo.
- [ ] Registrar resultados e limitações, distinguindo gargalo do código e
  lacuna da MIP. Corrigir MIPs por mudança revisada quando necessário.
- [ ] Commit: docs: record prototype measurements.

## Saída deste plano

Usuário consegue criar/importar/selecionar perfis, desbloquear a assinatura,
editar/publicar/retirar artigo e ler/sincronizar outro relay pela TUI.
O cliente continua instalável separadamente. Não declarar suporte a DMs,
renderização completa de Pandoc ou interoperabilidade independente.
