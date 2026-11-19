# Plano: parar de estourar a memória do build e preparar o repositório para ~100% Rust

Estado em 18/11/2026. Este arquivo **não está commitado** de propósito (commitar dispara outro build do CI).
Serve para o dono do projeto e para os agentes (Claude e Codex) que forem executar. Cada etapa diz o que
fazer, em quais arquivos e como saber que terminou.

## 1. Resumo em cinco linhas

- O RPM no GitHub Actions vem sendo **morto por falta de RAM** (exit 137, SIGKILL). O runner tem 16 GB.
- A causa é estrutural: quase todo o Rust novo cai num **único crate, o `axia-shell`**, e cada compilação
  dele usa ~7-8 GB com as flags do Fedora (`opt-level=3`, `debuginfo=2`, `codegen-units=1`).
- Ele é compilado **várias vezes** no mesmo RPM (release principal, `axia-bench`, biblioteca de `cargo test`
  e testes) e **cresce a cada migração**.
- Com ~100% em Rust ele chegaria a algo como o dobro do tamanho atual. **Estimativa** (não medida):
  14-16 GB para um único `rustc`, ou seja, estoura o runner mesmo compilando uma coisa por vez.
- Conclusão: **dividir o `axia-shell` em crates menores é obrigatório** para chegar a ~100% em Rust.
  As correções baratas (seção 4) só compram tempo.

## 2. Diagnóstico (o que foi medido e o que não foi)

### 2.1 Tamanho do código (medido em 18/11, linhas de `.rs` nossas, sem `vendor/`)

| Parte | Linhas |
|---|---|
| Rust nosso, total | ~188 mil |
| C em `server/src` | ~100 mil |
| JS/JSX (sdk, runtime, themes, apps) | ~28 mil |
| Crate `axia-shell` (`rust/axia-shell/src`) | ~56 mil |
| `apps/*.rs`, compilados **dentro** do `axia-shell` via `#[path]` | ~74 mil |
| `server/src/**/*.rs` (isolated app etc.), também via `#[path]` | ~16 mil |
| **Unidade de compilação do `axia-shell` hoje** | **~146 mil** |
| `axia-codecs` / `axia-mail` / `axia-crypto` | 25 mil / 12 mil / 3 mil (já separados) |

Apps por tamanho (linhas de `.rs`): files 12,1 mil, settings 9,1 mil, monitor 6,6 mil, passwords 5,7 mil,
notes 4,0 mil, contacts 3,7 mil, photos 3,6 mil, calendar 3,1 mil, pdf 2,9 mil, reminders 2,8 mil,
store 2,2 mil, backup 2,0 mil, fonts 2,0 mil, help 1,8 mil, sysinfo 1,7 mil, logs 1,6 mil,
scanner 1,6 mil, recorder 1,6 mil, terminal 1,6 mil, clock 1,3 mil, camera 1,0 mil,
characters 0,7 mil, forcequit 0,5 mil, calculator 0,5 mil, about 0,3 mil, appabout 0,2 mil.

### 2.2 Memória (medida)

- Dois `rustc` do `axia_shell` ao mesmo tempo: ~7,4 GB + ~5,8 GB de RSS (soma ~13 GB). Medido com
  `podman top` num container Fedora 44 com as flags do `rpmbuild`.
- O segundo `rustc` vinha do `axia-bench` (`--features native-host`, `--target-dir build/bench-native`).
- O `cargo test` do `%check` compila a biblioteca e a biblioteca de testes do `axia-shell` em paralelo.

### 2.3 Linha do tempo das falhas (resumo)

