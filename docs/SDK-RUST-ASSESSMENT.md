# Avaliação de migração do SDK para Rust

Data: 13/11/2026. Base inspecionada: commit `eca7b122`.

Os números do inventário representam a captura inicial. O repositório está sendo
editado em paralelo pelo Claude Code; a barra nativa adicionada depois dessa
captura não foi contabilizada como trabalho pendente já executado por outro agente.

## Conclusão

`sdk/` é a camada compartilhada da shell, dos temas e dos aplicativos da Axia DE.
Não é uma biblioteca externa isolada. Ela compõe interfaces React, mantém estado
observável, inicia serviços da sessão e conecta esses consumidores às APIs nativas
do objeto `lord`. Migrá-la integralmente envolve contratos fora da pasta.

Já existem implementações Rust importantes. A migração deve reutilizá-las e
substituir os adaptadores gradualmente, evitando uma segunda implementação das
mesmas regras.

Estimativa preliminar, com integração e testes, para profissionais experientes no
projeto e em Rust/UI:

| Entrega | Esforço total | O que continua em JavaScript |
| --- | ---: | --- |
| A. Migrar regras e estado ainda próprios do SDK | 350–650 h | Interfaces React, hooks e adaptadores; não entrega a migração integral |
| B. Implementar também componentes/serviços do SDK nativamente | 1.000–1.800 h | Fachada de compatibilidade para temas/apps React; os arquivos originais podem ser substituídos, mas ainda existe integração JS |
| C. Substituir o SDK e adaptar seus consumidores para dispensar a API JS atual | 1.800–3.200 h | Pode permanecer JS em partes dos apps fora do escopo; retirar todo JS de todo o produto exige outro inventário |

As faixas são alternativas cumulativas de escopo: não devem ser somadas. A opção
C é a referência para eliminar a dependência dos arquivos JS/JSX do SDK. A opção
B permite executar as responsabilidades do SDK em Rust sem converter todos os
consumidores de uma vez. Mover os mesmos adaptadores para outra pasta não conta
como eliminá-los.

Essas horas são estimativas de engenharia, não medições de produtividade nem
orçamentos de mercado. Incluem aproximadamente 25% de reserva sobre o trabalho
planejado. A confiança é baixa a média até um piloto validar os contratos de UI
e o esforço de adaptação dos consumidores. A fronteira nativa existente reduz
trabalho, mas não prova que todos os controles React já tenham equivalentes.

## Inventário medido

- 106 arquivos: 81 `.js` e 25 `.jsx`.
- 14.009 linhas físicas, incluindo comentários e linhas em branco.
- 756.049 bytes, aproximadamente 738 KiB de código-fonte.
- 56 arquivos importam diretamente `react` com `from 'react'`.
- 51 arquivos importam diretamente `@lordos/runtime`.
- 86 arquivos contêm referências `lord.*`, incluindo comentários; esse indicador
  não é uma contagem de chamadas executáveis ou funcionalidades.
- Cinco maiores arquivos: `components.jsx` (599 linhas), `windows.js` (563),
  `dock.js` (466), `rustfiles.js` (437) e `hardware.jsx` (393).

Uma busca textual em fontes, excluindo bundles, encontrou referências ao SDK em
3 arquivos de `runtime/`, 15 de `themes/`, 81 de `apps/`, 3 de `examples/` e 17
de `server/tests/`. Isso mede alcance potencial, não quantidade de arquivos que
necessariamente precisarão ser reescritos. Os diretórios possuem, respectivamente,
673, 2.362, 23.892 e 312 linhas JS/JSX nos quatro primeiros grupos.

## Papel na arquitetura

```text
themes/ + apps/ + pacotes .lpk
              |
              v
sdk/index.js: API pública, startShell e montagem de overlays
              |
              +--> componentes React, stores, serviços e efeitos do SDK
              |                |
              v                v
runtime/: reconciler React e eventos     objeto nativo lord.*
              |                              |
              +-------------> pontes QuickJS + C/Rust
                                             |
                          cena/layout, compositor e helpers
```

`tools/build-theme.mjs` resolve `@lordos/sdk` para `sdk/index.js` e empacota temas
com esbuild. O aplicativo em execução usa QuickJS e o renderer próprio: não é
um navegador nem uma webview. Node/npm são ferramentas de build.

`sdk/index.js` também inicia alarmes, mail, lembretes, hardware, monitoramento de
discos, teclado virtual, restauração da sessão e outros serviços. Seu `LocaleRoot`
monta notificações, prompts, seletores e overlays em camadas. O custo de substituí-lo
inclui preservar ordem de inicialização, descarte e troca de idioma/tema.

