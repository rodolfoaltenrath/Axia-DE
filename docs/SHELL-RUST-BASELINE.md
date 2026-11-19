# Linha de base da migração da shell

Fase 0, 2026-11-18. Código de referência: `ffabc1aa`; somente documentação
mudou desde a otimização de build `40243ed6`. Esta é uma **referência local
headless em software**, anterior à retirada do React da sessão. Não mede uma
sessão física nem o tempo total de um workflow.

## Ambiente e procedência

- Fedora 44 no WSL2, kernel `6.18.58.1-microsoft-standard-WSL2`, oito CPUs
  lógicas; Rust/Cargo 1.98.1; strace 7.3; prioridade `nice -n 19`.
- Executável do RPM local já validado `axia-de-0.8.7-3.fc44.x86_64.rpm`,
  SHA-256 `173cef5fc04756b66c3dbb51b958adcb2712b6c8c7e7593e461a74c80f57342a`.
  Nenhuma recompilação Rust ou bateria completa foi necessária nesta captura.
- O SRPM correspondente foi comparado com 1.565 arquivos das árvores de código
  e assets atuais, normalizando CRLF. As diferenças foram duas terminações de
  arquivo Rust que só mudam espaços finais (`axia-fs/src/lib.rs` e
  `axia-native/src/icons.rs`) e 27 arquivos gerados de `assets/sysapps/`, ausentes
  no checkout limpo. As fontes restantes comparadas coincidem. Esse recorte
  não é uma comparação de todos os arquivos do repositório.
- O bundle `app-host` reconstruído em memória com as fontes atuais tem exatamente
  o mesmo SHA-256 do RPM:
  `6376f28c0a1620b2735acef1ab2ffd3a1e1d43d926bc6337206004a5dcae0659`.
- Instalação extraída em `/var/tmp/axia-phase0-20261118`, sem instalar o RPM
  no host. Cada execução teve HOME e XDG próprios, usuário `nobody` (UID 65534),
  `LORD_GPU=0`, `AXIA_NET_OFFLINE=1`, modo escuro e configurações
  `welcomeShown=true`, `theme=default`. As chaves `...NativeView` não foram
  forçadas: valem os defaults do código dessa captura.
- Não havia PipeWire, servidor de notificações, XWayland ou delegação de cgroup
  nessa sessão de teste. Os logs registram esses limites. Landlock/seccomp dos
  helpers permaneceram ativos; permissões não foram afrouxadas para medi-los.

Os dados brutos, scripts locais de coleta, metafile e comparações estão em
`build/validation/shell-phase0/`, ignorado pelo Git. Os números abaixo ficam
versionados para não depender dessa pasta local. As execuções foram sequenciais,
com cache do sistema de arquivos já aquecido. Não misturar esses resultados com
boot frio, sessão física, perfil de desenvolvimento ou binário com outras flags.

## Sessão ociosa

Tema padrão original do RPM, sem instrumentação JS, em 1180×700. Cada amostra
aguardou oito segundos, coletou vinte segundos de RSS com `/proc/<pid>/status`
(uma leitura por segundo) e contou as chamadas completas do laço principal
registradas por strace. Nenhum app aberto, sem ações de entrada. O tour inicial
foi desativado para não abrir uma janela automaticamente.

| Amostra | Duração de coleta | Chamadas de poll | Chamadas/s | RSS mediano | RSS máximo |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 20,011 s | 249 | 12,443 | 101,352 MiB | 101,387 MiB |
| 2 | 20,011 s | 246 | 12,293 | 101,707 MiB | 101,742 MiB |
| 3 | 20,011 s | 245 | 12,243 | 101,297 MiB | 101,332 MiB |
| Mediana das amostras | — | — | **12,293** | **101,352 MiB** | — |

Contagem com `strace -qq -ttt -T -e
trace=poll,ppoll,epoll_wait,epoll_pwait`, sem `-f`: só a thread principal da
sessão, sem somar os helpers ou as threads de trabalho. A janela temporal é
aplicada aos timestamps das linhas completas do trace. **Chamadas de poll/s
são um indicador da atividade do laço**, não a contagem de todos os despertares
do kernel. Não comparar esse número com context switches, ticks de CPU ou um
perfil de todas as threads como se fossem a mesma métrica.

