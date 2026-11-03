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