`sdk/store.js` expõe subscriptions e `useSyncExternalStore`; `components.jsx`
expõe controles, foco, texto e hospedagem de janelas. Não existe uma tradução
direta desses hooks para uma função Rust que preserve a API dos chamadores.
É necessário manter uma fachada JS ou adaptar os consumidores.

## Trabalho Rust que pode ser reaproveitado

| Área | Evidência no SDK | Implementação Rust existente |
| --- | --- | --- |
| Janelas, desktops e snapping | `windows.js`, `winmodel.js`, `desktopmodel.js` | `winmodel/`, `snap/`, `window_policy.rs` |
| Launcher e busca | `apps.js`, `rustlauncher.jsx`, `search.js`, `rustsearch.jsx` | `applications/`, `launcher/`, `search/` |
| Relógio | `alarms.js`: snapshots e apresentação de efeitos | `clock/` e `apps/clock/` |
| Clipboard e animações | `clipboard.js`, `anim.js`, `dockpreviews.jsx` | `clipboard/`, `anim.rs`, `dock_preview.rs` |
| Rede, brilho e energia | `network.jsx`, `brightness.jsx`, `power.jsx` | `network.rs`, `network_bus.rs`, `brightness.rs`, `power.rs` |
| IME, teclado e entrada | `ime.js`, `keyboard.js`, `osk.js` | `input_method/`, `keyboard/`, `onscreen_keyboard.rs`, `text_edit.rs` |
| Aparência e cena | `theme.js`, `glass.js`, `NativeView` | `appearance.rs`, `native_style.rs`, `runtime/`, `native_view/` |
| Apps e arquivos | `nativeapps.js`, `nativeview.js`, `rustfiles.js`, `nativefiles.js` | `apps/files/`, demais apps compilados e `native_view/` |

Isso não significa que esses arquivos JS sejam dispensáveis hoje. Por exemplo,
`windows.js` mantém o espelho React da lista Rust, preserva identidade dos objetos
e aplica efeitos; `nativeview.js` ainda fornece fatos, executa efeitos e verifica
permissões descritas no manifesto. `rustfiles.js` interpreta árvores/deltas de UI.
Cada responsabilidade precisa de destino e validação antes de remover a ponte.

Os módulos Rust de `axia-shell` são em geral internos (`mod`), com entradas FFI.
O acesso direto por um novo módulo Rust pode exigir uma API interna e tipos
compartilhados; uma ABI JSON existente não é automaticamente a melhor API nativa.

## Escopo herdado do LordOS

`__LORD_STANDALONE__` é definido como `true` pelo build atual. `updates.js`
desabilita explicitamente sua integração com o gerenciador de pacotes LordOS/
Debian nesse modo. `smart.js` também trata a indisponibilidade de administração
no standalone. O README registra outras limitações das integrações antigas.

Logo, existência de um arquivo ou export não comprova que a função esteja ativa
na instalação Fedora. Cada API deve ser classificada como ativa, compatibilidade,
indisponível ou candidata a retirada. Não foi demonstrado que esses módulos
inteiros sejam código morto; parte deles continua exposta e pode ter consumidores.

Preservar o comportamento atual de indisponibilidade é diferente de implementar
o recurso no host. Criar um atualizador Fedora/DNF, restaurar serviços upstream
ausentes ou entregar novos recursos não está incluído nas estimativas.

## Composição do esforço para a opção C

| Frente de trabalho | Horas antes da reserva |
| --- | ---: |
| Contratos, inventário de API, desenho da integração e piloto | 80–140 |
| Modelos, configurações, localização, estado e ciclo da sessão | 160–280 |
| Serviços, efeitos assíncronos, descritores e isolamento | 220–380 |
| Controles nativos, menus, inputs, overlays e UI compartilhada | 300–520 |
| Adaptar temas, host de apps, consumidores e build | 380–650 |
| Paridade, testes de integração, sessão física e estabilização | 300–590 |
| Total antes da reserva | 1.440–2.560 |
| Reserva de 25% | 360–640 |
| Total de planejamento | **1.800–3.200** |

Premissas: reaproveitar renderer/cena existentes; manter aparência e comportamento
atuais; adaptar somente o necessário nos consumidores; não reescrever o compositor
C inteiro; não substituir bibliotecas do sistema; não criar um toolkit gráfico
externo ou redesenhar o produto. Preservar a API de temas/pacotes JS requer um host
de compatibilidade, mesmo que a shell principal passe a ser nativa.

A política para pacotes `.lpk` precisa ser decidida: preservar o host JS, versionar
a API com transição ou aceitar quebra. A opção C prevê adaptar os consumidores
do repositório e manter um host de compatibilidade quando necessário; não prevê
reescrever pacotes externos desconhecidos. Eliminar QuickJS de todos os processos
do produto é um escopo maior.

### Prazo e custo ilustrativos

