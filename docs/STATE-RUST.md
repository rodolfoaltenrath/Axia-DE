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

## Configurações residentes por perfil

`rust/axia-shell/src/settings.rs` guarda o estado autoritativo de configurações
em `axia-state`, na thread da interface e por perfil do processo. O caminho do
perfil vem do adaptador C, não do pedido JSON. Padrões usam uma fonte de dados
comum em `sdk/settings-defaults.json`, incluída no Rust e no SDK isolado; o
Makefile acompanha esse arquivo como dependência do arquivo estático Rust.

O bootstrap lê o arquivo pela ABI nativa, aplica os mesmos
schemas 1–3 e preserva campos desconhecidos e versões futuras. Migração pública
continua pura. Arquivos ilegíveis recebem os padrões, com aviso; não são gravados
durante a inicialização. O modelo inicializa uma vez por perfil e preserva
alterações pendentes quando o espelho ou a view são recriados.

Patches alteram apenas os campos enviados; substituições por função usam o
snapshot atual do proprietário. Leituras e alterações sem efeito não invalidam
consumidores. Leitores Rust acessam os valores por empréstimo. A API da sessão
em `sdk/settings.js` é um espelho com revisões: descartam-se respostas atrasadas,
e campos inalterados conservam suas referências para as assinaturas existentes.
A mesma preservação de referências vale para `migrateSettings`.

O contexto validado possui o publicador `session_preferences.rs`, atendido pelo
loop Rust sem montar `shell:settings`. Ele consome invalidações e publica
snapshots somente quando a revisão muda, sem polling ou estado de apresentação. As views da sessão que consomem
preferências agora as leem por empréstimo do proprietário Rust; esses dados
não entram nos fatos JSON de NativeBridge.
O host `.lpk` preserva o store e as migrações locais.

O modelo também possui o prazo monotônico de 300 ms, serialização e confirmação
da gravação. A fila participa do tick/timeout nativo mesmo sem nenhuma view
montada, inclusive no greeter e com a sessão em segundo plano. Não há leitura/gravação
de arquivo ou timer de gravação JS na sessão principal.
`saveSettingsNow(true)` chama a operação nativa durável e cancela o prazo anterior.
Leitura, limite de 1 MiB, arquivo temporário/rename e fsync reutilizam as primitivas
C existentes. O host `.lpk` mantém seu caminho local.

Falha de gravação conserva as alterações e desarma o prazo, sem repetir I/O a
cada quadro. Uma nova alteração, inicialização/remontagem do adaptador ou pedido
explícito pode tentar novamente. Salvar não altera a revisão das preferências.
A API de arquivo legada continua disponível; uma gravação bem-sucedida de objeto
válido atualiza o proprietário e cancela sua gravação pendente. O gravador C
compartilhado não chama de volta ao modelo durante um save nativo.
Idioma/região também têm proprietário nativo, descrito abaixo. Bootstrap de
outros serviços e demais consumidores de fatos de preferências continuam pendentes.

`native_settings_runtime` exige quatro cenários claro/escuro e 640/1000 px:
observador sem preferências nos fatos, espelho parado, updater com dados nativos,
descarte de respostas antigas, referências preservadas, nomes de propriedades
próprios, alterações sem efeito, gravação durável e remontagem sem nova gravação.
Bloqueia as funções JS de leitura/gravação antes de importar o SDK, observa a
ausência de timers JS de 300 ms e exige gravação automática antes de montar views.
Também verifica atualização do proprietário após gravação pela API legada.
O CI GPU=1 exige essa integração e publica `axia-native-settings`.
Evidências locais da persistência em `build/validation/native-settings-io/`
(a entrega inicial está em `build/validation/native-settings/`). A regressão de Arquivos
também exige que a migração pública preserve a referência do campo `files`,
além dos valores migrados. Configurações, Arquivos, notificações e relógio
foram verificados em vinte cenários gráficos, além dos apps compilados e do
encerramento instalado. Não houve nova medição física.

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
o controlador guarda esse catálogo num `axia-state::Store` e o empresta aos pedidos seguintes.
Atualizações inválidas preservam os dados anteriores e o limite do transporte
JSON continua valendo. O controlador expõe um leitor nativo para notificações;
a descoberta e importação do catálogo ainda precisam sair do JS.

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
Consulta dos menus e escolha dos alvos de encerramento migraram na etapa abaixo;
apresentação dos menus, callbacks e APIs de compatibilidade ainda usam JS.

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

## Grupos dos menus e encerramento normal

O menu de aplicativo consulta `groupInfo` no controlador do contexto. Rust
resolve identidade e nome atual; a resposta não contém uma lista de janelas.
A antiga resolução de grupo/nome em `appcontrol.js` foi retirada. A API pública
`appGroup(id)` na sessão conserva objetos do espelho, obtidos por uma consulta
separada `group`; `runningApps` também mantém seu contrato de compatibilidade.
Esses wrappers não são usados para escolher os alvos dos menus ativos.

Sair no menu de app, no menu da Dock e no atalho passa identidade ou pede o foco
atual ao controlador Rust. `GracefulQuit` escolhe o grupo no momento da ação,
incluindo janelas abertas depois da construção do menu. A fila guarda IDs;
antes de cada efeito, consulta novamente pertencimento, existência e superfície.
Janelas fechadas ou que mudaram de grupo são ignoradas. Um ticket distingue as
tentativas: uma resposta atrasada não avança a fila de uma tentativa posterior.

Apps isolados recebem primeiro `quitPackage`; confirmação do broker conclui
a tentativa sem fechar superfícies à força. Sem confirmação, segue o fechamento
normal. Janelas Linux recebem `closeSurface` com o identificador atual; janelas
da sessão recebem `requestCloseWindow`, que chama `wm.close` com suas guardas.
Recusar fechamento mantém aquela janela e permite processar as demais. Nenhum
desses caminhos usa `kill` nem o fechamento forçado do Forçar encerramento.

Execução de IPC, guardas, callbacks e aplicação do fechamento permanecem no
adaptador JS. A função pública `quitApp(windows)` conserva sua semântica legada
para listas explícitas; os menus da sessão e o atalho não a usam mais. A barra
e a Dock ainda leem o espelho para outros estados e apresentações; a Fase 2
continua aberta.

No host `.lpk`, `runtime/lpk/appcontrol.js` conserva consulta de grupo, menus,
ocultação, ciclo e memória da área de trabalho sobre as janelas locais do app.
A seleção ocorre no build por `__AXIA_APP_HOST__`; esse módulo é excluído do
bundle da sessão. O contrato Rust/Node verifica a entrega nos dois bundles,
identidade dos objetos de janela, ações locais e ausência de chamadas ao
controlador privilegiado. Isso também preserva os comandos locais migrados
na etapa anterior, sem dar ao processo isolado acesso ao estado da sessão.

`app_menus_runtime` cobre consulta com espelho parado, clique no menu nativo,
novas janelas após abrir o menu, guarda em outro desktop, menu da Dock de app
Linux sem catálogo, atualização de superfícies, confirmação/fallback do broker
e atalho com foco JS desatualizado. As verificações são Rust, em claro/escuro e
640/1000 px. O CI exige o teste e publica `axia-app-menus`; evidências locais
em `build/validation/app-menus/`.

Validação local: 520 testes da shell aprovados, sete ignorados; três contratos
LPK aprovados. Passaram os quatro cenários novos de menus, quatro da Dock
(cada um com três posições), oito do alternador, quatro de Forçar encerramento
e quatro de comandos de janelas. Compositor e frontend recompilados; a validação
offline do alternador passou na instalação preparada para RPM. Não houve RPM
completo nem medição física nesta etapa.

## Menu Janela da barra

`native_view/window_menu.rs` pertence à barra vinculada ao leitor do contexto.
Monta a lista, títulos, sufixos de desktop, estados habilitados e submenus no
Rust. A montagem de `MenuBar.jsx` passa o proprietário opaco e metadados de
telas/apps; o getter JS `window` foi retirado. As linhas não fazem uma viagem
por `barOpen`/`barBridge` nem retornam como uma árvore JSON ao host.

O menu mantém a janela de origem da abertura, inclusive se o foco mudar depois.
Revisões atualizam título, ordem e pertencimento enquanto estiver aberto.
Antes de enviar uma ação, o host consome mudanças pendentes e o menu verifica
se a janela ainda existe. Fechar o menu desativa essa invalidação; a assinatura
não estabelece timer. Idioma e revisões de catálogo também atualizam as legendas.

Arranjar janelas e trazer todas à frente escolhem os alvos no controlador Rust,
ignorando painéis e janelas minimizadas ou de outro desktop. Os IDs não são
enviados pelo JS. Arranjos recebem somente geometria de telas e tamanhos de
apps; esses fatos agora vêm do registro, sem percorrer o espelho de janelas.
Cada comando publica no máximo uma revisão. Demais comandos ainda executam
`wm` e guardas pelo adaptador, e o retorno do modelo mantém o espelho existente.

As APIs públicas `windowMenuItems`/`moveResizeItems` continuam disponíveis ao SDK
legado e aos `.lpk`; a barra da sessão não usa esses produtores. Menus de apps,
logo e ajuda mantêm os callbacks transitórios. A barra guarda a última medição
dos botões para posicionar menus recebidos por ações reais do host, que não
carregam fatos; esse caso também está coberto por teste.

`window_menu_runtime` verifica o proprietário correto, títulos/pertencimento
com espelho parado, desktop de destino, ação sobre janela ausente do espelho,
arranjo após fechar outro alvo, guarda de ação obsoleta e coexistência de um
menu legado. São quatro cenários claro/escuro e 640/1000 px, exigidos pelo CI,
com capturas em `build/validation/window-menu/` e artefato `axia-window-menu`.

Validação desta etapa: 525 testes unitários da shell aprovados (sete testes
especiais ignorados), três contratos LPK e quatro cenários gráficos do menu
Janela. Passaram também as regressões de menus de app, menus de contexto,
Forçar encerramento e comandos de janela (quatro cenários cada), além dos
oito cenários do alternador. Compositor e frontend recompilados; instalação
em diretório temporário e `switcher-check` usando os arquivos instalados
aprovados. Arranjos recusam uma origem removida, em outro desktop ou que seja
painel interno. Medições físicas e RPM completo continuam para o encerramento.

## Indicador de desktop

`shell:desktop-hud` exige a fonte nativa do contexto e observa as revisões de
`WindowState`. O próprio app compara o desktop ativo, monta o nome e os pontos
e mantém um prazo monotônico de 800 ms após a última troca. A primeira leitura
estabelece a referência sem mostrar o indicador. Overview e desktop único
cancelam o prazo; sair do Overview não repete a indicação. Oculto, o app não
pede atualização periódica. O assinante e o prazo acabam ao desmontar a view.

`sdk/desktops.js` fornece apenas o mount, aparência e geometria do monitor
principal, com assinatura de eventos de tela. A tentativa de avançar além do
limite encaminha um comando `show`; não envia desktop ativo, lista ou árvores.
Removidos `hudState`, `hudTimer` e a assinatura JS que mostrava o indicador.
Animação de janelas continua no adaptador; persistência e restauração têm
seus serviços nativos descritos nas seções seguintes.

Os comandos `desktopCommand` também pertencem ao controlador vinculado ao
contexto: `step`, `add` e `closeActive` consultam a lista e o desktop atuais no
próprio modelo. Criar e mostrar é uma única mutação observável; fechar move as
janelas e escolhe o destino nas regras existentes, também em uma revisão.
Passos fora do intervalo não alteram o modelo e sinalizam o limite ao indicador.
Pedidos inválidos e controladores sem proprietário são recusados. O SDK aplica
o delta de compatibilidade, mas não escolhe os alvos pelo espelho. O host `.lpk`
mantém o caminho local anterior, selecionado na compilação.

