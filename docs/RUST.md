# Rust na Axia DE

O primeiro módulo Rust da DE é o modelo do alternador de aplicativos em
`rust/axia-shell`. Ele é compilado como biblioteca estática e ligado ao
mesmo executável `axia-server` em C. Não há serviço Rust separado.

Este documento descreve a integração inicial e os contratos de cada módulo.
A substituição completa do código próprio do server está em andamento; o
inventário e as validações por etapa estão em [SERVER-RUST-MIGRATION.md](SERVER-RUST-MIGRATION.md).

## Responsabilidades

- Rust mantém o histórico de foco, ordena os candidatos e controla a seleção,
  avanço, retrocesso, cancelamento e remoção de candidatos fechados.
- C oferece uma ponte pequena para QuickJS e continua responsável por input,
  Wayland e demais recursos do compositor.
- O SDK monta a lista de aplicativos e suas janelas, envia identificadores ao
  modelo e aplica o foco escolhido por meio do gerenciador de janelas existente.
- React desenha o seletor usando o mesmo material glass da Dock, compartilhado
  em `sdk/glass.js`. Cores, blur e bordas acompanham o tema; efeitos
  desativados produzem uma superfície opaca.

O comportamento de agrupar janelas por aplicativo foi preservado. Se uma janela
representante fechar e houver outra do mesmo app, ela assume o lugar. Sem outras
janelas, o candidato é removido. Aplicativos em tela cheia ainda têm a limitação
do compositor existente: a superfície de tela cheia fica acima dos overlays e o
alternador não abre nessa situação.

A política anterior de MRU e seleção em JavaScript foi removida, assim como os
handlers e bindings duplicados do alternador nos dois temas. O ponto de entrada
dos atalhos é o dispatcher compartilhado do SDK; o modelo Rust é a única
implementação da seleção. React e C continuam como interface e integração ativas.

## Fronteira entre as linguagens

A interface está em `server/src/ui/switcher_ffi.h` e a ponte em
`server/src/ui/js_switcher.c`. Os identificadores são copiados durante a
chamada. Rust possui seu estado e suas strings; nenhum ponteiro para uma janela,
node ou objeto JavaScript é guardado em Rust.

Cada contexto da sessão possui um objeto `lord.switcher`, cujo finalizador
libera o estado Rust. A API não é registrada nos processos de apps isolados.
O acesso é restrito à thread da interface, com no máximo 512 candidatos e
identificadores de até 1024 bytes.

## Build e testes

O build requer Cargo e rustc. As dependências externas estão fixadas em `rust/Cargo.lock` e incluídas em
`rust/vendor`. A configuração `.cargo/config.toml` direciona o Cargo para essas
cópias: a compilação usa `--locked --offline` e não baixa crates dentro do
builder do RPM. As licenças dos autores acompanham o pacote.

Os testes da biblioteca também precisam das bibliotecas nativas usadas pelos
módulos migrados: GLib/GIO, libxkbcommon e libwayland-server, com seus arquivos
de desenvolvimento para o linker. O teste `build/text-input-ffi-test` usa
libwayland-server real e dois clientes locais por socketpair, sem sessão gráfica.

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline
make -C server build/switcher-ffi-test test-rust
server/build/switcher-ffi-test
node server/tests/shortcut_profile_test.mjs
make check
```

`make check` cobre os testes de política em Rust, o contrato C/Rust, os
grabs dos dois perfis e um teste no compositor headless real. O último testa
MRU, renderização do overlay, cancelamento, confirmação ao soltar o modificador,
fechamento de janelas e troca de perfil ao vivo. Ele não valida teclado físico,
DRM/KMS nem o resultado visual do blur em hardware.

Os próximos módulos devem manter uma fronteira pequena, com responsabilidade
única e testes próprios. A interface de usuário continua no runtime existente;
a migração do núcleo pode prosseguir por módulos.

## Núcleo do runtime: árvore, estilos, layout e imagens

`runtime/` contém a implementação da árvore e dos estilos. Os antigos
`server/src/ui/node.c`, `style.c`, `layout.c`, `transform.c` e `image.c` foram removidos
do repositório e do build. O React continua enviando as mesmas operações.
Medição de texto, shaping e pintura continuam integrados ao renderer C;
árvore, estilos, geometria e gestão/reamostragem das imagens estão em Rust.

Para revisar o código:

- `scene.rs` possui as alocações nativas por RAII, mantém as relações por IDs, valida
  reparentamento e libera subárvores iterativamente. A tabela de views mantém
  os endereços estáveis e a fila de vagas reaproveita o menor ID disponível.
- `style.rs` resolve propriedades por tipos, sem offsets de bytes. Snapshots
  comparam os campos e os bits dos floats; padding e caches derivados não
  participam da comparação. As extensões são alocadas somente quando usadas.
- `damage.rs` mantém as regiões de repintura, incluindo a união de regiões
  sobrepostas e o limite de 16 retângulos.
- `layout.rs` mede e distribui os nós com flexbox, wrap, min/max, porcentagens,
  posicionamento absoluto e RTL. Pilhas explícitas de trabalho substituem a
  recursão; buffers reutilizáveis são reservados antes de alterar a geometria.
  A medição chama os serviços existentes de texto e imagens através de `Metrics`.
- `edges.rs` resolve bordas, margens, padding e cantos físicos/lógicos.
  `transform.rs` contém a matemática afim compartilhada com pintura e hit testing.
- `image.rs` possui as imagens e o cache por caminho, preserva handles durante
  alterações da tabela e integra o desenho ao canvas atual. `image_sampling.rs`
  mantém pixels ARGB premultiplicados, bilinear e médias por área.
- `types.rs` define as views `repr(C)` e `mod.rs` contém a ABI e seus contratos.
  `state.rs` fornece o armazenamento do estado global compartilhado.

Rust possui e libera nós, texto, extensões de estilo, transformações e imagens.
O dono do nó devolve sua referência à imagem ao ser destruído. O cache possui
uma referência independente; imagens geradas de wallpapers, tray e busca saem
do cache quando o último consumidor devolve sua referência. Views C são
empréstimos síncronos na thread da interface:
o renderer pode atualizar geometria e pintura, mas não liberar os dados nem
alterar as relações da árvore. Os blocos `unsafe` nessa fronteira continuam
necessários enquanto layout e pintura usam as views C.

Criação de nós não varre mais toda a tabela desde o início; a contagem de nós
é direta. Repetir um texto idêntico preserva suas alocações e caches. Resetar
a árvore libera também as tabelas, e operações que criariam ciclos são
recusadas. IDs e operações da interface continuam compatíveis.

A integração com as animações nativas preserva os caches em alterações de
opacidade e translação, e marca a geração de pintura quando o conteúdo muda.
O callback `node_destroy_hook` continua disponível para cancelar animações
antes de liberar os nós; resetar a cena também notifica todos os IDs liberados,
para que uma animação antiga não atinja um novo nó que reutilize o mesmo ID.

O layout não limita mais cada contêiner a 1.024 filhos nem a correção da altura
de linhas expansíveis a 16 itens. Todos os filhos são distribuídos e o texto
dessas linhas é medido na largura resultante. Mudanças de fontes ou direção
invalidam também as medidas de texto, em todas as raízes da cena. A colocação
absoluta e a propagação das transformações percorrem a árvore sem recursão.

O workspace dimensiona a fila pelo número de folhas e a pilha de medição pelos
contêineres intrínsecos simultâneos. Uma cadeia de tamanho fixo não precisa de
uma pilha proporcional à profundidade. As reservas são falíveis: se falharem,
a geometria anterior é preservada. A forma da árvore fica em cache até uma
invalidação, evitando essa varredura em animações de posição/opacidade/transform.
As reservas incluem as matrizes dos nós transformados e de seus descendentes.
Ativar uma transformação repete essa preparação sem descartar as medidas.
`layout_root` retorna sucesso/falha ao host: na falta de memória, compositor
e aplicativos adiam a pintura, preservam o pedido de atualização e tentam
novamente. Alocações parciais de matrizes ficam sob propriedade da cena,
sem publicar alterações de geometria ou flags de transformação.

Resetar a cena libera os buffers. Um vetor auxiliar com mais de 16 KiB também
é liberado quando sua capacidade supera quatro vezes o número de nós vivos
na cena. Isso evita reter o pico de uma árvore temporária grande sem descartar
o espaço que outras raízes ainda precisam. Os buffers menores
continuam disponíveis para reutilização; isso reduz a memória reservada pelo
layout, sem garantir que o alocador devolva imediatamente essas páginas ao SO.
Os testes cobrem 8.192 níveis, 1.400 irmãos, mais de 16 itens expansíveis, caches, RTL,
aspecto de imagens, matrizes e falhas de alocação; o Miri usa cargas menores.
O stress de retenção do workspace com 1.400 nós é executado apenas nativamente.
Regressões também verificam min/max após shrink e em linhas que encaixam
exatamente, recuperação após falha parcial de alocação das matrizes e liberação
do workspace excedente. A inversão de matrizes rejeita resultados não finitos
e usa intermediários `f64` se o caminho habitual `f32` transbordar. Ângulos
finitos que transbordam na conversão para radianos são reduzidos módulo 360.
Esses casos grandes e profundos exercitam o layout. `paint.c` ainda usa recursão
e buffers de 1.024 filhos para pintura/hit testing; esses limites só serão
removidos na etapa de migração do renderer. As listas reais continuam virtualizadas.

Os testes novos são Rust e cobrem crescimento/reuso de IDs, endereços estáveis,
reparentamento, ciclos, descarte de subárvores profundas, limites de buffers,
ordem de pintura, invalidação, estilos e regiões de repintura. O teste de ABI
usa Clang para inspecionar o header C existente e verificar tamanho,
alinhamento e offset de todos os campos compartilhados, sem gerar teste C.

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline runtime::
# Requer Clang; também executado no CI Fedora:
cargo test --manifest-path rust/Cargo.toml --target-dir server/build/rust --locked --offline runtime::tests::native_views_match_c_layout -- --ignored --nocapture
# Regressões existentes de layout, texto, pintura e frames de arraste:
make -C server build/engine-test GPU=1
```