Considerando 30 horas produtivas por semana por pessoa, a opção C corresponde a
60–107 semanas-pessoa: aproximadamente 14–25 meses para uma pessoa. Para duas
pessoas, planejar aproximadamente 8–14 meses, com dependências, integração e
coordenação; não é uma divisão garantida por dois. Esse prazo supõe dedicação
contínua e não inclui novas funcionalidades concorrentes.

| Escopo | A R$ 150/h | A R$ 200/h | A R$ 250/h |
| --- | ---: | ---: | ---: |
| A: 350–650 h | R$ 52,5–97,5 mil | R$ 70–130 mil | R$ 87,5–162,5 mil |
| B: 1.000–1.800 h | R$ 150–270 mil | R$ 200–360 mil | R$ 250–450 mil |
| C: 1.800–3.200 h | R$ 270–480 mil | R$ 360–640 mil | R$ 450–800 mil |

As taxas são hipóteses para cálculo, não preços pesquisados. Usar a taxa interna
com encargos ou a proposta real do fornecedor. Infraestrutura, equipamentos,
impostos e custos indiretos não contidos nessa taxa devem ser acrescentados.
Não há licença Rust a contratar prevista nesta avaliação. Não é possível
estimar consumo/custo de um agente de IA sem definir modelo e fluxo de execução.

## Riscos e critérios de aceite

1. **Paridade de UI e entrada:** foco, seleção/caret, IME, teclado virtual,
   clipboard, scroll, atalhos, menus, RTL, escalas e múltiplos monitores.
2. **Ciclo de vida:** subscriptions e callbacks não podem sobreviver a janelas,
   contextos, troca de tema ou logout; cancelamentos devem liberar recursos.
3. **Assincronia:** mail/documentos/mídia não podem bloquear a thread da UI;
   falhas e reinício dos helpers devem encerrar pedidos corretamente.
4. **Segurança:** preservar allowlists de chamadas, políticas por app, passagem
   de FDs pelo broker e isolamento. Rust não substitui essas verificações.
5. **Compatibilidade:** manter formatos de settings/sessão, IDs, manifests,
   catálogos, abertura de apps e o contrato dos pacotes suportados.
6. **Limite de responsabilidade:** código próprio ainda em C, integrações do
   host e bibliotecas externas não migram automaticamente com o SDK.
7. **Performance:** medir latência de input, frame time, CPU/RSS e custo de
   snapshots antes/depois. Há potencial ao reduzir polling, JSON e travessias
   JS/nativo; a troca de linguagem sozinha não garante melhoria.

Um módulo só termina quando seus consumidores usam a implementação nova, a
implementação antiga sai do caminho ativo e os testes de comportamento passam.
Exigir também build/RPM offline, testes headless e sessão física para as áreas
afetadas, com registro das limitações de hardware verificadas.

## Sequência recomendada

1. Fixar a API e fazer um piloto de 40–80 h com `dockmodel.js` + um fluxo de
   interface que use o modelo. Essas horas fazem parte da fase inicial, não são
   adicionais ao total. Medir esforço de bindings, teste diferencial e descarte.
2. Migrar modelos puros e validadores: Dock, sessão, SMART, sugestões, scrollbar,
   filtros de rede e teclado virtual. Não duplicar as regras Rust existentes.
3. Consolidar serviços/efeitos e estado em Rust, mantendo temporariamente a
   fachada que entrega snapshots e eventos ao React.
4. Estender `native_view/` com os controles e a infraestrutura da shell; migrar
   módulos verticais completos, incluindo seus chamadores.
5. Adaptar temas, hosts, build e testes. Retirar os arquivos antigos somente
   depois de resolver compatibilidade e comprovar paridade.

Não se recomenda traduzir arquivo por arquivo na ordem alfabética. Os destinos
devem seguir responsabilidades: serviços/modelos Rust, UI nativa e uma camada
de compatibilidade deliberada enquanto existirem consumidores JS.

## Validação da referência, antes da implementação

Foram executadas as oito suítes existentes abaixo, todas aprovadas:

| Comando a partir da raiz | Verificações aprovadas |
| --- | ---: |
| `node server/tests/dock_model_test.mjs` | 94 |
| `node server/tests/session_model_test.mjs` | 48 |
| `node server/tests/desktop_model_test.mjs` | 8 |
| `node server/tests/osk_model_test.mjs` | 65 |
| `node server/tests/smart_parse_test.mjs` | 92 |
| `node server/tests/suggest_engine_test.mjs` | 73 |
| `node server/tests/scrollbar_model_test.mjs` | 43 |
| `node server/tests/netmodel_test.mjs` | 43 |
| Total | **466** |

Essas suítes validam a referência JS e ajudam a planejar testes diferenciais.
Não foi executado `make check` nem validada uma sessão gráfica. Os resultados
históricos na documentação não foram tratados como execução atual.