O teste Rust `desktop_hud_runtime` monta o componente real e exige fonte opaca
correta, troca com espelho parado, extensão/expiração do prazo, indicação no
limite, supressão no Overview/desktop único e desmontagem. Quatro cenários,
claro/escuro em 640/1000 px, produzem capturas em
`build/validation/desktop-hud/`; o CI GPU=1 exige o teste e publica
`axia-desktop-hud`. O teste unitário usa instantes explícitos para verificar
extensão e expiração sem espera. Não houve medição física nesta etapa.

Compositor/frontend recompilados; passaram 526 testes unitários da shell (sete
especiais ignorados), três contratos LPK e as regressões gráficas dos comandos
de janela e menu Janela, em quatro cenários cada.

Na etapa seguinte dos comandos de desktop, passaram 527 testes unitários da
shell (sete especiais ignorados), quatro contratos LPK e quatro cenários
gráficos de cada uma das integrações de indicador e comandos de janela.
O cenário do indicador também executa avanço, retorno, criação e fechamento
com estado JS desatualizado, exigindo a migração das janelas para o destino
correto. O novo contrato LPK executa esses mesmos comandos locais e exige
zero acesso à autoridade da sessão. Compositor e frontend recompilados.

## Inicialização e persistência dos desktops

`desktop_session.rs` pertence ao `Session` do contexto, com leitor nativo e
assinatura cancelável. `desktopSession` inicializa a lista validada e o desktop
ativo numa mutação tipada, uma única vez; chamadas repetidas não apagam janelas
ou desktops já criados. O serviço também resolve posição/ID e prepara desktops
ausentes em uma revisão, sem receber a lista ou a seleção do espelho JS.
Consultas a posições existentes não produzem um delta de janelas.

Na troca de tema, a associação superfície Linux → posição é coletada por
empréstimo do proprietário nativo. O novo contexto valida a contagem, o índice
ativo e cada associação antes de aplicá-los. O formato persistido continua
`desktopCount`/`themeDesktops`. A persistência desses campos usa o proprietário
de preferências Rust; o snapshot de compatibilidade continua para a interface
legada. O SDK não reenvia os campos no bootstrap nem os grava novamente.

O observador sem interface `shell:desktops` foi removido. O loop Rust atende o
serviço diretamente, independentemente do indicador visual do tema. Revisões
agrupam alterações da quantidade; valores iguais não geram nova gravação e
não existe polling ocioso dessa assinatura. A persistência é nativa; o caminho
local de compatibilidade dos `.lpk` permanece separado no build. Animação já
usa seleção/conclusão Rust; efeitos finais e o espelho legado seguem pendentes.

Os sete testes novos verificam inicialização atômica/idempotente, isolamento
e vida útil, validação das preferências, consultas atuais, associação de
superfícies, agrupamento, remontagem e ausência de timer. A suíte completa
tem 547 testes aprovados e sete especiais ignorados.

`desktop_hud_runtime` também cobre agora contagem com espelho desatualizado,
snapshot de troca de tema e consultas com `windows.get()` bloqueado, criação
de desktops ausentes, reinicialização sem reset, remontagem do observador e
persistência após retirar o indicador. Passaram os quatro cenários
claro/escuro em 640/1000 px, quatro contratos LPK e as regressões de
restauração, comandos de janela e encerramento do desktop instalado.
Compositor e bundles reconstruídos; instalação temporária e `switcher-check`
aprovados. Capturas e logs em `build/validation/desktop-session/`; o CI já
exige `desktop_hud_runtime` e publica `axia-desktop-hud`. Sem nova medição
física ou RPM completo nesta entrega.

## Agendamento e indicador do relógio

O engine residente de `axia-clock` mantém timer, cronômetro e alarmes como antes,
compartilhado pelas views nativas do processo da sessão. `subscribe()` agora
oferece invalidações canceláveis por `axia-state`. Escritas e mudanças na fila
de efeitos notificam depois de liberar o empréstimo; leituras e consultas sem
mudança não invalidam. O sinal agrupa várias escritas antes da sincronização.

O root monta `shell:clock` mesmo quando o tema não inclui StatusItems. Esse
observador usa um único prazo nativo para processar ocorrências e entregar
efeitos, preservando as fatias de um/dez minutos para mudanças do relógio e
suspensão. Sem ocorrências pendentes, não há prazo. Remontagens não reinicializam
o engine nem prorrogam o vencimento. Comandos nativos que alteram o engine
invalidam o observador sem depender de atualizar o espelho JS primeiro.

`shell:clock-status` calcula texto, visibilidade e aba de destino a partir do
engine Rust. O mount fornece apenas cor/altura e uma fonte opaca validada; não
fornece timer, cronômetro, próximo alarme ou config. A contagem usa um prazo
nativo de um segundo apenas enquanto executa; pausada, não pede atualizações.
O limiar de 24 horas do próximo alarme também é nativo. A formatação regional
reutiliza `axia-native::date_format`. Um clique revalida a aba pelo estado atual.

`sdk/alarms.js` ainda sincroniza preferências e publica snapshots para APIs
legadas. Os templates e as ações dos alarmes agora pertencem ao serviço Rust,
como descrito abaixo. A repetição do toque
agora usa o serviço nativo de notificações descrito abaixo, mantendo a correção
de permissões/silêncio/retirada sem intervalo JS na sessão.
O adaptador isolado preserva o caminho legado de agendamento e apresentação.
Esses itens continuam pendentes para a sessão sem JS.

## Serviço e banners de notificações

Cada `Session` possui um serviço `notifications` compartilhado apenas pelos
clones daquele contexto. Seu `axia_state::Store` contém até quatro banners,
50 itens de histórico, agrupamento e posicionamento; IDs não são reutilizados.
Leitores e assinaturas nativas acessam os dados por empréstimo. Publicação,
dispensa e ação emitem uma revisão; consultas e operações sem mudança ficam
silenciosas. O histórico pode ser limpo sem dispensar banners ativos.

A política distingue recusa por permissão/firewall (sem histórico), Não
Perturbe/estilo sem banner (somente histórico) e entrega visual. A agenda usa
minutos locais e aceita intervalos que cruzam meia-noite; um horário final de
DND usa o relógio civil. Expiração de banners usa `Instant`, mínimo de 1.500 ms
e nenhum prazo para sticky/alert. Banners retirados pelo limite não deixam
temporizadores pendentes. A view agenda somente a próxima expiração; ociosa,
não pede atualizações periódicas. O serviço mantém o prazo durante remontagens
e expira itens vencidos na próxima sincronização ou comando.

`shell:notifications` é ligada ao proprietário validado do contexto e constrói
os widgets diretamente do estado Rust. Agrupamento mantém o banner mais novo
por app/legenda e conta os demais; dispensá-lo revela o anterior. A view
revalida a ação contra IDs e índices atuais; um clique antigo não aciona uma
notificação removida ou reutilizada. Fechamento precede o callback da ação.

`sdk/notify.js` fornece metadados e executa callbacks genéricos. Entrega,
agrupamento e toque leem as preferências do proprietário Rust do perfil;
o registro de apps vistos também é atualizado nesse proprietário. O Rust
consulta permissões e firewall diretamente nos
armazenamentos do host. Os stores públicos recebem
snapshots por revisão para compatibilidade, sem alimentar a view nativa.
O adaptador descarta snapshots com revisão anterior ou igual à já aplicada;
efeitos atrasados da view não podem ressuscitar itens removidos por um comando.
Referências de callbacks são limitadas aos banners/histórico mantidos, com
liberação após drenar efeitos. Geometria do mount segue tema, área útil,
direção e posicionamento. O host LPK mantém seu serviço local sem acessar o
controlador privilegiado. Metadados, controles legados e consumidores dos
stores continuam pendentes; as APIs de preferências já usam o perfil nativo.

### Repetição do toque de alarmes

O pedido interno de toque só aceita o app Clock. Um banner entregue e audível
recebe um prazo monotônico de 60 segundos e um próximo pulso a cada 2,5 segundos.
Não Perturbe, estilo sem banner, recusa, silêncio e firewall impedem o início;
mudanças de preferências cancelam o toque ativo. A agenda DND é reavaliada com
a hora local nos pulsos, inclusive quando cruza meia-noite. O adaptador publica
a identidade do app; o Rust lê as preferências do perfil e consulta a permissão atual
e resolve os padrões e opções efetivas. O native view não recebe lista ou política.

O prazo pertence à notificação no contexto. Remontar a view preserva a cadência;
um atraso produz no máximo um pulso, sem recuperar sons perdidos em rajada.
Dispensa, ação ou retirada pelo limite cancela o prazo. Aos 60 segundos, o toque
termina e o banner continua para reconhecimento. Reativar o som/DND não retoma
alarmes já cancelados. Sem toques ou banners temporários, não há prazo de refresh.

Pulsos atualizam apenas o próximo instante interno, sem revisão de apresentação
ou snapshot JS a cada som. A fila interna de áudio contém ID e tipo do pulso;
o serviço valida o banner e o firewall antes de chamar `snd_play("notify")`
diretamente pela ABI nativa. Não há efeito `notificationSound`, pedido
`ringAudible` ou reprodução pelo adaptador JS na sessão. Um banner dispensado
ou silenciado não continua tocando por efeitos antigos. `ringing`
agora é apenas um espelho de compatibilidade. O host isolado mantém o caminho
local corrigido anteriormente. Callbacks genéricos e configurações permanecem
pendentes. O cliente de áudio existente continua no C; falha/desconexão descarta
a tentativa, sem fila para tocar sons antigos nem novo timer de recuperação.
Testes unitários observam as chamadas de áudio com prazos explícitos; a integração
real exige zero efeitos de som na ponte e zero chamadas JS, inclusive após
remontagem. Saída física de áudio segue reservada à validação final.
Logs e capturas desta etapa: `build/validation/native-notification-audio/`.

## Resolução de preferências de notificações

`notifications/preferences.rs` aplica os padrões globais e por app, a precedência
da permissão de pacotes e a mesclagem de alterações. Campos desconhecidos,
opções de outros apps e histórico de apps vistos são preservados. A mesclagem
global continua rasa, conservando o formato já persistido pelo SDK. Pacotes usam
o estado atual da permissão consultado pela API nativa; apps internos usam o campo `allow`.

Entrega, toque de alarmes e as APIs `notifyPrefs`/`appNotifyPrefs` da sessão
usam essas funções Rust. Pedidos de entrega não transportam `appPrefs`
pré-calculados, e o relógio não recebe opções efetivas produzidas pelo JS.
`setAppNotifyPrefs` pede ao proprietário a alteração mesclada e a gravação
nativa da permissão, quando aplicável. Consultas e cálculo de patches são
puros: não expiram banners, não drenam efeitos nem invalidam leitores.
As consultas `notifyPrefs`, `appNotifyPrefs` e `dndActive` da sessão leem o
proprietário atual e não transportam preferências JS. `setAppNotifyPrefs`
também mescla e grava nesse proprietário, antes de publicar avisos aos controles.
O SDK recebe snapshots de compatibilidade por revisão e não reaplica o patch
nem refaz sua mesclagem. Snapshots opcionais das APIs continuam locais ao host
isolado; na sessão, a fonte autoritativa é sempre o perfil nativo.
Persistência já pertence ao modelo Rust de configurações. Metadados de catálogo
e consumidores dos snapshots continuam pendentes. O host `.lpk` conserva as funções locais.