Essa etapa migra a árvore, os estilos, o layout, as transformações e a gestão
das imagens. Texto, shaping, pintura e as pontes QuickJS são etapas seguintes; não há promessa de
redução de RAM ou latência apenas pela troca de linguagem.

### Imagens do runtime

O cache usa lookup por hash, sem varrer uma lista ligada a cada acesso. Handles
opacos continuam com endereço estável. O cache e os nós têm referências
independentes, inclusive durante a destruição dos estados locais da thread:
destruir o cache não invalida imagens ainda usadas por nós. Imagens dos temas
continuam em cache; imagens geradas são liberadas após o último consumidor.

São mantidas duas versões redimensionadas por imagem para alternar tamanhos
como dock/top-bar sem recalcular. Uma terceira dimensão reaproveita o buffer
evicto se houver capacidade; reservas são exatas e um buffer com capacidade
superior a duas vezes a nova necessidade é descartado. `managedImageKB`
contabiliza a capacidade dos buffers de pixels, incluindo as duas versões.

Desenhos com opacidade zero ou clip vazio são descartados antes de reservar
buffers. O filtro por área reaproveita médias de footprints idênticos ao ampliar
um eixo e reduzir o outro, sem mudar o arredondamento dos canais. Buffers
reciclados sobrescrevem seus pixels e limpam as margens de contain sem uma
segunda inicialização completa.

Atualizações RGBA/ARGB do mesmo tamanho reaproveitam a imagem e invalidam as
versões redimensionadas. Trocar tamanho ou substituir uma imagem compartilhada
só consome a referência antiga depois de concluir a nova alocação. Falhas
retornam NULL e preservam a imagem anterior; os consumidores C respeitam esse
contrato. Inputs são limitados a 32.768 por lado e 64 megapixels. Arquivos são
regulares, lidos com limite de 64 MiB e dimensões verificadas antes da decodificação.
No preview da dock, uma captura que falhou mantém a imagem anterior, mas não
confirma sua assinatura como atualizada. A próxima tentativa respeita o cooldown
agregado existente, mesmo quando o conteúdo e a imagem anterior são os mesmos.
Se o redimensionamento falhar durante a pintura, os nós que compartilham a
imagem são marcados para outra tentativa, com nova geração de pintura para
invalidar também os rasters de transformações e as camadas de arraste. Isso
não depende de outra interação do usuário nem descarta as medições do layout.

O decodificador PNG/JPEG e a rasterização de fontes agora usam `axia-codecs`,
em Rust, com dependências vendorizadas. A troca de rasterizador pode alterar
a cobertura nas bordas dos glifos. O blit ainda usa o canvas C.
Veja `RUST-CODECS.md` para limites, memória do cofre e testes. Não há fallback para o antigo `image.c`. Os testes Rust
substituem `wallpaper_image_test.c`, removido do build e do repositório.

`image_tests.rs` cobre referências, caches, troca de tamanho, OOM, ordem de
destruição, handles após rehash, clipping e o overflow corrigido ao reduzir
imagens grandes. O teste nativo `image_runtime.rs` liga os consumidores reais
e compara 13 hashes registrados do renderer C antes da remoção, além de um
digest de 90 quadros de amostragem obtido da mesma referência congelada, sem atualizar
resultados esperados a partir da implementação Rust. Ele também verifica PNG,
JPEG, caminhos não UTF-8, arquivos excessivos/truncados, FIFOs, desenhos
invisíveis sem alocação de escalas e vida útil de wallpapers em dois
consumidores. O `make -C server test-platform` executa esse teste após o build:

```sh
cargo test --manifest-path rust/Cargo.toml --target-dir server/build/rust --locked --offline --test image_runtime -- --ignored --nocapture
```

Na fronteira com o renderer, a propriedade de cada nó é transferida de `Box`
para um registro RAII com `NonNull`, antes de publicar qualquer ponteiro.
Isso permite mover os slots sem retaggear a alocação observada pelo C. O dono
reconstitui o `Box` somente na destruição. A tabela publicada usa `Cell` para
atualizar suas entradas sem invalidar empréstimos anteriores da tabela.
Snapshots C de `StyleExt` inicializam apenas `STYLE_EXT_VALUES`: o adaptador
copia esse prefixo para um snapshot inicializado, sem referenciar o cache C
que não foi copiado.

