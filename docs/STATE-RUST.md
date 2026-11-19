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

## Foco do conteúdo dos apps nativos

`sdk/nativeview.js` também vincula as views de apps que possuem uma janela ao
leitor do contexto. Envia `windowId` e a permissão de entrada `inputAllowed`,
sem reenviar o foco da janela no JSON. O host Rust determina a janela ativa
diretamente do modelo. O adaptador continua suspendendo entrada quando um
campo de texto JS ou uma sobreposição da shell possui o teclado.

A consulta acontece antes de despachar uma tecla, inclusive quando o modelo
muda na mesma tarefa JS e o compositor ainda não executou outro quadro.
Uma permissão de entrada não pode ativar uma janela em segundo plano.
Revisões consumidas não deixam timer; mudanças só de título não reconstroem
o conteúdo. Ícones e views sem janela, como os painéis da shell, conservam
seus protocolos próprios. A moldura e o espelho JS continuam pendentes.

## Lista do Forçar encerramento

O app `forcequit` recebe o mesmo leitor, através de `axia_native::windows`.
Esse contrato permite visitar referências emprestadas de identidade, título
e argumentos, sem construir uma lista JSON para cruzar a fronteira entre
modelo e app. O proprietário do contexto continua explícito no mount.

A view monta os grupos a partir dessas referências. O catálogo compartilhado
de apps ainda fornece nomes localizados, indicadores de pacote/painel e
descritores de ícone. O catálogo só depende de apps e idioma; o adaptador
`NativeView` deixou de assinar o espelho de janelas e retirou `windowFacts`.
O manifesto do painel agora pede metadados de apps, não fatos de janelas.
A correspondência de identidade desktop/WM_CLASS é compartilhada com o launcher.

A revisão do leitor invalida a view, inclusive quando o modelo muda sem
atualizar o espelho JS. Antes de despachar uma ação, o host sincroniza qualquer
invalidação pendente: uma seleção removida não pode iniciar encerramento entre
a mutação do modelo e o próximo quadro. A assinatura não adiciona polling.

Esta etapa migra a leitura da lista no painel. `runningApps()`, menus e a
execução dos efeitos de encerramento ainda usam `sdk/appcontrol.js` e seu
adaptador transitório; o catálogo de apps, a moldura e o espelho JS também
continuam no escopo das próximas etapas. Não é uma sessão sem JavaScript.

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

O teste Rust `native_focus_runtime` cobre duas calculadoras pelo adaptador
real: fonte opaca correta, ausência de foco nos fatos, foco direto com espelho
JS desatualizado, prioridade de TextInput e sobreposição, e retomada do teclado.
São quatro cenários, claro/escuro em 640/1000 px. O CI GPU=1 exige o teste e
publica `axia-native-focus`; a coleta local fica em
`build/validation/native-focus/`. O host tem 109 testes aprovados, incluindo
isolamento, remoção, vida útil do leitor e ausência de reconstrução por título.
Compositor e frontend recompilados, integração dos apps compilados, dois
contratos LPK e oito cenários de cada uma das regressões de Arquivos e dos
cabeçalhos também passaram. Não houve medição física nesta etapa.

Para a lista do Forçar encerramento, passaram 12 testes do app, 110 do host,
34 do modelo de janelas, 51 do launcher e o contrato compartilhado de
identidade desktop. A integração `forcequit_runtime` passou em quatro cenários
claro/escuro e 640/1000 px, com fonte opaca verificada e sem campo `windows`
nos fatos. Abre, fecha e altera argumentos diretamente no modelo, mantém
o espelho JS parado e impede encerramento de uma seleção já removida.
Capturas e logs: `build/validation/forcequit/`; o CI GPU=1 exige o teste e
publica `axia-forcequit`.

Compositor e frontend foram recompilados. Também passaram a integração dos
apps compilados, seis cenários gráficos do launcher, quatro de foco e dois
contratos LPK. Medições físicas e RPM completo não foram executados nesta fatia.
