# Shell nativo em Rust

As configurações têm proprietário observável Rust por perfil. O contexto
publica suas revisões e as do idioma pelo loop nativo, sem view observadora
ou polling; `SettingsSession` e `shell:settings` foram retirados. O SDK mantém o espelho por revisão
para os consumidores de compatibilidade, com referências estáveis para blocos
inalterados. Padrões, migrações, carregamento, prazo de gravação e serialização
são nativos. O prazo participa do loop mesmo sem views; o arquivo usa as
primitivas atômicas/duráveis C existentes. Contratos em [STATE-RUST.md](STATE-RUST.md).

A restauração da sessão também consulta esse proprietário no bootstrap e nos
comandos, sem reenviar o perfil/revisão pelo SDK. Opções, autosave e marcadores
de saída/retomada/troca de tema são gravados no Rust; os marcadores usam a
gravação durável antes de devolver os efeitos. Snapshots de compatibilidade
atualizam o espelho público, sem executar outro save JS. A abertura de apps,
política de navegador e metadados de geometria do tema continuam no adaptador.

Desktops e restauração são atendidos diretamente pelo loop Rust, com referências
fracas ao contexto e prazo nativo único. A confirmação não agenda o serviço;
o observador invisível de desktops foi retirado. O adaptador ainda drena efeitos
de compatibilidade pelo controlador validado, mesmo sem nenhuma view montada.
Relógio e notificações também são atendidos pelo loop Rust: não existe mais
observador invisível do relógio, e o banner não agenda expiração nem toque.
A memória de posições também pertence ao contexto e é atendida por esse loop.
O observador do tema foi retirado; catálogo e telas físicas vêm de leitores
Rust compartilhados. O compositor publica mudanças de telas antes de atualizar
o JS. A restauração ainda recebe áreas e opções do tema pelo adaptador.

Desktops e memória de posições também usam esse perfil: bootstrap, contagem,
troca de tema e movimento reduzido são consultados/persistidos no Rust. Abertura
de janelas empresta somente a posição solicitada, sem transportar a lista de
posições pelo JS. Mapa e ordem de uso são gravados numa única revisão, com limite
de 200 apps. Metadados de telas/área útil e o espelho público ainda usam adaptadores.

A paleta da shell e os materiais da Dock são resolvidos por `shell_theme.rs`,
com as primitivas de vidro do compositor. O adaptador `themes/default/tokens.js`
envia somente modo, destaque e efeitos quando mudam, preserva a identidade do
resultado entre renderizações e associa o catálogo compartilhado de papéis de
parede. Cores e regras de transparência deixaram o JavaScript. A integração do
portal também pertence ao serviço residente: lê modo, destaque e disposição
dos botões por empréstimo e aplica diretamente no backend C, sem provider ou
snapshot JS. O efeito `syncPortal` e a ponte `lord.portal.set` foram removidos.
Montagem do tema e publicação para apps isolados ainda usam adaptadores.

O objetivo é retirar QuickJS e React da sessão. Reescrever um interpretador
JavaScript não faz parte dessa migração: cada interface passa a usar a cena
Rust já existente, até não restarem consumidores do motor.

## Foco das janelas nativas

O conteúdo dos apps nativos usa o leitor tipado da lista de janelas do seu
contexto para decidir quem recebe teclado. `NativeView` fornece identidade
e bloqueios de entrada para campos JS e painéis da shell. O foco não depende
da atualização do espelho JS, nem exige reenviar fatos à view.
`native_focus_runtime` verifica esse caminho em claro/escuro e 640/1000 px;
capturas locais em `build/validation/native-focus/`, artefato CI
`axia-native-focus`. Estado e limitações em [STATE-RUST.md](STATE-RUST.md).

## Estado validado

O painel Forçar encerramento lê a lista de janelas por referências Rust,
vinculadas ao modelo do seu contexto. Nomes e ícones vêm do catálogo de apps;
não recebe uma cópia JSON das janelas. `forcequit_runtime` verifica mutações
diretas com espelho JS parado e remoção da seleção antes de uma ação, em
claro/escuro e 640/1000 px. Artefato CI: `axia-forcequit`; captura local:
`build/validation/forcequit/`. Os efeitos de encerramento e outros consumidores
de `appcontrol.js` continuam no adaptador transitório.