As reservas de metadados e as alocações de nós, texto, estilos e transformações
são falíveis. Uma falha preserva os dados anteriores; a destruição não precisa
de worklist nem de alocação para registrar vagas. Valores sem número ou não
finitos, e dimensões/floats que transbordam `f32`, são rejeitados antes da
alteração. Uma propriedade inválida não modifica overrides ou invalida o
cache existente. Inserções que já correspondem à ordem atual não repintam.

Os contratos de memória rodam no Miri, com proveniência estrita, nos modelos
Stacked Borrows e Tree Borrows. O CI fixa `nightly-2026-11-09`. O teste da ABI
continua sendo nativo com Clang, e dois testes que chamam o parser numérico da
libc rodam nativamente, pois o Miri não interpreta `strtof`. Os casos de stress
usam árvores menores no interpretador e mantêm a carga completa nativamente.

```sh
rustup toolchain install nightly-2026-11-09 --profile minimal --component miri --component rust-src
# Executar fora do repositório para preparar a std sem o override de rust/vendor:
cargo +nightly-2026-11-09 miri setup
# De volta à raiz do repositório:
MIRIFLAGS=-Zmiri-strict-provenance cargo +nightly-2026-11-09 miri test --manifest-path rust/Cargo.toml --locked --offline --lib runtime::
MIRIFLAGS='-Zmiri-tree-borrows -Zmiri-strict-provenance' cargo +nightly-2026-11-09 miri test --manifest-path rust/Cargo.toml --locked --offline --lib runtime::
```

## Aplicativos Linux e associações de arquivos

`applications/` substitui integralmente `apps.c`, `hw/mimeapps.c` e
`hw/defaults.c`. A descoberta e o cache de aplicativos, parsing de `.desktop`,
traduções, ícones PNG, ações, expansão de `Exec`, resolução do aplicativo padrão,
edição de associações MIME e preferência do terminal passam a ser Rust.
Os headers C continuam como ABI para QuickJS e demais consumidores existentes.
Foram removidos os exports antigos de parsing MIME e alocação de texto que já
não tinham consumidores de produção. Os buffers desses arquivos pertencem ao
Rust; a ponte C recebe apenas os resultados necessários à interface.

O launcher continua executando o programa diretamente, sem shell, com o arquivo
como um único argumento. A precedência de associações, os aplicativos internos
`lordos-*.desktop`, os filtros de visibilidade e os identificadores de cgroup
são preservados. O plano de permissões também é construído em Rust; as primitivas
existentes de cgroup, namespaces e Landlock permanecem em `sec/`. Strings,
argumentos e planos são preparados antes de `fork`; o filho não faz alocações
Rust nem acessa o cache da interface.

As gravações de associações e terminal agora respeitam `XDG_CONFIG_HOME`, assim
como a leitura já fazia, com fallback para `~/.config`. A publicação usa arquivo
temporário exclusivo, escrita completa, `fsync` e `rename`. Associações ilegíveis,
maiores que 256 KiB ou acessadas por link simbólico são preservadas. Arquivos
`.desktop` são limitados a 1 MiB e comandos que excedem a capacidade de argumentos
são recusados inteiramente, em vez de executados com argumentos truncados.

Todos os novos testes são Rust, incluindo parsing, buffers da ABI, comandos,
preferências XDG e lançamento em um processo de teste isolado. Os testes C
existentes de ações, padrões e ajustes continuam verificando a ABI e a integração
com as implementações reais de permissões. Eles agora fazem parte de
`test-platform`, usado por `make check` e pelo `%check` do RPM.

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline applications::
make -C server build/apps-actions-test build/defaults-test build/settings-hw-test
make check
```

A validação headless não substitui a sessão física no Fedora 44: launcher,
ações da Dock, “Abrir com”, associações persistentes, terminal preferido e apps
restritos ainda devem ser conferidos antes da publicação final.

## Catálogo e menus de aplicativos

`launcher.rs` e `launcher/` mantêm o catálogo e a grade de apps do Lord Flow.
Rust recebe os metadados brutos do registro e dos aplicativos Linux, aplica
traduções e visibilidade, prepara Unicode, ordena, categoriza e filtra. Também
possui o estado da pesquisa e da categoria, o menu de contexto e as descrições
de widgets. (O menu Iniciar do Lord Classic também era Rust; saiu com o tema.)

Os arquivos `themes/default/components/Launchpad.jsx` e
`themes/classic/components/StartMenu.jsx` foram removidos (e depois o tema Classic inteiro). `sdk/rustlauncher.jsx`
interpreta os widgets e liga eventos e efeitos aos serviços existentes. O SDK
mantém apenas uma cópia dos metadados retornados para assinaturas React e
vinculação dos ícones. Mudanças do registro, idioma ou atualização explícita
dos aplicativos Linux invalidam o catálogo; notificações do mesmo turno são
agrupadas. Abrir o menu, mover o ponteiro e mudar o foco não o reconstroem.

A ponte `js_launcher.c` possui um estado Rust por contexto QuickJS, liberado
pelo finalizador. Recebe JSON limitado a 8 MiB e até 4.096 aplicativos. Somente
metadados são copiados; funções de ícones, eventos e objetos de aplicativos
permanecem no SDK. NFC usa a GLib já ligada ao servidor; caixa, cabeçalhos e
normalização da consulta são preparados em Rust, preservando o contrato de
Unicode e espaços do runtime. Não há dependência nova. A busca Flow continua
ignorando a categoria selecionada; limpá-la restaura o filtro de categoria.
Cada menu tem um identificador isolado e liberado ao desmontar; há um limite
de 16 menus simultâneos por contexto.

O `ScrollView` aceita também uma função de conteúdo com o deslocamento e a altura
medidos. Os menus usam a altura total para manter a barra de rolagem e montam
somente as linhas visíveis com uma linha de margem em cada extremidade. As
posições, tamanhos, materiais e conteúdo dos menus são descritos em Rust.
Os controles compartilhados continuam desenhando inputs, ícones e popups no
runtime existente. Os efeitos acionam os serviços ativos da Dock, preferências,
gerenciador de janelas e sessão. A pesquisa geral do tema clássico usa o serviço
compartilhado `search/`, também em Rust, incluindo pontuação, arquivos, notas e
calculadora. O menu recebe suas seções e decide seleção e apresentação.
Esses serviços e os controles comuns continuam usados em outras partes da DE.

Os testes novos são Rust: incluem cobertura de viewport em milhares de posições,
catálogos grandes, categorias, busca por campo, invalidação, memória da ABI,
transições de estado, isolamento dos menus e permissões das ações de sessão.
O teste de integração também é dirigido e verificado em Rust; seu módulo
JavaScript embutido somente opera a interface React real e relata observações.
Ele usa um HOME e um socket privados e não modifica a sessão do usuário.

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline launcher::
# Depois do build completo, com node_modules/esbuild disponíveis:
cargo test --manifest-path rust/Cargo.toml --target-dir server/build/rust --locked --offline --test launcher_runtime -- --ignored --nocapture
# Medição do modelo/JSON em release; não mede apresentação de frames no hardware:
cargo test --manifest-path rust/Cargo.toml --release --locked --offline launcher::tests::measure_large_catalog -- --ignored --nocapture
```