1. 16-17/11: panic de thread-local ao desligar o `axia-server` (Notes). Corrigido (`ddd05593`).
2. 17/11: teste do Photos (permissão de diretório). Corrigido pelo Codex (`74b69bd6`).
3. 17/11 12:13: **último verde** (rpm em ~20 min).
4. O tempo do rpm foi subindo: 20, 23, 22, 29, 33 min.
5. 17/11 19:52: catálogo de apps do teste `native_apps_runtime` sem Contacts/Calendar. Corrigido (`e97cae2b`).
6. 17/11 22:33 e 23:59: rpm morto (137) durante o `%build`.
7. 18/11 01:31: com a correção do `%build` (`80771ac2`), o rpm passou dele e foi morto no `%check`.
8. Execuções "cancelled" do dia 17 **não são falhas**: `desktop` e `rpm` passaram; só o job `copr` foi
   cancelado pela fila de publicação.

### 2.4 O que NÃO foi medido (não afirmar como fato)

- O pico real de memória do RPM inteiro numa máquina de 16 GB.
- A proporção memória x linhas. A projeção de 14-16 GB é extrapolação linear; pode ser pior ou melhor.
- O efeito de `debuginfo=1` e de `codegen-units` maiores. Precisa de teste antes de prometer ganho.
- Se `#[no_mangle] extern "C"` em crates dependentes continua indo para o `libaxia_shell.a` (ver risco R1).

## 3. Estado atual do CI/commits (para não repetir trabalho)

Já commitados (no `origin/main`):

- `ddd05593` Notes fecha sem tocar numa sessão destruída (thread-local).
- `5eda85b4` correções dos testes de integração (catálogo, botão de busca nativo).
- `6542e6a1` duas melhorias do Codex: reconciliação de árvores nativas e fatos React->Rust.
- `80771ac2` `axia-bench` espera o build principal (`server/mk/lordbench.mk`, pré-requisito order-only).
- `040e8f72` `CARGO_BUILD_JOBS=1` no `%check` do spec (`packaging/rpm/axia-de.spec`).

Em andamento quando este plano foi escrito:

- Execução do GitHub `44227245381` (commit `040e8f72`). **Esperar o resultado antes de mexer no RPM.**
- Medição local do pico de `rustc` com `CARGO_BUILD_JOBS=1` (container, árvore em `~/.cache/claude-axia-rpm2`).

## 4. Fase 0: parar de falhar agora (poucas horas)

Só começar depois do resultado de `44227245381`. Se ele passou, as medidas abaixo viram margem de segurança;
se falhou por memória, são urgentes.

### 4.1 Tirar o `cargo test` do `%check` do RPM

- Por quê: os testes Rust já rodam no job `desktop` do CI, no mesmo commit. No RPM eles só recompilam o
  `axia-shell` duas vezes (~15 min, ~8 GB) para repetir o que já passou.
- Como: em `packaging/rpm/axia-de.spec`, no `%check`, não chamar o alvo que executa `test-rust`.
  Hoje `test-platform` depende de `test-rust` (`server/Makefile`, linha do `test-platform`). Opções:
  criar `test-platform-rpm` sem `test-rust`, ou definir uma variável (`SKIP_RUST_TESTS=1`) que o
  Makefile respeite. Manter `tp-test` e `check.py --require-gpu`.
- Custo: builds avulsas (Copr, Fedora) não rodam os testes Rust. Aceitável enquanto o CI `desktop` rodar.
- Pronto quando: o RPM passa no GitHub e o `%check` não compila mais o `axia-shell` em perfil de teste.

### 4.2 Reduzir a memória por compilação no RPM

- Hipótese: `-Cdebuginfo=1` (ou `line-tables-only`) reduz memória e tempo; os pacotes `-debuginfo`
  continuam sendo gerados. **Medir antes** (ver 4.4).
- Como: sobrescrever `RUSTFLAGS` no `%build` do spec depois de `%set_build_flags`.
- Cuidado: não remover as flags de hardening/relro; só trocar `-Cdebuginfo=2`.

### 4.3 Reaproveitar o `axia-shell` no `axia-bench` (opcional, risco médio)