O controlador da sessão que resolve `groups`/`quit` também lê o modelo Rust.
`appControlFacts()` foi removido; os pedidos já não incluem a lista de janelas.
Metadados de apps são compartilhados por `nativecatalog.js` e enviados uma vez
por alteração de catálogo/idioma. A integração verifica superfícies atualizadas
e fechamento de janelas ausentes do espelho JS. A execução final dos efeitos
continua em JS, assim como o adaptador de objetos de `runningApps()` para o
alternador e demais consumidores legados.

- Monitor: árvore, rasters, entrada e atualização periódica nativos.
- Calculadora: árvore e ícone nativos, cálculo por teclado e ponteiro, foco,
  separador decimal e feedback de pressão/hover em Rust.
- Force Quit e Sobre: árvores e ícones nativos; o adaptador React de widgets
  desses apps foi removido. Efeitos ainda usam os serviços da sessão.
- Host compartilhado: reconciliação por chave, botões, campos, rolagem,
  eventos de ponteiro em views e teclado de aplicações. Teclas só são consumidas
  mediante resposta explícita do app; atalhos do shell mantêm a precedência.
- Dock: ordenação visual, separadores, espaço de arraste, ampliação, RTL,
  posicionamento por monitor e coordenadas estáveis de drop em Rust, com
  96 referências de geometria e integração nas três posições. Ícones do lançador e da lixeira nativos, com os dois temas e estados
  vazio/cheio. A geometria foi comparada com 15 árvores capturadas do JSX anterior.
- Ícones: geometria dos apps compilados e de Arquivos, imagens do catálogo e
  fallback de inicial sobre gradiente podem ser montados pelo host Rust.

A moldura das janelas, Dock, busca e ajustes ainda usam os adaptadores
JavaScript/React. O host de apps `.lpk` também executa JavaScript. Portanto,
QuickJS **ainda é uma dependência necessária** e não foi retirado do build.

## Menus da sessão — 2026-11-19

A barra de menus recebida já usava o caminho nativo definitivamente. Os menus
de contexto agora também usam `NativeMenu` sem consultar `menuNativeView`;
preferências antigas não reativam o renderer React. O adaptador conserva
callbacks e repassa a camada solicitada ao host Rust.

O contrato dos `.lpk` exige manter o componente `Menu` no processo dos apps.
Seu renderer React fica em `runtime/lpk/menu.jsx`. O build do app-host define
`__AXIA_APP_HOST__=true`; o da shell define `false`. A pasta de compatibilidade
declara somente exports sem efeitos de inicialização para permitir a exclusão
completa do módulo não usado. O teste Rust inspeciona o metafile real do esbuild:
zero bytes desse renderer no bundle da shell e contribuição presente no host.
Isso separa apenas esse componente, não todo o SDK nem os serviços da sessão.

Testes do contrato e da separação de bundles:

```text
cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test lpk_api -- --include-ignored
```

Com `axia-server`, `axia-media` e os pacotes npm compilados, o driver Rust
executa a sessão real privada em claro/escuro, com larguras de 640 e 1000 pixels.
Define deliberadamente a preferência antiga como `false` e verifica cliques,
itens desabilitados, seleção por teclado, submenu, Escape e clique externo.
Gera uma captura PNG por combinação e preserva logs quando indicado:
As capturas usam os tokens reais do tema padrão, com efeitos desligados para
manter a comparação em software determinística.

```text
AXIA_MENUS_OUTPUT=/var/tmp/axia-menus cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test menus_runtime -- --ignored --nocapture
```

O CI GPU=1 exige essa integração e publica o artifact `axia-menus`. As quatro
capturas locais ficam em `build/validation/native-menus/`, ignorado pelo Git.
As asserções e a gestão dos processos são Rust; o fixture JavaScript apenas
aciona a interface existente. Não foi executado um novo RPM completo nesta
fatia. Passaram 47 testes leves, dois contratos Rust/Node e a integração real
com quatro casos e capturas. Métricas físicas e avaliação final de desempenho ficam para o encerramento
do roadmap, por decisão do usuário; o orçamento existente continua no CI.

## Agendamento e indicador do relógio — 2026-11-20

O root monta `shell:clock` independentemente da barra do tema. O engine Rust
invalida seus assinantes após comandos e mudanças de efeitos; o observador
agenda o próximo vencimento no host. A sessão não arma o timeout JS de
`alarms.js` para esse trabalho. O engine e os vencimentos sobrevivem à
remontagem das views.