A integração verifica a grade, identidade estável da lista, geometria,
rolagem, pesquisa/Enter, categorias, contexto, fixar/desafixar na Dock, início
de arraste e instalação/remoção de pacote, mudança de idioma, Unicode, redimensionamento e touchpad.
As coordenadas visíveis são verificadas contra valores fixos da interface validada,
sem carregar os componentes antigos; o limite de nós montados também é verificado.
`AXIA_LAUNCHER_OBSERVATIONS` permite salvar as observações em JSON.
Ele roda também no job Fedora com GPU habilitada; a sessão
de teste usa composição por CPU. A avaliação de fluidez, aparência do blur e
latência no computador físico continua necessária.

## Pesquisa global residente

`search/` possui o motor, normalização, relevância, calculadora/conversões,
estado por cliente, teclado, previews e descrições de widgets da pesquisa global.
`sdk/spotlight.js` foi removido fisicamente. `sdk/search.js` transporta fatos e
executa efeitos nos serviços compartilhados; `sdk/rustsearch.jsx` interpreta
widgets usando os controles e o renderer existentes. O botão da barra e os
atalhos continuam usando a mesma API pública.

Uma thread adormecida por condição executa arquivos, notas, ajuda, previews e
persistência. Consultas substituídas invalidam resultados por geração; fechar
libera o estado visual e cancela a consulta. O catálogo de apps é preparado nas
invalidações, com resultados limitados durante a classificação. O índice de
arquivos fica em disco (até 16 MiB), lido em fluxo; não há lista permanente de
100 mil caminhos na RAM. Miniaturas de até 232 × 160 usam o ciclo de vida
gerenciado do renderer para liberar pixels ao fechar. Não há daemon adicional
nem polling da busca fechada.

A ponte C habilita acesso ao HOME/assets apenas no contexto da sessão; contextos
de aplicativos isolados conservam somente operações puras de busca/calculadora.
O JSON não pode configurar raízes de leitura. Os testes novos são Rust,
incluindo cancelamento determinístico com I/O bloqueado e integração real com
QuickJS/React: botão/atalho, geometria histórica, ações, previews e 160 ciclos
de abertura/busca/fechamento. Limites, comandos de teste e restrições da medição
estão em [SPOTLIGHT.md](SPOTLIGHT.md). A validação física permanece necessária.

## Arquivos e seletores nativos

O explorador e os seletores de abrir/salvar/pasta usam o módulo `files/`.
Rust possui o estado e descreve a interface; a ponte QuickJS e o adaptador de
widgets preservam o runtime da DE. A implementação anterior do app e de
`fsops.c` foi removida; consumidores C mantêm a ABI declarada em `fsops.h`.
Veja [FILES.md](FILES.md) para arquitetura, limites e testes.

## Aparência de aplicativos externos

`appearance.rs` encontra os temas GTK `Axia`/`Axia-dark` nos diretórios XDG
de dados e em `~/.themes`. Sem o tema correspondente, usa Adwaita. O instalador
publica as duas variantes em `PREFIX/share/themes`, onde o GTK pode encontrá-las.
O portal C continua publicando modo, destaque e ordem dos botões por D-Bus;
Rust substitui o código C de descoberta e gravação das preferências GLib.

O módulo respeita `XDG_CONFIG_HOME`, preserva grupos e preferências de outros
aplicativos e troca o arquivo atomicamente, com permissão 0600. Arquivos maiores
que 1 MiB, ilegíveis ou com UTF-8 inválido são preservados. Conteúdo idêntico
não causa escrita. Não há thread adicional, consultas periódicas, alterações em
perfis de navegador ou cópia de pixels por conta dessa integração.

Aplicativos que negociam decoração do servidor usam a moldura da Axia existente.
Os que desenham a própria barra conservam abas, menus e áreas de clique. O tema
GTK 3 adapta seus controles compatíveis; GTK 4/libadwaita e outros toolkits
continuam recebendo as preferências que suportam pelo portal. Não se força
`GTK_THEME`, transparência ou blur sobre o conteúdo de aplicativos externos.

O RPM inclui agora GBM/EGL/GLES (`GPU=1`), permitindo ao compositor existente
usar aceleração e importar buffers de apps externos quando o hardware e o
formato forem compatíveis. A detecção e o retorno à composição por CPU são
preservados. Isso não altera o motor interno dos aplicativos nem garante ganho
em toda carga. Para desativar durante diagnóstico, inicie uma nova sessão com
`LORD_GPU=0`; não é preciso recompilar o pacote.

## Brilho e previews da Dock

`backdrop.rs` integra as texturas de aplicativos externos ao glass com GPU ativa.
O compositor fornece somente amostras reduzidas da região que o blur precisa.
Rust limita o cache a quatro entradas e 2 MiB, invalida pela geração do conteúdo
e combina essas amostras com a transparência da interface antes do filtro.
Os kernels e o desenho do glass continuam em `glass_render.rs`. O callback GL
roda na thread da interface, antes dos workers; os workers somente leem os dados.
As capturas usam leitura completa apenas sob demanda, sem guardar cópias extras
de todas as janelas ou desligar a aceleração. Preferências de barras e temas
internos dos navegadores e do VS Code continuam pertencendo aos aplicativos.

`external_style.rs` faz a integração reversível dos controles de janela dos
aplicativos externos. Firefox e LibreWolf recentes já não renderizam esses
botões com o CSS GTK: um import identificado em `userChrome.css` carrega somente
o estilo da janela do navegador. A opção de carregar esse CSS é habilitada em
`user.js` apenas quando não existe uma recusa explícita do usuário. Os perfis
relativos do Firefox e LibreWolf são descobertos via `profiles.ini`; caminhos
com links simbólicos, arquivos inválidos e alterações nas partes gerenciadas
são preservados em vez de sobrescritos. Há backup inicial dos arquivos editados.

No VS Code (perfil padrão da instalação RPM), um tema de ícones de produto usa
uma fonte COLR original de poucos KiB para os quatro controles. A extensão é
declarativa: não tem `main`, eventos de ativação, scripts nem serviço residente.
Se o índice de extensões do perfil padrão já existe, somente a entrada da Axia
é acrescentada, preservando as demais entradas e metadados. Sem índice, o
editor descobre todas as pastas normalmente na próxima abertura.
Somente a preferência ausente `window.controlsStyle` recebe o valor `custom`,
em bloco JSONC identificado e removível. O tema Dracula, ícones de arquivos e
escolhas explícitas do usuário têm prioridade. Os controles mantêm a posição
à direita, o espaçamento e o hover do VS Code; não se injeta CSS no editor.
A cor da barra segue o modo da Axia na próxima abertura/releitura de extensões.

