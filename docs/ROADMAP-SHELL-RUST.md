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
| 2 | Temas de terceiros | **Em aberto.** Opções: tema como dados (cores e medidas) com componentes em Rust, ou tema só do projeto. Recomendação: dados. | |
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
- Cerca de 456 funções `lord.*` expostas ao JS em `server/src/ui/js*.c`.

O tema Classic já foi removido; resta o `default`.

## Definição de pronto de uma fatia

Uma fatia (um app, um painel ou um serviço) só é marcada como concluída quando
**todos** estes pontos valem:

1. A chave de transição foi removida e o código React/JS antigo foi **apagado**.
2. Testes unitários em Rust cobrem a regra; um teste de integração headless
   cobre o fluxo real.
3. Capturas de tela nos modos claro e escuro, na largura padrão e na mínima,
   iguais ou melhores que as anteriores.
4. As métricas da Fase 0 não pioram: despertares por segundo em ociosidade,
   memória, tempo de quadro.
5. O estado passa por referência: sem cópia de listas ou de árvores JSON só para
   cruzar a fronteira (por exemplo `Arc` no lugar de `clone`).
6. A documentação da fatia foi atualizada e o commit descreve o que mudou.

## Fase 0: decisões, inventário e linha de base

- [x] Decisão 1 (`.lpk`): opção A.
- [ ] Decisão 2 (temas de terceiros).
- [ ] Inventariar as chamadas `lord.*` usadas pelo JS e classificar cada uma como
      *ativa*, *usada pelos `.lpk`*, *compatibilidade*, *indisponível no Fedora*
      ou *retirar*. Registrar a tabela em `docs/` (herança do LordOS: parte já
      está desligada, como `updates.js` e `smart.js`).
- [ ] Linha de base de desempenho, registrada em tabela neste arquivo:
      despertares por segundo com a sessão parada (`strace -c -e poll` ou `perf`),
      memória residente da sessão, tempo até a primeira janela, tempo de quadro
      (o benchmark do mouse na Dock já existe) e custo do ciclo de janelas.
- [ ] Teste que falha se os despertares ociosos passarem de um orçamento.
- [ ] Opções de compilação do Rust (`lto`, `codegen-units`) em `rust/Cargo.toml`,
      medidas contra a linha de base.
- [ ] Lista de arquivos do `sdk/` que o host dos `.lpk` (`runtime/app-host.js`)
      precisa; o restante é "só da shell".

**Saída:** decisões fechadas, números registrados, orçamento de ociosidade no CI.

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

- [ ] Decisão 2 aplicada: substituir `themes/default/index.jsx` (papel de parede,
      janelas, barra de menus, Dock, camadas e modais) por uma view de shell em
      Rust.
- [ ] Tokens do tema como dados lidos pelo Rust (`tokens.js`, `theme.json`).
- [ ] Aposentar `startShell` e o `LocaleRoot` do `sdk/index.js`.
- [ ] Atualizar `THEMES-GUIDE.md` e `examples/theme-minimal` conforme a decisão 2.

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
