# Roadmap: shell, SDK e temas em Rust

Este roadmap acompanha **uma migração específica**: tirar React e JavaScript da
sessão da Axia DE (`sdk/`, `runtime/` e `themes/`, 16.368 linhas em 2026-11-18)
e substituí-los por Rust. Ele complementa:

- [`ROADMAP.md`](ROADMAP.md): o roadmap geral do produto, desde o LordOS;
- [`SERVER-RUST-MIGRATION.md`](SERVER-RUST-MIGRATION.md): a migração do servidor
  em C para Rust (outra frente, com critérios próprios);
- [`SDK-RUST-ASSESSMENT.md`](SDK-RUST-ASSESSMENT.md): inventário e estimativas
  de esforço (13/11/2026) que servem de base a este plano;
- [`RUST.md`](RUST.md) ("Visões nativas") e [`STORE.md`](STORE.md): o host
  nativo e a primeira loja feita nele.

Marque as caixas conforme o trabalho for concluído, no mesmo commit que o conclui.

## Objetivo e decisões

**Objetivo:** a sessão principal (`axia-server`) roda sem QuickJS, sem React e sem
timers JavaScript. Ganho esperado: menos memória residente (o heap JS e o código
parseado no arranque), menos despertares em ociosidade e menos conversões JSON na
fronteira. A troca de linguagem sozinha não garante isso: cada fatia é medida.

| # | Decisão | Escolha | Data |
| --- | --- | --- | --- |
| 1 | Apps `.lpk` de terceiros | **A.** Mantêm o host JavaScript, só no processo isolado de cada app. A sessão não carrega QuickJS. Não se reescreve o contrato dos `.lpk` agora. | 2026-11-18 |
| 2 | Temas de terceiros | **Não haverá.** O visual da shell é só do projeto, em Rust. Não se cria uma segunda forma (JS/React ou dados) de terceiros descreverem a interface. Claro/escuro, cor de destaque e papéis de parede seguem como configurações do usuário, não como temas. | 2026-11-18 |
| 3 | Chaves de transição | Cada fatia nativa fica atrás de uma chave (`...NativeView`) só até passar nos testes; depois o caminho React é apagado. | 2026-11-18 |

Descartado por ora: apps de terceiros como clientes Wayland comuns com um crate
Rust opcional (opção C da conversa). É o destino natural se a plataforma ganhar
desenvolvedores de fora; levaria meses e hoje não há quem dependa dela. A opção A
não a impede: o manifesto, as permissões e a assinatura dos `.lpk` ficam
independentes da forma como o app executa.

## Onde estamos (2026-11-18)

**Já em Rust, sem React:** Monitor, Calculadora, Axia Store, Sobre, Ajuda, Relógio,
Caracteres, Notas, Lembretes, Contatos, Calendário, Câmera, Fotos, Gravador,
Scanner, Backup, Registros, Fontes, Force Quit, Configurações e Arquivos (esse
último atrás de chave). Host em `rust/axia-shell/src/native_view/`.

**Shell com versão nativa atrás de chave:** barra de menus, menus, notificações,
Dock (dica e fantasma de arraste), launcher, busca (Spotlight), barra de título.
Chaves em uso: `menuBarNativeView`, `menuNativeView`, `notificationsNativeView`,
`dockNativeView`, `launcherNativeView`, `spotlightNativeView`,
`titleBarNativeView`, `filesNativeView`, `lordNativeView`.

**Já em Rust no lado dos modelos:** lista de janelas, desktops e encaixe
(`winmodel/`, `snap/`), política da Dock, launcher e busca, clipboard, rede,
brilho e energia, método de entrada, aparência.

**Ainda em JavaScript:**

- 9 apps: `disks`, `editor`, `mail`, `passwords`, `pdf`, `player`,
  `screenshots`, `sysinfo`, `terminal`.
- `sdk/`: 114 arquivos (60 com interface React e 54 só de lógica).
- `themes/default/` (a montagem da shell) e `runtime/` (reconciler React e eventos).
- A captura da Fase 0 encontrou 386 caminhos `lord.*` distintos na AST do JS
  e 491 entradas ao incluir wrappers, manifestos e o prelude. São métodos,
  propriedades e testes de capacidade, não uma nova contagem das funções C
  expostas. Classificação em [SHELL-RUST-API-INVENTORY.md](SHELL-RUST-API-INVENTORY.md).

O tema Classic já foi removido; resta o `default`.

## Definição de pronto de uma fatia