A opção Ajustes → Aparência → Estilo Axia nos aplicativos externos desfaz os
imports e os defaults que ainda pertencem à Axia. Os arquivos inertes do tema
do VS Code permanecem disponíveis para não quebrar fontes de janelas abertas;
sem os defaults, deixam de selecionar o estilo na próxima abertura. Feche e
reabra o aplicativo para aplicar ou desfazer. Instalações com outro diretório
de extensões, perfis externos/absolutos, Flatpak e temas de produto escolhidos
manualmente não são forçados. O adaptador roda ao iniciar a sessão e ao mudar
essa opção ou o modo claro/escuro, nunca por quadro. O compositor, o glass e os
caminhos de entrada/captura permanecem iguais aos da correção anterior.

`brightness.rs` identifica os controladores em `/sys/class/backlight` e aplica
valores pela sessão do `systemd-logind` (`SetBrightness`). Não usa o helper
administrativo antigo nem executa comandos externos. Uma thread recebe apenas
o último valor pendente; nenhuma leitura de sysfs ou chamada D-Bus bloqueia a
interface. O SDK agrupa movimentos do slider em 70 ms e consulta o estado apenas
enquanto o painel ou a página de Ajustes estiverem abertos. Sem controlador
compatível, o slider fica desativado, como em monitores de desktop sem backlight
controlável por essa interface. Valores enviados ao hardware nunca chegam a zero.

`dock_preview.rs` abre o painel após 140 ms e mantém a posição até fechar ou
mudar de aplicativo. A captura de apps Linux usa diretamente as superfícies
retidas no compositor, mesmo com a janela minimizada. Os limites de captura e
a adaptação ao custo real continuam ativos, sem forçar o aplicativo a produzir
novos quadros. Os testes privados de rede/bandeja também simulam o logind para
validar o brilho e a recusa de permissões sem alterar o monitor real.

## Ferramentas de build (`rust/xtask`)

`rust/xtask` reúne os comandos de empacotamento, rodados a partir da raiz do
repositório e nunca instalados com a DE:

```sh
cargo xtask install --prefix /usr --libexecdir /usr/libexec --destdir "$PWD/stage"
cargo xtask dist
cargo xtask srpm --outdir "$PWD/build/srpm"
```

O crate fica fora dos `default-members`, então o build do `axia-shell` não o
compila. Não acrescente crates novos sem necessidade: cada um entra em
`rust/vendor` e no arquivo-fonte do RPM. `make check` roda os testes do crate.

## Custo do glass sobre aplicativos externos

`surface_damage.rs` responde qual parte de uma janela o commit de um aplicativo
mudou, a partir de `wl_surface.damage` e `damage_buffer`. O glass só é recalculado
quando essa parte alcança ele ou a margem que o blur amostra. Em qualquer dúvida
(janela com transformação, mudança de tamanho ou posição, recorte de viewport,
buffer sem dano declarado), a resposta é a janela inteira.

No caminho GPU, um quadro cuja única mudança é esse recálculo pinta cada glass em
separado (`glass_only_frame` em `paint.c`), em vez de unir a barra e a Dock numa
caixa que ocupa quase a tela toda. Qualquer outra mudança no quadro volta a uma
caixa só; `paint_multi_damage` continua desligado. A aparência do glass não muda.

## Histórico da área de transferência

`clipboard/` possui o modelo e a persistência em uma thread da sessão. A ponte C
transporta cópias concluídas, sensibilidade e descritores de PNG; o SDK mantém a
API pública e os painéis existentes. Escrita, leitura, classificação, limites,
fixados e limpeza saíram do JavaScript. Mudanças chegam por eventfd e deltas,
sem polling em repouso. A seleção nativa do Wayland continua no compositor.
Veja [CLIPBOARD.md](CLIPBOARD.md) para compatibilidade, limites e validação.


A transferência nativa e os painéis da área de transferência também passaram
para `clipboard/native.rs` e `clipboard/ui.rs`. Rust possui texto, PNG, seleção
primária, leitura/escrita parcial, snapshots compartilhados e conversão Latin-1
do XWayland. O C mantém recursos/eventos do protocolo e validações de foco.
O SDK liga os widgets compartilhados às árvores e ações produzidas em Rust,
sem manter um segundo desenho ou controlador do painel. O renderizador e o
glass existentes continuam sendo usados. Os testes de protocolo rodam em uma
sessão privada, nunca na DE em uso.

## Temporizadores e quadros da interface

`rust/axia-shell/src/scheduler.rs` mantém uma fila por contexto QuickJS. O mesmo
modelo atende o tema do compositor e os aplicativos isolados. Rust possui os
identificadores, os prazos ordenados, os intervalos, os cancelamentos e os lotes
de `requestAnimationFrame`. `server/src/ui/js_timers.c` converte os argumentos e
expõe um objeto com finalizador; nenhum valor QuickJS é guardado em Rust.

O pequeno preâmbulo em `server/src/ui/js_prelude.h` conserva apenas as referências
às funções JS e seus argumentos, para que o coletor do QuickJS continue enxergando
seu ciclo de vida. As políticas de agendamento saíram do JavaScript:

- callbacks vencidos são ordenados por prazo e depois pelo identificador;
- o lote é uma fotografia: um callback pode cancelar outro ainda não executado;
  um temporizador criado durante o lote só entra na próxima passagem;
- intervalos são reagendados antes da chamada, a partir do tempo daquela passagem,
  com pelo menos 1 ms, permitindo cancelar o próprio intervalo;
- callbacks de quadro agendados durante um quadro ficam para o próximo; cancelar
  durante a execução não altera a fotografia que já foi entregue;
- prazos não são varridos quando nada vence, e a consulta ociosa não aloca arrays JS;
- cada contexto possui um limite de 65.536 temporizadores e 65.536 quadros pendentes;
- atrasos inválidos não produzem um tempo de espera fora do intervalo de `poll()`.

A fila também informa se JS criou trabalho durante a pintura. O laço consulta
esse aviso antes de dormir, evitando perder um `setImmediate` criado por um
callback de animação. Nos apps isolados, a política Rust espera a apresentação
Wayland; só utiliza a cadência de 60 Hz quando não existe um quadro submetido.
Um evento real de apresentação libera o quadro seguinte imediatamente, inclusive
em monitores acima de 60 Hz. Isso impede que atualizações React com quadros ainda
pendentes criem um laço de CPU.

`server/tests/timer_order_test.c` compara 400 roteiros aleatórios com a fila JS
congelada do commit `7628f771`, incluindo ordem, argumentos, cancelamentos,
exceções, leituras de relógio e tempos retornados. Também compara fotografias de
quadros com cancelamentos e novos pedidos durante o callback. O teste participa
de `make check`. O modo `bench` inclui uma chamada nula, a implementação JS
recente e uma versão histórica que varria a fila; esta última não representa a
versão instalada antes desta migração.

O teste headless real verifica a cadência dos quadros e uma atualização agendada
durante a pintura. Uma segunda sessão inicia um app isolado de teste usando o
host real, React e callbacks Wayland, para detectar quadros travados ou sem limite
de frequência. Só o pacote temporário de teste é alterado.

## Ativação local com backup

