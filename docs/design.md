# Murm em Rust: desenho do primeiro protótipo

Estado: proposta para revisão. Este documento e os planos não implementam funcionalidades.

## Objetivo e origem das decisões

Construir um relay persistente e um cliente instalável com comandos e TUI,
compartilhando a implementação do protocolo. O cliente mantém perfis, chaves,
rascunhos e dados sincronizados em SQLite; o relay tem seu próprio SQLite.

Decisões do usuário: Rust, Cargo workspace, três crates, AGPL-3.0-only,
TUI no cliente, perfis persistentes, criação e importação de chaves.
Identidade de protocolo continua sendo somente a chave pública; perfil local
não é conta no relay nem um evento de perfil com kind ainda não definido.

São propostas deste plano: modo somente leitura para chave pública isolada,
cofre cifrado com senha local, Ratatui/Crossterm, rusqlite com SQLite embarcado,
Axum/Tokio, cliente HTTP com Reqwest e editor externo para escrever artigos.
Essas escolhas ficam sujeitas à revisão do usuário; nenhuma já foi implementada.

A referência de protocolo é o repositório MIPs no commit
[c3d6018](https://github.com/mi66mc/mips/tree/c3d601848cd05c41b3726a9dc2631239656da8e7).
A sugestão antiga de TypeScript na MIP-05 é não normativa; a decisão posterior
do usuário é Rust. Os planos não alteram o protocolo para acomodar uma biblioteca.

## Abordagens consideradas

1. Cliente com perfis e chaves cifradas no SQLite: portátil, funciona offline e
   não exige gerenciador de segredos do sistema. É a proposta adotada no plano.
2. SQLite somente para dados públicos e chaves num chaveiro do sistema:
   integração adicional por plataforma; pode ser um backend futuro.
3. Assinador externo obrigatório: mantém o segredo fora do app, mas impediria
   o fluxo local de criar/importar chave solicitado. Pode ser adicionado depois.

A biblioteca do protocolo fica sem HTTP, SQLite, TUI ou cofre.
O relay não importa código do cliente. Comandos e TUI usam os mesmos serviços
do cliente, dentro de murm-cli, sem duplicar regras nem criar outra crate.

## Responsabilidades e interfaces entre os planos

| Plano | Responsabilidade | Entrega aos demais |
| --- | --- | --- |
| [01](plans/01-protocol.md) | Eventos, JSON/JCS, assinatura, tipos HTTP, filtros e revisões. | Tipos e validação determinística. |
| [02](plans/02-relay.md) | SQLite do servidor, API, políticas, cursores e replicação. | Relay HTTP do perfil publications-test-v1. |
| [03](plans/03-client.md) | SQLite local, perfis, cofre, rascunhos, rede e comandos. | ClientService utilizado também pela TUI. |
| [04](plans/04-tui.md) | Navegação, formulários e tarefas de terminal. | Interface sobre ClientService, sem SQL ou criptografia nas telas. |

Tipos públicos da biblioteca: EventId, SignerKey, Event, UnsignedEvent,
VerifiedEvent, ParseLimits, ProtocolError, PublicationKey, PublicationMeta,
EventQuery, SyncRequest, QueryResponse, SyncResponse, RelayInfo, StoreReceipt,
Problem e ProblemCode. VerifiedEvent tem construção controlada por verificação.
Verificação criptográfica não significa validação do conteúdo editorial.

Contratos principais:
- parse_and_verify(bytes: &[u8], limits: ParseLimits) -> Result<VerifiedEvent, ProtocolError>.
- sign_event(draft: &UnsignedEvent, seed: &[u8; 32]) -> Result<VerifiedEvent, ProtocolError>.
- validate_publication(event: &VerifiedEvent) -> Result<PublicationMeta, ProtocolError>.
- compare_revisions(a: &PublicationMeta, b: &PublicationMeta) -> Result<Ordering, ProtocolError>,
  recusando comparar publicações distintas.
- matches_filter(event: &VerifiedEvent, filter: &Filter) -> bool, após validação do pedido.

Tipos concretos do cliente são definidos no plano 03. Telas não recebem seeds,
senhas, master keys nem uma conexão SQLite.

## Perfis e chaves

Cada perfil local tem ID interno, nome de exibição, chave pública única, modo
de uso, relays associados e preferências. A chave pública é a identidade real;
o nome pode mudar sem mudar identidade. Assinatura e modo somente leitura
são estados locais, nunca permissões impostas pelo relay.

Fluxos:
- Criar: gerar seed Ed25519 de 32 bytes com RNG do sistema e derivar a pública.
- Importar privada: aceitar seed hexadecimal de exatamente 64 caracteres;
  derivar a pública. Se o usuário informar também uma pública, exigir igualdade.
- Importar pública: validar uma pública hexadecimal de 64 caracteres e criar
  um perfil somente leitura, sem chave privada.
- Reimportar a mesma pública: não duplicar identidade. Uma privada correspondente
  pode promover o perfil existente a assinante, preservando relays e rascunhos.
- Renomear, selecionar, bloquear, desbloquear e remover um perfil local.
  Remoção local nunca publica retirada ou apaga conteúdo dos relays.

Seeds expandidas de 64 bytes, formatos Nostr, frases de recuperação e hardware
signers não são aceitos implicitamente. Dar erro de formato em vez de adivinhar.

## Cofre local proposto

Ao criar o primeiro perfil assinante, inicializar um cofre:
- Master key aleatória de 32 bytes, cifrada por uma chave derivada da senha.
- Argon2id v19: memória 65536 KiB, 3 iterações, paralelismo 1, saída 32 bytes,
  salt aleatório de 16 bytes. Validar parâmetros antes de executar o KDF;
  no leitor v1 aceitar somente esse conjunto, evitando custo arbitrário no arquivo.
- XChaCha20-Poly1305, nonce aleatório novo de 24 bytes em cada cifragem.
  Nunca reutilizar nonce com a mesma chave.
- Master key envelopada autentica versão e parâmetros do cofre.
  Seed cifrada autentica versão, ID do perfil e pública como dados associados.
- Guardar somente ciphertext, salt, nonce e parâmetros em SQLite. Nem seeds
  nem senha podem entrar em SQL, WAL, logs, erros, argumentos ou variáveis de ambiente.
- Solicitar senha de no mínimo 12 caracteres Unicode na criação; não normalizar
  a senha. Desbloqueio aceita a senha existente sem aplicar políticas novas.
- Cofre permanece bloqueado no início. Bloquear limpa segredos em memória usando
  tipos zeroizantes; isso não promete apagar swap, dumps ou cópias já existentes.
- Trocar senha reencapsula a master key atomicamente. Senha errada ou corrupção
  não sobrescreve o cofre. Não há recuperação por servidor.
- Cofre tem operações explícitas de inicializar, desbloquear e trocar senha.
  create_profile com privada exige um cofre já desbloqueado; não cria um
  silenciosamente. A TUI/CLI conduz a inicialização antes do primeiro perfil.
- Cada operação de cofre tem token e geração controlados pelo serviço.
  lock/cancel invalidam a tentativa sob a mesma exclusão que instala a master key.
  KDF roda fora dessa exclusão; antes de instalar o resultado é preciso conferir
  geração/token novamente. Uma resposta tardia não desbloqueia o cofre.
  Se a inicialização já fez commit antes do cancelamento, pode restar um cofre
  vazio e bloqueado, nunca uma chave desbloqueada ou um arquivo sobrescrito.
- Desbloqueio em memória não cifra rascunhos nem eventos públicos: o arquivo
  SQLite inteiro não é um banco cifrado.

Senha é proteção local do aplicativo, não login de protocolo.
Perfis somente leitura podem existir sem inicializar/desbloquear o cofre.
Backup automático e formato de exportação de segredos ficam fora desta etapa.

## Persistência e concorrência

Dois bancos independentes, nunca o mesmo arquivo:
- Relay: eventos, admissão monotônica, preferências de revisão, filas de
  propagação, memória de IDs, estado de sincronização e segredos dos cursores.
- Cliente: perfis, cofre cifrado, relays, rascunhos, eventos verificados,
  preferências, cursores e outbox por destino.

Usar diretório de dados da plataforma; aceitar --data-dir e --db para testes.
Migrações versionadas, foreign_keys=ON, WAL e busy_timeout de 5 segundos.
Cada processo possui uma conexão gerenciada por worker dedicado. Chamadas
assíncronas enviam trabalho ao worker; não executar SQL/KDF pesado no loop da TUI.
Um app não abre o banco do outro. Erro de migração não apaga o banco.

Adicionar trava exclusiva de instância do cliente para impedir duas TUIs
desbloqueando/editando o mesmo cofre simultaneamente. Instâncias independentes
usam diretórios separados. Conexões curtas de inspeção read-only são opcionais.

## Cliente e TUI

Executar murm-cli sem subcomando abre a TUI se houver terminal interativo;
em entrada não interativa imprime ajuda e não espera uma senha indefinidamente.
Subcomandos permitem os mesmos fluxos para scripts e testes.

Telas: perfis, criar/importar perfil, desbloquear cofre, relays, artigos,
leitura de edição/histórico, rascunhos e fila de publicação. Exibir perfil ativo,
estado do cofre, tarefas em andamento e erros sem interromper o desenho da tela.

Publicação exige perfil assinante desbloqueado. Somente leitura pode consultar,
sincronizar e ler cache offline. Alterar perfil não muda a identidade de tarefas
já criadas: cada tarefa/outbox carrega seu ProfileId e um identificador de geração.
Resultados antigos não podem substituir o estado da tela do novo perfil.

Artigos são fonte Markdown do Pandoc. Primeira versão lê como texto com rolagem;
não promete renderização completa de citações. Proposta: escrever em editor
externo configurado como executável + lista de argumentos, sem shell. Restaurar
o terminal antes de abrir e retomá-lo mesmo em erro; preservar o rascunho no banco.

## Fluxo do relay e do cliente

Validar limites antes de processar dados não confiáveis. Relay e cliente
recalculam IDs e verificam assinaturas; valores desconhecidos não são descartados.
Receiver aceita metadados públicos de publicações, não certifica fontes do artigo.

O relay confirma somente depois do commit de evento, admissão e estado de revisão.
Replicação é best effort e persistente. O cliente reutiliza o mesmo evento assinado
ao repetir um envio; sucesso significa armazenamento no destino informado.

Sincronização persiste eventos, preferência e cursor numa transação. Batch
inválido não avança marcador conforme MIP-03. Expiração de conteúdo não remove
a memória de uma retirada nem a preferência já verificada.

O relay tem import_sync_batch separado de admit: valida primeiro o lote completo
e depois admite seus eventos, heads, trabalhos de propagação e marcador do peer
na mesma transação. A operação interna de admissão não faz commits próprios.
Um evento inválido no fim do lote ou erro no commit não deixa o marcador avançado
nem entradas parciais daquele lote. O cliente segue a mesma regra para cache/heads/cursor.

## Limites da primeira entrega

Publicações kind 1, consulta, sync, perfil atual e propagação manualmente configurada.
Sem DMs ou Double Ratchet implementado, descoberta assinada automática, lookup
entre relays, backup padronizado, delegação de dispositivos ou promessa de
resistência forte a bloqueios. Os perfis locais não criam outro tipo de evento.

Os testes de aceitação usam dois relays e dois diretórios de cliente, incluindo
crash/restart, duplicatas, edições concorrentes, retirada e expiração.
Compartilhar a crate não demonstra interoperabilidade independente; conferir
também o exemplo criptográfico conhecido das MIPs.

## Ordem e critérios de revisão

1. Protocolo compartilhado.
2. Relay e persistência/perfis do cliente podem avançar depois das interfaces do 01.
3. Rede e comandos do cliente, depois da API do relay.
4. TUI sobre serviços testados.
5. Fluxo completo com falhas e medição de gargalos.

Antes de implementar, revisar especialmente as propostas de cofre, perfis somente
leitura e editor externo. Estes documentos são o resultado da solicitação de
planejamento; não autorizam iniciar as etapas nem instalar dependências.

## Referências técnicas consultadas

- [Ratatui](https://ratatui.rs/): widgets e terminal.
- [rusqlite](https://docs.rs/rusqlite/latest/rusqlite/): SQLite no processo.
- [Axum](https://docs.rs/axum/latest/axum/): camada HTTP.
- [serde_jcs](https://docs.rs/serde_jcs/latest/serde_jcs/): canonicalização.
- [Ed25519 dalek](https://docs.rs/ed25519-dalek/latest/ed25519_dalek/struct.VerifyingKey.html): diferenças entre verificadores.
- [Argon2](https://docs.rs/argon2/latest/argon2/): derivação de chave.
- [ChaCha20Poly1305](https://docs.rs/chacha20poly1305/latest/chacha20poly1305/): AEAD e nonce estendido.