`notifications_runtime` modifica os padrões JS durante uma consulta para provar
que a sessão usa o Rust e verifica a edição sem perder opções. A entrega não
transporta preferências e o mount não recebe agrupamento. O teste muda o
agrupamento e DND diretamente no proprietário, mantendo o espelho JS antigo,
e exige que os banners, consultas e política respondam ao estado nativo. A edição
preserva campos que só existem no proprietário, e todo pedido de notificações
do SDK deve omitir `prefs`. O contrato
isolado mantém a compatibilidade legada.
Logs e capturas desta etapa: `build/validation/native-notification-preferences/`.

O serviço recebe um leitor do perfil privado ao vincular o controlador ou a view
ao contexto. A assinatura solicita sincronização sem polling; revisões de outros
campos não invalidam a apresentação. Reutilizar o vínculo numa remontagem mantém
o serviço. Cancelamento por mudança de preferências ocorre no próximo tick nativo,
sem assinatura de settings ou pedido `ringPolicy` pelo JS. Atualizações de `seen`
publicam somente um snapshot de compatibilidade por revisão; não devolvem um patch
para ser regravado pelo SDK. Evidências dessa ligação direta em
`build/validation/native-notification-settings/`. A migração das consultas e
edições está em `build/validation/native-notification-edit/`.

## Apresentação e horário de notificações

`notifications/presentation.rs` compartilha catálogos e formatação com os alarmes.
Nome e seleção do ícone vêm do registro nativo: tradução de `apps.<id>`, fallback
inglês e nome do manifesto. Apps ausentes usam a legenda fornecida pelo emissor.
O serviço ignora `hasApp`, `displayLabel` e `timeLabel` recebidos pela ponte na
sessão principal. Catálogo é lido por empréstimo, sem copiar a lista.

Entrega e histórico usam o relógio civil nativo, inclusive para a agenda DND;
expiração permanece monotônica. A formatação respeita idioma, região e preferência
de 12/24 horas do perfil Rust, mesmo com espelho JS antigo. Idioma/região e revisão
de catálogos dos serviços vêm de `locale.rs`, sem contexto enviado pelo SDK.
O store público é um espelho do mesmo proprietário; tradução e nomes de apps
usam o resolvedor compartilhado Rust. Formatação do SDK continua pendente.
Catálogos privados e fallback inglês permanecem disponíveis.

O SDK da sessão não calcula nome/horário nem transporta timestamp/minuto na
entrega. O host LPK mantém seu contrato local. Testes Rust verificam dados
forjados ignorados, tradução/fallback, ícone de app conhecido e horário nativo.
`notifications_runtime` exige apresentação portuguesa e relógio de 24 horas com
contexto JS inglês antigo, além da ausência desses fatos nos pedidos SDK.
Capturas/logs em `build/validation/notification-presentation/`.

## Fonte nativa de idioma e região dos serviços

`locale.rs` mantém um proprietário observável por perfil e raiz de assets.
Preferência salva prevalece; a seleção automática usa `LANG` de
`~/.config/locale.conf`, depois `/etc/locale.conf` sobre `/etc/default/locale`.
Valores têm a validação do formato do host, leitura limitada a 8 KiB e idioma
derivado sem executar comandos. Um catálogo `shell` válido habilita a escolha
automática; ausência/invalidez usa inglês. Região vem do perfil.

A leitura nativa sincroniza a revisão das preferências antes de emprestar o
estado. Alterações irrelevantes e leituras repetidas não invalidam esse estado.
Gravação bem-sucedida de locale.conf recalcula a escolha automática; instalação
de arquivos de idioma e `lord.locale.reload()` invalidam os catálogos por revisão,
inclusive quando uma instalação altera arquivos antes de falhar parcialmente.
Não há timer nem dependência de `lord.locale.get` pelo caminho dos serviços.

Notificações e entrega de alarmes usam essa fonte. Pedidos do SDK não carregam
lang/region/revision/localeRevision. `reloadCatalogs` mantém a invalidação dos
consumidores JS e avisa a fonte nativa. O store público do SDK espelha essa fonte
por revisão. Formatos, enumeração de idiomas e demais consumidores continuam pendentes.

Os testes Rust cobrem precedência/validação, limite, preferência salva,
invalidações silenciosas, seleção automática, metadados privados e recarga.
A integração instala pacote e altera locale.conf no perfil isolado enquanto
`lord.locale.get` JS lança erro, exigindo atualização dos banners sem contexto
de locale nos pedidos. Evidências em `build/validation/native-locale-source/`.

Também foi retirado o acesso direto a `lord.settings.save` da troca de tema:
ela usa `saveSettingsNow`, assim como o restante da sessão principal. O caminho
de arquivo JS permanece no host LPK.

### Locale público e aplicação na interface

Na sessão principal, `locale.get/subscribe` leem o espelho e `locale.set` envia
patches ao proprietário Rust. Updaters consultam primeiro o estado nativo atual;
respostas antigas não sobrescrevem revisões novas. A revisão do proprietário é
independente da revisão dos catálogos, que invalida os caches JS de tradução.
Overrides temporários, inclusive direção explícita e campos extras, ficam no
modelo nativo. A direção calculada vem dos metadados sem acrescentar `dir` ao
objeto público quando não foi fornecido. Revisões recebidas não são aceitas como
revisões do proprietário.

O comando de inicialização ativa apenas um contexto de interface e aplica idioma
da fonte e direção diretamente pela ABI C. Leitores de outros perfis não alteram
o estado global de texto. O loop sincroniza revisões de preferências sem polling;
o publicador do contexto transmite também `localeState`, sem fatos
de locale vindos do SDK. Respostas síncronas de settings incluem o locale atual,
preservando a atualização imediata dos consumidores existentes. Remontar a
apresentação mantém overrides; reinicializar o contexto os descarta.

`native_locale_runtime` cobre claro/escuro em 640/1000 px: espelho parado,
updater sobre estado atual, respostas antigas, direção por metadados e explícita,
remontagem, pacote instalado e recarga de catálogos. Bloqueia `locale.get` legado
e os setters JS de idioma/direção. O host LPK conserva seu store local, coberto
por contrato Rust separado. Passaram 610 testes unitários da shell, sete contratos
LPK, 24 cenários gráficos de locale/configurações/notificações/relógio/Forçar
encerramento/menu, encerramento instalado, smoke e apps compilados em GPU=1.
Compositor e bundles recompilados; captura inspecionada. Evidências locais em
`build/validation/native-locale-session/`; CI GPU=1 exige o teste e publica
`axia-native-locale`. Medições físicas continuam reservadas ao final do roadmap.

### Tradução do SDK no resolvedor compartilhado

Na sessão principal, `t`, `tOptional` e `appName` consultam os catálogos do
proprietário Rust, com idioma atual mesmo quando o espelho JS está antigo.
Catálogos do perfil precedem assets; fallback inglês e chave original continuam
disponíveis. O SDK só adapta strings de interpolação e a coerção numérica de
`count` na fronteira pública; seleção de plural e resolução ficam no `I18n`
compartilhado com as views e serviços nativos. Interpolação mantém o valor
original de `count` (por exemplo, `"01"`), independentemente da seleção numérica.
Contagens fracionárias não são mais truncadas para inteiro no resolvedor nativo.

Testes Rust cobrem as sete regras de plural, frações, NaN/infinito, fallback,
placeholders ausentes, nomes com fallback de manifesto, recarga e leituras sem
alteração de revisão. A integração `native_locale_runtime` também bloqueia os
leitores JS `locale.catalog/languages` nas funções migradas, exige tradução com espelho antigo e
atualização após instalar pacote. O contrato LPK exige tradução local sem usar
a autoridade da sessão. A entrega seguinte migra formatação e enumeração pública
dos idiomas, descritas abaixo. Demais consumidores de preferências permanecem
no escopo da Fase 2.

Validação desta entrega: 611 testes da shell, sete contratos LPK, 24 cenários
gráficos de locale/configurações/notificações/relógio/Forçar encerramento/menu,
encerramento instalado, smoke e integração dos apps compilados passaram em
GPU=1. Compositor e bundles recompilados; captura do texto recarregado inspecionada.
Evidências locais em `build/validation/native-translation/`. O CI mantém o teste
`native_locale_runtime` e o artefato `axia-native-locale`. Trabalho pausado a
pedido do usuário após esta entrega; medições físicas permanecem para o final.

### Formatos e idiomas públicos do SDK no Rust

`formatDate`, `formatTime`, `formatNumber`, `monthName`, `weekdayName` e
`languages` da sessão consultam o proprietário em `locale.rs`, usando idioma e
região atuais e a preferência nativa de relógio, mesmo com espelhos JS antigos.
O adaptador conserva apenas coerção pública e extração dos componentes locais de
`Date`: não converte a data para UTC nem lê catálogos ou configurações para formatar.
O host isolado `.lpk` mantém sua implementação local e não acessa a autoridade da sessão.

`axia-native::date_format::Formats` resolve os catálogos completos do SDK e os
mantém em cache por idioma/região/12–24 h/revisão. Nomes seguem o idioma; data
numérica, separadores e relógio seguem a região. Os formatadores existentes dos
apps nativos conservam seus fallbacks por campo para pacotes parciais.
A interpolação percorre o padrão uma vez: chaves dentro de nomes traduzidos não
são reinterpretadas. Datas inválidas conservam a apresentação legada com `NaN`.

`axia-native::number_format` implementa precisão fixa com o valor binário exato,
até 100 casas, sem nova dependência. Mantém zeros finais, sinal de valores
negativos arredondados a zero, arredondamento de empate para longe de zero,
NaN/infinito, representação exponencial e `RangeError` para precisão fora do limite.
Sem precisão explícita, a string pública mantém a coerção anterior do SDK.

A lista pública de idiomas lê `meta.json` com limite de 4 KiB, ignora metadados
inválidos, preserva a precedência dos idiomas embarcados em duplicatas e marca
pacotes exclusivos do perfil com `user`. A ordenação normaliza NFC e compara
pontos de código, como o `localeCompare` do QuickJS do projeto. A lista fica no
cache do proprietário; instalação e recarga de catálogos a invalidam. A tradução
e a formatação continuam dando precedência ao catálogo do perfil.

Testes Rust cobrem regiões diferentes do idioma, 12/24 h, pacotes parciais,
recarga, metadados inválidos/grandes, duplicatas, ordenação Unicode e leituras
sem alteração de revisão. `native_locale_runtime` compara 570 combinações de
valor/precisão por cenário com o comportamento anterior, bloqueando `toFixed`
durante as chamadas migradas; também bloqueia leitores JS de locale e catálogos
de formatos e exige resultados com espelho antigo. O teste continua obrigatório
no CI GPU=1, com capturas claro/escuro em 640/1000 px.

Validação de 2026-11-21 no Fedora 44 em contêiner: 617 testes da shell,
17 do toolkit, sete contratos LPK e 16 cenários gráficos de locale/configurações/
relógio/notificações passaram. Compositor e bundles recompilados, instalação
privada gerada e capturas dos formatos inspecionadas. Três amostras do coletor
Rust, com oito segundos de aquecimento e vinte segundos de observação cada,
passaram com imposição dos limites existentes: mediana de 12,200 poll/s e
104,770 MiB de RSS (limites 15 poll/s e 120 MiB). Escopo: thread principal,
renderização em software e rede desativada; não é comparação antes/depois nem
medição de apresentação física. Evidências em `build/validation/native-formats/`.
A sessão instalada do usuário não foi substituída nem reiniciada. Próxima frente:
consumidores que ainda enviam locale e preferências nos fatos de `NativeBridge`
em `sdk/nativeview.js`, além dos demais serviços da Fase 2.