O engine usa o leitor de configurações do perfil privado. Alarmes e cidades
são gravados diretamente no proprietário Rust, sem efeitos `settings` pelo JS.
Uma assinatura nativa acorda o agendador quando a configuração muda; comandos
da sessão não transportam `config`, e a lista pública de alarmes consulta o engine.
O host isolado conserva o contrato anterior. Validação e limites em `STATE-RUST.md`.

O item da barra monta `shell:clock-status`: texto, visibilidade e aba de destino
são calculados no Rust, com símbolos vetoriais para evitar dependência de glifos
emoji. A contagem tem atualização nativa apenas enquanto o timer executa; o
estado não entra nos fatos JS. Templates e ações Parar/Adiar/Repetir também
pertencem ao serviço Rust, inclusive no histórico. Repetir usa a duração do
engine atual; a entrega consulta a regra atual do firewall por API nativa.
Padrões e opções de notificações por app são resolvidos no Rust. Entrega,
agrupamento e toque consultam o leitor nativo do perfil, sem preferências nos
pedidos de entrega nem agrupamento nos fatos da view. O serviço atualiza o registro
de apps vistos no proprietário de configurações. As consultas e edições do SDK
também leem/mesclam/gravam no proprietário, sem transportar preferências nem
reaplicar patches em JS. A identidade de acesso vem do leitor compartilhado do
catálogo do controlador, sem `source`/`notificationPolicy` nos pedidos. Mudanças
do catálogo invalidam a política de toque sem alterar settings. O adaptador
ainda importa o catálogo e publica snapshots para os controles legados.
Nome, seleção do ícone e horário dos banners também são resolvidos no serviço
Rust. Timestamp e minuto de entrega vêm do relógio nativo; nome e horário
calculados pelo JS não entram no pedido. Idioma/região/12–24 h salvos no perfil
têm prioridade. A fonte nativa `locale.rs` seleciona o idioma inicial e controla
recargas de catálogos; notificações/alarmes não recebem contexto de locale pelo
SDK. O store público de i18n espelha essa fonte por revisão, pelo observador
`shell:settings`; idioma e direção são aplicados diretamente pela ABI C no
contexto ativo. Leitores de outros perfis não alteram a interface. Updaters
consultam o estado nativo e respostas antigas são descartadas. Tradução e nomes
de apps do SDK consultam o resolvedor compartilhado Rust com o idioma atual;
formatos de data/hora/número, nomes de meses/dias e lista de idiomas também usam
esse proprietário, com caches invalidados por revisão e sem catálogos no JS.
Views da sessão recebem um leitor/assinatura nativos desse proprietário: idioma,
região, direção, separadores e revisão não cruzam o NativeBridge em JS. O host
acorda por invalidação, inclusive sem timer do app, e ignora fatos de locale
antigos. Separadores usam o cache compartilhado do perfil. A ABI de montagem da
sessão exige o proprietário do contexto validado pelo adaptador; apps isolados
continuam no caminho de fatos locais. Preferências das views também vêm do
proprietário Rust: NativeBridge não envia `settings` nem `preferenceRevision`
na sessão. Somente modelos interessados assinam as mudanças; o sync recebe
uma referência ao perfil, liberada antes de executar efeitos de persistência.
Relógio guarda região/formato de hora; Monitor guarda seu bloco; Configurações
mantém apenas os blocos editáveis e os valores pendentes. Não há cópia do perfil
completo por view. Bootstrap de serviços, tema e escala ainda têm consumidores JS.
`native_locale_runtime` cobre quatro variantes de tema/tamanho, com setters JS
de texto e leitores legados de formatos bloqueados; o CI GPU=1
publica `axia-native-locale`. A integração também congela o espelho JS de
preferências e exige que o Relógio alterne entre 12/24 h sem novos fatos.
Evidências mais recentes em `build/validation/native-view-preferences/`.
O Rust consulta os armazenamentos nativos de permissões e firewall na entrega
e nos pulsos; revogar uma permissão cancela o toque sem atualização de settings.
A gravação de permissões de notificações, validação de ID e revogação aos apps
também ficam no Rust, tanto pelo SDK quanto pela entrada pública de permissões.
O aviso de revisão do SDK continua como adaptação para os controles legados.
O serviço chama o cliente de áudio diretamente pela ABI nativa; não há efeito
de som na ponte JS nem callback de reprodução para alarmes/notificações.
A repetição do toque já pertence ao
serviço nativo de notificações: cadência e prazo monotônicos, cancelamento por
política/retirada e validação de pulsos enfileirados antes do som.

