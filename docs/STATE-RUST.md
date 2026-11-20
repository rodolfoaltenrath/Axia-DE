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
`desktopCount`/`themeDesktops`; apenas patches de preferências e o snapshot
necessário à troca de contexto saem do serviço.

O observador sem interface `shell:desktops` é montado pelo root do SDK,
independentemente do indicador visual do tema. Revisões agrupam alterações
da quantidade; valores iguais não geram nova gravação e não existe polling
ocioso. O serviço sobrevive à remontagem do observador. O JS grava os patches;
o caminho local de compatibilidade dos `.lpk` permanece separado no build.
Animação, efeitos finais e o espelho legado seguem para as próximas etapas.

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

`sdk/notify.js` fornece metadados e preferências brutas; executa patches e
callbacks genéricos. O Rust consulta permissões e firewall diretamente nos
armazenamentos do host. Os stores públicos recebem
snapshots por revisão para compatibilidade, sem alimentar a view nativa.
O adaptador descarta snapshots com revisão anterior ou igual à já aplicada;
efeitos atrasados da view não podem ressuscitar itens removidos por um comando.
Referências de callbacks são limitadas aos banners/histórico mantidos, com
liberação após drenar efeitos. Geometria do mount segue tema, área útil,
direção e posicionamento. O host LPK mantém seu serviço local sem acessar o
controlador privilegiado. Configurações, permissões e os
consumidores dos stores continuam como pendências da migração completa.

### Repetição do toque de alarmes

O pedido interno de toque só aceita o app Clock. Um banner entregue e audível
recebe um prazo monotônico de 60 segundos e um próximo pulso a cada 2,5 segundos.
Não Perturbe, estilo sem banner, recusa, silêncio e firewall impedem o início;
mudanças de preferências cancelam o toque ativo. A agenda DND é reavaliada com
a hora local nos pulsos, inclusive quando cruza meia-noite. O adaptador publica
as preferências brutas e a identidade do app; o Rust consulta a permissão atual
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
A persistência das configurações e metadados de catálogo
e o snapshot de configurações ainda passam pelo adaptador; a migração completa
de `settings.js` permanece pendente. O host `.lpk` conserva as funções locais.

`notifications_runtime` modifica os padrões JS durante uma consulta para provar
que a sessão usa o Rust, verifica a edição sem perder opções e exige transporte
de preferências brutas. O contrato isolado mantém a compatibilidade legada.
Logs e capturas desta etapa: `build/validation/native-notification-preferences/`.

## Consulta nativa de permissões e firewall

`notifications/access.rs` lê `perm_get` e `restrict_blocks` pela ABI do host,
sobre os mesmos armazenamentos usados pelos controles de privacidade. A entrega
e a consulta de opções não aceitam estado de permissão ou bloqueio calculado
pelo JS. A identidade ainda chega do catálogo transitório: pacote, isolado ou
app interno. O Rust deriva o escopo `lpk:<id>` ou `lordos` e preserva as exceções
de avisos de Configurações, notificações sem app e do namespace Linux, cuja
verificação ocorre na entrada D-Bus.

O toque conserva essa identidade no proprietário do contexto e reconsulta a
permissão/firewall nos pulsos e antes de reproduzir uma tentativa enfileirada.
Revogar uma permissão sem alterar `settings` cancela o toque; reautorização não
retoma um alarme já cancelado. Espelhos/avisos de revisão no SDK e a migração
do catálogo continuam pendentes. A gravação de notificações também migrou,
conforme a etapa abaixo.

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
aviso podem alterar configurações: nesse caso, o adaptador pede uma nova
mesclagem ao Rust antes de aplicar o patch, preservando as alterações recebidas.

A entrada pública `lord.perms.set` para notificações delega ao mesmo Rust por
`axia_notification_permission_set`, incluindo os pedidos do painel Configurações.
As demais permissões conservam sua implementação anterior. O host `.lpk`
mantém o contrato do SDK. Avisos de revisão, persistência de settings, catálogo
e demais consumidores JS continuam pendentes da migração completa.

Testes unitários verificam validação, falha sem revogação, transição para `ask`
e cancelamento do toque. A integração usa os armazenamentos reais, impede
chamadas aos setters JS pelo SDK e exige revisões corretas, ausência de revisão
extra para mudanças de som e preservação de alterações feitas por assinantes.
Logs e capturas desta etapa: `build/validation/native-notification-permission-write/`.

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

O efeito contém somente alterações das preferências. Antes de persistir, o
adaptador fornece os pares ordenados atuais a `memoryMerge`, que aplica as
alterações no Rust e limita a 200 chaves. Isso preserva a ordem de uso e uma
edição de preferências feita depois da coleta. O formato `windowPlaces` no
disco permanece o mesmo. Configurações e sua gravação continuam no adaptador
JS até a migração desse serviço. Live, greeter e processos de apps isolados
não ativam a coleta.

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
entre controlador e view. O serviço retém somente um leitor do modelo;
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
adaptador `sdk/sessionrestore.jsx` persiste preferências, abre apps, aplica
efeitos ao espelho legado e executa a ação fechada de política de navegador.
Mantém apenas um booleano de apresentação para a camada modal; a interface
oculta não bloqueia o teclado e a visível libera o foco de campos inferiores.
Configurações, efeitos e espelho JS ainda aguardam as próximas etapas.

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