`cargo xtask deploy --source DIRETORIO --target DIRETORIO --backups DIRETORIO
--server-sha256 HASH` aplica um payload local já compilado. A operação verifica o
manifesto e o hash do compositor antes de escrever, conserva os auxiliares do
host e cria um backup dos arquivos que serão alterados, incluindo uma assinatura
anterior. Cada arquivo novo é sincronizado e renomeado sobre o destino, sem
substituir os diretórios existentes nem truncar o executável em uso.

Depois da cópia, o novo compositor valida o manifesto. Erros de cópia ou
verificação restauram os arquivos anteriores e removem os novos que não existiam.
Os testes cobrem instalação sobre diretórios não vazios, executável em execução,
entrada corrompida, links no destino e falha de verificação com restauração.
O instalador pode ser compilado em contêiner e executado no host sem Cargo.

## Aplicativos compilados: Force Quit e Files

`apps/forcequit/` e `apps/files/` contêm apenas fontes `.rs`. Os antigos
`index.jsx` e manifestos JSON foram removidos. `manifest.rs` descreve os dados do
app; `rust/native_apps.rs` compila um único catálogo, compartilhado entre o
compositor e `cargo xtask native-apps`. O bundler consulta esse catálogo e registra
os apps pelo host comum `sdk/nativeapps.js`, sem gerar um JSX por aplicativo.
A compilação incremental acompanha também os fontes Rust dentro de `apps/`.

Force Quit possui em Rust o agrupamento, exclusão dos painéis e do próprio app,
seleção por chave, invalidação quando a janela desaparece, ícone, árvore de widgets
e fila de efeitos. O resultado de cada ação volta a Rust. Um SIGKILL bem sucedido
suprime apenas as outras janelas do mesmo app isolado; uma falha permite tentar
a próxima superfície. Janelas X11 recebem pedido de fechamento, nunca SIGKILL
contra o XWayland. A primeira mensagem de erro é preservada.

Cada janela do Force Quit possui um estado no host nativo Rust: conteúdo,
ícones e entrada não passam pelo React. O clique envia somente a chave do grupo;
`forceQuit()` obtém os fatos atuais da sessão antes de chamar o controlador Rust
compartilhado com `runningApps()` e com os menus do shell. A FFI de controle
continua pertencendo a um objeto QuickJS com finalizador. O compositor verifica
usuário, cliente Wayland e PID antes de encerrar um processo; nenhum PID vindo
do app é aceito.

Files mantém seu serviço e sua interface Rust, agora fisicamente em `apps/files/`.
Navegador e seletores continuam compartilhando o mesmo modelo. `sdk/rustfiles.js`
é seu adaptador comum para os componentes já existentes do Axia. Esse
renderizador de navegador e diálogos ainda usa React; o antigo `WidgetHost` dos
outros apps compilados foi removido de `sdk/nativeapps.js`.

Os testes Rust verificam política de encerramento, falhas, seleção obsoleta,
isolamento entre instâncias, metadados e ausência dos fontes legados. A integração
`native_apps_runtime` exercita a tela real com cliques, remoção de janelas,
reabertura, close guards e efeitos simulados em uma sessão privada:

```sh
cargo test --manifest-path rust/Cargo.toml --target-dir server/build/rust \
  --locked --offline --test native_apps_runtime -- --ignored --nocapture
```

`make check` abre ambos os aplicativos compilados nos temas padrão e clássico,
além dos testes existentes de navegação, seletores e operações de arquivos.

## Calculadora e Sobre o aplicativo

`apps/calculator/` e `apps/appabout/` também são fontes Rust completas, com
metadados em `manifest.rs` e sem `index.jsx` ou manifesto JSON. Ambos pertencem
ao host de visões nativas em Rust, incluindo seus nós, ícones e entrada. O Sobre
não usa mais a FFI de objetos do antigo renderizador React.

A Calculadora mantém expressão, resultado, erro, hover e pressionamento em
Rust. O parser limita o tamanho e a profundidade e exige consumir toda a entrada.
Não descarta caracteres desconhecidos nem aceita parênteses incorretos. Resultados
em notação científica podem ser reutilizados sem serem interpretados como outra
operação. O arredondamento não multiplica resultados enormes até gerar infinito.
O limite de entrada de 40 caracteres, separador decimal regional, apresentação
LTR, atalhos e cores dos dois estilos são conservados.

O Sobre recebe fatos serializáveis do catálogo e do registro. Rust escolhe nome,
versão, descrição, origem, indicação de isolamento, ícone e título da janela.
Dados de um alvo anterior não são conservados ao mudar para um app inexistente.
O host comum resolve traduções e teclado/foco e aplica o título, sem wrappers
específicos por app. `native_view/icons.rs` compartilha os desenhos dos apps; os
ícones de pacotes e janelas Linux recebem descritores de arquivo ou de letra,
sem transportar componentes React. Os fatos necessários vêm dos metadados Rust;
a Calculadora trata teclas diretamente no host nativo, sem adaptador JS
específico nem transporte do catálogo a cada tecla.

Os testes unitários cobrem expressões malformadas, limites, valores grandes,
precedência, teclado, estados independentes e as origens dos aplicativos. A
integração real usa o teclado da Calculadora, provoca divisão por zero,
reabre a janela e confirma que seus textos não passam pelo React. Os testes
Rust do host também verificam cliques, hover, pressão, cancelamento, foco,
instâncias e ícones. A integração confere os títulos e versões do Sobre. Os apps compilados
são abertos nos temas padrão e clássico pelo teste headless.

## Sobre o Axia

`apps/about/` agora contém apenas Rust: `manifest.rs`, `mod.rs`, `ui.rs` e
`tests.rs`. O host nativo monta o conteúdo e o ícone diretamente na cena.
A árvore conserva o logotipo, a tipografia, os espaçamentos, os indicadores de
assinatura, as linhas opcionais de gabinete/tempo de boot e o botão de relatório.
O botão usa as dimensões pequenas do controle original, incluindo a borda do tema.

O shell comum (`sdk/nativeview.js`) fornece fatos dos serviços existentes,
sem definir o layout ou tratar o estado do app. `serviceIntervals` conserva os
períodos de 5 segundos para sistema e 2 segundos para estatísticas do servidor;
o contexto fornece resolução, versão do motor e separadores regionais. O relatório
é um efeito `openApp` do host. A moldura da janela e o shell continuam em React;
a migração deste app não significa que o compositor inteiro esteja em Rust.

## Emoji e Símbolos

`apps/characters/` contém apenas fontes Rust. O app monta abas, categorias,
seletor de tons, busca, grade Unicode, prévia e ação de cópia no host nativo.
A busca mantém a ordem Unicode, a preferência por início de palavra e a consulta
simultânea aos termos em inglês, espanhol e português, ignorando acentos.
Os nomes respeitam o idioma da interface e os pacotes de idioma do usuário.

`assets/emoji.json` é a única lista de categorias e bases que aceitam tons de
pele; o Rust a compila e `sdk/emojidata.js` a exporta para o seletor rápido do
shell. `tools/gen-emoji.mjs` atualiza esse arquivo compartilhado. O seletor
rápido continua no shell; o conteúdo do app não passa pelo React.
Recentes e tom continuam nas preferências `emojiRecent` e `emojiTone` existentes.
A cópia é um efeito do host para a área de transferência do sistema.