### Locale emprestado pelas views da sessão

O NativeBridge da sessão não assina o espelho JS de locale nem calcula separadores
por `formatNumber`. Seus fatos deixam de incluir `lang` e os campos de contexto
`region`, `direction`, `decimal`, `group` e `localeRevision`. Cada view recebe o
proprietário validado do contexto na montagem, inclusive overlays sem janela.
Ícones continuam no caminho próprio, sem assinatura de locale ou timer adicional.

O adaptador C seleciona `axia_view_mount_session` após validar `lord.windowList`;
essa entrada recusa proprietário ausente. No processo isolado, a restrição de ID
e a montagem anterior são preservadas. A escolha entre autoridade da sessão e
fatos locais não depende de um booleano enviado nos fatos pelo aplicativo.

`native_view/locale_source.rs` mantém leitor/assinatura de `locale::State` e
somente os pequenos campos de apresentação. Se a revisão muda, o deadline do host
fica imediato; o próximo tick sincroniza a árvore e consome a invalidação. Não há
polling. Fatos de locale recebidos por engano são sobrescritos pelos valores atuais.
Idioma, direção, região e recarga de catálogo funcionam com o espelho JS parado;
leituras quietas não repintam a view. A assinatura é liberada ao desmontar.

Os separadores consultam uma API Rust tipada que reutiliza o cache de formatos do
perfil. Não se carregam catálogos de formatos por janela nem se copia uma lista
entre JS e Rust. O pedido interno de sync dos modelos ainda recebe os campos
escalares de apresentação; essa compatibilidade interna não serializa JSON pela
ponte. O host isolado mantém o locale local. Snapshots de `settings`,
`preferenceRevision`, escala, tema e outros serviços ainda estão no adaptador;
retomar pela leitura tipada de preferências nos consumidores, sem introduzir uma
cópia do perfil completo por view. Essa etapa foi implementada na fatia abaixo.

Testes Rust verificam precedência sobre fatos falsos, português/inglês, região,
direção explícita, recarga de catálogo, deadline imediato e retorno à ociosidade,
além do caminho local e da recusa de proprietário nulo. `native_locale_runtime`
monta um Relógio real, segura o espelho em inglês e exige texto em português sem
novos fatos enviados à view. Inspeciona montagem e atualizações para proibir os
campos removidos, em quatro combinações de modo e largura com capturas.

Validação de 2026-11-21 no Fedora 44 em contêiner: 619 testes da shell,
sete contratos LPK, integração dos apps compilados e 32 cenários gráficos de
locale, menus, foco, configurações, relógio, notificações e barras de título
passaram. A integração também exige recarga de texto no Relógio aberto após
instalar o catálogo; capturas claro/escuro e 640/1000 px inspecionadas.
Compositor/bundles recompilados e instalação privada gerada. Três amostras do
coletor Rust passaram nos limites impostos: mediana de 12,350 poll/s e
104,582 MiB de RSS, abaixo de 15 poll/s e 120 MiB. Escopo: thread principal,
software e offline, com oito segundos de aquecimento e vinte de observação por
amostra; não é medição física nem comparação controlada antes/depois.
Evidências em `build/validation/native-view-locale/`. A sessão instalada do
usuário não foi substituída nem reiniciada. A Fase 2 continua aberta pelos
consumidores de preferências e demais serviços, não por locale no NativeBridge.

## Consulta nativa de permissões e firewall

`notifications/access.rs` lê `perm_get` e `restrict_blocks` pela ABI do host,
sobre os mesmos armazenamentos usados pelos controles de privacidade. A entrega
e a consulta de opções não aceitam estado de permissão ou bloqueio calculado
pelo JS. A identidade é consultada no leitor compartilhado do catálogo do
controlador: pacote, isolado ou app interno. Pedidos do SDK não transportam
`source` nem `notificationPolicy`. O Rust deriva o escopo `lpk:<id>` ou `lordos` e preserva as exceções
de avisos de Configurações, notificações sem app e do namespace Linux, cuja
verificação ocorre na entrada D-Bus.

O toque reconsulta a identidade atual, permissão e firewall nos pulsos e antes
de reproduzir uma tentativa enfileirada. A assinatura do catálogo também
invalida os banners: trocar a identidade durante o toque aplica a política
atual no próximo tick, sem modificar settings nem enviar uma política JS.
Revogar uma permissão sem alterar `settings` cancela o toque; reautorização não
retoma um alarme já cancelado. Espelhos/avisos de revisão no SDK e a migração
do catálogo continuam pendentes. A gravação de notificações também migrou,
conforme a etapa abaixo.

`sdk/nativecatalog.js` centraliza a importação dos metadados para o controlador,
com uma assinatura compartilhada por controle de apps, notificações e relógio.
Não reenvia dados iguais nem cria um timer. O proprietário validado do contexto
retém o leitor; as notificações não mantêm uma segunda cópia da lista.
O catálogo ainda é descoberto/codificado pelo adaptador JS, e a view do Forçar
encerramento ainda recebe seus fatos próprios. Esses caminhos continuam pendentes.
Testes Rust cobrem metadados inválidos/iguais, identidade do catálogo prevalecendo
sobre fatos do pedido e cancelamento após mudança de identidade. A integração
altera o catálogo durante um toque, exige ausência de identidade nos pedidos
SDK e mantém os contratos isolados. Evidências em
`build/validation/native-notification-catalog/`.

A integração gráfica modifica os armazenamentos reais enquanto as consultas JS
simulam liberação: exige recusa de pacote, isolamento dos escopos do firewall,
exceções preservadas e cancelamento de um toque por revogação. Também exige
ausência dos fatos `permission`/`blocked` nos pedidos da sessão.
Logs e capturas desta etapa: `build/validation/native-notification-access/`.

## Gravação nativa das permissões de notificações

`access::update` seleciona a escrita de permissão para pacotes com alteração de
`allow`. O Rust reutiliza o validador de IDs `lpk_valid_id`, verifica o estado e
chama `perm_set`, preservando o armazenamento protegido existente. Uma recusa
ou retorno a `ask` só envia `brokersrv_revoke` após gravação bem-sucedida.
IDs inválidos, UTF-8 inválido, ponteiro nulo e estados fora da enumeração não
chegam à gravação. Outros campos e apps internos não alteram permissões.

`setAppPreferences` aplica essa operação no controlador vinculado ao contexto.
Uma recusa cancela o toque ativo e publica um aviso de revisão para o espelho
de privacidade. O SDK executa somente essa atualização de compatibilidade;
não chama `perms.set`/`lord.perms.set` para essa API da sessão. Assinantes do
aviso podem alterar configurações: a edição já está confirmada no proprietário
e seu snapshot é publicado antes do aviso. Alterações desses assinantes criam
uma revisão posterior e não são sobrescritas por uma reaplicação do patch.

A entrada pública `lord.perms.set` para notificações delega ao mesmo Rust por
`axia_notification_permission_set`, incluindo os pedidos do painel Configurações.
As demais permissões conservam sua implementação anterior. O host `.lpk`
mantém o contrato do SDK. Avisos de revisão, catálogo
e demais consumidores JS continuam pendentes da migração completa.

Testes unitários verificam validação, falha sem revogação, transição para `ask`
e cancelamento do toque. A integração usa os armazenamentos reais, impede
chamadas aos setters JS pelo SDK e exige revisões corretas, ausência de revisão
extra para mudanças de som e preservação de alterações feitas por assinantes.
Logs e capturas desta etapa: `build/validation/native-notification-permission-write/`.

## Configuração do relógio por perfil

`clock_settings.rs` vincula o engine ao leitor de configurações do perfil
privado. `axia-clock::Preferences` permite leitura emprestada e gravação direta;
o core mantém o contrato anterior quando não há provedor nativo, como no host
isolado. Na sessão, comandos ignoram configurações JS antigas, e alarmes/cidades
são gravados no proprietário Rust, preservando os demais campos do perfil.
Os efeitos de gravação `settings` não atravessam o SDK nesse caminho. O setter
nativo atualiza somente o campo `clock`, sem construir um snapshot completo das
preferências para descarte; alterações sem efeito não rearmam a gravação.

Cada acesso ao engine sincroniza a configuração atual antes de usar seus dados.
A assinatura nativa também invalida o observador de agendamento quando as
preferências mudam, mesmo sem comandos ou atualização do espelho JS. Assim, um
alarme criado diretamente no proprietário recebe seu prazo. Remover a configuração
restitui o objeto vazio. Reverter uma edição para um valor anterior também vale:
a comparação é com a configuração atual do engine, sem aguardar confirmação JS.

`startAlarms` não assina nem serializa configurações na sessão; pedidos
`clockCommand` não transportam `config`. A lista pública de alarmes consulta o
engine atual. O serviço de publicações do contexto transmite revisões para
consumidores legados. Idioma/região, fatos gerais da janela do app e os espelhos
de timer/cronômetro continuam pendentes da migração completa.

`native_clock_runtime` muda alarmes diretamente no Rust com o espelho JS antigo,
exige atualização do indicador e da lista pública, altera cidades sem regravação
JS e verifica preservação de campos e ausência de `config`/efeitos de gravação
na ponte. Testes Rust do core e da ligação ao perfil cobrem persistência, reversão,
limpeza da configuração e silêncio em leituras repetidas. Evidências locais em
`build/validation/native-clock-settings/`.

## Templates e ações de alarmes no proprietário Rust

`notifications/clock_delivery.rs` transforma os efeitos do engine diretamente
em notificações do contexto. Textos, nome do app, hora e botões usam os catálogos
nativos, incluindo pacotes do usuário, fallback inglês e invalidação por revisão
de idioma. O observador `shell:clock` entrega vencimentos sem passar por
`fireAlarm`/`fireTimer` no JS. Pedidos do SDK usam `clockCommand` no controlador
vinculado ao contexto; ambos os caminhos entregam cada conclusão uma única vez.

Parar, Adiar e Repetir são ações tipadas privadas, associadas ao ID da
notificação. A view e o histórico apenas enviam ID/índice; não há closure JS
para essas ações. Adiar usa a identidade do alarme e preserva patches pendentes
para o observador. Repetir consulta a duração atual no engine, mesmo com o
espelho JS desatualizado. O serviço conserva ações apenas enquanto a notificação
pertence aos banners ou ao histórico limitado. Callbacks de outros apps continuam
no adaptador de compatibilidade.

A entrega e os pulsos consultam a restrição atual de notificações do firewall
pela API nativa `restrict_blocks`, sobre o mesmo armazenamento usado pelo SDK.
Uma alteração entre o início e o vencimento do timer não depende de um novo
snapshot JS. Transporte de preferências e persistência continuam transitórios. O som já é
executado nativamente pelo serviço. O host `.lpk` mantém templates, reprodução
e callbacks locais.

`native_clock_runtime` também exige textos nativos, ação Repetir com duração
JS incorreta e bloqueio de firewall alterado durante a espera. O teste do
firewall reproduziu falha antes da consulta nativa. Testes unitários cobrem
ações pelo histórico, Adiar, Parar, limites, idioma e cancelamento do toque.

## Gestos horizontais de desktop

`desktopCommand/swipe` resolve no proprietário Rust se o gesto troca desktops
ou alterna janelas. O pedido contém direção e RTL, sem lista ou seleção JS.
Com vários desktops, respeita a ordem visual e retorna o limite sem mutação;
com um só, reutiliza a seleção do ciclo legado e exclui janelas minimizadas,
fullscreen e PiP. A operação publica no máximo uma revisão. O adaptador só
encaminha o gesto e mostra o indicador no limite; o host LPK preserva a API local.
O teste Rust verifica direções, RTL, filtragem, limites e entradas inválidas.
A integração do indicador bloqueia leituras do espelho de desktops durante o gesto.

