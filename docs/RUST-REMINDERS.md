# Reminders em Rust

`apps/reminders/` contém apenas Rust: manifesto, estado, regras, interface,
seletores de data/hora e testes. O aplicativo mantém o processo isolado,
os tamanhos de janela e as permissões do manifesto anterior.

A migração preserva as listas Hoje, Agendados, Todos, Sinalizados e Concluídos,
seus contadores, agrupamento por lista/data, subtarefas, prioridades e ordenação.
Hoje inclui tarefas vencidas; datas de dia inteiro usam o calendário local.
Os cálculos usam `localtime_r`/`mktime`, inclusive ao atravessar mudanças de
horário de verão. A interface conserva as dimensões, cores, ícone, sidebar,
toolbar, linhas, edição de título, inspector, calendário, menus e folha de listas.

O inspector conserva notas multilinha, vencimento ausente/data/data e hora,
prioridade, sinalização, transferência entre listas e exclusão. Um vencimento
com hora mantém os alarmes existentes ou cria `[0]`; remover o vencimento ou
usar dia inteiro remove os alarmes, como antes. `Ctrl/Meta+N`, Enter e Escape
mantêm os caminhos de edição. Seleção e exibição de concluídos sobrevivem à
remontagem da mesma janela e são descartadas quando ela fecha.

`axia-maild` continua como armazenamento local/CalDAV e serviço de alarmes.
O transporte compartilhado encaminha suas mensagens e eventos; Rust decide
as consultas e todas as mutações. Respostas de consultas substituídas são
descartadas. Falhas preservam os dados carregados, e alternar a conclusão é
otimista com restauração em caso de erro. Listas remotas não são excluídas pelo
menu. A primeira tarefa cria a lista padrão quando necessário.

O build gera a pequena entrada do pacote isolado a partir do manifesto Rust,
sem manter uma segunda implementação do aplicativo em JSX. O host Rust monta
os nós, encaminha input, dimensiona os popovers após layout e preserva os
widgets de edição ao reconciliar a árvore.

Validação:

```sh
cargo test --manifest-path rust/Cargo.toml --target-dir server/build/rust \
  --locked --offline --lib reminders_app -- --test-threads=1
TZ=America/New_York cargo test --manifest-path rust/Cargo.toml \
  --target-dir server/build/rust --locked --offline --lib reminders_app \
  -- --test-threads=1
```

Os testes em Rust exercitam regras, datas, gerações, falhas e a cena nativa,
incluindo edição, notas, menus, calendário, movimentação, criação de lista
padrão, persistência da janela e as mensagens exatas enviadas ao serviço.
