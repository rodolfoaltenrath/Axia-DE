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

**Decisão do usuário em 2026-11-19:** deixar as medições em hardware real para
o encerramento do roadmap. Elas permanecem pendentes, mas não bloqueiam o início
da Fase 1. Manter os testes Rust/headless e os limites já impostos no CI durante
a migração; não declarar medições físicas concluídas sem executá-las.

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

## Onde estamos (2026-11-19)

**Já em Rust, sem React:** Monitor, Calculadora, Axia Store, Sobre, Ajuda, Relógio,
Caracteres, Notas, Lembretes, Contatos, Calendário, Câmera, Fotos, Gravador,
Scanner, Backup, Registros, Fontes, Force Quit, Configurações e Arquivos (esse
último atrás de chave). Host em `rust/axia-shell/src/native_view/`.

**Menus definitivos em Rust:** barra de menus e menus de contexto da sessão;
as chaves `menuBarNativeView` e `menuNativeView` não selecionam mais caminhos.
O menu React dos apps `.lpk` é mantido somente no bundle do host isolado.

**Banners definitivos no host Rust:** a apresentação das notificações da sessão
não consulta mais `notificationsNativeView`. Serviços, timers e descrição da
árvore ainda estão em JS e continuam no escopo da Fase 2; o banner React fica
somente no host `.lpk` para compatibilidade da API.

**Partes da Dock definitivas no host Rust:** dicas e fantasma de arraste não
consultam mais `dockNativeView`. Montagem da barra, estado e prévias ainda
usam adaptadores JS/React e permanecem no escopo das fases seguintes.

**Spotlight definitivo no host Rust:** painel e entrada nativos, com catálogo
de ícones. Estado da sessão, provedores e efeitos ainda passam pelo adaptador JS.

**Launcher definitivo no host Rust:** grade, entrada, menus e rolagem por
viewport. O adaptador JS ainda executa abertura de apps e efeitos da Dock.

**Barra de título definitiva no host Rust:** título e controles nos estilos
Axia e Adwaita; a moldura e os efeitos do gerenciador ainda usam adaptadores JS.

**Arquivos definitivo no host Rust:** navegador e ícone sem chave de transição.
Seletores e transferências ainda usam o adaptador de widgets React e ficam na
Fase 3. Efeitos e serviços seguem no adaptador JS até as fases seguintes.

**Fase 1 concluída:** as chaves listadas foram retiradas dos caminhos ativos.
O item `lordNativeView` foi auditado e documentado abaixo: não existe como
preferência no código atual. O trabalho atual é a Fase 2; validação física
e medições continuam reservadas ao encerramento.

**Já em Rust no lado dos modelos:** lista de janelas, desktops e encaixe
(`winmodel/`, `snap/`), política da Dock, launcher e busca, clipboard, rede,
brilho e energia, método de entrada, aparência.

**Ainda em JavaScript:**

- 9 apps: `disks`, `editor`, `mail`, `passwords`, `pdf`, `player`,
  `screenshots`, `sysinfo`, `terminal`.
- `sdk/`: 116 arquivos JS/JSX. A divisão histórica da Fase 0 está em
  `LPK-SDK-BOUNDARY.md`; os módulos novos continuam no escopo da migração.
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
      frame completo em hardware ficam para o encerramento, por decisão do usuário.
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
Fedora; os números estão no relatório específico. A coleta física permanece
pendente para o encerramento; a Fase 1 está liberada pela decisão de 2026-11-19.

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

### Última entrega concluída antes da pausa

Inicialização da sessão e seus testes migrados para `rust/axia-session`:
`fedora-session` e o comando `axia-session` são executáveis Rust, sem os scripts
Python/shell anteriores. Instalação e ISO usam os novos binários. Passaram 18
testes automáticos, a recusa de root e a integração instalada com compositor e
Calculadora reais. Protocolo em [RUST-SESSION.md](RUST-SESSION.md).

Esta fatia está concluída; a sessão física existente não foi reiniciada.
O trabalho foi pausado a pedido do usuário após a validação e o commit.
Ao retomar, continuam faltando as medições físicas acima e os testes antigos
de Copr, clipboard/rede e verificações gerais de empacotamento. A Fase 1 não
foi iniciada nesta entrega.

## Fase 1: terminar o que já começou

Retomada em 2026-11-19: comparação de perfis e calibração do CI já concluídas;
não repeti-las. O usuário adiou as medições físicas para o encerramento e
liberou a Fase 1 com validação Rust/headless. Também foi iniciada a parte
independente da Fase 7: declaração e documentação da API JavaScript dos `.lpk`.

Para cada chave de transição: ligar de vez, apagar o caminho React e os `.jsx`
correspondentes, remover a chave.

- [x] `menuBarNativeView` e `menuNativeView` (barra de menus e menus).
      Barra já definitiva na revisão recebida; menus de contexto passam a ser
      Rust independentemente das preferências antigas. O menu React pertence
      ao host `.lpk` e é excluído do bundle da shell. Interação e quatro capturas
      headless cobertas por driver Rust; detalhes em `NATIVE-SHELL.md`.