Uma fatia (um app, um painel ou um serviço) só é marcada como concluída quando
**todos** estes pontos valem:

1. A chave de transição foi removida e o código React/JS antigo foi **apagado**.
2. Testes unitários em Rust cobrem a regra; um teste de integração headless
   também em Rust cobre o fluxo real. Novos testes e coletores não usam Python
   nem scripts shell.
3. Capturas de tela nos modos claro e escuro, na largura padrão e na mínima,
   iguais ou melhores que as anteriores.
4. As métricas da Fase 0 não pioram: despertares por segundo em ociosidade,
   memória, tempo de quadro.
5. O estado passa por referência: sem cópia de listas ou de árvores JSON só para
   cruzar a fronteira (por exemplo `Arc` no lugar de `clone`).
6. A documentação da fatia foi atualizada e o commit descreve o que mudou.

## Fase 0: decisões, inventário e linha de base

- [x] Decisão 1 (`.lpk`): opção A.
- [x] Decisão 2 (temas de terceiros): não haverá.
- [x] Inventariar as chamadas `lord.*` usadas pelo JS e classificar cada uma como
      *ativa*, *usada pelos `.lpk`*, *compatibilidade*, *indisponível no Fedora*
      ou *retirar*. Registrar a tabela em `docs/` (herança do LordOS: parte já
      está desligada, como `updates.js` e `smart.js`). Tabela e pontos dinâmicos
      em [SHELL-RUST-API-INVENTORY.md](SHELL-RUST-API-INVENTORY.md); a API de
      hardware ativa não foi confundida com o antigo motor de pacotes.
- [ ] Linha de base de desempenho, registrada em tabela neste arquivo:
      despertares por segundo com a sessão parada (`strace -c -e poll` ou `perf`),
      memória residente da sessão, tempo até a primeira janela, tempo de quadro
      (o benchmark do mouse na Dock já existe) e custo do ciclo de janelas.
      Referência headless inicial registrada abaixo; primeira apresentação e
      frame completo em hardware seguem pendentes.
- [x] Teste que falha se a atividade ociosa passar de um orçamento:
      `cargo xtask idle-check` compara três amostras de poll/RSS; coletor e
      testes implementados em Rust.
      O Actions foi calibrado com seis amostras de dois workflows e agora
      impõe 15 poll/s e 120 MiB por padrão; veja `SHELL-RUST-BASELINE.md`.
- [x] Opções de compilação do Rust (`lto`, `codegen-units`) em `rust/Cargo.toml`,
      medidas contra a linha de base.
      Três builds frios e integração real concluídos em
      [RUST-PROFILE-COMPARISON.md](RUST-PROFILE-COMPARISON.md): vínculo,
      nove amostras ociosas e paridade de pixels/texto. Mantidas as flags Fedora.
- [x] Lista de arquivos do `sdk/` que o host dos `.lpk` (`runtime/app-host.js`)
      precisa: [LPK-SDK-BOUNDARY.md](LPK-SDK-BOUNDARY.md). O grafo atual traz
      **114/114 arquivos**: nenhum é exclusivamente da shell no bundle de hoje.
      Separar os exports e efeitos de inicialização antes de remover módulos.

### Referência inicial, 2026-11-18

Fedora 44 no WSL, usuário `nobody`, `nice -n 19`, sem GPU, sem rede, três
execuções sequenciais. Medianas entre execuções; condições, faixas e evidências
em [SHELL-RUST-BASELINE.md](SHELL-RUST-BASELINE.md). Nenhuma flag Rust foi alterada.

| Métrica | Referência local | Limite da medição |
| --- | ---: | --- |
| Chamadas de poll da thread principal, ociosa | 12,293/s | Indicador do laço; não soma todos os despertares/threads |
| RSS mediano da sessão ociosa | 101,352 MiB | Não inclui os helpers |
| Processo → marcador da primeira janela | 747,325 ms | Calculadora + duas callbacks de frame; não mede apresentação física |
| Layout + pintura médios no fixture da Dock | 3,12 ms/render | Não é frame completo nem p99 |
| Extra por movimento na Dock | 0,345 / 2,926 ms | Sem/com magnificação; não é tempo de quadro |
| Ciclo síncrono WM/JSON/espelho, 50 janelas | p50 0,8120 / p99 1,3074 ms | Alteração de título; sem pintura posterior |