O RSS inclui os mapeamentos da sessão e não inclui a memória dos helpers. Os
processos terminaram com status zero após SIGTERM. Em toda a execução ocorreram
apenas três ou quatro renders; a frequência de poll não é taxa de quadros.

## Abertura da primeira janela

Em outra execução, uma cópia do tema recebeu instrumentação temporária. No
primeiro callback após inicializar o SDK, abriu-se `calculator` e registrou-se
o relógio após duas callbacks de `requestAnimationFrame`. O início do processo
e o marcador usam CLOCK_MONOTONIC. O instrumento não foi aplicado à medição
ociosa e não entrou no produto.

| Amostra | Processo → marcador da janela | wm.open → duas callbacks de frame |
| --- | ---: | ---: |
| 1 | 752,180 ms | 134,166 ms |
| 2 | 644,914 ms | 128,369 ms |
| 3 | 747,325 ms | 124,767 ms |
| Mediana | **747,325 ms** | **128,369 ms** |

Isso é um **marcador de abertura**, incluindo inicialização do processo/tema.
Não certifica a primeira apresentação de todos os pixels nem mede latência
até um monitor. Na sessão física, medir o primeiro frame apresentado com a
janela pronta e manter essa série separada. Os logs válidos são `window-2.log`
a `window-4.log`; `window-1.log` foi uma coleta de desenvolvimento do instrumento
e não entra na tabela.

## Custo do ciclo da lista de janelas

Na mesma execução instrumentada, o SDK registrou um app de teste sem conteúdo
e abriu 1, 10 ou 50 janelas. Após 30 alterações de aquecimento, mediram-se 250
alterações de título, alternando as janelas. Cada alteração usa `wm.setTitle`,
que chama a operação `patch`; a instrumentação confirmou 250 chamadas reais
à ponte por cenário. As janelas de um cenário foram fechadas antes do próximo.

O tempo total engloba a chamada síncrona do SDK, preparação/serialização dos
fatos, ponte, parsing e atualização do espelho/subscribers. O tempo da ponte
engloba o transporte C e o modelo Rust, inclusive seu JSON; não é o tempo do
Rust puro. Não inclui a pintura posterior da árvore React.
A instrumentação acrescenta leituras do relógio e um registro por chamada;
o ciclo total inclui esse custo. Comparações futuras devem usar o mesmo
instrumento, sem tratá-lo como custo de produção sem observação.

Medianas dos percentis de cada uma das três execuções, em ms:

| Janelas | Ciclo total p50 | Ciclo total p99 | Ponte p50 | Ponte p99 | Entrada/saída da primeira operação |
| --- | ---: | ---: | ---: | ---: | --- |
| 1 | 0,1671 | 0,2668 | 0,0304 | 0,0636 | 203 / 416 unidades UTF-16 |
| 10 | 0,2881 | 0,5809 | 0,0728 | 0,1373 | 203 / 980 unidades UTF-16 |
| 50 | 0,8120 | 1,3074 | 0,2938 | 0,4338 | 204 / 3609 unidades UTF-16 |

Os tamanhos são `String.length` dos argumentos/respostas de uma operação, não
bytes de alocação. Não foram agregadas 750 observações para inventar um novo
p99. O maior p99 total entre as execuções com 50 janelas foi 1,6904 ms.

O modelo já mantém `rev` e envia deltas, mas
[`sdk/windows.js`](../sdk/windows.js) ainda serializa os fatos, chama
`JSON.parse(lord.windowList.call(...))`, preserva partes com comparações JSON
e mantém o espelho React. A Fase 2 deve retirar esse transporte do caminho
nativo, mantendo snapshots compartilhados e revisões tipadas. Não clonar a
lista completa para reproduzir o mesmo custo do outro lado da fronteira.

## Dock e trabalho de renderização

Foi usado o fixture existente
[`server/tests/dock_bench.jsx`](../server/tests/dock_bench.jsx), sem alterar
seu código, em 1920×1080: 24 ícones, 600 movimentos por modo, com e sem
magnificação, descontando um laço ocioso com os mesmos yields. O bundle de teste
foi produzido com as opções de build atuais e colocado apenas na instalação
temporária. O manifesto de desenvolvimento dessa cópia recebeu o hash correto;
a verificação de código continuou ativa.