`native_clock_runtime` verifica os quatro cenários claro/escuro e 640/1000 px:
fonte opaca, ausência de estado nos fatos, espelho JS parado, contagem,
remontagem, vencimento único, clique na aba correta, indicador de alarme,
templates, Repetir com espelho incorreto e firewall alterado durante a espera.
O teste recusa o timeout de agendamento e intervalos JS durante a observação;
timers independentes de persistência de preferências continuam permitidos.

```text
AXIA_NATIVE_CLOCK_OUTPUT=/var/tmp/axia-native-clock cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test native_clock_runtime -- --ignored --nocapture
```

O CI GPU=1 exige essa integração e publica `axia-native-clock`. Logs e capturas
locais ficam em `build/validation/native-clock/`. Contratos e pendências em
`STATE-RUST.md`.

## Notificações da sessão — 2026-11-19

`NotificationBanner` usa sempre o host de views Rust na sessão. A preferência
antiga `notificationsNativeView` não escolhe mais um renderer. A apresentação
React foi retirada de `sdk/notify.js` e mantida em `runtime/lpk/notifications.js`,
apenas para o build do host isolado, assim como o menu de compatibilidade.
O teste Rust de metafile exige zero bytes desse renderer no bundle da shell e
presença no host `.lpk`.

Na Fase 2, lista, histórico, agrupamento, política de entrega e prazos passaram
ao serviço `notifications.rs`, pertencente ao contexto. A view
`shell:notifications` lê esse estado por empréstimo e monta a árvore no Rust;
não recebe lista nem árvore JS. A expiração usa o relógio monotônico e um prazo
nativo, sem timer JS ou polling quando não há banners temporários. Remontar a
view preserva os IDs e os prazos.

O adaptador ainda consulta permissões/firewall, fornece preferências e metadados
traduzidos, executa som e callbacks dos apps e mantém stores de compatibilidade.
Esses stores não escolhem o que a view apresenta. O renderer e os timers locais
do host `.lpk` continuam isolados da autoridade da sessão. Detalhes em `STATE-RUST.md`.

O driver Rust exige `notificationsNativeView=false` e exercita dois botões sem
duplicar ações, ação pelo corpo, fechamento, reapresentação do item anterior de
um grupo, Não Perturbe com histórico, expiração e posicionamento inferior em
RTL numa área de trabalho deslocada. Também verifica fonte opaca, ausência de
árvore/lista nos fatos, expiração sem armar timer JS, notificação nativa ausente
do espelho e prazo preservado após remontar a view. O toque é observado com
intervalos JS bloqueados, remontagem durante o alarme, retirada do banner e
silêncio ativado enquanto toca. Executa os quatro casos claro/escuro em
640/1000 px e salva PNGs com os tokens reais do tema, efeitos desligados:

```text
AXIA_NOTIFICATIONS_OUTPUT=/var/tmp/axia-notifications cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test notifications_runtime -- --ignored --nocapture
```

O CI GPU=1 exige essa integração e publica `axia-notifications`. As capturas e
logs locais ficam em `build/validation/native-notifications/`. O suporte Rust
de processos, permissões da instalação privada, coleta e PNG foi compartilhado
com o teste de menus; essa regressão também foi executada. Não houve novo RPM
completo nem novas medições físicas nesta fatia.

## Dicas e arraste da Dock — 2026-11-19

As dicas e o fantasma de arraste usam sempre `NativeDockTip` e
`NativeDockGhost`, sem consultar `dockNativeView`. Os dois renderers React
substituídos foram removidos de `Dock.jsx`. O fantasma resolve IDs tipados
`app:` e `lpk:` para a chave do catálogo de ícones e preserva o namespace
`desktop:`. A resolução é memorizada por ID, sem nova consulta a cada movimento.
A montagem da barra, prévias, estado e eventos ainda usam JS/React; esta entrega
não conclui a migração da Dock inteira.

