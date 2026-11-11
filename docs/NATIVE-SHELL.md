# Shell nativo em Rust

O objetivo é retirar QuickJS e React da sessão. Reescrever um interpretador
JavaScript não faz parte dessa migração: cada interface passa a usar a cena
Rust já existente, até não restarem consumidores do motor.

## Estado validado

- Monitor: árvore, rasters, entrada e atualização periódica nativos.
- Calculadora: árvore e ícone nativos, cálculo por teclado e ponteiro, foco,
  separador decimal e feedback de pressão/hover em Rust.
- Force Quit e Sobre: árvores e ícones nativos; o adaptador React de widgets
  desses apps foi removido. Efeitos ainda usam os serviços da sessão.
- Host compartilhado: reconciliação por chave, botões, campos, rolagem,
  eventos de ponteiro em views e teclado de aplicações. Teclas só são consumidas
  mediante resposta explícita do app; atalhos do shell mantêm a precedência.
- Dock: ícones do lançador e da lixeira nativos, com os dois temas e estados
  vazio/cheio. A geometria foi comparada com 15 árvores capturadas do JSX anterior.
- Ícones: geometria dos apps compilados e de Arquivos, imagens do catálogo e
  fallback de inicial sobre gradiente podem ser montados pelo host Rust.

A moldura das janelas, Dock, menu, busca e ajustes ainda usam os adaptadores
JavaScript/React. O host de apps `.lpk` também executa JavaScript. Portanto,
QuickJS **ainda é uma dependência necessária** e não foi retirado do build.

## Sequência de retirada

1. Completar widgets e integração nativa e migrar os consumidores do host React.
2. Migrar as superfícies do shell, preservando camadas, foco, atalhos,
   vários monitores, arraste, menus e acessibilidade.
3. Substituir os efeitos e serviços intermediados pelo JS por interfaces Rust.
4. Converter os apps restantes e o contrato de pacotes; remover o motor,
   reconciliador, ferramentas e testes que deixarem de ter consumidores.

Cada etapa deve funcionar antes da remoção do caminho anterior. Não há
compromisso de compatibilidade binária entre bytecode QuickJS e apps Rust;
a transição de pacotes precisa ser implementada antes da exclusão do motor.

## Verificação

`cargo test --manifest-path rust/Cargo.toml --locked --offline --lib` cobre os
controladores e a cena Rust. `native_apps_runtime` é a integração com o compositor
headless, teclado real da ponte, remontagem e ausência de textos React na
Calculadora e no Monitor. Os testes do host Rust também exercitam cliques,
limites entre ancestrais no hover, liberação fora do controle e atalhos/foco.
