# Roadmap: `axia-libs`, a interface comum dos apps nativos

Objetivo: todos os apps nativos com as mesmas cores, medidas e componentes, tendo o **Monitor**
como referência visual. Este documento registra o diagnóstico de 2026-11-26 e o plano. Nada aqui
foi implementado.

## Diagnóstico

### 1. Três fontes de cor

| Fonte | Onde | Quando vale | Janela / lateral / cartão (escuro) |
|---|---|---|---|
| Axia | `rust/axia-shell/src/shell_theme.rs` (e os valores iniciais de `appearance_source.rs`) | opção "Visual tradicional nos aplicativos nativos" **desligada** | `#1f1f23` / `#26262b` / sem token |
| Adwaita (estilo GNOME) | `files_theme` em `rust/axia-native/src/style.rs` | opção **ligada**, que é o padrão (`nativeAppearance != "axia"`) | `#2d2d2d` / `#303030` / sem token |
| Paleta própria | `apps/monitor/theme.rs` | sempre no Monitor; ignora a opção acima | `#1b1b1e` / `#232327` / `#2a2a2f` |

É a principal razão de o Monitor destoar: com o padrão atual, os outros apps usam cinzas neutros
e cantos de 3 a 6 px, e o Monitor usa a própria paleta azulada, com cartões, sombra e cantos de
12 px. **Teste sem código:** desligar a opção em Configurações → Aparência já aproxima os outros
apps do Monitor.

Também têm cores próprias, em parte por bons motivos (sintaxe, papel, dados): `editor/ui.rs`,
`notes/editor.rs`, `mail/html.rs`, `passwords/data.rs`, `store/ui.rs`, `scanner/mod.rs`,
`terminal/prefs_ui.rs` e `screenshots/markup.rs`.

Faltam tokens para o que o Monitor usa: fundo atrás de cartões, cartão, trilho de barras,
item selecionado discreto e sombra. Por isso cada app inventa os seus.

### 2. Cada app tem seus componentes

Quase todo app define suas próprias funções `view`, `text`, `button`, `row` etc. (mais de 10 em
`passwords`, `mail` e `sysinfo`; 7 a 9 na maioria dos grandes). Folhas de diálogo, campos de
formulário, interruptores, seletores segmentados e linhas de barra lateral são reescritos em
cada um.

### 3. Cores fixas no código

Literais `"#…"` e `rgba(` em fontes que não são de teste:

| App | Cores fixas | App | Cores fixas |
|---|---:|---|---:|
| terminal | 546 | store | 65 |
| settings | 204 | calendar | 61 |
| mail | 165 | screenshots | 57 |
| passwords | 132 | contacts | 56 |
| reminders | 113 | notes | 53 |
| files | 102 | disks | 49 |
| editor | 85 | monitor | 42 |
| pdf | 70 | demais | menos de 40 cada |

A contagem inclui cores legítimas (dados, sintaxe, ícones). Ela serve de linha de base: o número
só deve cair.

### 4. Medidas sem escala

- Linhas, itens e pílulas de 26 a 36 px de altura: alturas 26, 28, 30, 32, 33 e 36, com cantos
  5, 6, 7, 8, 13, 14 e 18.
- Cantos: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11 e 12 aparecem muitas vezes.
- Fontes: pelo menos 16 tamanhos diferentes (12, 11 e 13 são os mais comuns; há 8, 9, 10, 14,
  15…).

## Decisões antes de começar

1. **Nome.** `axia-libs` (proposto por você). Os outros crates seguem `axia-<assunto>`; se a
   biblioteca for só de interface, `axia-ui` diz mais. O plano abaixo usa `axia-libs`.
2. **Padrão visual.** Recomendação: o desenho do Monitor passa a ser o tema Axia, e o Axia passa a
   ser o padrão.
3. **O que fazer com "Visual tradicional" (Adwaita).** Recomendação: manter como **variante de
   tokens** (outros valores, mesmos componentes), nunca como caminho de código separado. A outra
   opção é removê-lo, o que simplifica mais.
4. **Claro e escuro.** O Monitor já define as duas paletas; elas são o ponto de partida.

## Arquitetura proposta

Um crate `rust/axia-libs`, usado pelos apps e pelas telas do shell escritas em Rust. Ele **não**
é um novo motor de desenho: produz as mesmas árvores de nós (JSON) que o host nativo já entende.
Assim, a migração pode ser feita um app por vez.

| Módulo | Conteúdo |
|---|---|
| `tokens` | `Palette` tipada, calculada do tema do sistema (modo, destaque, efeitos, variante). Inclui os que faltam: `canvas`, `card`, `track`, `selected`, `shadow`, `line`, `danger`, `success`, `warning`. |
| `metrics` | Escalas fixas. Espaço: 4, 8, 12, 16, 24. Cantos: 6 (controle), 8 (linha), 12 (cartão e folha). Fonte: 11, 12, 13, 15, 20, 26. Alturas: linha lateral, barra de ferramentas, campo, botão. |
| `data` | Cores de séries (gráficos, categorias, etiquetas), iguais nos dois modos. |
| `widgets` | Componentes abaixo. |