| Amostra | Extra por movimento, sem magnificação | Com magnificação |
| --- | ---: | ---: |
| 1 | 0,345 ms | 2,926 ms |
| 2 | 0,375 ms | 2,934 ms |
| 3 | 0,316 ms | 2,512 ms |
| Mediana | **0,345 ms** | **2,926 ms** |

Nas três execuções, sem magnificação houve zero renders de ícone por movimento
e 0,12 chamadas de layout; com magnificação, 4,24 renders e uma chamada de
layout. O custo mediano dessa chamada, contabilizado por movimento, foi
0,017/0,126 ms respectivamente.

Os logs do servidor, que somam ambos os modos e a inicialização do fixture,
registraram 23 renders por execução, layout médio 0,07/0,07/0,05 ms e pintura
média 3,13/3,05/2,30 ms. A mediana da soma dessas etapas é **3,12 ms por render**.
Esse recorte não inclui todo o trabalho de JS, espera, composição/apresentação
de GPU ou p99 de frame. **Extra por movimento também não é tempo de quadro.**
A referência de frame completo e input-to-present em hardware segue pendente.

## Protocolo para comparar uma fatia

1. Usar uma instalação do mesmo perfil e guardar revisão, flags, versões,
   resolução, tema/modo, chaves nativas e serviços disponíveis. No Windows,
   preparar o checkout LF com `git -c core.autocrlf=false clone` em
   `/var/tmp/<nome>` no Fedora WSL; aplicar só o diff da fatia e executar
   `npm ci` uma vez. Builds e testes com `nice -n 19`, `make -j2`, sem bateria
   completa quando ela não for necessária.
2. Guardar o pacote/bundle anterior e o posterior; verificar a correspondência
   das fontes antes de reutilizar um binário. Alterações de caminho de build ou
   documentação não autorizam presumir que o executável contém outras mudanças.
3. Executar como `nobody` em HOME/XDG novos e privados, nunca no HOME do dono.
   Manter as opções `--headless 1180x700 --timeout 35 --theme <bundle>
   --fonts <assets/fonts>`, com `LORD_GPU=0`, `AXIA_NET_OFFLINE=1`. Só o fixture
   da Dock usa 1920×1080. Repetir três vezes, sem outros testes pesados em paralelo.
4. Para ociosidade, manter o tema original, aguardar 8 s e coletar 20 s, sem
   timers extras do instrumento no processo. Ler o RSS externamente e guardar
   o trace bruto da thread principal. Repetir também com a árvore de processos
   quando a fatia transferir trabalho para helpers, para não esconder consumo.
5. Para abertura e ciclo de janelas, usar uma cópia instrumentada separada.
   O wrapper da ponte chama a função original com o mesmo `this`; não faz
   `JSON.parse` adicional dentro do intervalo da ponte. As quantidades e
   percentis devem ser os mesmos desta captura. Restaurar o bundle depois.
6. Repetir o benchmark da Dock e registrar suas duas modalidades. Para a
   aceitação visual de uma fatia de UI, obter as quatro capturas exigidas pelo
   roadmap e executar `engine-test`, preservando os golden pixels. Esta fatia
   documental não alterou UI e não substitui esses testes.

Os helpers locais `baseline.py`, `bundle.cjs` e `inventory.cjs` guardam a
instrumentação exata da captura, com logs e JSONs no diretório de evidências.
O arquivo reconstruído do app-host e o metafile não sobrescreveram assets do
checkout. A instalação descartável pode ser removida após preservar os dados.

## Orçamento proposto e trabalho pendente

Como referência **para este ambiente e protocolo**, propõe-se 15 chamadas de
poll/s (maior valor observado + cerca de 20%) e 120 MiB de RSS da sessão.
São margens de detecção, não metas universais de produto. Os limites foram
calibrados depois com seis amostras do runner Fedora e passaram a ser impostos
por padrão no CI; veja a seção de calibração abaixo. Não copiar os números
do WSL para outra máquina sem medição.

O teste posterior deve falhar quando a mediana exceder o orçamento, guardar
as três amostras/versões/logs mesmo na falha e recusar coleta sem sessão viva,
sem janela temporal completa ou com erro no carregamento do tema. Deve contar
sempre o mesmo escopo de threads e usar processo filho rastreável, sem depender
de attach arbitrário a uma sessão do usuário. Uma série de todas as threads ou
em hardware precisa de orçamento próprio.

