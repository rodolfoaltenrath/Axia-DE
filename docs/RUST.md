# Rust na Axia DE

O primeiro módulo Rust da DE é o modelo do alternador de aplicativos em
`rust/axia-shell`. Ele é compilado como biblioteca estática e ligado ao
mesmo executável `axia-server` em C. Não há serviço Rust separado.

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

## Núcleo do runtime: árvore de nós e estilos

`runtime/` contém a implementação da árvore e dos estilos. Os antigos
`server/src/ui/node.c` e `style.c` foram removidos do repositório e do build.
O React continua enviando as mesmas operações; layout, texto, pintura e o
cache de imagens continuam integrados ao renderer existente.

Para revisar o código:

- `scene.rs` possui os nós em `Box`, mantém as relações por IDs, valida
  reparentamento e libera subárvores iterativamente. A tabela de views mantém
  os endereços estáveis e a fila de vagas reaproveita o menor ID disponível.
- `style.rs` resolve propriedades por tipos, sem offsets de bytes. Snapshots
  comparam os campos e os bits dos floats; padding e caches derivados não
  participam da comparação. As extensões são alocadas somente quando usadas.
- `damage.rs` mantém as regiões de repintura, incluindo a união de regiões
  sobrepostas e o limite de 16 retângulos.
- `types.rs` define as views `repr(C)` e `mod.rs` contém a ABI e seus contratos.
  `state.rs` fornece o armazenamento do estado global compartilhado.

Rust possui e libera nós, texto, extensões de estilo e transformações. O cache
C ainda possui as imagens; o dono do nó devolve cada referência ao cache ao
ser destruído. Views C são empréstimos síncronos na thread da interface:
o renderer pode atualizar geometria e pintura, mas não liberar os dados nem
alterar as relações da árvore. Os blocos `unsafe` nessa fronteira continuam
necessários enquanto layout e pintura usam as views C.

Criação de nós não varre mais toda a tabela desde o início; a contagem de nós
é direta. Repetir um texto idêntico preserva suas alocações e caches. Resetar
a árvore libera também as tabelas, e operações que criariam ciclos são
recusadas. IDs e operações da interface continuam compatíveis.

Os testes novos são Rust e cobrem crescimento/reuso de IDs, endereços estáveis,
reparentamento, ciclos, descarte de subárvores profundas, limites de buffers,
ordem de pintura, invalidação, estilos e regiões de repintura. O teste de ABI
usa Clang para inspecionar o arquivo C de layout existente e verificar tamanho,
alinhamento e offset de todos os campos compartilhados, sem gerar teste C.

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline runtime::
# Requer Clang; também executado no CI Fedora:
cargo test --manifest-path rust/Cargo.toml --target-dir server/build/rust --locked --offline runtime::tests::native_views_match_c_layout -- --ignored --nocapture
# Regressões existentes de layout, texto, pintura e frames de arraste:
make -C server build/engine-test GPU=1
```

Essa etapa migra a árvore e os estilos. O motor de layout, pintura e as pontes
QuickJS são etapas seguintes; não há promessa de redução de RAM ou latência
apenas pela troca de linguagem.

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

`launcher.rs` e `launcher/` mantêm o catálogo e a implementação dos menus Flow
e Classic. Rust recebe os metadados brutos do registro e dos aplicativos Linux,
aplica traduções e visibilidade, prepara Unicode, ordena, categoriza e filtra.
Também possui o estado da pesquisa, categoria, seleção, fixados/recentes,
menus de contexto e energia, atalhos e descrições de widgets dos dois temas.
O posicionamento do painel Classic, inclusive em múltiplos monitores e RTL,
é calculado em Rust.

Os arquivos `themes/default/components/Launchpad.jsx` e
`themes/classic/components/StartMenu.jsx` foram removidos. `sdk/rustlauncher.jsx`
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

A integração verifica os dois menus, identidade estável da lista, geometria,
rolagem, pesquisa/Enter, categorias, contexto, fixar/desafixar na Dock e no
Classic, fixados/recentes, busca geral, energia, início de arraste e instalação/
remoção de pacote, mudança de idioma, Unicode, redimensionamento e touchpad.
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