- [x] `notificationsNativeView`: apresentação sempre nativa na sessão, renderer
      React excluído do bundle da shell e preservado apenas no host `.lpk`.
      Integração Rust cobre ações, fechamento, agrupamento, Não Perturbe,
      expiração e posição inferior/RTL em claro/escuro, 640/1000 px.
- [x] `dockNativeView`: dicas e fantasma de arraste sempre nativos; caminhos
      React dessas partes removidos. Integração Rust cobre as três posições,
      cancelamento, itens fixos, remoção e ícones com IDs tipados em claro/escuro,
      640/1000 px. A montagem restante da Dock continua para as fases seguintes.
- [x] `spotlightNativeView`: painel sempre no host Rust, widgets React
      substituídos removidos; catálogo de ícones e transferência/restauração de
      foco corrigidos. Integração com teclado, ponteiro e quatro capturas, além
      da regressão residente de busca, documentadas em `NATIVE-SHELL.md`.
- [x] `launcherNativeView`: grade sempre no host Rust, renderer React removido.
      Rolagem por viewport substitui a faixa fixa de 16.384 px; integração com
      2.048 apps alcança e abre o último, com seis capturas (640/1000/1920 px).
      Grade ampliada a pedido do usuário: sem limite de 780 px, até 14 colunas
      em 1920 px, respeitando margens laterais. Cache, categorias,
      menus, teclado, drag, pacotes, idioma, Unicode e redimensionamento cobertos.
- [x] `titleBarNativeView`: cabeçalho sempre nativo; fallback React removido.
      Oito cenários com os dois estilos, claro/escuro e 640/1000 px verificam
      foco, título, arraste, maximizar/restaurar, minimizar, fechamento com
      guarda e exceções CSD/ownFrame. Corrigido duplo clique nos controles.
- [x] `filesNativeView`: navegador e ícone sempre nativos; escolha por
      preferência e renderer React do navegador removidos. Oito cenários
      Axia/Adwaita, claro/escuro e 640/1000 px cobrem arquivos privados reais,
      navegação, busca, criação, renomeação, prévia PDF e volumes simulados.
      Pixels comprovam página inteira; seletores de abrir/salvar/pasta também
      passam. Seletores e transferências continuam pendentes na Fase 3.
- [x] `lordNativeView`: auditoria do estado atual não encontra essa preferência
      em `sdk/`, `runtime/`, `themes/` ou no host. Apps compilados montam
      `nativeView: 'rust'` diretamente; o navegador Arquivos tinha a chave
      separada acima. `__lordNativeView` é o despachante de efeitos do host,
      ainda necessário nesta fase, e não uma chave de transição. Portanto,
      não há um segundo renderer a promover sob esse nome.

**Saída:** nenhuma chave `...NativeView` restante para esses itens.

## Fase 2: estado e serviços, sem interface

Os módulos de lógica de `sdk/` (54 na captura da Fase 0). O trabalho central é um **mecanismo de
estado em Rust** que substitua `createStore` e `useSyncExternalStore`, para as
views nativas lerem direto, sem JSON.

- [ ] Mecanismo de estado observável em Rust, com notificação de mudança.
      Base `axia-state` implementada e usada pelo proprietário do modelo de
      janelas: leitura por empréstimo, revisão numérica, assinaturas canceláveis
      e invalidações agrupadas, sem dependências externas. Ainda falta ligar
      os demais consumidores e substituir os stores JS dos serviços. O host
      já agenda sincronização imediata de revisões pendentes; a barra de título
      e o conteúdo dos apps nativos leem seu foco diretamente do modelo.
      Contrato e limites em `STATE-RUST.md`.
- [ ] Lista de janelas: trocar o espelho em JS (`sdk/windows.js`) e o ciclo
      `JSON.stringify` / `JSON.parse` por revisões numéricas (o campo `rev` já
      existe) lidas pelo Rust.
      `Session.reader()` já expõe o estado diretamente para consumidores Rust;
      ordem, remoção, desktops e overview também invalidam essa revisão.
      ABI JSON, espelho JS e envio de fatos às views continuam pendentes.
      Título e foco dos cabeçalhos já são lidos diretamente do modelo; o mount
      valida e vincula o proprietário do contexto. A integração altera título
      com o espelho JS parado para comprovar esse caminho. Restam moldura,
      overview, catálogos de janelas e demais consumidores.
      O conteúdo nativo recebe identidade e permissão de entrada, sem foco
      dinâmico no JSON. Quatro cenários gráficos verificam troca direta do
      foco e prioridade de campos JS e sobreposições, com capturas no CI.
      O painel Forçar encerramento também lê a lista por referências Rust;
      retirados sua assinatura do espelho e o envio de `windows` pelo mount.
      Metadados de apps seguem no catálogo compartilhado. Apresentação dos menus,
      APIs de compatibilidade e efeitos ainda usam adaptadores JS.
      O controlador `groups`/`quit` da sessão também lê o modelo Rust, sem
      `appControlFacts` nem janelas nos pedidos. Metadados são compartilhados
      com a view e enviados só quando muda o catálogo/idioma. `runningApps`
      ainda adapta IDs para objetos do espelho; os consumidores dessa API e
      a execução final dos efeitos permanecem em JS.
      O alternador também lê esse modelo: histórico de foco, grupos, escolha
      de janela, seleção e cena ficam em Rust. O adaptador envia só comandos,
      metadados e aparência; não assina a lista nem envia itens/índice. O comando
      separado `cycleAppWindows` agora também resolve grupo e foco no Rust.
      Ocultar app/outros, mostrar todos e mostrar área de trabalho executam uma
      mutação nativa por comando; a memória por desktop pertence ao modelo.
      A resposta ainda atualiza o espelho para a interface legada, mas não envia
      janelas ao Rust para decidir os alvos. Menus consultam `groupInfo`, que
      devolve somente nome e chave. A escolha dos alvos de Sair nos menus de app,
      da Dock e no atalho também é nativa, preservando guardas e IPC de saída.
      `appGroup`/`runningApps` mantêm objetos do espelho por compatibilidade;
      apresentação da barra, callbacks e execução de efeitos continuam em JS.
      O menu Janela da barra agora monta comandos, lista, títulos e desktops
      diretamente no host Rust. Não pede linhas a `barBridge` nem envia uma
      lista de janelas; revisões atualizam o menu aberto. Arranjos e trazer
      todas à frente escolhem alvos atuais em uma transação nativa. A API
      `windowMenuItems` continua disponível por compatibilidade; não é usada
      pela barra da sessão. Outros menus e serviços seguem pendentes.
      Alternador com integração Rust e capturas Axia/Adwaita em claro/escuro e
      640/1000 px; comandos com quatro cenários gráficos. Detalhes em `STATE-RUST.md`.