Pendências que **não estão concluídas por esta documentação**:

- Medir frame completo/primeira apresentação em hardware e ociosidade com os
  serviços reais da sessão. RSS, poll/s e marcadores headless já têm referência.
- Comparar `lto` e `codegen-units`, mantendo flags da distribuição e o mesmo
  protocolo. [`rust/Cargo.toml`](../rust/Cargo.toml) só declara `panic="abort"`
  no perfil release; não acrescentamos flags. O build Fedora já medido usa
  `codegen-units=1`, `debuginfo=2`, `opt-level=3` e frame pointers.

| Variante a avaliar | Situação | Critério |
| --- | --- | --- |
| Flags Fedora atuais, sem novo override de LTO | Referência de runtime acima; build em [RUST-BUILD.md](RUST-BUILD.md) | Preservar comportamento e hardening |
| Thin LTO, se compatível com o empacotamento | Não medida | Runtime, tamanho, tempo e pico de memória do build |
| Outros codegen-units aceitos no perfil de distribuição | Não medidos | Comparar as mesmas métricas; não trocar a política Fedora sem evidência |

O ganho de build do benchmark de 563,96 s para 21,19 s, registrado em
`RUST-BUILD.md`, vem de reaproveitar artefatos. Ele não é resultado de alterar
LTO/codegen-units nem evidência de menor consumo da sessão em runtime.

## Coletor executável e calibração no Actions

`cargo xtask idle-check` recebe `--installation` com a raiz instalada
que contém `bin/`, `assets/` e `themes/`, e `--output` para o JSON. Faz uma cópia
privada legível em `/var/tmp`, usa `nobody` quando iniciado como root e coleta
três amostras de 8 s de aquecimento + 20 s de observação, com prioridade baixa.
O tema original e seu manifesto não são modificados. HOME/XDG são privados e
as conexões herdadas à sessão gráfica/áudio/D-Bus do dono são removidas.

Por padrão, retorna erro se a mediana ultrapassar 15 poll/s ou 120 MiB. Um
poll bloqueado durante toda a janela pode produzir zero; um trace ausente,
uma sessão encerrada cedo ou um tema que falhou são coleta inválida, não passe.
O JSON e os traces/logs de cada amostra são preservados em falhas ordinárias.

O job desktop executa o coletor após `make install`, só na variante GPU=1,
sempre com renderização em software. O artifact `axia-idle` inclui JSON e logs.
As primeiras execuções usaram `--enforce false` para calibrar o runner sem impor
os números do WSL. Após conferir seis amostras válidas de dois workflows, o
default do workflow passou a `true`. `AXIA_IDLE_ENFORCE=false` permite retornar
explicitamente à calibração; falhas de coleta continuam falhando o job.
`AXIA_IDLE_MAX_POLLS` e `AXIA_IDLE_MAX_RSS_MIB` ajustam os limites, inicialmente
15 e 120. Alterar limites exige registrar as novas condições e medições.

O coletor e seus testes estão em Rust, no pacote `xtask`; os arquivos Python
introduzidos para essa coleta foram removidos. A suíte cobre traces incompletos,
medianas, limites, argumentos inválidos e falhas da sessão com diagnóstico.
A integração real, executada separadamente, também verifica o modo de calibração
e a recusa de regressão forçada. Para reproduzir no Linux com `strace` instalado:

```text
cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline
AXIA_IDLE_TEST_INSTALLATION=/caminho/instalado/axia cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test idle real_session_calibration_and_enforcement -- --ignored --nocapture
cargo xtask idle-check --installation /caminho/instalado/axia --output /var/tmp/axia-idle/report.json --enforce false
```

A validação Rust passou 30 testes automáticos e a integração real separada.
As três amostras reais tiveram medianas de 13,050 poll/s e 102,547 MiB de RSS.
O limite forçado de 0,01 poll/s produz `exceeded`: retorna erro com imposição e
sucesso em calibração, preservando o relatório em ambos os modos.

Esta substituição não migra as verificações Python anteriores de empacotamento,
Copr, clipboard e rede. Elas continuam no CI até receberem equivalentes Rust;
novos testes e coletores desta migração devem ser implementados em Rust.
Nenhuma flag ou implementação de C, JS ou React foi otimizada nesta entrega.

### Calibração do runner e imposição, 2026-11-18