## Serviço de alarmes, temporizador e cronômetro

`rust/axia-shell/src/clock/` é o modelo de sessão do Relógio. Ele conserva os
formatos de `settings.clock`, a soneca de 9 minutos, a janela de atraso de
10 minutos, a duração para repetir o temporizador e as 200 voltas mais recentes.
A recorrência usa o calendário local, incluindo mudanças de horário de verão;
num horário ambíguo, escolhe a primeira ocorrência, como o `Date` anterior.

`sdk/alarms.js` publica snapshots e apresenta sons, notificações e o item da
barra. O próximo intervalo vem do Rust; o agendamento não depende de uma janela
aberta. A FFI JSON fica em `js_nativeview.c` (`lord.clock.request`).

## Relógio nativo

`apps/clock/` contém apenas Rust: metadados, controlador, árvore de widgets e
testes. As quatro abas, seleção e remoção de cidades, editor de alarmes,
preferências de 12/24 horas, presets do temporizador e voltas do cronômetro são
montados diretamente no host nativo. A leitura dos fusos conserva o serviço
TZif existente do compositor, por efeitos `nativeRead` autorizados no manifesto.

O host admite cadência variável por app: relógio mundial a cada segundo,
alarmes a cada 30 segundos, temporizador em execução a cada 250 ms e cronômetro
em execução a cada 47 ms. Temporizador pausado e cronômetro parado não mantêm
um timer de desenho. Fechar a janela libera seus nós e preserva o serviço de
sessão. O shell recebe apenas snapshots para o indicador e os efeitos de
notificação/configuração. Enter nos campos e hover das linhas também são do host.

## Monitor application

`apps/monitor/*.rs` owns all eight Monitor tabs, privacy panels, collection,
process actions, history, charts and map. Its application-specific C/JS sources
and tests have been removed. Monitor is the first native view (below): React
draws no part of it. Collection and filesystem work run on one Rust worker
thread. See [MONITOR.md](MONITOR.md) for the language boundary, memory
accounting, sampling cadence and validation commands.

## Visões nativas

Um app compilado com `"nativeView": "rust"` é montado pelo Rust diretamente na
cena do runtime, sem React. O host fica em `rust/axia-shell/src/native_view/`:

- `host.rs` espelha a árvore de widgets do app (`view`, `text`, `button`,
  `scroll`, `input`, `image`, `raster`) em nós da cena e a reconcilia por chave:
  só os estilos, textos e pixels que mudaram são aplicados, e os nós são
  reaproveitados. Botão (hover e pressão locais, sem reconstruir a árvore),
  rolagem (roda, touchpad e arraste da barra) e campo de texto (cursor
  piscando, colar pelo shell) são do host. Uma árvore igual à anterior
  (`{"same": true}`) não toca a cena.
- `style.rs` adapta os objetos de estilo ao conversor único `css_style.rs` (cores,
  `padding`, `flex`, `border`, `boxShadow`, gradientes…), o mesmo que o renderizador React
  usa por `lord.css`, e envia só as diferenças.
- `i18n.rs` traduz as legendas como `t()` e `errorText()`: pacote do usuário,
  depois os assets, inglês como reserva, plurais de `meta.json`.
- `runtime/native.rs` é a API segura da cena usada pelo host (criar, inserir,
  estilo, texto em nó RAW como o React faz, imagens por caminho ou RGBA, caixa
  do último layout).

O C entrega cada evento de ponteiro e gesto ao host antes do JS
(`server/src/ui/js_nativeview.c`, `js_pointer`/`js_gesture`): o host trata os
seus nós e o JS recebe o mesmo evento no nó de montagem, então foco e elevação
da janela e os ouvintes do shell continuam funcionando. As teclas passam
primeiro pelos atalhos do shell (`runtime/events.js`) e depois pelo campo de
texto nativo focado; Ctrl, Alt e Super seguem para o shell, exceto colar. O
laço principal chama `axia_view_tick`/`axia_view_timeout` para a atualização
periódica, redimensionamento, cursor e barras de rolagem.

`sdk/nativeview.js` é a parte do shell: dá o nó de montagem dentro da moldura do
gerenciador de janelas, envia os fatos (configurações, tema, serviços do
firewall, catálogo, foco da janela) e executa os efeitos do shell: abrir apps,
gravar configurações, chamadas ao firewall, área de transferência e título da
janela. O mesmo host monta o ícone da Dock e os painéis de privacidade dentro
das Configurações. Calculadora, Force Quit e Sobre também são montados pelo
Rust, incluindo seus ícones. O host React de widgets desses apps foi removido;
os efeitos ainda passam pelo adaptador de serviços da sessão. Os ícones do
lançador e da lixeira da Dock também usam visões Rust. A retirada do restante
do shell é acompanhada em [NATIVE-SHELL.md](NATIVE-SHELL.md).
das Configurações. Sobre o Axia, Calculadora e Monitor são montados diretamente em Rust.
A Calculadora conserva a mesma árvore de widgets e o ícone; hover, pressão,
cancelamento do clique, traduções e teclado são tratados pelo host nativo.
Teclas só chegam à janela focada, nunca aos ícones; atalhos Ctrl/Alt/Super
seguem para o shell, e a Calculadora não mantém timer quando ociosa.
A Axia Store também é uma visão nativa ([STORE.md](STORE.md));
`native_view/paint.rs` tem as formas com antialiasing que ela usa para ícones e
artes. Force Quit e Sobre continuam no host React
(`sdk/rustwidgets.js`): o Force Quit mostra ícones de apps, que são componentes
do shell.

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline --lib native_view
make -C server
cargo test --manifest-path rust/Cargo.toml --target-dir server/build/rust \
  --locked --offline --test native_apps_runtime -- --ignored --nocapture
```

Os testes unitários cobrem a conversão de estilos, traduções, reconciliação no
lugar, clique, pressão que termina fora, campo de texto, colar, foco da janela,
ícones sem temporizador e a desmontagem. A integração abre o Monitor numa sessão
real e verifica que nenhum texto ou pixel dele passa pelo React.

## Encaixe de janelas e Snap Assist

`snap/` substitui `sdk/snap.js` e passa a ter toda a regra do encaixe: os
retângulos das zonas (metades, quartos, terços e dois terços), a zona sob o
ponteiro durante o arraste, os movimentos de Super+setas, o Snap Assist e o
redimensionamento conjunto de janelas encaixadas. O teste em node
`server/tests/snap_test.mjs` foi removido; seus casos estão em `snap/tests.rs`.

- O arraste continua em C (`grab.c`), sem JS por quadro. Cada movimento do
  ponteiro e cada mudança do Shift vão ao `DragZones`, que diz se a zona mudou.
  Só então o servidor chama `__lordWm('zone')`. O polling de 90 ms em JS foi
  removido. Com Shift, as laterais viram os terços externos e o topo o terço do
  meio.
- O `LinkedResize` move a linha compartilhada por janelas encaixadas na mesma
  tela, respeitando o tamanho mínimo de cada uma. O `grab.c` aplica os frames
  calculados aos nós; essas janelas não usam a camada de arraste, que guarda o
  fundo de uma janela só.
- O Snap Assist calcula as zonas livres do layout (ou do layout escolhido no
  menu do botão maximizar) e a grade das miniaturas dentro da zona. A interface
  é um modo do Overview, que já desenha as janelas reais em escala.

A ABI está em `server/src/ui/snap_ffi.h` e a ponte para a sessão em
`server/src/ui/js_snap.c` (`lord.snap`). Rust não guarda ponteiros de nós nem
objetos JS: o estado do arraste pertence ao grab e é liberado ao terminar ou
cancelar. A lista de janelas continua em `sdk/windows.js`.

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline snap::
make -C server build/snap-ffi-test && server/build/snap-ffi-test
```

