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
no crate. O host de views consulta `NativeApp::invalidated`: uma revisão
pendente estabelece prazo imediato no loop e é consumida no próximo `sync`.
Depois do consumo não resta timer dessa assinatura. `update` é um limite de commit, sem
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
A barra de título é o primeiro consumidor desses leitores; os demais
consumidores de janelas ainda precisam migrar e retirar esse transporte. Portanto,
esta base não conclui a lista de janelas nem a Fase 2, e ainda não demonstra
redução de memória ou despertares.

## Primeiro consumidor: título e foco

O mount da barra recebe o objeto opaco da lista de janelas do seu contexto.
O adaptador C valida sua classe, empresta o proprietário somente durante a
chamada e entrega ao Rust um leitor tipado. A view não retém ponteiro C nem
objeto QuickJS; não existe uma lista global implícita compartilhada entre
contextos. O leitor mantém o armazenamento seguro até o descarte da view.

`TitleBar` lê título e janela em foco diretamente do modelo. Os fatos enviados
por `sdk/nativetitlebar.jsx` carregam identidade, nome localizado do app para
títulos vazios e aparência; título e foco dinâmicos não são reenviados nesse
JSON. A escolha da janela em foco usa a mesma regra do modelo que alimenta o
adaptador transitório. Os efeitos de controles, a moldura e o espelho JS ainda
fazem parte das etapas pendentes.

O protocolo antigo de fatos explícitos permanece para controladores puros sem
proprietário de janelas. A sessão vincula o leitor e prioriza os valores do
modelo mesmo que fatos explícitos estejam desatualizados.

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

Os testes do cabeçalho verificam título/foco com fatos discordantes, atualização
sem novos fatos, isolamento de proprietários, vida útil e consumo de prazo
imediato. A integração `titlebar_runtime` altera o modelo sem atualizar o
espelho JS e exige que o novo título apareça na cena; também rejeita fontes
opacas inválidas e verifica ausência de título/foco dinâmicos nos fatos.

Validação local desta fatia: compositor e frontend recompilados, quatro testes
da base observável no Windows e Linux, 34 testes do modelo, 108 do host nativo,
oito cenários gráficos dos cabeçalhos, dois contratos LPK e integração dos
apps compilados passaram. Capturas e logs estão em
`build/validation/native-titlebar/{axia,adwaita}/`; o CI já exige a integração
e publica `axia-titlebar`. Não houve novo RPM completo nem medição física.