Foram conferidos os artifacts `axia-idle` de duas execuções concluídas com
sucesso, com três amostras completas de 20 s em cada uma. O escopo é a thread
principal, software, offline e UID 65534, como no protocolo acima.

| Workflow / revisão | Poll/s: amostras | Poll/s mediano | RSS mediano da sessão |
| --- | --- | ---: | ---: |
| [44393655897](https://github.com/rodolfoaltenrath/Axia-DE/actions/runs/44393655897), `61cd4bb8` | 12,650 / 12,550 / 12,500 | 12,550 | 102,969 MiB |
| [44422355912](https://github.com/rodolfoaltenrath/Axia-DE/actions/runs/44422355912), `c5e595f1` | 12,300 / 12,400 / 12,100 | 12,300 | 103,148 MiB |

Os relatórios estavam em calibração (`enforced=false`), com `status=passed` e
`exceeded=[]`. Cada amostra registrou 20 leituras de RSS, duração completa e
saída zero. Mantêm-se os limites de **15 poll/s e 120 MiB**; agora o workflow os
impõe por padrão. Não foi alterada uma variável remota de repositório: o commit
muda o fallback versionado, preservando o override explícito do operador.

Os ZIPs e traces originais foram preservados localmente em
`build/validation/profile-compare-resume/` no checkout isolado da retomada.
Os links dos workflows acima permitem recuperar os artifacts enquanto retidos
pelo Actions. Essa calibração não conclui as medições de apresentação física.

## Instrumentação de referência do ciclo de janelas

Para reproduzir a série sem depender do helper local ignorado, o prefixo abaixo
vai antes do bundle padrão **na cópia de teste**. A duração da ponte é medida
antes de inserir o registro no array; o ciclo total inclui esse registro.

```js
globalThis.__phase0Calls = [];
const original = lord.windowList.call;
lord.windowList.call = function (op, json) {
  const t = lord.now(), out = original.call(this, op, json);
  __phase0Calls.push({ op, ms: lord.now() - t,
    inputUnits: json.length, outputUnits: out.length });
  return out;
};
```

O sufixo vai depois do bundle, sem alterar seus imports. Os percentis usam o
elemento `ceil((N - 1) × q)` da sequência ordenada. O coletor externo subtrai
o instante de criação do processo de `monotonicMs` para obter processo → marcador.
O resumo dos logs de renderização é produzido pelo próprio servidor ao terminar.

```js
setTimeout(() => {
  const sdk = globalThis.__lordos.require('@lordos/sdk');
  const opened = lord.now(), first = sdk.wm.open('calculator');
  requestAnimationFrame(() => requestAnimationFrame(() => {
    console.log('phase0-first ' + JSON.stringify({ id: first,
      monotonicMs: lord.now(), openAndTwoFramesMs: lord.now() - opened }));
    sdk.wm.close(first, true);
    sdk.registerBuiltinApp({ id: 'phase0', name: 'Phase 0',
      size: [320, 200], Component: () => null });
    const quantile = (a, q) => [...a].sort((x, y) => x - y)[Math.ceil((a.length - 1) * q)];
    for (const n of [1, 10, 50]) {
      const ids = Array.from({ length: n }, () => sdk.wm.open('phase0', { new: true }));
      for (let i = 0; i < 30; i++) sdk.wm.setTitle(ids[i % n], 'warm ' + i);
      __phase0Calls.length = 0;
      const total = [];
      for (let i = 0; i < 250; i++) {
        const t = lord.now();
        sdk.wm.setTitle(ids[i % n], 'measured ' + i);
        total.push(lord.now() - t);
      }
      const calls = __phase0Calls.filter(x => x.op === 'patch');
      console.log('phase0-window ' + JSON.stringify({ windows: n,
        samples: total.length, bridgeCalls: calls.length,
        totalP50Ms: quantile(total, .5), totalP99Ms: quantile(total, .99),
        bridgeP50Ms: quantile(calls.map(x => x.ms), .5),
        bridgeP99Ms: quantile(calls.map(x => x.ms), .99),
        inputUnits: calls[0]?.inputUnits, outputUnits: calls[0]?.outputUnits }));
      for (const id of ids) sdk.wm.close(id, true);
    }
    console.log('phase0-measurements-done');
  }));
}, 0);
```