## Animação de troca de desktops

`desktop_animation.rs` acompanha o desktop ativo no leitor do contexto e
seleciona as janelas visíveis diretamente no modelo. As molduras registram
somente `(id da janela, id do nó)` em `windowList.bindFx`; não enviam lista,
seleção ou geometria de janelas. A ligação retém também a geração do nó na
cena, evitando reaproveitar um ID já removido. Frames Linux que se montam
durante a transição usam o tempo restante; após 170 ms não há repetição.

O animador Rust restaura os valores iniciais no último frame e libera seu
estado. A conclusão nativa é distinguida na ABI sem alterar seu layout; C
aplica os valores e só entrega ao JS conclusões das animações legadas.
Reversões, movimento reduzido, Overview, desmontagem e encerramento cancelam
apenas as animações ainda pertencentes ao controlador. Uma alteração
explícita de transformação/opacidade interrompe o efeito antes de aplicar
o estilo, preservando transformações da interface, como as do Overview.

O root fornece apenas movimento reduzido e direção. Não há assinatura de
desktop ativo, escolha de janelas ou callbacks de término em JS na sessão;
o host `.lpk` conserva o caminho local de compatibilidade. Não é uma shell
sem JS: molduras, Overview, configurações e efeitos de outros serviços
continuam pendentes no roadmap.

Cinco testes nativos cobrem seleção, restauração automática, montagem tardia,
reversões, RTL, redução de movimento, Overview, reciclagem de IDs e animações
legadas intercaladas. `desktop_animation_runtime` usa molduras reais com
as entradas JS de animação bloqueadas, exige ausência de callbacks e renders
React por frame e verifica pixels após a conclusão e uma edição explícita.
O CI GPU=1 exige quatro cenários e publica `axia-desktop-animation`.

Validação: 552 testes unitários aprovados (sete especiais ignorados), quatro
cenários gráficos de animação com checagens de pixels, quatro do indicador,
quatro contratos LPK e saída normal/SIGTERM com apps nativos abertos passaram.
Compositor e bundles reconstruídos. Evidências locais em
`build/validation/desktop-animation/`. Sem nova medição física.

## Memória de geometria das janelas

`window_memory.rs` lê o proprietário Rust por assinatura. A coleta aguarda um
prazo de 800 ms, agrupa as mudanças e compara os valores por janela sem
serialização para detectar igualdade. Diálogos, painéis internos, fullscreen,
PiP e superfícies sem identidade não fornecem uma posição. A identidade de
launcher/Dock é compartilhada com o agrupamento do Forçar encerramento.

`shell:window-memory` é um serviço sem árvore visual, montado no host nativo
pelo adaptador do tema. Recebe metadados de apps/telas; não recebe janelas nem
assina o espelho JS. Oculto e sem revisão pendente, não agenda polling.
`rememberedFrame` também resolve a quantidade de janelas já abertas no Rust,
preservando deslocamento por instância, tamanho fixo e escolha do monitor.

A persistência também ocorre no proprietário de preferências Rust, mesclando
as alterações com os dados atuais e limitando a 200 chaves. Não há efeito JS
`windowPlaces`, pedido `memoryMerge` ou lista de posições no pedido de abertura.
`windowPlaces` mantém o formato de objeto; `windowPlacesOrder` registra a ordem
de uso para a expulsão sobreviver aos snapshots JSON de objetos ordenados por
chave. Os dois campos são publicados numa única revisão nativa. Live, greeter
e processos de apps isolados não ativam a coleta. Detalhes da atualização abaixo.

Validação: 531 testes unitários da shell aprovados (sete especiais ignorados),
13 do Forçar encerramento e 15 do núcleo nativo. Os quatro cenários de
`window_memory_runtime` bloqueiam `windows.get()` no JS, movem/criam janelas
diretamente no proprietário, verificam exclusões, ordem de preferências,
deslocamento por instância e parada após desmontar o serviço. Capturas e logs
em `build/validation/window-memory/`; o CI GPU=1 exige o teste e publica
`axia-window-memory`. Passaram também quatro contratos LPK e as regressões de
menus de app, Forçar encerramento e alternador. Medições físicas e RPM completo
continuam reservados ao encerramento.

Compositor e frontend recompilados; o teste do serviço passou novamente no
binário final. Instalação temporária e `switcher-check` com os arquivos
instalados aprovados.

## Restauração da sessão

`session_restore.rs` guarda o serviço no contexto `Session`, compartilhado
entre controlador e view. O serviço retém leitores do modelo e do perfil;
remontar a view ou trocar o idioma não repete a inicialização. Snapshot,
deduplicação, fila, geometria, desktops e associação de novas superfícies
Linux usam esse leitor, sem transportar a lista de janelas pelo JS.

O Rust controla os prazos: início de 1,5 s, abertura a cada 450 ms, espera de
10 s antes de retomar a coleta, autosave agrupado de 2 s e validade de 90 s
para posições de superfícies externas. A lista validada limita-se a 12
janelas e aos argumentos permitidos por app. O serviço também decide os
marcadores de saída, o congelamento antes de fechar apps e a retomada após
cancelar uma saída. A revisão das preferências impede que fatos atrasados
da view reativem uma opção recém-desligada no controlador.

`session_restore_view.rs` desenha a confirmação e trata Enter/Escape. O
adaptador `sdk/sessionrestore.jsx` abre apps, aplica
efeitos ao espelho legado e executa a ação fechada de política de navegador.
Mantém apenas um booleano de apresentação para a camada modal; a interface
oculta não bloqueia o teclado e a visível libera o foco de campos inferiores.
Preferências e marcadores duráveis já usam o proprietário nativo, conforme
a atualização abaixo. Efeitos de abertura/política e espelho JS continuam pendentes.

Validação desta entrega: 539 testes unitários da shell aprovados (sete
especiais ignorados) e quatro contratos LPK. `session_restore_runtime` usa
o compositor real em claro/escuro e 640/1000 px: confirmação com campo
inferior focado, remontagem, aceitação/recusa, exclusão de duplicatas,
associação de superfícies, autosave com espelho parado, saída, retomada e
desativação. O CI GPU=1 exige o teste e publica `axia-session-restore`.
Também passaram os quatro cenários de cada regressão de foco, indicador de
desktops e memória de posições. Compositor e frontend recompilados,
instalação temporária e `switcher-check` aprovados. Capturas e logs locais:
`build/validation/session-restore/`. Medições físicas e RPM completo
permanecem para o encerramento do roadmap.

## Encerramento das views e serviços

`js_shutdown()` chama `axia_view_shutdown()` antes de encerrar mídia e liberar
o contexto QuickJS. O registro Rust retira todas as views antes de executar
seus callbacks de fechamento, fora do empréstimo do registro. A sequência
dos handles permanece monotônica, inclusive após reiniciar o contexto.

Isso corrige o aborto visto nos jobs `desktop (0)` e `desktop (1)` dos runs
44570061368 e 44576784606: a sessão funcionava, mas os hosts permaneciam em
TLS até a saída do processo. O callback de fechamento de Arquivos acessava
seu serviço TLS já destruído (`AccessError`, seguido de SIGABRT).

`session_shutdown_runtime` instala o desktop em uma árvore temporária,
restaura Store/Configurações/Arquivos/Calculadora, abre Forçar encerramento
e Sobre o app e exige status de saída zero, ausência de panic e marcador
limpo nas preferências. Cobre timeout e SIGTERM, com logs publicados pelo
CI em ambas as variantes GPU. O teste reproduziu o SIGABRT antes da correção.

Validação local: builds GPU=0 e GPU=1 e os dois cenários de encerramento em
cada variante passaram. Também passaram 540 testes unitários da shell (sete
especiais ignorados), quatro contratos LPK, oito cenários de Arquivos e
quatro de restauração. Evidências em `build/validation/ci-shutdown/`.

## Integração antiga da barra de título no Actions