O orçamento inicial proposto para o mesmo ambiente é 15 chamadas de poll/s
e 120 MiB de RSS. Após conferir os artifacts de dois workflows, o coletor no CI
impõe esses limites por padrão (`AXIA_IDLE_ENFORCE=false` permite calibração
explícita). A comparação de `lto`/`codegen-units` foi concluída, mantendo as flags
Fedora; os números estão no relatório específico. Não marcar a Fase 0 inteira
como concluída: primeira apresentação e frame completo em hardware ainda faltam.

**Saída:** decisões fechadas, números registrados, orçamento de ociosidade no CI.

### Ponto de retomada — 2026-11-18

O desligamento interrompeu a primeira tentativa de comparar os perfis; aqueles
registros parciais não entram nos resultados. A retomada concluiu três builds
frios de `c5e595f1`, sem artefatos reaproveitados, em clone isolado Fedora 44 com
Rust 1.99.1. Continuamos na **Fase 0**; nenhuma chave nativa foi removida e a
Fase 1 não começou.

- `fedora`: 389,76 s e pico individual de 2,520 GiB; `thin`: 420,24 s e
  2,961 GiB; `cgu8`: 426,73 s e 1,677 GiB. Uma compilação por variante.
- Integração Rust real concluída: três vínculos, nove amostras ociosas válidas,
  pixels idênticos entre perfis/threads e mesmas métricas de texto. Os arquivos
  da shell medidos permaneceram inalterados durante o vínculo e os instrumentos.
- **Decisão: manter as flags Fedora.** Oito unidades reduzem a memória do
  compilador, mas aumentam tempo, artefatos e RSS da sessão; a pequena vantagem
  de vazão precisa de rodadas independentes. Thin LTO não mostrou vantagem.
- Calibração dos limites do runner concluída com seis amostras de dois
  workflows. O CI impõe 15 poll/s e 120 MiB por padrão.
- O RPM de referência do Actions passou: comando `rpmbuild` em 17min09s,
  pico individual de 2,576 GiB e árvore de 4,534 GiB. Não é o tempo total do
  workflow; veja [RUST-BUILD.md](RUST-BUILD.md).

Protocolo e tabelas em [RUST-PROFILE-COMPARISON.md](RUST-PROFILE-COMPARISON.md).
O clone local é `/var/tmp/axia-phase0-20261118-resume`, com relatórios em
`build/validation/profile-compare-resume/`. Esses diretórios **não acompanham
o push** e podem ser recriados. Não houve novo RPM completo nesta retomada;
o RPM usado como referência é o artifact do Actions em `c5e595f1`.

Continuam pendentes também:

- Medir primeira apresentação e frame completo em hardware real.
- Migrar os testes Python antigos restantes; novos testes e coletores são Rust.
- Iniciar a Fase 1 depois dos critérios da Fase 0, começando por uma fatia
  completa com remoção do React, testes Rust, capturas e comparação de métricas.

## Fase 1: terminar o que já começou

Para cada chave de transição: ligar de vez, apagar o caminho React e os `.jsx`
correspondentes, remover a chave.

- [ ] `menuBarNativeView` e `menuNativeView` (barra de menus e menus).
- [ ] `notificationsNativeView`.
- [ ] `dockNativeView` e as partes nativas da Dock.
- [ ] `launcherNativeView` e `spotlightNativeView`.
- [ ] `titleBarNativeView`.
- [ ] `filesNativeView`.
- [ ] `lordNativeView`: descobrir o que cobre e fechá-la ou documentá-la.

**Saída:** nenhuma chave `...NativeView` restante para esses itens.

## Fase 2: estado e serviços, sem interface

Os 54 arquivos de `sdk/` que são só lógica. O trabalho central é um **mecanismo de
estado em Rust** que substitua `createStore` e `useSyncExternalStore`, para as
views nativas lerem direto, sem JSON.

- [ ] Mecanismo de estado observável em Rust, com notificação de mudança.
- [ ] Lista de janelas: trocar o espelho em JS (`sdk/windows.js`) e o ciclo
      `JSON.stringify` / `JSON.parse` por revisões numéricas (o campo `rev` já
      existe) lidas pelo Rust.
- [ ] Desktops, sessão e restauração (`desktops.js`, `windowmemory.js`,
      `sessionrestore.jsx`).
- [ ] Configurações, idioma e regiões (`settings.js`, `i18n.js`).
- [ ] Notificações, relógio e alarmes (`notify.js`, `alarms.js`).
- [ ] Correio, calendário e contatos (`mail.js`, `calendar.js`, `contacts.js`).
- [ ] Rede, discos e hardware: modelos e vigilância (`netmodel.js`,
      `diskwatch.js`, `hardware.jsx`).