Componentes, quase todos já existentes no Monitor (`apps/monitor/dashboard.rs` e `ui.rs`):

- **Estrutura:** `AppShell` (lateral + conteúdo, com recolhimento em janela estreita),
  `SidebarNav` (seção, item com ícone, contador e seleção), `PageHeader` (ícone em ladrilho,
  título, subtítulo, ações), `Toolbar` com `ToolButton`.
- **Conteúdo:** `Card`, `CardTitle`, `Stat`, `Legend`, `Grid`, `Separator`, `ListRow`,
  `EmptyState`, `Banner`.
- **Controles:** botões (primário, normal, destrutivo, de texto), `Segmented`, `Switch`, `Field`,
  `FormRow`, `Link`.
- **Sobreposições:** `Sheet`/`Modal`, menu e dica.
- **Ícones:** desenhados com views ou rasters, como os do Monitor. Hoje vários apps usam
  caracteres como `✎ ⚑ ⚙`, que dependem de fontes de símbolos instaladas no sistema.

Regras: componentes recebem a `Palette`, nunca cores soltas; textos vêm do catálogo de tradução;
todos funcionam com RTL; o que for clicável tem estado de foco e de passagem do ponteiro.

## Fases

### Fase 0 — referência
- [ ] Capturar cada app no compositor, claro e escuro, antes de qualquer mudança (base de
      comparação).
- [ ] Fechar as quatro decisões acima.
- [ ] Registrar os valores do Monitor (cores, espaços, cantos, fontes) como especificação.

### Fase 1 — tokens
- [ ] Criar `rust/axia-libs` com `tokens`, `metrics` e `data`.
- [ ] Fazer `shell_theme.rs`, `appearance_source.rs` e `files_theme` usarem essa única fonte.
- [ ] Testes: toda combinação de modo, destaque, efeitos e variante produz todos os tokens, com
      contraste mínimo de texto.

### Fase 2 — componentes, com o Monitor como primeiro cliente
- [ ] Extrair os componentes do Monitor para `widgets`.
- [ ] Migrar o Monitor **sem mudança visual**: as capturas da Fase 0 devem coincidir. É a prova
      de que a biblioteca reproduz a referência.
- [ ] Galeria de componentes (uma janela de teste que mostra todos, nos dois modos e em RTL).

### Fase 3 — migração, um app por commit
Cada leva termina com capturas comparadas.

- [ ] Pequenos, para validar a API: Sobre, Sobre o app, Forçar encerrar, Calculadora, Relógio,
      Caracteres, Ajuda.
- [ ] Os mais visíveis: Arquivos (inclui o seletor de arquivos do sistema) e Configurações.
- [ ] Sistema: Discos, Informações do sistema, Registros, Loja, Backups, Fontes.
- [ ] Pessoais: Mail, Calendário, Contatos, Lembretes, Senhas.
- [ ] Mídia: Fotos, Player, PDF, Câmera, Gravador, Scanner, Capturas de tela.
- [ ] Com superfícies próprias (a moldura migra; sintaxe, papel e terminal mantêm suas cores):
      Terminal, Editor de Texto, Notes.

### Fase 4 — telas do shell
- [ ] Menus, popovers, diálogos de permissão, impressão e restauração de sessão.

### Fase 5 — não deixar divergir de novo
- [ ] Teste que falha quando aparece uma cor literal fora de `axia-libs` (exceções listadas: dados,
      sintaxe, ícones) ou uma função local que duplica um componente. Começa com a contagem atual
      como teto e só permite reduzir.
- [ ] Guia curto em `docs/` com os componentes e quando usar cada um.

## Pronto, para cada app

- Cores e medidas só de `axia-libs` (salvo as exceções listadas).
- Componentes da biblioteca para lateral, cabeçalho, cartões, botões, campos e folhas.
- Teste no compositor com capturas claro e escuro, na largura mínima e numa larga.
- Teclado, foco, RTL e rolagem (roda comum, roda de alta resolução e touchpad) conferidos.

## Cuidados

- Vários testes no compositor conferem cores de pixels e chaves de nós; eles mudam junto com
  cada app.
- Quem prefere o visual Adwaita não deve perdê-lo sem aviso (decisão 3).
- Apps Linux (GTK e Qt) recebem modo e destaque pelo portal; os tokens precisam continuar
  coerentes com `server/src/ui/portal.c`.
- Migrar por app, com commits isolados, evita uma troca grande difícil de revisar e de reverter.