- [ ] Desktops, sessão e restauração (`desktops.js`, `windowmemory.js`,
      `sessionrestore.jsx`).
      Indicador de troca de desktop agora lê o modelo Rust diretamente: nome,
      posição e pontos são nativos, assim como a expiração de 800 ms. Retirados
      o store, a assinatura JS de apresentação e o temporizador desse indicador.
      Avançar/voltar, criar e mostrar, e fechar o desktop ativo também escolhem
      os alvos no proprietário Rust, com uma revisão por comando. Atalhos
      encaminham somente a intenção; não enviam lista ou desktop ativo.
      Gestos horizontais também decidem no Rust entre trocar desktops e
      alternar janelas, com RTL e limites; não leem o espelho JS da lista.
      Animação também escolhe janelas e acompanha término no Rust, com
      ligações tipadas de molduras e gerações de nós. Não usa assinatura ou
      callbacks JS na sessão; o compositor restaura o frame final.
      A memória de geometria (`windowmemory.js`) também coleta posições e
      conta instâncias diretamente do modelo Rust, com prazo e deduplicação
      nativos. Mesclagem ordenada e limite de 200 apps ficam no Rust; o
      adaptador fornece metadados e persiste as preferências existentes.
      Restauração também pertence ao serviço Rust do contexto: snapshot por
      assinatura, prazos, plano de abertura, posições, confirmação nativa e
      marcadores de saída. Remontar a interface não reinicia o serviço.
      O adaptador ainda grava preferências, abre apps e executa a política
      de navegador. Inicialização, contagem e posições na troca de tema agora
      pertencem a `desktop_session.rs`, com observador sem interface no root
      do SDK. Não dependem do indicador montado pelo tema nem de assinatura
      da lista JS. O adaptador grava patches de preferências; molduras,
      comandos de interfaces legadas, efeitos e configurações continuam pendentes.
- [ ] Configurações, idioma e regiões (`settings.js`, `i18n.js`).
      Configurações já têm proprietário observável Rust por perfil, com padrões
      compartilhados, schemas 1–3, patches, substituições e leitores nativos.
      O SDK é um espelho por revisão, com observador sem polling no root;
      preserva referências de campos inalterados e descarta snapshots antigos.
      Dados pendentes sobrevivem à recriação do adaptador. Carregamento, prazo
      monotônico de 300 ms e serialização/gravação já pertencem ao modelo;
      o loop nativo atende o prazo sem depender de views. API legada de arquivo
      e primitivas C atômicas/duráveis preservadas. Idioma/região e os demais
      consumidores dos fatos de preferências ainda precisam migrar.