- Hoje o `axia-bench` recompila o crate inteiro com `native-host` e outro `--target-dir`.
- Depois da divisão (Fase 1+) pode deixar de ser necessário (ele passaria a depender só do crate de base).
  Se a divisão for começar logo, **não gastar tempo aqui**.

### 4.4 Instrumentar: registrar o pico de memória no CI

- Adicionar ao job `rpm` (e ao `desktop`) um passo que mostre o pico de RSS (por exemplo
  `/usr/bin/time -v` em volta do `rpmbuild`, ou amostrar `/proc/meminfo` em segundo plano e imprimir o máximo).
- Meta: ter um número por execução para ver a tendência, em vez de descobrir pela falha.
- Regra: se o pico passar de ~11 GB (70% do runner), tratar como alerta.

## 5. Fase 1: o plano de divisão do `axia-shell`

### 5.1 Objetivo e meta de memória

- Nenhum `rustc` acima de ~4-5 GB no RPM (com as flags do Fedora).
- Nenhum crate acima de ~40-50 mil linhas.
- O ciclo local melhora: mexer numa app recompila só a app.

### 5.2 Arquitetura alvo

```
axia-native        (crate de base: o que TODAS as apps usam)
  trait NativeApp, Jobs/async_jobs, i18n, date_format, paint, icons, style,
  retained (revisões), clock (now/local), civil_date, interface para consultar o cenário (ver R2)

axia-app-<nome>    (um crate por app, ou grupos pequenos)
  depende só de axia-native (+ axia-codecs/crypto quando precisar)
  expõe: ID, manifest, App (impl NativeApp), model/policy quando houver

axia-shell         (cola + plataforma, fica bem menor)
  host.rs e o resto de native_view (precisam do cenário), runtime/, wayland/, tray, network,
  clipboard, winmodel, applications, registro das apps (native_view::app(id)) e native_apps.rs
```

Notas:

- As apps ficam fora do `axia-shell` e **o registro** (`native_view::app(id)`) é quem as une. O `lib.rs`
  troca `#[path = "../../../apps/x/mod.rs"] mod x;` por dependência de crate.
- O `libaxia_shell.a` (staticlib) continua sendo o artefato que o C linka.

### 5.3 O que as apps importam hoje do crate (levantado com `grep crate::`)

Aparece nas apps, em ordem de frequência: `crate::clock::{local,now}` (~75x), `crate::native_view::*`
(~55x: `date_format`, `app`, `paint`, `icons`, `retained`, `icon_tree`, `civil_date`),
`crate::runtime::node_get` (14x), `crate::passwords_model`, `crate::settings_model`, `crate::search::matching`,
`crate::files::policy`, `crate::network::request`, `crate::winmodel::call`, `crate::native_style::*`,
`crate::applications`, `crate::native_apps_host`, `crate::help::markdown`.

Cada um desses vira uma decisão: **vai para `axia-native`** (se for genérico: clock, date_format, paint,
icons, style, i18n, Jobs), **fica no shell e a app o acessa por uma interface** (runtime/node_get,
network, winmodel, applications, search), ou **a dependência entre apps é resolvida** (ex.: `help::markdown`
usado por Notes e Passwords: mover o Markdown para `axia-native` ou para um crate `axia-markdown`).

### 5.4 Passos

**Passo 1: spike (uma semana de trabalho no máximo, sem pressa de mover muita coisa)**

1. Criar `rust/axia-native` com o mínimo: trait `NativeApp`, `clock`, `i18n`, `async_jobs`.
2. Mover **uma app minúscula** (ex.: `calculator` ou `forcequit`, ~0,5 mil linhas) para `rust/axia-app-calculator`.
3. Registrar no `native_view::app(id)` e em `native_apps.rs` (que é a fonte de metadados).
4. Atualizar o `Makefile`: `RUST_INPUTS` (a lista de arquivos que dispara rebuild) e `members` do
   `rust/Cargo.toml`. Atualizar o `xtask`/spec se algum caminho de código mudar.
