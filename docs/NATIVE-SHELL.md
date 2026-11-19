# Shell nativo em Rust

O objetivo é retirar QuickJS e React da sessão. Reescrever um interpretador
JavaScript não faz parte dessa migração: cada interface passa a usar a cena
Rust já existente, até não restarem consumidores do motor.

## Estado validado

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

## Notificações da sessão — 2026-11-19

`NotificationBanner` usa sempre o host de views Rust na sessão. A preferência
antiga `notificationsNativeView` não escolhe mais um renderer. A apresentação
React foi retirada de `sdk/notify.js` e mantida em `runtime/lpk/notifications.js`,
apenas para o build do host isolado, assim como o menu de compatibilidade.
O teste Rust de metafile exige zero bytes desse renderer no bundle da shell e
presença no host `.lpk`.

A lista, a política de agrupamento, as permissões, os timers e as ações de
`notify.js` não foram reescritos nesta fatia. A descrição da árvore ainda vem do
adaptador JS e é desenhada pelo host Rust. A Fase 2 deve levar o estado e os
serviços para Rust; não se declara aqui uma sessão sem JavaScript.

O driver Rust exige `notificationsNativeView=false` e exercita dois botões sem
duplicar ações, ação pelo corpo, fechamento, reapresentação do item anterior de
um grupo, Não Perturbe com histórico, expiração e posicionamento inferior em
RTL numa área de trabalho deslocada. Executa os quatro casos claro/escuro em
640/1000 px e salva PNGs com os tokens reais do tema, efeitos desligados:

```text
AXIA_NOTIFICATIONS_OUTPUT=/var/tmp/axia-notifications cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test notifications_runtime -- --ignored --nocapture
```

O CI GPU=1 exige essa integração e publica `axia-notifications`. As capturas e
logs locais ficam em `build/validation/native-notifications/`. O suporte Rust
de processos, permissões da instalação privada, coleta e PNG foi compartilhado
com o teste de menus; essa regressão também foi executada. Não houve novo RPM
completo nem novas medições físicas nesta fatia.

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