## Primeira implementação autorizada: modelo da Dock

Foi implementado `rust/axia-shell/src/launcher/dock_model.rs` com validação dos
IDs, conversão de listas antigas, itens obrigatórios, pin/move/unpin e validação
das ações rápidas. A nova operação `op: "dock"` usa a ponte existente do launcher,
disponível na sessão e no host isolado, sem uma nova capacidade de filesystem.

`sdk/dockmodel.js` mantém a API dos consumidores e passa a ser adaptador. Continua
responsável por executar os callbacks JS, preservar identidade de arrays/objetos,
coerções na fronteira e valores que JSON não representa, como NaN/Infinity nos
argumentos aceitos de ações. As regras da Dock são executadas em Rust; não há
fallback JavaScript para a implementação anterior.

Testes Node usam `server/tests/dock_model_node.mjs` e o exemplo Rust
`dock-model`, compilados em `build/sdk-rust`, separado do build do Claude. O
runner foi atualizado para exigir Node e Cargo nas suítes que importam esse
modelo. Não foram alterados os arquivos reservados para a barra do Claude:
`sdk/nativeview.js`, `sdk/index.js`, `sdk/nativemenubar.js`, `native_view/` e
`themes/default/components/MenuBar.jsx`.

Validação após a implementação:

- Build Rust offline do exemplo e da biblioteca: aprovado.
- Oito suítes JS: **483 verificações aprovadas**, sendo 111 da Dock, 48 da sessão,
  8 dos desktops e as 316 das outras cinco suítes. Os 17 casos novos da Dock
  verificam callbacks, coerções, identidade, valores não serializáveis e limites.
- Comparação temporária com a implementação JS capturada antes da migração:
  **1.397 comparações aprovadas**, incluindo operações em listas, posições,
  identidade, normalização, classificadores e ações rápidas.
- Testes Rust do launcher: **27 aprovados e 1 ignorado** (benchmark manual já
  existente), incluindo quatro testes do novo modelo e um teste da nova operação
  pela ABI de produção, sem catálogo inicializado.
- Sintaxe do runner shell e `git diff --check` dos arquivos próprios: aprovados.
- Bundle completo de `sdk/index.js` com esbuild, resolvendo o runtime real:
  aprovado em memória, sem escrever bundles/manifests compartilhados com o Claude.

O linker inicial não encontrou os nomes de desenvolvimento GLib/GIO/GObject,
XKB e Wayland. Para os testes, foram criados somente links locais em
`build/sdk-rust/native-libs` para os `.so.0` já instalados em `/usr/lib64`.
Nenhum pacote ou arquivo do sistema foi alterado. A execução usada foi:

```sh
cargo build --manifest-path rust/Cargo.toml --example dock-model \
  --target-dir build/sdk-rust --locked --offline
node server/tests/dock_model_test.mjs
LIBRARY_PATH="$PWD/build/sdk-rust/native-libs${LIBRARY_PATH:+:$LIBRARY_PATH}" \
  cargo test --manifest-path rust/Cargo.toml -p axia-shell --lib launcher:: \
  --target-dir build/sdk-rust --locked --offline
```

O compositor completo, a ligação QuickJS em execução e a sessão física ainda
precisam ser validados com uma recompilação do servidor que inclua a nova
operação. Um servidor antigo não entende `op: "dock"`. A suíte de ABI verifica
o dispatcher Rust e a propriedade das respostas; o adaptador Node testa a fonte
de política de produção, mas não substitui um teste completo no QuickJS.

Não foram feitos commits, staging, push ou alterações nos arquivos do Claude.
Esta é a primeira etapa funcional, não a conclusão da migração dos 106 arquivos.

## Fontes locais principais

- `sdk/index.js`, `sdk/store.js`, `sdk/windows.js`, `sdk/components.jsx`.
- `sdk/nativeview.js`, `sdk/nativefiles.js`, `sdk/rustfiles.js`, `sdk/nativeapps.js`.
- `sdk/alarms.js`, `sdk/network.jsx`, `sdk/updates.js`, `sdk/smart.js`.
- `runtime/index.js`, `runtime/app-host.js`, `tools/build-theme.mjs`.
- `rust/axia-shell/src/lib.rs`, `rust/axia-shell/src/native_view/host.rs`.
- `server/tests/run_suites.sh`, `server/tests/windows_bridge_test.mjs`, `server/Makefile`.
- `README.md`, `docs/RUST.md`, `docs/SERVER-RUST-MIGRATION.md`, `docs/STANDALONE.md`.

Os arquivos e chamadas atuais prevalecem sobre descrições antigas: há comentários
herdados que ainda descrevem código C ou uma etapa anterior da migração.
