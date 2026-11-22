# Planos de implementação do Murm

Estado: propostas para revisão; nenhuma tarefa foi executada.
Comece pelo [desenho comum](../design.md), que separa decisões do usuário
das propostas de implementação.

1. [Protocolo compartilhado](01-protocol.md): eventos, assinaturas, tipos e regras.
2. [Relay](02-relay.md): SQLite, HTTP, políticas, cursores e replicação.
3. [Cliente e perfis](03-client.md): SQLite local, geração/importação, cofre,
   rascunhos, transporte e comandos.
4. [TUI e integração](04-tui.md): telas, tarefas assíncronas e testes completos.

A ordem é 01, depois 02/03 conforme suas interfaces, e por último 04.
Os testes de carga vêm depois de validar comportamento e falhas.

Não iniciar implementação até o usuário revisar o desenho e estes planos.
As dependências listadas são propostas; nenhuma foi instalada nesta etapa.