O driver Rust define a preferência antiga como `false`, verifica as dicas nas
posições inferior, esquerda e direita, preservação de item fixo, cancelamento
com Escape, remoção por arraste e ausência de abertura acidental ao soltar.
Também verifica os IDs enviados ao renderer e salva o fantasma com indicação
de remoção em claro/escuro, 640/1000 px, usando os tokens reais do tema:

```text
AXIA_DOCK_OUTPUT=/var/tmp/axia-dock cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test dock_runtime -- --ignored --nocapture
```

O CI GPU=1 exige a integração e publica `axia-dock`. As quatro capturas e logs
locais ficam em `build/validation/native-dock/`. A validação usa o suporte Rust
compartilhado com menus e notificações. Medições físicas continuam reservadas
para o encerramento do roadmap.

Validação desta fatia: build da interface, 47 testes leves, dois contratos de
compatibilidade e as integrações reais de menus, notificações e Dock passaram
(quatro combinações de tema/largura por integração). Sem novo RPM completo.

## Spotlight definitivo — 2026-11-19

O painel usa sempre o host de árvore Rust, sem consultar `spotlightNativeView`.
O renderer de widgets React foi removido de `sdk/rustsearch.jsx`. O adaptador
envia o catálogo de apps para os ícones das linhas e da prévia, preserva os
provedores e encaminha ações e modificadores de teclado à sessão de busca.
Abrir a busca guarda e suspende o foco do campo JS anterior; fechar devolve
esse foco, impedindo que a digitação na busca altere o campo atrás do painel.

O novo driver Rust verifica abertura de apps, cálculo, setas, Tab entre seções,
ação de provedor por clique, Escape, clique externo, restauração de foco e
limpeza dos clientes. Define a preferência antiga como `false` e salva quatro
capturas em claro/escuro, 640/1000 px, com os tokens reais do tema:

```text
AXIA_SPOTLIGHT_OUTPUT=/var/tmp/axia-spotlight cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test spotlight_runtime -- --ignored --nocapture
```

O CI GPU=1 exige a integração e publica `axia-spotlight`. As capturas locais
ficam em `build/validation/native-spotlight/`. O lookup somente de leitura
`lord.nativeViews.hitTest(x, y)` permite que o driver use o alvo real da cena
para cliques por coordenada, inclusive no fundo externo do painel; o restante
da entrada continua pela mesma ponte usada pelo compositor.

A regressão residente `search_runtime` também foi adaptada para observar
textos e geometria do host nativo, sem depender de chamadas React `setText`.
Mantém as verificações de arquivos, fotos, notas, ajuda, clipboard, provedores,
reveal, liberação de imagens e 160 ciclos sem crescimento contínuo de memória
ou polling após fechar. Estado, timers, provedores e efeitos ainda usam o
adaptador JS; QuickJS continua necessário. Ícones arbitrários fornecidos por
funções JS de provedores usam o placeholder já existente no host nativo;
um contrato de recursos serializáveis continua pendente na migração do SDK.

Validação desta fatia: build da interface, 47 testes leves, dois contratos,
regressão residente de busca e quatro cenários de cada integração de menus,
notificações, Dock e Spotlight passaram. Não houve novo RPM completo nem
medição em hardware real.

## Launcher definitivo — 2026-11-19

`RustLauncher` monta sempre `shell:launcher`; a preferência `launcherNativeView`
e o renderer React de widgets foram removidos. O host mantém a altura completa
da grade, mas solicita ao modelo apenas as linhas do viewport e a margem de
uma linha em cada extremidade. O limite fixo anterior de 16.384 px não restringe
mais os apps alcançáveis. Roda, touchpad e arraste da barra notificam o mesmo
callback Rust `onScroll`; entradas já limitadas na extremidade não invalidam
a lista novamente. Uma nova consulta/categoria/catálogo descarta o offset antigo.

Mudanças do catálogo também atualizam categorias e conteúdo de um launcher
aberto. A pedido do usuário, o limite horizontal de 780 px foi retirado: a grade
usa a largura da área de trabalho com margens mínimas de 24 px e centraliza
colunas completas. São quatro colunas em 640 px, sete em 1000 px e quatorze
em 1920 px. O menu de contexto aceita setas, Home/End, Enter e Escape,
pula itens desabilitados e
fecha antes do launcher. O clique no fundo emite `cancel`: o nome `close`
pertence ao encerramento do host e antes destruía o modelo sem fechar a UI.
O adaptador suspende o foco de campos JS e impede abertura acidental após drag.

