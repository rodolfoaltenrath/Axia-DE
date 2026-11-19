# Estado observável em Rust

A Fase 2 começa com `rust/axia-state`, sem dependências externas, React ou
serialização. O modelo de janelas é o primeiro proprietário de estado a usá-lo.

`Store<T>` guarda o estado autoritativo na thread da interface. Uma operação
do modelo usa `update`, informa se houve mudança observável e publica uma
revisão depois de liberar o empréstimo mutável. `set` ignora valores iguais.
Não existe cópia automática do estado a cada notificação.

Os consumidores recebem `Reader<T>`, que permite ler e assinar mudanças,
sem escrever por fora das regras do modelo. A leitura empresta os dados Rust;
não cria um espelho nem codifica uma mensagem. Uma `Subscription` recebe a
última revisão pendente: várias alterações antes do próximo consumo ficam
agrupadas. Assinantes são independentes. Descartar uma assinatura a cancela;
ela não mantém o modelo ou componentes vivos. Registros cancelados são
recolhidos em novas assinaturas e commits, inclusive após ciclos em ociosidade.

Não há callbacks executados durante uma escrita, fila de snapshots ou timer
no crate. A integração da invalidação com o despacho das views e o despertar
do loop nativo ainda precisa ser feita. `update` é um limite de commit, sem
diário de rollback; o modelo deve validar uma operação antes de alterar dados
e informar corretamente se ela mudou estado. A API usa `Rc`/`RefCell` e não
aceita compartilhamento entre threads. Workers deverão entregar seus
resultados à thread da interface.

## Primeiro modelo: janelas

`axia_shell::winmodel::Session` guarda `WindowState` nesse store. `reader()`
expõe leitores nativos; janelas oferecem identidade, app, título, geometria,
desktop, minimização, revisão e argumentos por empréstimo. Os argumentos
dinâmicos dos apps ainda são `serde_json::Value`, assim como os campos internos
do modelo anterior. A leitura nativa dessas propriedades não serializa JSON.

Uma operação que altera título, geometria, argumentos, desktops, overview,
ordem ou remoção gera uma revisão do modelo. A revisão individual de cada
janela continua existindo. Operações sem efeito, leituras, erros de operação
desconhecida e `resync` do adaptador JS não geram invalidação nativa.

O retorno JSON da ABI e o espelho de `sdk/windows.js` continuam nesta etapa.
As views da sessão ainda não consomem esses leitores: a próxima fatia deve
ligar os consumidores ao estado nativo e retirar esse transporte. Portanto,
esta base não conclui a lista de janelas nem a Fase 2, e ainda não demonstra
redução de memória ou despertares.

## Verificação

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline -p axia-state
cargo test --manifest-path rust/Cargo.toml --locked --offline -p axia-shell --lib winmodel::
```

Os testes do crate verificam leitura compartilhada, silêncio em valores iguais,
um commit por operação, assinantes independentes, agrupamento de rajadas,
cancelamento e vida útil. Os testes do modelo leem diretamente o estado após
alterações reais e verificam invalidação por ordem/remoção e silêncio em
leituras/ressincronizações. As regressões anteriores do espelho permanecem.
