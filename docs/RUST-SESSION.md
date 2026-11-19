# Inicialização da sessão em Rust

`rust/axia-session` substitui os antigos `tools/desktop/fedora-session.in`
(Python) e `tools/desktop/axia-session.in` (shell), incluindo seus testes de
inicialização e pastas do usuário. Os scripts e testes antigos foram apagados.
O crate depende apenas de `libc` e `serde_json`, já vendorizados; não depende
da shell, dos apps nem de QuickJS.

## Pontos de entrada e instalação

- `/usr/libexec/axia/fedora-session`: executável Rust usado pelo gerenciador de
  login. Mantém o compositor e o agente polkit independentes.
- `/usr/libexec/axia/bin/axia-session`: executável Rust do comando público.
  `/usr/bin/axia-session` é um link para ele. Executa o compositor com `exec`,
  conservando os argumentos e o comportamento do comando anterior.

Os dois descobrem o diretório instalado pelo próprio executável, sem templates
ou caminhos de prefixo compilados. `PREFIX`, `LIBEXECDIR` e `DESTDIR` continuam
suportados. O instalador exige ambos os binários. O gerador da ISO conserva o
executável fornecido pelo RPM, em vez de sobrescrevê-lo com um script Python.
O fixture Rust dos testes não é instalado.

## Comportamento preservado

- Recusa root e exige um `XDG_RUNTIME_DIR` existente. Na ausência do barramento
  de usuário, inicia `dbus-run-session`; as variáveis da sessão identificam Axia
  e Wayland. Não cria um serviço privilegiado.
- Lê o tema salvo em `HOME/.config/axia/settings.json`, como o compositor atual,
  com limite de 1 MiB e nome de até 64 caracteres ASCII alfanuméricos, `_` ou
  `-`. Preferências inválidas ou bundle ausente mantêm a escolha do compositor.
- Inicializa as pastas XDG uma vez usando `xdg-user-dirs-update`, conservando
  pastas traduzidas já existentes e escolhas personalizadas ou desativadas.
- Rotaciona `session.log` para `session.previous.log`, respeita `XDG_CACHE_HOME`
  e cria o log com modo 0600. Uma falha no log não impede iniciar o desktop.
- Descobre sockets Wayland reais e publica o ambiente por D-Bus sem exigir
  Xwayland. Quando há Xwayland, confere o PID do lock e o dono do socket.
  Inicia o agente polkit quando disponível; falhas auxiliares são registradas
  e não abortam uma sessão válida.
- Encaminha SIGINT/SIGTERM ao compositor durante e após a descoberta. Os filhos
  recebem a máscara original de sinais. Filhos próprios são esperados e os
  helpers encerrados na saída, com fallback de término após três segundos.
- A cada 30 segundos, compara o executável em `/proc` com a versão instalada.
  Uma substituição gera aviso em português, espanhol ou inglês, sem reiniciar
  nem terminar o desktop. O aviso não se repete; falhas do notificador são
  toleradas.

A supervisão usa `signalfd` e `inotify`, sem thread auxiliar. Após a descoberta,
fica bloqueada até um sinal ou a verificação de atualização de 30 segundos.
Se inotify não estiver disponível, somente a descoberta inicial usa o fallback
limitado de 100 ms durante até 20 segundos. Comandos auxiliares têm timeout e
são recolhidos na falha. As ferramentas D-Bus/XDG e o agente continuam sendo
serviços do host; não são implementações Python ou shell do lançador da Axia.

## Validação

```text
cargo test --manifest-path rust/Cargo.toml -p axia-session --locked --offline
make -j2 -C server build/axia-session build/fedora-session GPU=1
```

Os 18 testes automáticos cobrem processos Rust reais em instalações com espaços
no caminho, link público, barramento presente/ausente, sockets, logs, pastas XDG,
localização, falhas e sinais. A atualização é verificada substituindo o arquivo
de um processo vivo e aguardando o aviso, inclusive com falha do notificador.
O CI também exige a recusa de root em container isolado:

```text
cargo test --manifest-path rust/Cargo.toml -p axia-session --locked --offline --test session both_entrypoints_reject_root_before_starting_a_compositor -- --ignored
```

A integração instalada inicia o compositor real com HOME/XDG privados e D-Bus
próprio, restaura a Calculadora e verifica o log, os executáveis ELF e a saída:

```text
AXIA_SESSION_TEST_STAGE=/stage/usr/libexec/axia AXIA_SESSION_TEST_OUTPUT=/var/tmp/axia-session-report cargo test --manifest-path rust/Cargo.toml -p axia-session --locked --offline --test installed -- --ignored --nocapture
```

Essa integração é exigida no CI GPU=1, com renderização em software, e publica
JSON e log no artifact `axia-session`. Somando os dois testes separados, foram
validados 20 casos desta migração. Os 47 testes leves do xtask e os três testes
da divisão Cargo/Make também passaram.

Na validação local Fedora 44, os novos lançadores foram compilados com as flags
Fedora, instalados por `cargo xtask install` e usados com o compositor de
referência `c5e595f1` e os assets do RPM correspondente. A Calculadora foi confirmada e a sessão saiu com
status zero. O RSS mediano observado do lançador Rust foi **2,555 MiB**, sem
somar compositor, D-Bus ou helpers. É uma sessão privada em software, não uma
comparação controlada com a sessão física nem uma medida de apresentação.
Os registros locais ficam no clone isolado em `build/validation/session-rust/`.

A sessão física existente não foi reiniciada. O novo lançador passa a ser usado
ao instalar o novo pacote e entrar numa nova sessão. A migração da UI da shell
e as medições físicas continuam nas etapas próprias do roadmap.