O teste de catálogo existente foi adaptado para observar nós nativos. Mantém
as verificações de 1.029 apps, identidade estável da lista, geometria, cache,
busca/Enter, categoria, menus, fixar/desafixar, início de drag, instalação/remoção
de pacote, idioma, Unicode e redimensionamento. Testes Rust do host exercitam
roda, touchpad, barra e limites; os do modelo cobrem posições do viewport.

O driver adicional usa 2.048 apps com a preferência antiga em `false`, vai até
o fim da grade, verifica o último item também no hit test real e o abre. Cobre
busca, filtros, Escape no menu, fixação por teclado, remoção por clique, catálogo
dinâmico, arraste sem abrir apps, clique externo e isolamento da digitação em
claro/escuro em 640, 1000 e 1920 px, com seis capturas. Em 1920 px os textos
da primeira linha ocupam mais de 85% da largura, verificados nos nós reais:

```text
AXIA_LAUNCHER_OUTPUT=/var/tmp/axia-launcher cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test launcher_native_runtime -- --ignored --nocapture
```

O CI GPU=1 exige essa integração e publica `axia-launcher`; as capturas locais
ficam em `build/validation/native-launcher/`. A abertura de apps, pins e arraste
ainda passam pelos efeitos do adaptador JS. Esta entrega não remove QuickJS
da sessão; medições físicas continuam para o encerramento do roadmap.

Validação: build do compositor e da interface, 104 testes do host nativo,
suíte de modelos do launcher (51 passaram, benchmark ignorado), 47 testes
leves, dois contratos e regressão de catálogo passaram. Passaram também os
seis cenários do launcher e os quatro de cada integração de menus,
notificações, Dock e Spotlight. Nenhum novo RPM completo foi executado.

## Barra de título definitiva — 2026-11-19

`Window.jsx` monta sempre `NativeTitleBar` nas janelas com moldura da shell,
sem consultar `titleBarNativeView`. O renderer React do cabeçalho, seus estados
de hover e a assinatura usada para escolher esse renderer foram removidos.
Os estilos Axia e Adwaita, títulos dinâmicos, foco e callbacks do gerenciador
permanecem. Janelas CSD e apps `ownFrame` continuam sem esse cabeçalho.

Na Fase 2, título e foco desse cabeçalho passaram a vir diretamente de
`Session.reader()`. O mount recebe e valida a lista de janelas do seu contexto,
sem reter ponteiro C ou objeto JS. A assinatura pede sincronização imediata no
loop nativo quando há revisão pendente e não mantém timer ocioso. O adaptador
envia aparência e nome localizado de fallback, sem título/foco dinâmicos no
JSON. A integração altera o modelo sem atualizar o espelho JS e exige o título
novo na cena. Propriedade, cancelamento, foco com fatos discordantes e rejeição
de fonte opaca inválida também são verificados; detalhes em `STATE-RUST.md`.
Moldura, efeitos e demais consumidores de janelas continuam pendentes.

Uma regressão Rust com o host real reproduziu um duplo clique num controle
emitindo duas ações e uma maximização indevida. Cada controle agora intercepta
o duplo clique; somente o fundo do cabeçalho executa a maximização. O teste
também verifica que os controles não iniciam arraste e que o duplo clique no
cabeçalho emite exatamente uma maximização.

A integração abre janelas pelo gerenciador real, usa os alvos do hit test da
cena e cobre foco, atualização do título, arraste, maximizar/restaurar pelos
controles e por duplo clique, minimizar/reabrir, guarda contra fechamento,
fechamento e rotas CSD/ownFrame. A captura aguarda a nova posição na cena,
além de conferir o estado Rust. Não há substituição das ações do gerenciador
por mocks; o fixture somente observa `beginMove` e chama sua implementação.
Em headless o grab nativo recusa iniciar sem botão físico pressionado: essa
coleta verifica o fallback JS sobre o modelo Rust. O grab físico fica para a
validação final da sessão, junto das medições em hardware adiadas pelo usuário.

São oito combinações dos dois estilos, claro/escuro e 640/1000 px:

```text
AXIA_TITLEBAR_AXIA_OUTPUT=/var/tmp/axia-titlebar/axia AXIA_TITLEBAR_ADWAITA_OUTPUT=/var/tmp/axia-titlebar/adwaita cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test titlebar_runtime -- --ignored --nocapture
```