- [ ] Notificações, relógio e alarmes (`notify.js`, `alarms.js`).
      Lista, histórico limitado, agrupamento, Não Perturbe, entrega, IDs e
      prazos de notificações pertencem ao contexto Rust. A view monta os
      banners diretamente desse proprietário, sem árvore/lista JS e sem
      timer JS. Ações revalidam IDs e índices antes dos callbacks; o host
      LPK preserva o comportamento local. Entrega, agrupamento e toque agora
      leem o proprietário de configurações do perfil, com assinatura nativa;
      o registro de apps vistos também é escrito diretamente nesse proprietário.
      Consultas/edições dos controles legados, metadados, callbacks e stores
      de compatibilidade ainda usam o adaptador.
      O relógio agora publica invalidações Rust. Um observador no root agenda
      a próxima ocorrência e entrega os efeitos sem setTimeout JS na sessão;
      o indicador da barra consulta o engine diretamente e atualiza a contagem
      no host, sem store/fatos de relógio ou intervalo JS de apresentação.
      Consultas não invalidam consumidores; remontagens preservam o engine.
      Configuração do engine do relógio, callbacks genéricos e demais efeitos
      continuam pendentes.
      Correção pontual: toque repetido só é armado para banner entregue e
      audível, e é cancelado na retirada, silêncio, recusa ou no prazo de
      um minuto. Após a confirmação de que o Actions foi corrigido e a meta
      foi retomada, avançou o agendamento acima. A repetição do toque agora
      também pertence ao serviço Rust de notificações: prazo monotônico de
      60 segundos, cadência de 2,5 segundos, cancelamento e revalidação de
      pulsos antes do som, sem intervalo ou seleção de banners JS na sessão.
      Templates e ações Parar/Adiar/Repetir agora pertencem ao serviço Rust,
      inclusive no histórico. Repetir consulta a duração atual do engine;
      entrega e pulsos verificam o firewall nativo atual. Catálogos do usuário,
      fallback e revisão de idioma preservados. Callbacks genéricos,
      consultas/edições legadas de preferências e espelhos continuam no adaptador. O serviço Rust agora
      também executa o som diretamente no cliente de áudio nativo: fila tipada,
      validação na drenagem e descarte de tentativas sem conexão, sem efeito
      de som ou chamada de reprodução JS na sessão. Saída física no encerramento.
      Padrões, opções por app, precedência da permissão de pacotes e mesclagem
      de alterações de notificações agora são resolvidos no Rust; entrega e
      toque leem as configurações atuais do perfil. APIs de consulta não invalidam o
      modelo nem drenam efeitos. Consultas/edições legadas de configurações,
      metadados e espelhos continuam pendentes. A leitura
      de permissões e firewall agora consulta os armazenamentos nativos na
      entrega e nos pulsos, sem fatos calculados pelo JS. Escopos de apps e
      exceções preservados; revogação sem alterar settings cancela o toque.
      A escrita de permissões de notificações, validação do ID e revogação
      após sucesso agora são compartilhadas no Rust pelo SDK e pela entrada
      pública usada por Configurações. O aviso de revisão para controles de
      privacidade permanece no adaptador, com mesclagem após alterações de assinantes.
- [ ] Busca: retirar estado/timers do adaptador `search.js`, manter os provedores
      e substituir ícones descritos por funções JS por recursos serializáveis;
      o host nativo ainda usa placeholder para esses ícones personalizados.
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
      Alternador nativo implementado, sem a antiga faixa React; abertura,
      atalhos e aplicação do foco ainda têm adaptador JS. Overview e a retirada
      desses adaptadores permanecem pendentes; esta caixa não está concluída.
- [ ] Central de Controle e bandeja (`ControlCenter.jsx`, `tray.jsx`).
- [ ] Diálogos de energia, confirmação e `powerbox` (`power.jsx`,
      `powerbox.jsx`).
- [ ] Teclado virtual (`osk.jsx`).
- [ ] Painéis de rede, hardware, refrigeração e telas (`network.jsx`,
      `hardware.jsx`, `cooling.jsx`, `displaysui.jsx`, `brightness.jsx`).
- [ ] Diálogo de impressão e seletor de arquivos (`printdialog.jsx`,
      `filepicker.jsx`).
- [ ] Widgets compartilhados dos seletores e da janela de transferências de
      Arquivos (`sdk/rustfiles.js`): preservar callbacks de abrir/salvar/pasta,
      múltipla seleção e a sessão dona dos trabalhos de transferência. O
      navegador já é nativo; isso não conclui os diálogos compartilhados.
- [x] Aviso de restauração de sessão: interface e teclado nativos, com serviço
      persistente no contexto e bloqueio de entrada somente enquanto visível.
