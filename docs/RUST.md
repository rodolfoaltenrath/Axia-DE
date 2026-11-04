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