5. **Verificar o risco R1** (símbolos `#[no_mangle]` no `.a`) e o R3 (tempo de link).
6. Rodar tudo: `make check` (GPU=1), os 3 testes ignorados do CI, `python3 -m unittest discover -s
   tools/desktop/tests`, `make install`, e o `rpmbuild --rebuild` local com limite de memória.
- Pronto quando: a app migrada funciona igual, o `libaxia_shell.a` continua exportando os mesmos símbolos
  (comparar `nm -g` antes/depois) e os testes passam.

**Passo 2: ondas de apps (uma por commit, das menores para as maiores)**

Ordem sugerida (menor risco primeiro):
1. `about`, `appabout`, `forcequit`, `calculator`, `characters`, `clock`, `sysprobe`
2. `camera`, `logs`, `recorder`, `scanner`, `fonts`, `backup`, `help`, `store`
3. `reminders`, `contacts`, `calendar`, `photos`, `pdf`, `notes`, `terminal`, `sysinfo`
4. `passwords`, `monitor`, `settings`, `files` (as maiores e mais acopladas, por último)

Regras:
- Um commit por app (ou por grupo coeso), com os testes da app indo junto.
- Cada app move seu `manifest.rs`, `mod.rs`, `ui.rs`, `tests.rs` etc.; testes de integração do `axia-shell`
  (`rust/axia-shell/tests/`) continuam no shell.
- `make check` verde **antes** de cada commit; o `cargo test` deve rodar o crate da app também.
- Apps que o processo isolado usa (`passwords`, via `native_view::app("passwords")` no `axia-server --app`)
  ganham um benefício extra: o processo isolado poderá linkar só o crate da app, e não o shell inteiro.
  Considerar isso, mas **não** misturar com a divisão (primeiro dividir, depois otimizar).

**Passo 3: dividir a parte de plataforma que sobrar no `axia-shell`**

Depois das apps, o `axia-shell` fica com ~56 mil linhas de plataforma. Candidatos a crate próprio:
`wayland/` (protocolos, DnD), `runtime/` (cenário, layout, window_grab), `clipboard/`, `network`/`tray`,
`winmodel/`, `applications/`. Só fazer se ainda estiver acima da meta de memória.

**Passo 4: migrações de C futuras já nascem no lugar certo**

- O C que ainda falta migrar (~100 mil linhas em `server/src`) **não deve entrar no `axia-shell`**.
  Cada migração vira crate próprio (ex.: `axia-vault-*`, `axia-compositor-*`) ou entra em
  `axia-codecs`/`axia-crypto` quando for esse o caso.
- Regra de revisão: se um commit adiciona > 3 mil linhas a `rust/axia-shell/src` ou inclui um novo
  `#[path]` no `lib.rs`, perguntar por que não é um crate.

## 6. Riscos e pontos a resolver (decidir na hora, com teste)

- **R1: símbolos `extern "C"` em crates dependentes.** O compositor C linka `libaxia_shell.a`. É preciso
  confirmar que as funções `#[no_mangle] extern "C"` que passarem a viver em crates dependentes continuam no
  `.a` final (e não são descartadas). Teste: `nm -g libaxia_shell.a` antes e depois; se faltar, reexportar
  explicitamente no `axia-shell` ou deixar a camada FFI sempre no shell.
- **R2: `crate::runtime::node_get` e o cenário.** Várias apps consultam o cenário (nós, layout). O cenário
  é do shell. Opções: uma interface (trait/funções registradas) em `axia-native`; ou mover só o mínimo
  necessário. Não mover o cenário inteiro para a base.
- **R3: tempo de link e de compilação total.** Mais crates = mais unidades; o link do `staticlib` agrega
  todas. Medir o tempo total do `make check` antes e depois do Passo 1.