O CI GPU=1 exige essa integração e publica `axia-titlebar`. As capturas e logs
locais ficam em `build/validation/native-titlebar/{axia,adwaita}/`. O contrato
de bundles Rust/esbuild agora inspeciona também a entrada `Window.jsx`, além
do SDK, exigindo ausência das chaves já retiradas nas duas entradas.
A moldura, o AppHost, o fallback de arraste e os efeitos ainda passam por JS;
isso continua no escopo das fases seguintes.

Validação desta fatia: compositor e interface compilados; 106 testes do host
nativo, 47 testes leves, dois contratos e os oito cenários com capturas passaram.
Não houve novo RPM completo nem validação de grab em sessão física.

## Sequência de retirada

1. Completar widgets e integração nativa e migrar os consumidores do host React.
2. Migrar as superfícies do shell, preservando camadas, foco, atalhos,
   vários monitores, arraste, menus e acessibilidade.
3. Substituir os efeitos e serviços intermediados pelo JS por interfaces Rust.
4. Converter os apps restantes e o contrato de pacotes; remover o motor,
   reconciliador, ferramentas e testes que deixarem de ter consumidores.

Cada etapa deve funcionar antes da remoção do caminho anterior. Não há
compromisso de compatibilidade binária entre bytecode QuickJS e apps Rust;
a transição de pacotes precisa ser implementada antes da exclusão do motor.

## Verificação

`cargo test --manifest-path rust/Cargo.toml --locked --offline --lib` cobre os
controladores e a cena Rust. `native_apps_runtime` é a integração com o compositor
headless, teclado real da ponte, remontagem e ausência de textos React na
Calculadora e no Monitor. Os testes do host Rust também exercitam cliques,
limites entre ancestrais no hover, liberação fora do controle e atalhos/foco.

## Arquivos definitivo (2026-11-19)

O navegador e o ícone de Arquivos montam sempre `FilesView`, independentemente
do valor antigo de `filesNativeView`. O renderer React do navegador foi
removido. Os seletores compartilhados e a janela de transferências ainda
dependem de `sdk/rustfiles.js` e estão explicitamente pendentes na Fase 3.
Os efeitos assíncronos comuns ficam em `sdk/fileshost.js`, sem importar esse
renderer; a sessão ainda usa JS para executar efeitos até as próximas fases.

A integração `rust/xtask/tests/files_runtime.rs` usa o manifesto compilado e
o mount real, com uma pasta privada. Cobre preferência antiga falsa, ícone,
navegação por ponteiro, busca, criação, renomeação e bytes preservados,
listagem, cache de miniatura e prévia PDF, menus estáveis durante rolagem,
seletores de abrir/salvar/pasta e notificações de volumes. Somente os fatos e
resultados de montagem/ejeção de disco são simulados; arquivos e helper de
documentos são reais. Nenhum disco da máquina é alterado.

O teste substitui o smoke de Arquivos em JS/Python e preserva também suas
verificações de migração de preferências e cache do plano de aparência. As
asserções, o processo e a inspeção dos PNGs pertencem ao Rust; o fixture JS
somente exercita os adaptadores que ainda existem.

Foi reproduzido um recorte de documentos: o ajuste padrão `cover` escondia o
topo e o rodapé de páginas verticais na área larga de prévia. As imagens de
documentos agora usam `contain`. Marcas conhecidas no centro e nas duas bordas
do PDF precisam aparecer nos pixels da captura, além de a operação retornar
um handle válido. O teste falha no compositor anterior e passa com a correção.

```sh
AXIA_FILES_AXIA_OUTPUT=build/validation/native-files/axia AXIA_FILES_ADWAITA_OUTPUT=build/validation/native-files/adwaita cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test files_runtime -- --ignored --nocapture
```

São oito cenários: Axia/Adwaita, claro/escuro e 640/1000 px. O CI GPU=1 exige
essa integração e publica `axia-files`. Validação com periféricos, sessão
gráfica física e medições continuam para o encerramento, conforme solicitado.

Verificação local desta etapa: compositor e frontend recompilados; 119 testes
do app Arquivos, 106 do host nativo, 47 leves do xtask, dois contratos LPK,
integração dos apps compilados e os oito cenários de Arquivos passaram.
Três testes dependentes do ambiente do crate Arquivos ficaram ignorados;
não houve novo RPM completo nem medição em hardware físico.
