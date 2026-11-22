# Plano 01 — Biblioteca compartilhada do protocolo

> Execução futura: usar superpowers:executing-plans ou superpowers:subagent-driven-development, tarefa por tarefa. Este arquivo não inicia implementação.

**Objetivo:** implementar as regras determinísticas compartilhadas por relay e cliente.
**Arquitetura:** crate murm-protocol independente de rede, SQLite, cofre e terminal.
**Stack proposta:** Serde, serde_json, serde_jcs, SHA-256, ed25519-dalek,
curve25519-dalek para validação de pontos e tipos zeroizantes quando necessário.
**Spec:** [desenho](../design.md) e MIPs 01–05 no
[commit de referência](https://github.com/mi66mc/mips/tree/c3d601848cd05c41b3726a9dc2631239656da8e7).

## Restrições globais

Rust edition 2024; licença AGPL-3.0-only; três crates existentes.
Evento com exatamente oito campos; hash de objeto JCS de seis campos; Ed25519
puro sobre 32 bytes do hash. Inteiros seguros até 9007199254740991.
Não adicionar campos de conta, perfil local, senha, dispositivo ou backup.
Os valores locais do relay não alteram validade criptográfica.

Dependências propostas serão resolvidas em versões estáveis compatíveis com
o toolchain e fixadas no Cargo.lock ao executar este plano, sem instalar agora.

## Foco da revisão

1. Propriedades JSON repetidas, inclusive em objetos desconhecidos.
2. Ordenação UTF-16/JCS, números, zero negativo e Unicode sem normalização.
3. Assinar texto hexadecimal em vez do digest.
4. Divergência entre política de pontos Ed25519 das MIPs e defaults da biblioteca.
5. Recriar artigo retirado ao filtrar revisões antes de selecionar a preferida.

## Arquivos e contratos

Criar em crates/murm-protocol/src:
event.rs, json.rs, crypto.rs, publication.rs, filter.rs, api.rs, error.rs.
Modificar lib.rs e Cargo.toml; dependências comuns em Cargo.toml da raiz.

Tipos públicos e funções têm os nomes do desenho. EventId e SignerKey são
wrappers de 32 bytes com codificação hex minúscula no JSON. SignatureBytes
contém 64 bytes. Event guarda os oito campos; header/content preservam JsonValue.
UnsignedEvent tem somente os seis campos assináveis. VerifiedEvent encapsula
Event após validação básica e criptográfica; não permite construção pública
com campos arbitrários. PublicationMeta contém chave lógica, revisão, ID e status.

ParseLimits recebe max_bytes e max_depth do chamador. ProtocolError diferencia
JSON inválido, shape, versão, ID, assinatura, kind, filtro e limites.
Não carregar limites de admissão de uma configuração global nessa crate.

### Task 1: Parsing e representação

**Arquivos:** src/event.rs, src/json.rs, src/error.rs, tests/event_input.rs.

**Produz:** parse_event(bytes: &[u8], limits: ParseLimits) -> Result<Event, ProtocolError>,
e os tipos acima. Ainda não retorna VerifiedEvent.

- [ ] Escrever testes duplicate_nested_member_rejected, unknown_outer_field_rejected,
  missing_field_rejected, unknown_nested_field_preserved, invalid_unicode_rejected,
  unsafe_integer_rejected, byte_and_depth_limit_rejected.
  Rejeitar uma repetição em header e em content antes de serde_json::Value
  descartar a ocorrência. Evento com campo externo extra é erro, não perda do campo.
- [ ] Rodar cargo test -p murm-protocol --test event_input; esperar falha porque
  o parser não existe.
- [ ] Implementar parser com visitor que detecta repetição em todos os objetos
  e valida números/Unicode antes de construir os tipos. Não usar só Value
  desserializado por um parser que sobrescreve chaves repetidas.
- [ ] Rodar o mesmo teste, agora com sucesso; revisar preservação de unknown fields.
- [ ] Commit: feat(protocol): add strict event parsing.

### Task 2: ID, assinatura e verificação

**Arquivos:** src/crypto.rs, src/lib.rs, tests/event_crypto.rs.
**Consome:** Event, UnsignedEvent, ParseLimits, ProtocolError.
**Produz:** canonical_unsigned(&UnsignedEvent) -> Result<Vec<u8>, ProtocolError>,
event_id(&UnsignedEvent) -> Result<EventId, ProtocolError>, sign_event e
parse_and_verify do desenho; derive_public_key(seed: &[u8; 32]) -> SignerKey.

- [ ] Testar mip02_known_event com seed/payload/assinatura publicados na MIP-02.
  Asserção central:
```rust
assert_eq!(id.to_string(),
    "945708370d265a3825922c890b2ac7f09fb0d5a68fa5e12e9b768ec0e3790791");
```
  Conferir também a assinatura completa e os bytes canônicos publicados.
  Adicionar raw_digest_not_hex, each_signed_field_changes_id,
  formatting_and_object_order_preserve_id, array_order_changes_id,
  unicode_not_normalized e negative_zero_uses_jcs_zero.
- [ ] Rodar cargo test -p murm-protocol --test event_crypto; observar falhas.
- [ ] Implementar com serde_jcs e primitivas estabelecidas; o payload é objeto.
  Conferir chaves e R com decoding canônico e torsion-free; pública não identidade;
  S canônico e menor que L. Depois usar a equação de verificação da biblioteca.
  Não substituir essas regras por verify_strict sem conferir diferenças:
  a identidade de R pertence ao subgrupo mas pode ser rejeitada pelo default
  estrito da biblioteca. O comportamento publicado da MIP é a referência.
  Não habilitar verificação em lote com semântica diferente da individual.
- [ ] Adicionar noncanonical_point_rejected, torsion_point_rejected,
  identity_public_key_rejected, scalar_out_of_range_rejected e teste de R
  identidade válido usando construção algébrica de teste independente, sem
  escrever primitivas criptográficas de produção. Documentar qualquer conflito
  de biblioteca como incompatibilidade a corrigir, não mudar a MIP silenciosamente.
- [ ] Rodar cargo test -p murm-protocol --test event_crypto; todos passam.
- [ ] Commit: feat(protocol): implement canonical IDs and event verification.

### Task 3: Publicações e preferência

**Arquivos:** src/publication.rs, tests/publications.rs.
**Consome:** VerifiedEvent e comparação binária de EventId.
**Produz:** validate_publication e compare_revisions do desenho.

- [ ] Testar identifier_bounds, metadata_bounds, withdrawn_requires_null,
  published_requires_string, missing_predecessor_allowed,
  greater_revision_then_greater_id, another_signer_is_another_publication,
  unknown_metadata_preserved e maximum_revision_does_not_wrap.
- [ ] Rodar cargo test -p murm-protocol --test publications; esperar falhas.
- [ ] Implementar exatamente os limites da MIP-04: ID de publicação ASCII de
  1–128 caracteres; título 200, resumo 1000, tópicos até 20 strings únicas de
  1–64 escalares Unicode, idioma 1–64. Comparação exata. Não renderizar Pandoc.
  compare_revisions retorna erro entre chaves lógicas diferentes.
- [ ] Rodar o teste; conferir kind 1 e revisão positiva segura.
- [ ] Commit: feat(protocol): validate publication revisions.

### Task 4: Filtros e contratos HTTP

**Arquivos:** src/filter.rs, src/api.rs, tests/query_contract.rs.
**Produz:** Filter, tipos API do desenho, validate_query e validate_sync
com RelayInfo/limites de capacidade, matches_filter e ProblemCode/Problem.

- [ ] Testar and_inside_or_between, deduplicate_by_id, typed_equality,
  missing_vs_null, contains_is_array_membership, since_inclusive_until_exclusive,
  unknown_request_member_rejected, header_requires_explicit_kinds,
  unsupported_path_rejected, current_requires_supported_kinds.
  Desserializar o /info completo da MIP-03; conferir relação entre accepted,
  validated e current kinds, políticas padrão/overrides e os códigos de erro.
- [ ] Rodar cargo test -p murm-protocol --test query_contract; esperar falhas.
- [ ] Implementar sem gerar SQL. O relay valida capacidade antes de filtrar;
  matches_filter nunca consulta rede. Cursor é string opaca no contrato.
  Manter separado o formato QueryResponse(next_cursor) do SyncResponse(cursor,
  has_more). Respostas aceitam metadados extras; pedidos têm shape fechado.
- [ ] Rodar o teste; conferir que assinatura válida não implica verdade editorial.
- [ ] Commit: feat(protocol): add query and relay API contracts.

## Saída deste plano

Rodar cargo fmt --all -- --check e cargo test -p murm-protocol.
Nenhuma dependência de Axum, Rusqlite ou Ratatui entra nesta crate.
Os testes usam exemplos inline nas suítes, sem novo diretório de schemas,
test-vectors ou geradores. Só seguir ao armazenamento usando estes contratos.