- **R4: `#[cfg(test)]` e testes que usam internos.** Testes que acessam `crate::...` interno precisam de
  `pub(crate)` ou mover para o crate certo. Priorizar manter testes junto do código.
- **R5: coordenação.** Várias pessoas/agentes mexem nos mesmos `lib.rs`, `native_apps.rs`, `Makefile`.
  Ver seção 7.
- **R6: erro de estimativa.** Se a memória não escalar linearmente, o plano pode ser menos urgente ou mais.
  Por isso o passo 4.4 (instrumentar) vem primeiro e as metas são em GB por crate, não em linhas.

## 7. Regras de coordenação entre agentes (Claude e Codex)

- Um agente por app em cada momento. Antes de mover uma app, avisar no chat/commit para o outro não mexer.
- **Nunca `git add -A`**: listar arquivos pelo nome e conferir `git diff --cached --stat` antes de commitar.
- Arquivos compartilhados que mudam em toda movimentação: `rust/axia-shell/src/lib.rs`,
  `rust/native_apps.rs`, `rust/axia-shell/src/native_view/mod.rs`, `rust/Cargo.toml`, `server/Makefile`.
  Quem mexer neles commita só o seu trecho (`git apply --cached` com patch, se o outro tiver mudanças
  não commitadas no mesmo arquivo).
- Commits sem linha de co-autor (regra do dono do projeto).
- O dono faz o `git push`; o Copr compila a partir do `main`.
- Validar antes de cada commit numa **árvore limpa** (HEAD + só os arquivos da tarefa), no container:
  `make -j2 check GPU=1 CFLAGS='-O2 -g -D_FORTIFY_SOURCE=3 -fstack-protector-strong'`.
  Mais, quando mexer em teste de integração:
  `cargo test --manifest-path rust/Cargo.toml --target-dir server/build/rust --locked --offline
  --test native_apps_runtime --test launcher_runtime --test search_runtime -- --ignored`.
  Mais `python3 -m unittest discover -s tools/desktop/tests` e `make install PREFIX=/usr DESTDIR=/tmp/stage`.
- Para mudanças que tocam no RPM, validar com `rpmbuild --rebuild` num container Fedora 44 com limite
  de memória (`podman run -m 15g ...`) e conferir que termina.

## 8. Ordem recomendada, resumida

1. Esperar o resultado do CI `44227245381` (commit `040e8f72`).
2. Fase 0: 4.1 (sem `cargo test` no RPM), 4.2 (`debuginfo=1`, medindo), 4.4 (instrumentar memória).
3. Fase 1, Passo 1 (spike com uma app minúscula e `axia-native` mínimo). Decidir R1/R2/R3 com dados.
4. Fase 1, Passo 2 em ondas, do menor para o maior, um commit por app.
5. Fase 1, Passo 3 só se o `axia-shell` ainda estiver acima da meta.
6. Em paralelo e desde já: Passo 4 (novas migrações de C não entram no `axia-shell`).

Marcos de acompanhamento (registrar o número em cada um):

- Pico de `rustc` no RPM, em GB, e tempo do job `rpm`, em minutos.
- Linhas do crate `axia-shell` (`find rust/axia-shell/src -name '*.rs' | xargs cat | wc -l` + apps incluídas).
- Meta final: nenhum `rustc` acima de ~5 GB, job `rpm` abaixo de ~30 min, margem de 30% para crescer.

## 9. Decisões que dependem do dono do projeto

- Aceitar que o RPM/Copr deixe de rodar `cargo test` (4.1)? (Os testes continuam no CI normal.)
- Aceitar `debuginfo=1` nos pacotes `-debuginfo` (4.2)?
- Aprovar o desenho de crates (seção 5.2) antes do spike, e se prefere um crate por app ou grupos
  (ex.: `axia-apps-office` com notes/contacts/calendar/reminders).
- Avisar o Codex da regra do Passo 4 (novas migrações já em crates próprios).