O run [44595705926](https://github.com/rodolfoaltenrath/Axia-DE/actions/runs/44595705926)
falhou no smoke agregado, após passar pela inicialização e pelo encerramento
que abortavam antes. A causa foi uma fixture não atualizada na migração
`e77d3bc5`: `NativeTitleBar` passou a ler título/foco do proprietário Rust,
mas `menubar_smoke.jsx` ainda usava `id: 'w1'` sem criar essa janela e fornecia
o título somente como prop JS. O clique procurava um texto inexistente.

A fixture agora abre a janela pelo `wm`, define seu título no modelo, testa
arraste e maximização e a fecha antes de prosseguir. O cenário agregado foi
transferido de `tools/desktop/check.py` para `shell_surfaces_runtime`, exigido
por `make check` em ambas as variantes GPU. O Rust controla o processo, a
captura e a exigência das sete etapas e de nenhum erro JS. O driver é
compilado no próprio teste; removida a geração do bundle antigo no build.
As outras verificações históricas de `check.py` permanecem para migração.

Validação: cenário antigo reproduziu a falha no compositor atual; corrigido,
passou nas variantes GPU=0 e GPU=1. O encerramento normal/SIGTERM também
passou em GPU=0. Bundles reconstruídos. Evidências locais em
`build/validation/ci-shell-surfaces/`; o CI publica `axia-shell-surfaces-0/1`.

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

### Preferências emprestadas pelas views da sessão

`NativeApp::uses_preferences` identifica os modelos que precisam do perfil.
Na montagem validada da sessão, o host lhes associa um `Reader<Value>` e uma
assinatura do proprietário de configurações. Durante `sync_with_preferences`,
o modelo recebe uma referência emprestada ao valor autoritativo. O empréstimo
termina antes da fila de efeitos; alterações persistentes continuam como efeitos
adiados. Não se deve gravar nesse proprietário dentro do sync emprestado.

NativeBridge não assina o espelho de preferências da sessão nem serializa
`settings`/`preferenceRevision`. O host descarta esses campos antigos, atribui
a revisão nativa ao contexto interno e reage às invalidações sem timer adicional.
Várias edições antes do tick se agrupam; após o consumo não resta prazo da
assinatura. Views sem interesse em preferências não assinam essas alterações.
O host isolado mantém fatos locais e a API de pedidos anterior.

Backup, Calendário, Câmera, Caracteres, Relógio, Contatos, Arquivos, Logs,
Monitor, Notas, Gravador, Lembretes, Scanner e Configurações usam essa entrada.
A restauração tem seu próprio leitor no serviço, descrito abaixo.
Relógio guarda só região e formato de hora;
o cache do Monitor guarda apenas seu bloco. O editor de Configurações conserva
somente os blocos usados por seus controles, além das alterações pendentes e
valores observados necessários para recuperar uma falha. Dados de outros apps,
posições de janelas e sessão salva não são copiados para esse editor.

Testes verificam identidade de ponteiro com o leitor nativo, prioridade sobre
fatos falsos, revisão atual, atualização sem transporte, retorno à ociosidade e
cancelamento pelo ciclo de vida do host. O editor também é exercitado com um
bloco alheio grande, edição pendente e falha de persistência. A integração do
Relógio congela o espelho JS de preferências, alterna 12/24 h diretamente no
proprietário e exige atualização na cena sem novos fatos à view. Montagens e
atualizações são inspecionadas para proibir os campos removidos.

Bootstrap de serviços e consumidores de tema/escala ainda precisam migrar;
o espelho público de configurações continua necessário para esses adaptadores.

Validação de 2026-11-22 no Fedora 44 em contêiner: 1.268 testes do workspace
Rust passaram, incluindo 620 da shell; 22 testes dependentes de ambiente ficam
ignorados nessa execução padrão. Também passaram sete contratos LPK,
integração dos apps compilados e 40 cenários gráficos de locale/preferências,
Arquivos, restauração, configurações, relógio, foco, notificações e cabeçalhos.
Compositor otimizado, bundles e instalação privada foram gerados; captura do
Relógio inspecionada. Logs/capturas em `build/validation/native-view-preferences/`.
Nenhuma instalação ou reinicialização da sessão física foi executada.

As três amostras da instalação privada passaram no orçamento de ociosidade:
mediana de 109.645.824 bytes (104,566 MiB) de RSS e 12,350 chamadas de espera/s,
com limites impostos de 120 MiB e 15/s. Relatório em
`build/validation/native-view-preferences/idle/report.json`. Essa coleta
headless verifica o orçamento; não substitui as medições físicas do encerramento.

### Preferências e marcadores de restauração no proprietário Rust

O controlador vinculado ao contexto e a view associam `session_restore::Service`
ao perfil validado do host, sem aceitar um caminho de perfil nos pedidos.
O serviço possui leitor/assinatura próprios. Inicialização lê `sessionSaved`
diretamente; modo, política de navegador e revisão também vêm do proprietário.
`settings` e `settingsRevision` saíram dos comandos do SDK; fatos antigos ou
com revisão falsa não conseguem substituir a configuração nativa.

A leitura do perfil é emprestada. O serviço retém a lista de recuperação que
precisa preservar; nunca retém uma cópia do perfil inteiro. Alterações de
política são aplicadas durante o sync/command. A assinatura invalida a view
sem polling, mesmo com o espelho JS parado. O empréstimo termina antes de
executar a persistência dos marcadores, evitando escrever num estado emprestado.

Início, autosave, saída limpa, retomada e troca de tema atualizam `sessionSaved`
e chamam a gravação durável existente no Rust. Não há efeito `sessionRestore/save`
no caminho vinculado da sessão, nem chamada a `settings.set`/`saveSettingsNow`
para esses marcadores. Início, flush e agendamento funcionam sem montar a view:
o loop Rust atende o serviço residente; a confirmação apenas desenha o estado
e recebe entrada. Executar abertura/política sem JS continua pendente.

A publicação `settingsState` mantém os consumidores de compatibilidade atualizados
por revisão após uma mutação; não grava novamente. Consultas públicas do modo e
da restauração de navegadores leem o proprietário sem consumir efeitos pendentes,
invalidar revisões ou tocar o disco. Setters validam valores no Rust e preservam
campos alheios. Falhas de gravação conservam o estado nativo e o indicador de
falha do proprietário; frames quietos não repetem I/O. Um próximo flush pode
repetir a gravação durável.

O SDK isolado conserva getters/setters locais. A view da sessão não assina o
perfil inteiro para lógica de restauração. O adaptador ainda observa somente
posição/tamanho/autohide da Dock e a opção de barra em todos os monitores para
atualizar os metadados de área útil fornecidos pelo tema; geometria nativa do
tema faz parte das próximas fases.

Validação de 2026-11-22 na árvore compartilhada, em Fedora 44 privado: 623
testes da shell, oito contratos LPK e 20 cenários gráficos de restauração,
locale/preferências, configurações, desktops e memória de posições passaram.
Encerramento do desktop instalado também passou por timeout e SIGTERM com
apps nativos restaurados. O teste de restauração começa com modo JS desligado
e modo nativo ask, proíbe os campos removidos nos comandos e as gravações de
restauração por JS; também congela o espelho para consultar/editar opções.
Os testes unitários usam o gravador nativo simulado para exigir gravação
durável, preservação de campos alheios, marcadores de início/flush/retomada/tema,
quietude de consultas, política por assinatura e retenção após falha de I/O.
Compositor e bundles recompilados, instalação privada gerada e confirmação
visual inspecionada. Evidências em `build/validation/restore-preferences/`.
A sessão física em uso não foi instalada nem reiniciada.

Três amostras da instalação privada passaram com limites impostos: mediana
de 109.596.672 bytes (104,520 MiB) de RSS e 12,500 chamadas de espera/s, abaixo
de 120 MiB e 15/s. Relatório em `build/validation/restore-preferences/idle/report.json`.
Essa coleta headless não substitui as medições físicas do encerramento.

### Preferências nativas de desktops e posições de janela

O controlador da sessão vincula um leitor ao perfil validado do host. O serviço
`desktop_session` compartilha essa fonte com o observador; montagem/recriação
não reinicializa os desktops. Bootstrap lê contagem e associação de superfícies
por empréstimo, consumindo o hand-off no proprietário. Mudanças de contagem e
a coleta da troca de tema gravam diretamente os campos existentes. O SDK não
envia `settings` no start nem reaplica efeitos `settings` desses serviços.
A troca de tema continua usando o adaptador para tema e apps a reabrir.

Movimento reduzido também vem do perfil, com assinatura nativa. Não há
`reduceMotion` nos fatos JS nem assinatura React dessa preferência em
`DesktopSession`. Tanto comandos quanto sync aplicam a política antes de
consumir a invalidação, inclusive para cancelar uma animação em andamento.
A direção chega pelo contexto interno de locale nativo; a animação mantém sua
seleção e conclusão no Rust.

`Memory` continua agrupando geometria por 800 ms. Ao persistir, consulta as
preferências atuais, mescla somente as posições alteradas e escreve diretamente
no proprietário. A gravação de arquivo segue o prazo nativo de configurações.
A ida por efeitos `windowPlaces`, a mesclagem `memoryMerge` e a gravação JS
foram removidas. Na abertura, o controlador empresta a posição do app solicitado;
não copia nem recebe uma lista de todas as posições. Metadados de apps/telas,
áreas úteis e opções explícitas continuam no adaptador transitório.

O objeto `windowPlaces` continua compatível. `windowPlacesOrder`, com até 200
chaves, preserva a recência quando o objeto JSON passa por mapas ordenados por
chave. O gravador usa a ordem persistida, inclui entradas externas ainda presentes,
move atualizações ao fim e elimina as mais antigas. Perfis sem esse metadado
começam pela ordem de chaves disponível; as atualizações seguintes registram a
recência. Uma mutação nativa `set_many` publica mapa e ordem juntos, em uma única
revisão; alterações sem efeito não invalidam. O empréstimo de leitura termina
antes da escrita. Dados alheios e edições posteriores à coleta são preservados.

As publicações `settingsState` atualizam somente consumidores de compatibilidade,
sem reaplicar patches ou gravar novamente. O host isolado mantém sua inicialização
e suas opções locais. Desktops, restauração e memória de posições são atendidos
pelo loop residente. Retirar metadados do tema/espelhos e desvincular os demais
serviços das views continua pendente.

Validação de 2026-11-22 em Fedora 44 privado: 627 testes da shell, oito contratos
LPK, 24 cenários gráficos de desktops, animação, posições, restauração,
configurações e locale passaram. Encerramento instalado passou por timeout e
SIGTERM. As integrações usam preferências nativas com espelhos antigos, proíbem
transporte dos campos removidos e escrita JS dos serviços. Movimento reduzido
exige o pixel final imediatamente, sem novos fatos; os testes unitários verificam
cancelamento por comando, ordem após nova gravação, expulsão por recência,
mesclagem com alterações externas e publicação atômica de mapa/ordem.
Compositor e bundles recompilados, instalação privada gerada. Evidências em
`build/validation/native-desktop-memory-preferences/`. Sessão física preservada.

A captura da animação foi inspecionada. Três amostras da instalação privada
passaram com limites impostos: mediana de 109.785.088 bytes (104,699 MiB) de RSS
e 12,350 chamadas de espera/s, abaixo de 120 MiB e 15/s. Relatório em
`build/validation/native-desktop-memory-preferences/idle/report.json`. Medições
físicas continuam reservadas ao encerramento do roadmap.

### Serviços residentes e vida útil do contexto

`Session` compartilha um `SessionCore` por `Rc`: modelo de janelas, serviços e
fila de efeitos pertencem ao mesmo contexto. Clones preservam a identidade;
leitores de janelas continuam emprestando somente o estado e não mantêm o
contexto vivo. O controlador criado pela entrada C validada ativa os serviços;
a ativação não vem de uma preferência ou de um campo nos pedidos JSON.

`session_runtime.rs` registra referências fracas, deduplica o proprietário e
remove registros cujo último dono terminou. O registro não retém o contexto,
nem ponteiros C/JS. A iteração libera o empréstimo do registro antes de atender
cada modelo, sem copiar uma lista de contextos a cada quadro. O loop existente
chama esse atendimento antes das views; assinaturas pendentes e deadlines entram
no mínimo nativo do poll. Não há timer JS de agendamento ou polling adicional.

Desktops aplicam preferências/locale atuais, concluem a seleção da animação e
persistem mudanças pelo serviço, sem `shell:desktops` ou `DesktopSession` no
bundle. A apresentação do indicador segue separada. A restauração atende início,
abertura escalonada, espera de estabilização, coleta e autosave mesmo sem a
confirmação montada. Seu catálogo acompanha o leitor publicado pelo controlador;
revisões são consumidas sem depender de novos fatos da view. Geometria ainda usa
a última configuração recebida do SDK e das telas, até migrar esse proprietário.

A view de restauração lê revisões e recebe Enter/Escape; no contexto residente
não mantém um segundo prazo nem atende o serviço no sync. Os pedidos diretos
continuam executando a intenção e devolvendo seus efeitos imediatamente. O
caminho interno não registrado usado pelos testes anteriores mantém o atendimento
explícito, sem reintroduzir um caminho alternativo no bootstrap da sessão.

Efeitos produzidos fora de uma view ficam na fila do contexto. Snapshots completos
de preferências por revisão se agrupam pelo último valor; comandos de restauração
mantêm a ordem. O adaptador C expõe `takeServices` somente no controlador do root,
com validação do objeto opaco; modelos criados no host isolado não recebem o método.
Uma leitura quieta retorna null sem serializar JSON. O SDK drena a fila pelos
adaptadores existentes, com guarda contra drenagem reentrante e sem exigir um
handle visual. Abertura de apps, política de navegadores e espelhos de compatibilidade
ainda usam JS. Relógio, notificações e memória de posições também são atendidos
pelo loop, como detalhado abaixo.

Validação de 2026-11-22 na árvore compartilhada, em Fedora 44 privado: 631 testes
da shell e oito contratos LPK passaram. Quatro testes novos verificam prazos e
autosave sem cena, confirmação e catálogo por revisão, vida útil/isolamento de
contextos e agrupamento/drenagem silenciosa. Passaram 44 cenários gráficos de
restauração, desktops, animação, Arquivos, foco, cabeçalhos, configurações, locale
e posições de janela, além do encerramento instalado por timeout e SIGTERM.
O teste real começa sem view de confirmação, exige o aviso após o prazo e só
então monta a apresentação; desmonta-a antes de mover a janela e exigir autosave.
Compositor e bundles recompilados, instalação privada gerada. Evidências em
`build/validation/resident-session-services/`. A sessão física foi preservada.

As mensagens da shell em português, inglês e espanhol usam o nome Axia.
A confirmação foi revalidada nos quatro cenários e a captura com o nome
corrigido foi inspecionada.

Três amostras de ociosidade do binário final passaram com limites impostos: 12,450 poll/s e 105,016 MiB de RSS, abaixo de 15 poll/s e 120 MiB.

### Relógio e notificações sem hosts visuais

O controlador validado ativa o agendador `session_clock.rs` e o atendimento das
notificações no mesmo contexto residente. A referência do agendador para o
serviço de notificações não retém o contexto: ambos pertencem ao `SessionCore`,
e o registro global continua fraco. Clones da sessão preservam os serviços;
a liberação do último proprietário permite a destruição, mesmo quando sobra
um leitor ou uma referência ao serviço de notificações.

O loop considera a revisão do engine, a assinatura das preferências e o prazo
monotônico da próxima ocorrência. Só chama `arm` quando há invalidação ou prazo
vencido. `ClockSession`, a montagem `shell:clock` e a árvore invisível foram
retirados; remontar o indicador da barra não substitui nem agenda esse serviço.
O indicador ainda atualiza sua apresentação da contagem por prazo próprio.
Templates de conclusão continuam lendo preferências e idioma nativos na entrega.

Expiração de banners, repetição e cancelamento de toque, revisões de catálogo e
preferências também são atendidos antes das views. O banner residente lê estado
e recebe ações; não agenda expiração nem executa esse atendimento ao sincronizar.
Instâncias internas sem ativação residente mantêm a API direta para consumidores
nativos e testes. Pulsos atrasados agendam uma vez a partir do instante atual;
mudanças de política cancelam o toque sem exigir banner. Um prazo vencido retorna
zero, sem epsilon positivo que impediria o loop de atendê-lo.

A fila do contexto também recebe `clockReply` e os efeitos de notificações.
Snapshots completos de `notificationState` são agrupados pela espécie, conservando
o mais recente; efeitos de fechamento e ações mantêm sua ordem. O adaptador usa
as pontes existentes, inclusive callbacks legados, com guarda de reentrância e
sem timer JS de serviço. Os hosts LPK continuam com seu agendamento local.
Espelhos/callbacks e demais consumidores legados permanecem
no roadmap; esta etapa não elimina o JS da sessão.

Validação em Fedora 44 privado em 2026-11-22: 634 testes da shell passaram, com
sete testes ignorados que dependem de integração específica; oito contratos LPK
foram executados e passaram. Os três testes novos exercitam expiração e callback
sem banner, conclusão única e vida útil do contexto, além de toque atrasado e
cancelamento por preferências nativas sem apresentação. Os dois testes do antigo
agendador passaram a testar diretamente o serviço sem implementar `NativeApp`.

Passaram 24 cenários gráficos de relógio, notificações, restauração, animação de
desktops, locale e memória de posições. O relógio exige ausência de `shell:clock`
e conclusão com indicador e banner desmontados; a integração de notificações
exige expiração e callback antes de remontar o banner. Encerramento instalado por
timeout e SIGTERM passou. Capturas do indicador claro e banner escuro foram
inspecionadas. Compositor/bundles compilados e instalação privada gerada;
evidências em `build/validation/resident-clock-notifications/`.

Três amostras de ociosidade passaram com limites impostos: 12,450 poll/s e 104,754 MiB de RSS, abaixo de 15 poll/s e 120 MiB. A sessão física foi preservada.

### Memória de posições e telas físicas no contexto

`Memory` deixou de implementar `NativeApp`. O `SessionCore` possui uma única
instância compartilhada pelos clones, com assinatura das janelas e do catálogo
observável do controlador. A coleta empresta esse catálogo quando chega o prazo;
não recebe registry/catalog em fatos da view. `WindowMemory`, a montagem
`shell:window-memory` e a assinatura React de telas foram retirados do tema/SDK.
O cache de deduplicação e o prazo de 800 ms sobrevivem à desmontagem e à troca de
tema. Novas mudanças não empurram indefinidamente o prazo já agendado.

A coleta usa somente coordenadas lógicas das telas físicas e sua identidade,
sem área reservada por barras/Dock. Por isso não foi criado outro transporte JS
de geometria. `js_set_displays` entrega o evento nativo à fonte observável
`session_displays.rs` antes da atualização de `lord.screen`; o Rust analisa o
JSON uma vez por evento, conserva até 16 telas e projeta somente os campos
usados pelo consumidor. Revisões sem mudança física não invalidam. A API C
rejeita ponteiro nulo, texto inválido e eventos acima do limite; descritores sem
frame utilizável são descartados. Eventos malformados conservam a fonte anterior.
Leitores emprestam a lista; nenhum contexto retém ponteiros C/JS.

O loop considera revisões de janelas, catálogo e telas, além do prazo monotônico.
Não há polling após consumo nem dependência de hosts visuais. Mudanças de
identidade do app ou disposição física também disparam a coleta sem novo fato
de janela. O resultado é mesclado no perfil atual com mapa/ordem em uma revisão;
a fila do contexto publica somente o snapshot de compatibilidade necessário.
Frames transitórios continuam excluídos e o limite de 200 apps foi preservado.

O bootstrap do SDK ativa a coleta por comando validado `memoryStart`, sem
janelas, catálogo, preferências ou telas. Mantém a guarda de sessão/live e a
consulta de posição usada pela abertura. Hosts LPK não ativam esse serviço.
Desativação cancela o prazo; reativação coleta o proprietário atual. A política
inicial de sessão/live, as áreas/opções do tema usadas para restaurar o frame,
metadados importados e consumidores dos espelhos continuam no escopo do roadmap.
O produtor de eventos de tela ainda é o compositor C existente.

Validação em Fedora 44 privado em 2026-11-22: 639 testes da shell passaram,
com sete ignorados de integrações específicas, além de oito contratos LPK.
Cinco testes novos cobrem prazo/cache e vida útil com clones, catálogo/telas por
revisão sem mudança de janela, desativação/reativação e publicação/validação da
fonte física pela ABI. Os seis testes existentes de memória foram adaptados
para leitores e atendimento direto, sem implementar uma view de teste.

Passaram 40 cenários gráficos de memória, animação de desktops, Arquivos,
cabeçalhos, relógio, notificações, locale e restauração, além do encerramento
instalado por timeout e SIGTERM. A integração exige coleta sem observador e com
conteúdo desmontado; também altera a cópia JS do nome/origem da tela e exige
que a posição gravada use os dados físicos originais. Preserva as verificações
de filtros, ordem, instâncias, preferências atuais e ausência de gravação JS.
Capturas da fixture foram geradas e a confirmação de restauração foi inspecionada.
Compositor/bundles compilados e instalação privada gerada. Evidências em
`build/validation/resident-window-memory/`; a sessão física foi preservada.

Três amostras de ociosidade passaram com limites impostos: 12,450 poll/s e 104,891 MiB de RSS, abaixo de 15 poll/s e 120 MiB.

### Publicação de preferências e idioma sem observador

`session_preferences.rs` pertence ao `SessionCore` e é vinculado pelo construtor
validado do controlador, com perfil e assets nativos. Guarda leitores e
assinaturas de configurações/idioma, além das revisões já publicadas; não possui
árvore, handle visual nem ponteiros C/JS. Ligar novamente o mesmo contexto com
o mesmo perfil/assets preserva o serviço e suas revisões consumidas.

O prazo de gravação continua no proprietário de configurações, independente de
qualquer view. O serviço de publicação mantém o rearm inicial de dados pendentes
e apenas pede atendimento imediato quando suas assinaturas têm alterações.
A publicação ocorre depois dos demais serviços do contexto, de modo que coleta
e persistência anteriores no tick já estejam incluídas no snapshot atual.
Locale inativo não cria tentativa periódica; alterações e recargas nativas
invalidam seu leitor quando há novas revisões a transmitir.

A fila do contexto agrupa também `localeState`, além de `settingsState` e
`notificationState`, mantendo o snapshot mais recente de cada espécie. O SDK
usa suas pontes existentes para compatibilidade; updaters continuam consultando
o proprietário atual e consumidores rejeitam respostas antigas. Leitores Rust
dos apps e serviços continuam acessando o estado por empréstimo.

`SettingsSession`, o arquivo `sdk/settingssession.js`, a montagem `shell:settings`
e o antigo `native_view/settings.rs` foram removidos. A publicação funciona
antes de renderizar a interface e depois de desmontar os apps. O módulo JS de
drenagem continua enquanto houver consumidores de compatibilidade; os stores,
callbacks e interfaces restantes da sessão continuam no roadmap. O host LPK
preserva seu estado e seus efeitos locais.

Validação em Fedora 44 privado em 2026-11-22: passaram 642 testes da shell
(sete ignorados de integrações específicas), oito contratos LPK e 44 cenários
gráficos de configurações, locale, desktops, Arquivos, relógio, notificações,
restauração, cabeçalhos e memória de posições. Encerramento instalado por timeout
e SIGTERM também passou. Três testes novos cobrem agrupamento de snapshots,
reattachment/vida útil, isolamento de perfis e idioma/recarga sem cenas.

As fixtures de configurações/locale proíbem o antigo observador e exigem
publicação antes de renderizar; locale também muda com o relógio desmontado.
As verificações de movimento reduzido e apresentação de notificações congelam
explicitamente a ponte JS para preservar a prova de independência do espelho.
Capturas de configurações e relógio traduzido foram inspecionadas. Compositor,
bundles e instalação privada gerados; evidências em
`build/validation/resident-preference-publications/`. A sessão física não foi alterada.

Três amostras de ociosidade passaram com limites impostos: 12,500 poll/s e 104,785 MiB de RSS, abaixo de 15 poll/s e 120 MiB. Medições físicas e comparação antes/depois permanecem para o encerramento.

### Paleta da shell e materiais no Rust

`native_style::shell_theme` (fonte `rust/axia-shell/src/shell_theme.rs`) resolve cores,
seis destaques em ordem estável, superfícies, barras de rolagem e materiais da
Dock. O transporte existente `lord.appearance.request` aceita `op: shellTheme`;
não inicia serviços nem lê arquivos. Cor e alpha usam as primitivas Rust do
compositor, com formato CSS independente do locale. O tratamento anterior de
campos opcionais foi preservado: vidro fica ativo salvo `effects: false`,
enquanto painel e blur seguem a presença de um valor verdadeiro.

O adaptador `themes/default/tokens.js` deixou de conter as tabelas e decisões
de cores. Solicita somente os três campos de aparência, retém um único plano e
reutiliza sua identidade em renderizações equivalentes; mudanças de outros
campos das configurações não atravessam a ponte. O catálogo de papéis de parede
continua compartilhado por referência e é associado depois da resposta. Pares
ordenados de destaques preservam a ordem dos controles ao cruzar JSON.

Isso não conclui a migração do tema: árvore JSX, publicação de tokens aos LPK,
integração do portal, geometria e transporte de aparência às views continuam
no escopo. O tema não é oferecido a terceiros; a API dos apps isolados permanece.

Validação em Fedora 44 privado em 2026-11-22: passaram os quatro testes novos
(opcionais, ordem/fallback de destaques, materiais e limites do resultado),
646 testes da shell no total, com sete ignorados de integrações específicas,
e oito contratos LPK. A compilação completa GPU=1 e os bundles passaram após
recompilar a árvore atualizada de dependências do trabalho paralelo.

Passaram 24 cenários gráficos: Arquivos e cabeçalhos nos estilos Axia/Adwaita,
mais Dock e menus, em claro/escuro e 640/1000 px. A integração de Arquivos
bloqueia o antigo acesso JS ao material e exige paleta nativa, ordem dos
controles, troca de efeitos/destaques e reaproveitamento de identidade em mil
chamadas equivalentes; pedidos ficam abaixo de 128 bytes e não incluem
catálogos ou janelas. As oito capturas de Arquivos são idênticas, byte a byte,
às da etapa anterior (`resident-preference-publications`); claro e escuro
foram inspecionados visualmente. Encerramento instalado por timeout/SIGTERM
passou. Evidências em `build/validation/native-shell-theme/`; sem instalação
ou reinício da sessão física.

Três amostras de ociosidade passaram com limites impostos: 12,500 poll/s e 104,590 MiB de RSS, abaixo de 15 poll/s e 120 MiB. São medidas headless do binário atual, sem prova de ganho causal; a medição física permanece para o encerramento.

### Portal de aparência atendido pelo contexto

`session_appearance.rs` é parte do serviço de preferências residente. Uma revisão
nova resolve um plano de escalares por empréstimo do perfil: modo, RGB, nome
GNOME do destaque e disposição dos botões. A paleta é a mesma de `shell_theme.rs`,
sem segunda tabela de cores. O empréstimo termina antes dos efeitos; o plano
usa strings C estáticas e o backend consome o vetor RGB apenas durante a chamada.
Não há referência de view, snapshot de perfil, ponteiro JS ou timer no serviço.

O cache do serviço compara os campos relevantes; revisões de outras preferências
não repetem `portal_set`. Mudança de cor não ressincroniza arquivos de apps
externos. Um cache de modo/disposição por thread preserva a deduplicação desses
arquivos entre recriações do contexto, como fazia a antiga ponte C. O backend C
continua servindo D-Bus, emitindo mudanças e usando a gravação Rust existente do
keyfile GTK, que preserva outros grupos e chaves.

`syncPortal`, seu mapa de nomes e seu cache JSON foram apagados de `sdk/theme.js`.
A ponte `lord.portal.set`, seu parser e cache de efeitos foram removidos de
`js_wm.c`. Um provider com tokens antigos ou locais não pode sobrepor a
preferência nativa. A seleção de arquivos do portal continua disponível. Apps
LPK recebem tokens pelo broker; a ponte removida não era exposta pelo host
isolado. Providers/publicação de tokens e demais consumidores JS seguem pendentes.

O teste Rust `portal_appearance_runtime` foi incluído no workflow GPU=1 com
capturas próprias. Ele mantém o espelho JS antigo, bloqueia a antiga chamada JS,
exige ausência da ponte e confere o keyfile real antes de renderizar, com um
provider antigo e depois de desmontá-lo. Também preserva um grupo customizado.
O fixture é privado e sem barramento de sessão: testa aplicação/persistência de
preferências, sem declarar validação de sinais D-Bus ou de apps GTK físicos.

Validação em Fedora 44 privado em 2026-11-22: passaram 648 testes da shell,
com sete ignorados de integrações específicas, e oito contratos LPK. Os dois
novos testes unitários verificam paleta/nomes/botões, tipos inválidos, cache de
campos relevantes e deduplicação da escrita externa na recriação do serviço.

Passaram 36 cenários gráficos de portal, configurações, locale, Dock, Arquivos,
menus e cabeçalhos, além do encerramento instalado por timeout e SIGTERM.
Capturas próprias do portal em claro/escuro e 640/1000 px foram geradas;
as duas variantes mínimas foram inspecionadas. As oito capturas de Arquivos
são idênticas byte a byte às de `native-shell-theme`. Compositor GPU=1,
bundles e instalação privada gerados. Evidências em
`build/validation/resident-portal/`; a sessão física foi preservada.

Três amostras de ociosidade passaram com limites impostos: 12,550 poll/s e 104,840 MiB de RSS, abaixo de 15 poll/s e 120 MiB. Sem alegação de ganho causal ou medição física; o protocolo físico permanece para o encerramento.

### Publicação nativa de aparência e idioma para apps isolados

`session_tokens.rs` pertence ao publicador residente de preferências. Resolve uma
chave dos campos de aparência do perfil, libera o empréstimo e serializa somente
quando essa chave muda. Usa a paleta da shell e o plano Adwaita compartilhados,
incluindo os padrões de controles, efeitos, scrollBars e reduceMotion. O estado
serializado não é retido pelo publicador: o broker C conserva a cópia limitada
necessária ao IPC e ao estado de arranque. Revisões de outros campos não repetem
a publicação. O perfil inteiro não atravessa a chamada.

`theme_catalog.rs` compila o índice de papéis de parede e os quatro gradientes,
com nomes, caminhos, dimensões e ordem anteriores. O catálogo tem uma única
alocação compartilhada; o serializador o empresta, sem clonar a lista a cada
mudança. O mapa de destaques usa serialização ordenada para manter a ordem dos
controles no JavaScript dos apps. `tokens.js` deixou de importar/montar esse
catálogo e recebe um snapshot de compatibilidade no carregamento; a migração
da árvore do tema continua pendente. O SDK ainda mantém seus helpers legados
de seleção de imagens e os padrões dos providers locais.

A fonte de idioma publica apenas lang, region e dir a partir do leitor nativo;
revisões exclusivas de catálogos não repetem um pacote idêntico. Os empréstimos
terminam antes das chamadas ao broker. Ativar locale também invalida assinantes
quando os valores não mudam; isso atende uma inicialização posterior a um tick
com a fonte inativa. A primeira publicação do contexto é atendida mesmo quando
as preferências já estavam carregadas; fonte inativa não gera polling.

Foram apagados publishTokens de `sdk/theme.js`, publishLocale e sua assinatura
de `sdk/privacy.js`, e a ponte `lord.broker.publish` de `js_sec.c`. O transporte
e a autoridade do broker permanecem: os apps leem o estado inicial e recebem
eventos, sem publicar configurações da sessão. O serviço funciona antes de
renderizar e depois de desmontar o tema, com os espelhos JS antigos. Providers
locais não sobrepõem o estado do broker. Root React, espelhos e fatos de aparência
das views continuam no roadmap; o host LPK conserva React/JS por decisão explícita.

O teste Rust `theme_tokens_runtime`, incluído no CI GPU=1 com capturas, prepara
dois pacotes somente no HOME privado do fixture e inicia processos reais pelo
host isolado. Um já está aberto quando as preferências mudam; o outro começa
depois e lê o estado atual de arranque. Ambos escrevem seus resultados pelo
broker no diretório de dados de cada app. O teste cobre Axia/Adwaita, controles,
efeitos, redução de movimento, catálogos, idioma/região/RTL, espelhos antigos,
provider antigo e desmontagem. As guardas e o sandbox de produção são preservados.
O helper de UI ganhou preparação explícita do HOME; os demais testes usam no-op.

O índice incorporado também é pré-requisito do arquivo Rust no Makefile. Um teste
de grafo incremental verifica que mudar o índice agenda a recompilação, sem
executar Cargo; o grafo normal da árvore compilada permanece atualizado.

Validação em Fedora 44 privado em 2026-11-22: passaram 653 testes da shell,
com sete ignorados de integrações específicas, oito contratos LPK e o novo teste
do grafo incremental. Cinco testes unitários novos da shell cobrem catálogo compartilhado,
planos/serialização e limite do broker, cache dos campos relevantes, projeção de
locale e publicação inicial/ativação tardia sem mudança de valores.

Passaram 48 cenários gráficos de publicação aos apps isolados, configurações,
locale, portal, Dock, Arquivos, menus, relógio, notificações e cabeçalhos, além
do encerramento instalado por timeout/SIGTERM. Na execução final, o fixture iniciou oito
processos isolados, dois por variante claro/escuro e 640/1000 px;
ambos recebem estado atual e atualizações antes de renderizar e com tema
antigo/desmontado. Capturas mínimas claro/escuro inspecionadas. As oito capturas
de Arquivos são idênticas byte a byte às de `resident-portal`. Compositor GPU=1,
bundles default/app-host e instalação privada gerados. Evidências em
`build/validation/resident-theme-tokens/`; a sessão física foi preservada.

Três amostras de ociosidade passaram com limites impostos: 12,550 poll/s e 104,672 MiB de RSS, abaixo de 15 poll/s e 120 MiB. Não é prova de ganho causal nem medição de hardware; as medições físicas continuam reservadas ao encerramento.

### Aparência compartilhada por perfil e leitura direta no Relógio

`appearance_source.rs` deriva o plano de cores de um leitor do perfil e guarda
`Store<Rc<Value>>`. A chave contém somente os campos relevantes; preferências sem
relação não mudam o plano. Planos com resultado igual também não publicam outra
revisão. O registro por perfil é fraco, sem ponteiros C/JS nem proprietário global
que retenha janelas. Cada consumidor tem uma assinatura; uma referência às cores
não mantém o modelo derivador vivo. Perfis distintos têm proprietários distintos.

O publicador LPK usa essa fonte em vez de gerar outro plano. Seu cache compara
identidades e o serializador empresta as cores e os catálogos. O Relógio também
retém Rc do mesmo objeto, sem clonar a árvore de cores nos syncs periódicos.
O contrato NativeApp ganhou uma capacidade explícita e um setter de referência;
os modelos antigos conservam seus métodos atuais. O Relógio é o primeiro app
ligado a essa capacidade. Os demais apps e cenas ainda recebem aparência pelo
adaptador e continuam no escopo; esta entrega não conclui o transporte de tema.

Somente o mount da sessão validado vincula a fonte. A capacidade do modelo Rust
manda no host; a propriedade `appearanceSource: native` do manifesto apenas
seleciona o adaptador SDK que não consulta useTheme. Views do host LPK usam seus
fatos locais, mesmo com esse metadado no manifesto, preservando o contrato isolado.
O host descarta theme/scale antigos quando está vinculado e lê a mesma referência
para os estilos dos seus widgets. Uma mudança efetiva acorda/renderiza a view;
assinaturas de aparência sem mudança efetiva são consumidas sem forçar render.
O Relógio ainda tem sua assinatura de preferências para configuração do engine.

Os ícones deixam de vincular locale e preferências de janela: seu contrato é
somente tamanho/args. O SDK deixou de enviar scale na sessão porque nenhum app
Rust o consome; o host isolado preserva esse campo. Não há timer de aparência,
consulta RPC de cores por frame ou snapshot de perfil por view.

Os testes novos cobrem compartilhamento e invalidação, perfis separados, vida
útil fraca e o host emprestando a referência sem theme no input. O teste gráfico
`native_appearance_runtime`, incluído no CI GPU=1 com capturas, usa o manifesto
compilado real do Relógio, congela o espelho JS, exige ausência de theme/scale e
nenhum novo fato JS na troca de cor, injeta fatos antigos e confere pixels verdes
produzidos pela preferência atual. O caminho legado de requests do app foi
preservado: só substitui sua referência por um contexto local quando ele existe.

Validação em Fedora 44 privado em 2026-11-22: passaram 656 testes da shell,
com sete ignorados de integrações específicas, cinco testes do app Relógio e
oito contratos LPK. Os três testes novos verificam estado compartilhado,
perfis/vida útil e a ligação do host sem cópia do plano nos fatos.

Passaram 48 cenários gráficos de aparência nativa, locale, agendamento, foco,
configurações, notificações, Dock, Arquivos, cabeçalhos e apps isolados, além
do encerramento instalado por timeout/SIGTERM. Capturas de aparência em
claro/escuro e 640/1000 px geradas e variantes mínimas inspecionadas; a integração
exige pixels da cor verde do proprietário após injetar dados vermelhos antigos.
As oito capturas de Arquivos são idênticas byte a byte às de
`resident-theme-tokens`. Compositor GPU=1, bundles default/app-host e instalação
privada gerados. Evidências em `build/validation/native-appearance-source/`;
sem instalação ou reinício da sessão física.

Três amostras de ociosidade passaram com limites impostos: 12,550 poll/s e 104,992 MiB de RSS, abaixo de 15 poll/s e 120 MiB. Sem prova de ganho causal nem medição física; o protocolo em hardware continua para o encerramento.
