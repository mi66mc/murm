# Murm

Base do workspace Rust para a implementação experimental do Murm.
Ainda não há funcionalidades do protocolo implementadas.

## Planejamento

O [desenho do protótipo](docs/design.md) e os [planos por componente](docs/plans/README.md)
descrevem protocolo compartilhado, relay, perfis/cofre/SQLite do cliente e TUI.
São propostas para revisão; nenhuma das etapas foi implementada.

## Projetos

- `crates/murm-protocol`: biblioteca compartilhada pelo relay e pelo cliente.
- `crates/murm-relay`: binário do relay, ainda vazio.
- `crates/murm-cli`: binário do cliente de terminal, ainda vazio.

As especificações ficam no repositório separado [mips](https://github.com/mi66mc/mips).

## Compilação

Na raiz do workspace:

```sh
cargo check --workspace
```

Para compilar apenas um dos binários e suas dependências:

```sh
cargo build -p murm-cli
cargo build -p murm-relay
```

As crates não têm dependências externas e não estão configuradas para publicação.

## Licença

Este projeto, incluindo a biblioteca, o relay e o CLI, usa a GNU Affero General
Public License versão 3 exclusivamente (`AGPL-3.0-only`). O texto completo está
em [LICENSE](LICENSE).