- [ ] Emoji e pré-visualizações da Dock.

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
- [x] Declarar um número de versão para a API dos `.lpk` e documentar o que ela
      garante (`NATIVE-APPS.md`). API JavaScript versão 1, publicada pelo host
      e pelo SDK; resolução de módulos e identidade dos stores cobertas por
      teste Rust exigido no CI. A separação dos entry points continua pendente.
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
| 2026-11-18 | Fase 0: testes da divisão de build migrados para Rust | Commit desta linha | Três verificações reais de Cargo/Make passaram; removido `test_rust_build_partition.py`. Os testes dependentes do ambiente são exigidos explicitamente no CI. Suíte leve: 47 testes passaram no host e no container. |
| 2026-11-18 | Inicialização da sessão e testes migrados para Rust | Commit desta linha | 18 testes automáticos, recusa de root e integração real instalada passaram. Removidos os dois scripts e testes Python da inicialização; memória observada do lançador: 2,555 MiB em sessão privada. Pausa solicitada pelo usuário após concluir esta entrega. |
| 2026-11-19 | Fase 7: API JavaScript dos `.lpk` declarada como versão 1 | Commit desta linha | 47 testes leves e contrato adicional Rust/Node passaram; bundles reconstruídos. Módulos e stores preservados, versão do host imutável; nenhum campo obrigatório ou alteração de assinatura/permissões. Separação dos entry points pendente. Medições físicas adiadas para o encerramento a pedido do usuário. |
| 2026-11-19 | Fase 1: menus da sessão sempre em Rust | Commit desta linha | Removida a escolha por preferência e o menu React do bundle da shell; compatibilidade `.lpk` selecionada no build. Testes Rust verificam a separação, cliques, submenus, itens desabilitados, Escape e clique externo em quatro combinações de modo/largura, com PNGs. Sem nova medição física ou alteração de flags. |
| 2026-11-19 | Fase 1: apresentação das notificações sempre no host Rust | Commit desta linha | Removido o fallback React da shell e a escolha por preferência; compatibilidade `.lpk` mantida no host. Quatro cenários reais e capturas, contratos de bundle e regressão dos menus passaram. Estado e timers continuam para a Fase 2; medições físicas para o encerramento. |
| 2026-11-19 | Fase 1: dicas e fantasma da Dock sempre no host Rust | Commit desta linha | Removida a chave e os dois renderers React substituídos. Corrigida a resolução de IDs tipados no ícone arrastado. Integração Rust real passou em quatro casos, cobrindo as três posições e política de arraste; capturas publicadas pelo CI. A Dock completa ainda depende de JS/React; métricas físicas seguem para o encerramento. |
| 2026-11-19 | Fase 1: Spotlight sempre no host Rust | Commit desta linha | Removido o renderer React substituído e a chave; catálogo de ícones enviado ao host e foco anterior suspenso/restaurado. Testes Rust cobrem entrada, seleção, provedores, clique externo e quatro capturas; regressão de busca residente preservada. Launcher continua pendente, incluindo a faixa fixa de rolagem. |
| 2026-11-19 | Fase 1: launcher sempre no host Rust | Commit desta linha | Removido o renderer React; rolagem monta apenas viewport e margem, alcançando o fim de 2.048 apps. Corrigidos catálogo dinâmico, menu por teclado e conflito entre clique no fundo e encerramento do host. A pedido do usuário, grade sem limite de 780 px e com colunas conforme a tela. Seis capturas em 640/1000/1920 px e regressão ampla com 1.029 apps; testes Rust também cobrem touchpad e barra. Efeitos ainda usam o adaptador JS; métricas físicas continuam adiadas. |
| 2026-11-19 | Fase 1: barra de título sempre no host Rust | Commit desta linha | Removida a escolha por preferência e o renderer React substituído. Corrigida a maximização indevida por duplo clique num controle, reproduzida e verificada em Rust. Oito cenários Axia/Adwaita, claro/escuro e 640/1000 px, com janelas reais e PNGs. A moldura, o fallback de arraste e os efeitos ainda usam o adaptador JS; validação de grab físico e métricas ficam para o encerramento. |
| 2026-11-19 | Fase 1: navegador Arquivos sempre nativo; chaves encerradas | Commit desta linha | Removidos o renderer React do navegador e sua preferência; ícone nativo. Corrigido o recorte de PDFs com contain, reproduzido no compositor anterior e verificado por pixels do centro/topo/rodapé. Oito cenários em Rust cobrem arquivos privados, navegação, renomeação, PDF, seletores e volumes simulados. Smoke JS/Python substituído pela integração Rust exigida no CI. lordNativeView auditado como preferência inexistente no código atual; o despachante __lordNativeView permanece até as fases seguintes. Seletores e transferências registrados na Fase 3; validação física adiada ao encerramento. |
| 2026-11-19 | Fase 2 iniciada: base observável e proprietário do estado de janelas | Commit desta linha | Crate axia-state sem dependências externas; leitores por empréstimo, revisões e assinaturas canceláveis com invalidações agrupadas. Session guarda WindowState nessa base e expõe leitura nativa. Testes distinguem mutações, ordem e remoção de leituras, resync e operações sem efeito. ABI JSON, espelho JS e consumidores gráficos ainda pendentes; sem nova medição física. |
| 2026-11-19 | Fase 2: cabeçalhos consomem título e foco do estado observável | Commit desta linha | Mount vincula leitor Rust à lista de janelas do contexto, validando o objeto opaco sem reter ponteiro C/JS. Revisões pendentes pedem tick imediato; não há timer de assinatura após consumo. Removidos título/foco dinâmicos dos fatos JSON do cabeçalho. Oito cenários gráficos comprovam título atualizado com espelho JS parado e fonte inválida rejeitada; 108 testes do host, 34 do modelo, contratos LPK e integração dos apps passaram. Espelho JS, ABI JSON e demais consumidores continuam pendentes; sem medição física. |
| 2026-11-19 | Fase 2: conteúdo nativo lê foco do modelo Rust | Commit desta linha | Apps com janela recebem leitor tipado e identidade; o JS só fornece bloqueios de entrada para campos e painéis. Troca de foco vale antes do próximo quadro, sem foco dinâmico nos fatos. Quatro cenários gráficos e capturas, 109 testes do host, integração dos apps, dois contratos LPK e oito cenários de Arquivos e de cabeçalhos passaram. CI GPU=1 exige native_focus_runtime e publica as capturas. Espelho JS, moldura e demais serviços continuam pendentes; medições físicas no encerramento. |
| 2026-11-19 | Fase 2: lista nativa do Forçar encerramento | Commit desta linha | Leitura emprestada do modelo por contexto, sem enviar lista de janelas à view. Retirado windowFacts do NativeView; catálogo de apps resolve nomes, ícones e identidade desktop com a regra compartilhada do launcher. Host revalida seleção antes de ações com revisões pendentes. Passaram 12 testes do app, 110 do host, 34 do modelo, 51 do launcher, contrato desktop, quatro cenários novos, regressões de foco/launcher, integração dos apps e dois contratos LPK. CI publica axia-forcequit. Efeitos de encerramento, menus, runningApps, espelho JS e demais serviços continuam pendentes. |
| 2026-11-19 | Fase 2: controlador da sessão consulta janelas Rust | Commit desta linha | O serviço groups/quit usa leitor do próprio contexto; removido appControlFacts e o transporte de janelas nos pedidos. Catálogo compartilhado com a view e atualizado apenas quando os metadados mudam, inclusive após desmontar a interface. Passaram 13 testes do app, três da ponte, quatro cenários gráficos com superfície atual e janela ausente do espelho, integração dos apps, regressões de Arquivos/launcher/foco e dois contratos LPK. Execução final dos efeitos, adaptador de objetos de runningApps, menus e espelho JS seguem pendentes. |
| 2026-11-19 | Fase 2: menu Janela lê o modelo diretamente | `8b37287d` | Lista, títulos, desktops e comandos montados pelo host Rust, com revisões enquanto aberto. Arranjar e trazer todas à frente escolhem alvos atuais em uma transação; preservada a API legada dos apps. 525 testes unitários, três contratos LPK, quatro cenários gráficos do novo menu e regressões aprovados; instalação e alternador instalado verificados. |
| 2026-11-19 | Fase 2: indicador de desktop e prazo nativos | `16ebe870` | Eliminados store e timer JS do indicador. Fonte tipada, legenda, pontos e prazo monotônico de 800 ms no host; sem polling oculto. 526 testes unitários, quatro cenários gráficos e três contratos LPK aprovados. Animação e restauração permanecem pendentes. |
| 2026-11-19 | Fase 2: navegação e ciclo de vida dos desktops no proprietário Rust | `8058267b` | Avançar/voltar, criar e mostrar, fechar o ativo consultam o estado atual sem lista/seleção JS nos pedidos. Uma revisão por operação, limites sem mutação e destino das janelas preservados. 527 testes unitários, quatro contratos LPK e integrações do indicador/comandos de janela aprovados; instalação e alternador instalado verificados. Persistência, restauração e demais serviços ainda pendentes; medições físicas reservadas ao encerramento. |
| 2026-11-19 | Fase 2: memória de geometria lê o proprietário Rust | Commit desta linha | Coleta, prazo de 800 ms, deduplicação, identidade, contagem de instâncias e mesclagem ordenada limitadas a 200 apps no Rust. Retirados timer e leitura/assinatura do espelho JS dessa função; adaptador ainda persiste preferências. 531 testes unitários da shell, quatro cenários reais com acesso ao espelho bloqueado, quatro contratos LPK e regressões aprovados. Restauração e migração das configurações continuam pendentes. |
| 2026-11-19 | Fase 2: serviço e confirmação da restauração no Rust | Commit desta linha | Snapshot nativo, fila, prazos, geometria, desktops, claims e marcadores pertencem ao contexto; remontagem não reinicia o serviço. Adaptador executa abertura, gravação e política de navegador. 539 testes unitários, quatro contratos LPK e 16 cenários gráficos de restauração/foco/indicador/memória passaram; instalação temporária e switcher-check aprovados. Configurações, efeitos e demais consumidores JS continuam pendentes; medições físicas no encerramento. |
| 2026-11-19 | Correção do aborto no encerramento detectado no Actions | Commit desta linha | Registro de views encerrado explicitamente antes dos serviços e do contexto JS, eliminando acesso ao serviço TLS de Arquivos já destruído. Regressão Rust reproduziu SIGABRT antes da correção; depois passaram timeout/SIGTERM em GPU=0 e GPU=1, 540 testes unitários, quatro contratos LPK e regressões de Arquivos/restauração. CI exige o novo teste em ambas as variantes e publica logs. |
| 2026-11-19 | Fase 2: inicialização e persistência dos desktops no contexto Rust | Commit desta linha | Inicialização única e atômica, contagem por assinatura, consultas posicionais e associação de superfícies na troca de tema sem ler o espelho JS. Observador sem interface no root, independente do indicador do tema e sem polling ocioso; adaptador grava patches. 547 testes unitários, quatro contratos LPK, quatro cenários ampliados do indicador e regressões de restauração/comandos/encerramento passaram. Instalação e switcher-check aprovados; animação e demais serviços continuam pendentes. |
| 2026-11-19 | Fase 2: seleção e conclusão da animação de desktops no Rust | Commit desta linha | Ligações de molduras validadas por geração, seleção nativa, montagem tardia e restauração automática sem callbacks JS. Reversões, RTL, movimento reduzido, Overview e edições explícitas de estilo preservados. 552 testes unitários, quatro cenários gráficos com pixels, quatro do indicador, quatro contratos LPK e encerramento instalado aprovados. Molduras, interfaces legadas e demais serviços ainda pendentes. |
| 2026-11-19 | Correção do smoke agregado desatualizado no Actions | Commit desta linha | Run 44595705926 falhou porque a fixture não registrava a janela após a migração de título/foco nativos em e77d3bc5. Reproduzido localmente; fixture agora usa janela real e a encerra após os cliques. Executor/assertivas agregadas migrados do check.py para shell_surfaces_runtime em Rust, obrigatório no make check para GPU=0/1, com sete etapas e captura. Ambas as variantes e saída normal/SIGTERM aprovadas; removido o bundle antigo do build. |
| 2026-11-19 | Fase 2: decisão dos gestos horizontais no proprietário Rust | Commit desta linha | Troca de desktops ou ciclo de janelas escolhidos pelo estado atual, com RTL, limites e filtragem compartilhada com a API legada. 553 testes unitários, quatro contratos LPK, oito cenários gráficos de indicador/animação, encerramento instalado e smoke agregado passaram em GPU=1. Integração bloqueia leitura do espelho da lista; fixtures agora definem RTL explicitamente, sem depender de catálogo árabe ausente. Reconsultado Actions: última execução ainda é 44595705926, cuja fixture de título foi corrigida em 796b6a89; confirmação remota aguarda push. |
| 2026-11-19 | Fase 2: serviço e banners de notificações no contexto Rust | Commit desta linha | Lista/histórico limitados, IDs, agrupamento, DND, política de entrega, ações e prazos monotônicos no proprietário nativo; a view não recebe árvore/lista JS. 558 testes unitários, cinco contratos LPK, quatro cenários gráficos com fonte opaca, espelho parado e remontagem, encerramento instalado e smoke agregado passaram em GPU=1. Captura inspecionada; callbacks e consumidores de compatibilidade, preferências, permissões/firewall, som e alarmes continuam pendentes. |
| 2026-11-19 | Correção pontual do toque de alarmes, sem migrar o relógio | Commit desta linha | Reproduzidos som repetido para entrega recusada/DND/silenciosa, retirada do banner e estado preso no prazo final. Guardas e cancelamento do intervalo existente corrigidos, com liberação das assinaturas. Seis testes Rust/Node do contrato, quatro cenários gráficos reais, encerramento instalado e smoke agregado aprovados em GPU=1. Migração do relógio adiada a pedido; push permanece com o usuário. |
| 2026-11-20 | Fase 2: agendamento e indicador do relógio no host Rust | Commit desta linha | Invalidações nativas do engine, prazo único no observador independente do tema e indicador que lê timer/alarmes por empréstimo, sem estado nos fatos nem timer JS de agendamento/apresentação. Engine preservado na remontagem; símbolos vetoriais. Passaram oito testes do core, 566 da shell, seis contratos LPK, quatro cenários novos com espelho parado, quatro de notificações, encerramento instalado, smoke agregado e integração dos apps compilados. Captura inspecionada; CI GPU=1 exige native_clock_runtime e publica evidências. Toque repetido, preferências, callbacks/templates e efeitos continuam pendentes. Alterações da Dock de 54d0e9ed preservadas. |
| 2026-11-20 | Fase 2: repetição do toque no serviço Rust de notificações | Commit desta linha | Cadência de 2,5 s e prazo monotônico de 60 s vinculados ao banner; sem intervalo JS na sessão. Política, dispensa, ação e retirada cancelam o toque; pulsos atrasados são revalidados e não formam rajadas. Reproduzido e corrigido snapshot antigo que ressuscitava itens do espelho, agora protegido por revisão numérica. Passaram 571 testes unitários, seis contratos de compatibilidade, oito cenários gráficos de relógio/notificações, encerramento instalado, smoke agregado e integração dos apps compilados. Captura inspecionada. Templates, callbacks, preferências e execução final do som permanecem pendentes. |
| 2026-11-20 | Fase 2: templates e ações de alarmes no serviço Rust | Commit desta linha | Conclusões do observador e comandos do SDK criam notificações diretamente no proprietário nativo. Textos, hora, catálogos do usuário e revisões de idioma no Rust; Parar/Adiar/Repetir são ações privadas, inclusive no histórico. Repetir usa a duração atual do engine. Reproduzida entrega após bloqueio tardio de firewall; corrigida com consulta ao armazenamento nativo na entrega e nos pulsos. Passaram 578 testes unitários, seis contratos LPK, oito cenários gráficos de relógio/notificações, encerramento instalado por timeout/SIGTERM e integração dos apps compilados. Evidências em build/validation/clock-delivery; captura inspecionada. Preferências, callbacks genéricos, espelhos e execução final do som continuam pendentes; medições físicas no encerramento. |
| 2026-11-20 | Fase 2: execução nativa do som de notificações | Commit desta linha | Serviço Rust guarda IDs/tipo dos pulsos em fila privada e chama snd_play diretamente, validando retirada e firewall antes da reprodução. Eliminados efeito de som e reconsulta ringAudible pelo JS da sessão; host LPK preservado. Falha de áudio descarta a tentativa sem adicionar timer de recuperação. Passaram 578 testes unitários, 17 testes focados após reforço da guarda, seis contratos LPK, oito cenários gráficos com ausência de efeitos/chamadas JS de som e encerramento instalado por timeout/SIGTERM. Captura inspecionada; evidências em build/validation/native-notification-audio. Preferências, callbacks genéricos e espelhos continuam pendentes; áudio físico no encerramento. |
| 2026-11-20 | Fase 2: resolução nativa das preferências de notificações | Commit desta linha | Padrões globais e por app, precedência da permissão de pacotes e mesclagem das edições no Rust. SDK transporta preferências brutas e estado da permissão; entrega não aceita opções efetivas pré-calculadas. Consultas/mesclagem não expiram banners, drenam efeitos ou invalidam leitores. Passaram 583 testes unitários, seis contratos LPK, oito cenários gráficos e encerramento instalado por timeout/SIGTERM. Integração verifica padrões JS alterados sem influência no resultado nativo, edição sem perda de outras opções e ausência de appPrefs no transporte. Evidências em build/validation/native-notification-preferences. Persistência/transporte de configurações, acesso a permissões, metadados, callbacks genéricos e espelhos permanecem pendentes; medições físicas no encerramento. |
| 2026-11-20 | Fase 2: consulta nativa das permissões e do firewall de notificações | Commit desta linha | Rust consulta perm_get e restrict_blocks nos armazenamentos reais, deriva escopos de apps internos/pacotes/isolados e preserva exceções de Configurações, app vazio e namespace Linux. Removidos fatos permission/blocked dos pedidos JS. Pulsos reconsultam a permissão atual e descartam tentativas enfileiradas após revogação, sem atualização de settings e sem retomar toque cancelado. Passaram 586 testes unitários, seis contratos LPK, oito cenários gráficos e encerramento instalado por timeout/SIGTERM. Integração usa permissões/firewall reais com consultas JS simulando liberação; catálogo do cenário mantém identidade coerente. Evidências em build/validation/native-notification-access. Escrita de permissões, catálogo, configurações, callbacks genéricos e espelhos ainda pendentes; medições físicas no encerramento. |
| 2026-11-20 | Fase 2: gravação nativa das permissões de notificações | Commit desta linha | SDK e entrada pública usada por Configurações compartilham escrita Rust, com validador de ID existente, perm_set protegido e aviso ao broker somente após sucesso ao negar/retornar a ask. API do SDK não chama setters JS; aviso de revisão mantém os controles legados atualizados. Mesclagem considera alterações de assinantes durante a atualização de permissão. Passaram 590 testes unitários, seis contratos LPK, oito cenários gráficos e encerramento instalado por timeout/SIGTERM em GPU=1. Integração verifica armazenamento real, revisões, setters JS bloqueados e ID inválido recusado. Evidências em build/validation/native-notification-permission-write. Configurações, catálogo, avisos/espelhos de compatibilidade e callbacks genéricos ainda pendentes; medições físicas no encerramento. |
| 2026-11-20 | Fase 2: proprietário observável das configurações por perfil | Commit desta linha | Padrões compartilhados por JSON de dados, schemas 1–3 e mutações no Rust; leitor nativo e observador sem polling no root. SDK é espelho por revisão, preserva referências inalteradas, descarta snapshots antigos e usa o proprietário atual em updaters/gravação. Inicialização única mantém dados pendentes; confirmações antigas não limpam alterações novas. Corrigida a identidade de campos na API pública de migração, exigida por Arquivos. Passaram 595 testes unitários, seis contratos LPK, vinte cenários gráficos de configurações/Arquivos/notificações/relógio, encerramento instalado e integração dos apps compilados. CI GPU=1 exige native_settings_runtime e publica captura; evidências em build/validation/native-settings. Gravação de 300 ms, persistência/serialização de arquivo, idioma/região e demais consumidores de preferências ainda pendentes; medições físicas no encerramento. |
| 2026-11-20 | Fase 2: carregamento e persistência das configurações no Rust | Commit desta linha | Bootstrap lê o perfil privado; modelo controla prazo monotônico de 300 ms, serialização e confirmação, com primitivas C existentes de limite/rename/fsync. Loop atende a gravação mesmo sem views; falhas conservam dados sem repetição por quadro. API legada sincroniza o proprietário após sucesso. Passaram 599 testes unitários, seis contratos LPK, vinte cenários gráficos de configurações/Arquivos/notificações/relógio, encerramento instalado por timeout/SIGTERM e integração dos apps compilados em GPU=1. Teste bloqueia I/O JS desde os imports, exige ausência de timers JS de gravação e salva antes da primeira view; captura inspecionada. Evidências em build/validation/native-settings-io. Idioma/região e consumidores de preferências continuam pendentes; medições físicas no encerramento. |
| 2026-11-20 | Fase 2: notificações leem as configurações nativas do perfil | Commit desta linha | Controlador e views vinculam leitor/assinatura ao proprietário privado; entrega, agrupamento e toque ignoram preferências antigas do transporte. Removidos agrupamento nos fatos, preferências nos pedidos de entrega e assinatura JS de política do toque. Registro de apps vistos é escrito no modelo Rust e publica snapshot por revisão. Passaram 601 testes unitários, dois testes focados após revisão, seis contratos LPK, doze cenários gráficos de notificações/relógio/configurações, encerramento instalado por timeout/SIGTERM, smoke agregado e integração dos apps compilados em GPU=1. Integração altera agrupamento/DND diretamente no Rust com espelho JS antigo; captura inspecionada. Evidências em build/validation/native-notification-settings. Consultas/edições legadas, configuração do engine do relógio, catálogo, callbacks genéricos e espelhos ainda pendentes; medições físicas no encerramento. |