- [ ] Sugestões, atualizações e `smart.js`, conforme o inventário da Fase 0
      (parte está indisponível no Fedora).
- [ ] Timers JS de 2 s ou menos (`netstatus`, `useStats`, `system.js`,
      `diskwatch`): trocar por eventos empurrados do Rust.

**Saída:** nenhum estado de janelas atravessa JSON; os serviços têm só um
adaptador fino para os `.lpk`; os despertares ociosos caem na tabela da Fase 0.

## Fase 3: o resto da interface da shell

Antes, completar os widgets de `native_view/`. Lacunas conhecidas:

- [ ] Composição do IME no campo de texto (o texto em composição ainda não é
      mostrado; o confirmado entra normalmente).
- [ ] Arrastar e soltar entre views.
- [ ] Navegação por teclado e anel de foco.
- [ ] Listas virtualizadas.
- [ ] Acessibilidade e leitura por tecnologias assistivas (avaliar o que a
      sessão oferece hoje).

Fatias de interface (cada uma completa, com seus chamadores):

- [ ] Moldura de janela (`windowshell.jsx`, `Window.jsx`).
- [ ] Overview e alternador de apps (`overview.jsx`, `appswitcher.jsx`).
- [ ] Central de Controle e bandeja (`ControlCenter.jsx`, `tray.jsx`).
- [ ] Diálogos de energia, confirmação e `powerbox` (`power.jsx`,
      `powerbox.jsx`).
- [ ] Teclado virtual (`osk.jsx`).
- [ ] Painéis de rede, hardware, refrigeração e telas (`network.jsx`,
      `hardware.jsx`, `cooling.jsx`, `displaysui.jsx`, `brightness.jsx`).
- [ ] Diálogo de impressão e seletor de arquivos (`printdialog.jsx`,
      `filepicker.jsx`).
- [ ] Emoji, aviso de restauração de sessão, pré-visualizações da Dock.

## Fase 4: o tema

- [ ] Substituir `themes/default/index.jsx` (papel de parede, janelas, barra de
      menus, Dock, camadas e modais) por uma view de shell em Rust.
- [ ] Tokens (cores, medidas, claro/escuro, cor de destaque) como dados internos
      lidos pelo Rust, no lugar de `tokens.js` e `theme.json`.
- [ ] Remover o mecanismo de trocar de tema: `lord.themes`, o seletor "Estilo da
      área de trabalho" nas Configurações, a proteção contra tema quebrado e o
      carregamento de bundles por `--theme`.
- [ ] Aposentar `startShell` e o `LocaleRoot` do `sdk/index.js`.
- [ ] Apagar `examples/theme-minimal`, `themes/README.md` e `THEMES-GUIDE.md`, ou
      deixá-los marcados como históricos, e ajustar os documentos que os citam
      (`DESIGN.md`, `LAYERS.md`).
- [ ] Manter como configurações: modo claro/escuro, cor de destaque e papéis de
      parede, e tudo o que o usuário já escolhe hoje.

## Fase 5: os 9 apps que ainda são JSX

Do mais simples ao mais difícil. Podem andar em paralelo com a Fase 3.

- [ ] `sysinfo`
- [ ] `disks`
- [ ] `screenshots`
- [ ] `pdf`
- [ ] `player`
- [ ] `terminal` (já usa um nó nativo próprio para o conteúdo)
- [ ] `mail` (exibição de HTML)
- [ ] `editor` (edição de texto complexa)
- [ ] `passwords` (processo isolado; manter a fronteira de segurança)

## Fase 6: sem JavaScript na sessão

- [ ] Remover `react-reconciler`, `runtime/index.js` e `runtime/events.js` da
      sessão.
- [ ] Remover as pontes `server/src/ui/js*.c` que só a sessão usava; manter só as
      funções que o host dos `.lpk` chama.
- [ ] A sessão sobe sem QuickJS. Refazer as medições da Fase 0 e registrar a
      comparação aqui.

**Saída:** processo da sessão sem QuickJS; ganho de memória e de despertares
medido e documentado.

## Fase 7: host dos `.lpk` (opção A)

Aqui não se reescreve nada: mantém-se e congela-se.

- [ ] Separar o SDK dos `.lpk` (`runtime/app-host.js` e o que ele importa) do
      resto do `sdk/`, com a lista da Fase 0.
- [ ] Declarar um número de versão para a API dos `.lpk` e documentar o que ela
      garante (`NATIVE-APPS.md`).
