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

O catálogo de apps, a moldura e o espelho JS continuam no escopo das próximas
etapas. Não é uma sessão sem JavaScript.

## Controlador de encerramento da sessão

O serviço `lord.nativeApps` da sessão também é vinculado à lista Rust do seu
contexto durante a inicialização C, depois de `lord.windowList`. Esse vínculo
não depende de um objeto global Rust e não expõe uma nova operação ao JS.
Modelos puros criados com `nativeApps.create()` mantêm o protocolo independente
usado pelos adaptadores e pelos apps isolados.

`sdk/appcontrol.js` retirou `appControlFacts()`: pedidos `groups` e `quit` não
carregam janelas, superfícies ou argumentos. O controlador visita o modelo atual
e resolve o grupo no momento do pedido. Assim, uma superfície alterada ou uma
janela recém-aberta no modelo não depende do espelho JS para ser encerrada.

`sdk/nativecatalog.js` compartilha os metadados de apps com a view. O adaptador
envia um pedido `metadata` quando muda o catálogo, o registro ou o idioma;
o controlador guarda esse catálogo e o empresta aos pedidos seguintes.
Atualizações inválidas preservam os dados anteriores e o limite do transporte
JSON continua valendo. O catálogo ainda precisa migrar para estado nativo.

A execução final dos efeitos Wayland e `wm.close` ainda passa pelo adaptador
JS. `runningApps()` conserva o contrato dos consumidores legados: converte IDs
em objetos do espelho, omitindo entradas cujo objeto ainda não chegou.
Portanto, os menus e outros consumidores ainda precisam migrar; essa adaptação não é
evidência de que o espelho inteiro foi retirado.

## Alternador nativo

`native_view/switcher.rs` monta `shell:switcher` com o leitor do contexto.
O histórico MRU agora pertence a `WindowState`: alterações semânticas atualizam
o foco recente e removem IDs fechados; leituras e ressincronizações são silenciosas.
A view reutiliza o agrupamento nativo do Forçar encerramento e a política
`WindowState.pick_window`, sem pedir resumos JSON ao controlador da sessão.
Revisões invalidam a cena e são consumidas antes de confirmar uma ação; fechar
a janela escolhida substitui sua representante ou remove o grupo.

Seleção, ícones e faixa por viewport são nativos. `appswitcher.jsx` conserva
somente o ciclo de abertura, os atalhos e a aplicação do foco escolhido.
`nativeswitcher.js` enfileira comandos recebidos antes do mount e usa um nonce
para que efeitos de uma abertura antiga não encerrem a seguinte. Metadados e
aparência ainda chegam pelo adaptador compartilhado. O comando separado de
ciclar janelas do mesmo app usa o controlador nativo descrito abaixo; overview
e o antigo binding `lord.switcher` permanecem para as próximas etapas.

O teste `switcher_runtime` verifica os dois estilos, claro/escuro em 640/1000 px,
com MRU, janela representante removida antes do commit, foco em janela ausente
do espelho, comandos antes do mount, troca de desktop, tela cheia, perfil, dispatcher,
faixa com 18 apps, clique e desmontagem. O driver JS somente informa observações;
as verificações e o processo pertencem ao Rust. Capturas ficam em
`build/validation/switcher/`; o CI publica `axia-switcher`.

`cargo xtask switcher-check --installation DIR --output DIR` executa o mesmo
contrato com o fixture pré-compilado, sem Node ou compilação de perfil de testes.
O RPM chama esse comando em `%check`; os testes legados de energia, wallpaper e
MIME conservam sua cobertura separada. Isso não encerra a Fase 2 nem a caixa
compartilhada com overview na Fase 3, e não substitui medições físicas.

Validação local: 113 testes do host nativo, 35 do modelo de janelas e 35 do
xtask; os oito cenários do alternador e quatro de cada uma das regressões de
foco e Forçar encerramento passaram. Compositor e frontend recompilados;
o contrato offline também passou com a instalação preparada para RPM.
Os dois contratos LPK também passaram. Não houve reconstrução completa do RPM
nesta fatia.

## Comandos de janelas do controlador da sessão

`app_control::Controller` retém um proprietário compartilhado de `Session`,
sem guardar o ponteiro C/QuickJS original. Só o controlador vinculado ao contexto
recebe essa autoridade; modelos criados por `nativeApps.create()` não podem
executar `windowCommand`. As views continuam recebendo somente leitores.

Ciclar janelas do app, ocultar app, ocultar outros, mostrar todos e mostrar a
área de trabalho leem os grupos atuais em Rust. O comando de ciclar e os atalhos
de ocultação consultam o foco nativo; ações de menu podem indicar uma janela
explicitamente. O painel Forçar encerramento pode ser ocultado como qualquer
app, embora continue excluído de sua própria lista de encerramento. Painéis
internos são preservados, e ocultar outros respeita o desktop ativo e PiP.

Os IDs selecionados são emprestados a `Session.app_command`, sem serialização
intermediária. Cada comando publica no máximo uma revisão após a mutação inteira,
inclusive ao ciclar para trás. `WindowState` guarda os IDs ocultados por desktop;
restaurar ignora janelas fechadas ou transferidas para outro desktop e não
restaura as que já estavam minimizadas antes. Fechar um desktop recolhe sua
memória. O histórico MRU continua seguindo o resultado final do comando.

O adaptador JS envia ação, direção e identidade opcional. O controlador reutiliza
os metadados já armazenados; nenhum pedido leva listas de janelas ou grupos.
A resposta ainda contém o delta habitual de `WindowState`, aplicado pelo SDK
para manter a interface React restante. Portanto, esta entrega retira as regras
e a memória desses comandos do JS, mas não elimina o espelho nem seu retorno JSON.
`appGroup`, apresentação dos menus e encerramento continuam pendentes.

`window_commands_runtime` executa os comandos e o atalho de ocultação no
compositor real, com espelho desatualizado, janelas recém-criadas, remoção,
transferência de desktop, grupos Linux, painéis e cache de metadados.
O CI GPU=1 exige quatro cenários em claro/escuro e 640/1000 px, publicando
`axia-window-commands`; evidências locais em `build/validation/window-commands/`.

Validação local: 517 testes aprovados e sete ignorados na biblioteca da shell,
incluindo quatro novos testes de comandos; os 13 do app Forçar encerramento
também passaram. Passaram os quatro cenários gráficos
novos, oito do alternador, quatro de Forçar encerramento e dois contratos LPK.
Compositor e frontend recompilados; o contrato offline do alternador passou na
instalação preparada para RPM. RPM completo e medições físicas ficam pendentes.

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

Na migração do controlador da sessão, passaram 13 testes do app e três da
ponte Rust, incluindo referências dos modelos antigos e isolamento de contexto.
Os quatro cenários de `forcequit_runtime` agora exigem grupos e superfícies
atuais com espelho JS parado, fechamento de janela ausente desse espelho,
ausência de `windows` nos pedidos, cache de metadados reutilizado após desmontar
a view e atualização após trocar idioma. Compositor/frontend recompilados;
integração dos apps compilados, oito cenários de Arquivos, seis do launcher,
quatro de foco e dois contratos LPK também passaram. Capturas e logs continuam
em `build/validation/forcequit/`, com o mesmo teste obrigatório no CI.