Os testes cobrem layouts sem sobreposição nem lacunas em áreas ímpares, as zonas
de soltura com e sem Shift, várias telas, Super+setas, linhas entre metades e
quartos, bordas da área de trabalho, mínimos, as zonas oferecidas pelo Assist e a
grade das miniaturas. Falta validar na sessão física o arraste com preview, o
Assist com o mouse e o teclado e o redimensionamento conjunto com apps Linux.

## Regras puras do gerenciador de janelas

`winmodel/` substitui `sdk/placemodel.js` e `sdk/displaylayout.js` e a parte do
`sdk/desktopmodel.js` que não depende da lista de janelas:

- onde cada app reabre (`place.rs`: identidade da tela por nome do monitor e
  conector, área visível quando a tela sumiu, limite de 200 apps);
- a lista de desktops (`desktops.rs`: adicionar, fechar, vizinhos, setas e gestos
  em RTL, valores salvos);
- o grid de miniaturas e a faixa de desktops do Overview (`layout.rs`). O mesmo
  grid serve o Snap Assist, que deixou de ter um próprio no `snap/`;
- a organização de telas em Configurações → Telas (`displays.rs`: numeração,
  posição magnética e soltura sempre encostada e sem sobreposição).

A ponte é uma chamada só, `lord.winmodel(op, json)` (`server/src/ui/js_winmodel.c`,
ABI em `winmodel_ffi.h`), envolvida por `sdk/winmodel.js`. Os wrappers enviam só
os campos que cada regra usa, nunca a janela inteira. Sem estado, a ponte também
está nos processos de apps isolados. Uma chamada recusada lança `TypeError` com
o motivo dado pelo Rust.

Os predicados de visibilidade (`onDesktop`, `visibleOn`, `mountedOn`,
`focusedOn`, `keyboardSurface`...) continuam em `sdk/desktopmodel.js`. Eles rodam
sobre a lista de janelas a cada render, e atravessar a ponte serializaria essa
lista a cada chamada. Migram junto com o store do `sdk/windows.js`.

Os testes `place_model_test.mjs` e `display_layout_test.mjs` foram portados para
`winmodel/tests.rs`, incluindo a varredura de ~25 mil solturas de telas. O
`desktop_model_test.mjs` ficou com os predicados.

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline winmodel::
make -C server build/winmodel-ffi-test && server/build/winmodel-ffi-test
```

## Lista de janelas da sessão

`winmodel/session.rs` passa a ser o dono das janelas: a lista em ordem de
empilhamento, os desktops, o desktop visível e o Overview ligado ou não. Todas as
regras que os alteram também ficam ali:

- abrir (posição pedida, lembrada ou em cascata), focar, subir, mandar para trás
  e alternar;
- minimizar, maximizar, encaixar, desencaixar, Super+setas e organizar;
- centralizar, mover entre telas e reposicionar quando uma tela some;
- tela cheia, picture-in-picture e o levantamento pelo teclado da tela;
- o fim de um arraste em C;
- criar e fechar desktops.

As visões que os temas desenham são calculadas ali uma vez por mudança: visíveis,
montadas, a janela da frente (que recebe o teclado), escondidas, estacionadas,
placeholders do Overview e as janelas de cada desktop. Por isso os predicados de
`sdk/desktopmodel.js` saíram, e o arquivo ficou só com as constantes e o
hit-test.

`sdk/windows.js` virou o espelho para o React. `windows`, `activeDesktop`,
`desktopList` e `overviewShowing` mantêm a mesma API de leitura, e `windowViews`
traz as visões. Toda mudança passa por `wm.*` (os escritores diretos do store em
`trackpad.js`, `components.jsx` e `appswitcher.jsx` viraram `wm.cycle`,
`wm.setPip`, `wm.exitFullscreen` e `wm.toBack`). Cada janela tem uma revisão, e o
espelho só troca o objeto das que mudaram, então o React mantém os demais.

Garantias do espelho:

- **Somente leitura:** `windows`, `activeDesktop` e `desktopList` recusam `set`
  com um aviso, porque uma escrita por fora separaria o espelho da lista do Rust.
- **Reentrância:** se um subscriber muda as janelas durante uma atualização (o
  Overview se fechando quando o desktop muda), o estado mais novo prevalece. A
  atualização mais antiga para no meio, sem sobrescrevê-lo.
- **Identidade:** uma janela alterada vira um objeto novo, mas as partes que não
  mudaram (`args`, `restore`, `pip`...) mantêm o objeto anterior. Um efeito que
  depende de `args` só dispara quando o conteúdo de fato muda.
- **Recuperação:** se chegar uma janela numa revisão que o espelho não tem (um
  estado perdido), ele pede tudo de novo (`resync`).

O JS envia junto de cada chamada os fatos que só ele conhece: as telas com as
áreas de trabalho do tema, os tamanhos dos apps abertos e se o tema encaixa
janelas. O registro de apps, os guards de fechamento e os menus continuam no JS,
porque são funções. A ponte é `lord.windowList.call(op, json)`
(`js_winmodel.c`), com um estado por contexto: a sessão e cada app isolado têm a
sua lista.

Testes: `winmodel/tests.rs` traz os casos do antigo `desktop_model_test.mjs`
(incluindo a regressão das 720 ordens para o teclado) e as operações. O
`server/tests/windows_bridge_test.mjs` empacota o `sdk/windows.js` real com o
esbuild e o roda no QuickJS (`build/windows-bridge-host`), com a ponte C real e o
Rust real. Ele confere stores, objetos mantidos, visões, desktops, encaixe e o
fim de um arraste. É o único teste que exercita as três camadas juntas sem a
sessão.

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline winmodel::
make -C server build/windows-bridge-host && (cd server && node tests/windows_bridge_test.mjs)
```

Ainda precisa de validação na sessão física: temas, Dock, Overview, alternador,
gestos e apps Linux sobre a lista nova.

## Parsers do e-mail

Os parsers do `axia-maild` que leem dados de fora (MIME e charsets, XML do
WebDAV, o sanitizador de HTML, iCalendar e vCard) estão em `rust/axia-mail`,
com a mesma ABI C de `server/src/mail/*.h`. Só o `axia-maild` liga essa
variante do `axia-codecs` (feature `mail`). Detalhes, limites e validação em
[RUST-MAIL.md](RUST-MAIL.md).