- [ ] Manter o manifesto, as permissões e a assinatura independentes da forma de
      execução do app.
- [ ] O QuickJS fica no binário, carregado só quando um app `.lpk` é aberto.

## Ordem e paralelismo

As Fases 0, 1 e 2 vêm primeiro, nessa ordem; a 2 é a que mais rende em consumo.
As Fases 3 e 5 andam juntas depois que os widgets estiverem completos. As Fases 4
e 6 dependem das anteriores. A Fase 7 pode começar assim que o inventário da
Fase 0 existir.

## Riscos

1. **Dois caminhos por tempo demais.** Daí a decisão 3: a chave vive só até a
   fatia passar nos testes.
2. **Paridade de entrada:** IME, RTL, atalhos, vários monitores e escalas. Só se
   confirma em máquina real.
3. **Edição em paralelo.** Vários agentes mexem nos mesmos arquivos (já houve
   conflito em `native_view/host.rs` e `mod.rs`). Dividir as fatias por arquivo e
   commitar cada fatia sozinha.
4. **Cópias na fronteira** Rust ↔ C ↔ JS (um snapshot ou um catálogo copiado a
   cada consulta). Passar por referência.
5. **Regressão visual silenciosa.** O teste de pixels do `engine-test` já pegou
   uma quebra das sombras; manter esse tipo de teste em cada fatia.
6. **Compatibilidade dos `.lpk`.** A API congelada na Fase 7 existe para não
   quebrar quem já criou apps.

## Esforço

[`SDK-RUST-ASSESSMENT.md`](SDK-RUST-ASSESSMENT.md) estima 1.000 a 1.800 h para
entregar as responsabilidades do SDK em Rust e 1.800 a 3.200 h para substituí-lo
por completo, com confiança baixa a média. Parte da shell (barra de menus,
notificações, Dock, launcher) avançou depois dessa estimativa, então o número real
deve ficar na parte baixa das faixas. O prazo de calendário depende do ritmo de
validação em hardware, que não é automatizável.

## Registro de progresso

Acrescente uma linha por fatia concluída: data, fatia, commit e o que as medições
mostraram.

| Data | Fatia | Commit | Medição |
| --- | --- | --- | --- |
| 2026-11-18 | Roadmap criado; decisão 1 (opção A) | | |
| 2026-11-18 | Fase 0: inventário classificado, dependências do host .lpk e referência headless | Commit desta linha (`git log -- docs/ROADMAP-SHELL-RUST.md`) | 491 entradas; 114/114 módulos no host; 12,293 poll/s; 101,352 MiB; detalhes em `SHELL-RUST-BASELINE.md`. CI, flags e apresentação física pendentes. |
| 2026-11-18 | Fase 0: coletor de orçamento ocioso e calibração no CI | Commit desta linha | 40 testes Python; sessão real: 12,400 poll/s e 102,539 MiB; regressão forçada recusada com relatório. Calibração do runner e imposição obrigatória pendentes. |
| 2026-11-18 | Fase 0: coletor e testes migrados para Rust (`cargo xtask idle-check`) | Commit desta linha | 30 testes automáticos e integração real Rust; 13,050 poll/s e 102,547 MiB. Regressão forçada verificada com imposição e em calibração; removidos o coletor e os testes Python desta fatia. |
| 2026-11-18 | Fase 0: telemetria de compilação e seus testes em Rust (`cargo xtask build-memory`) | Commit desta linha | 41 testes Rust; coleta validada em compilação real do xtask com dois jobs. Desktop/RPM usam o coletor Rust; comparação de LTO/codegen-units permanece pendente. |
| 2026-11-18 | Fase 0: protocolo e ferramentas para comparar perfis; ponto de parada | Commit desta linha | 43 testes Rust passaram; primeira compilação cancelada para desligamento. Thin LTO, cgu8 e integração comparativa não executados. Nenhuma flag alterada. |
| 2026-11-18 | Fase 0: calibração confirmada no runner e orçamento obrigatório por padrão | Commit desta linha | Seis amostras válidas em dois workflows: medianas de 12,550/12,300 poll/s e 102,969/103,148 MiB. Mantidos os limites de 15 poll/s e 120 MiB. |
| 2026-11-18 | Fase 0: comparação de perfis concluída com instrumentos e testes Rust | Commit desta linha | Três builds frios e três vínculos reais; nove amostras ociosas e paridade de pixels/texto. Mantidas as flags Fedora. Primeira apresentação e frame completo em hardware continuam pendentes. |
