# Migração dos modelos dos aplicativos

As etapas desta frente preservam a interface existente enquanto transferem
suas regras para Rust. Um modelo migrado **não significa** que o aplicativo
inteiro já deixou React. A troca da renderização é uma etapa separada, que
precisa manter o desenho, a interação e os serviços de cada tela.

## Configurações

`apps/settings/model.rs` controla o catálogo ordenado das 23 páginas, os quatro
grupos da barra lateral, cores e identificadores de ícones, busca pelos nomes
e palavras-chave traduzidos e visibilidade por touchpad, Raspberry Pi e modo
desktop independente. Mantém também a última página aberta e o alias antigo
`themes` → `appearance`.

`index.jsx` conserva os componentes, estilos e efeitos React existentes. Envia
os fatos para `lord.nativeApps.create('settings')` e resolve os identificadores
das páginas para seus componentes. A instância do modelo dura pelo contexto
QuickJS, preservando a seleção quando a interface é remontada por troca de idioma.

Validação: testes de política em Rust, compilação dos temas e servidor, e teste
de integração `native_apps_runtime` no compositor headless real.

## Terminal

`apps/terminal/model.rs` e `data.rs` são donos dos 16 esquemas de cores, dos
valores iniciais dos perfis, da composição das preferências, da seleção por
perfil padrão e da conversão para `lord.term.configure`. O catálogo usa uma
lista de pares para preservar a ordem dos esquemas na tela de preferências.
`profiles.js` transporta dados e mantém os efeitos de salvar e traduzir nomes.
`keys.rs` controla também códigos físicos, modificadores, bindings, precedência
das atribuições por perfil, bytes enviados ao PTY e símbolos dos menus. A ordem
de inserção dos atalhos é preservada, inclusive quando duas ações disputam a
mesma combinação. Texto é devolvido como unidades UTF-16 para manter escapes,
caracteres suplementares e substitutos isolados. A ponte restaura ArrayBuffer
para atribuições hexadecimais, como antes. Sessões, preferências visuais e
emulador ainda têm etapas pendentes.

## Senhas

`apps/passwords/model.rs` e `data.rs` controlam categorias internas e do usuário,
tipos e modelos de campos, herança por categoria/template, pertencimento,
filtros e contagens da barra lateral. A contagem é calculada em lote, sem uma
chamada à ponte por entrada. A ordenação por nome continua usando o comparador
localizado existente no host.

O adaptador envia apenas metadados de listagem. Valores secretos não entram no
modelo; os pedidos ao cofre, cópia sensível, proteção de capturas, desbloqueio e
eventos continuam pelo caminho existente. Pais cíclicos em categorias importadas
têm travessia limitada. A interface ainda usa os componentes JSX existentes.

## Informações do Sistema

`apps/sysinfo/checks.rs` controla as verificações de configuração, o resultado
do Sandbox Probe e o resultado do Test Bench. `model.rs` controla camadas de GPU,
motivos da composição por CPU, compatibilidade, seções de hardware, revisão e
throttling do Raspberry Pi, nomes da máquina e parsers das linhas de memória e
números de série. Os periféricos são reconstruídos por campos permitidos, sem
endereços ou caminhos D-Bus, e mantêm a ordenação estável por tipo.
Preserva níveis e estados, chaves
de tradução, notas e parâmetros. O limite de Landlock continua sendo ABI 6 e a
presença de um mecanismo não equivale a um teste efetivo. Datas locais e
serialização JSON continuam pelo host. O coletor, desempenho, compartilhamento
e interface ainda precisam migrar.

## Comparação com a implementação anterior

`rust/axia-shell/tests/fixtures/app_models.json` contém casos de entrada e saída
capturados antes da troca das implementações JavaScript. `terminal_keys.json`
e `sysinfo_hardware.json` completam os casos dos atalhos e hardware. Os testes Rust e
`server/tests/app_model_bridge_test.mjs` usam os mesmos casos, verificando também
os adaptadores JS reais, persistência e listagem sem valores secretos.

O executável de testes `app-model` compila os mesmos módulos Rust usados pelo
compositor. Serve apenas de ponte para as suítes Node; não é usado pela sessão.

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline --lib
cargo build --manifest-path rust/Cargo.toml --locked --offline --example app-model
cd server
node tests/app_model_bridge_test.mjs
node --import ./tests/native_app_model_node.mjs tests/sysinfo_model_test.mjs
```

## Limites com a frente Mail

Esta frente não altera `rust/axia-mail`, `rust/axia-codecs`, arquivos Cargo,
`server/src/mail`, `server/mk/mail.mk` ou o Makefile do servidor. A migração de
MIME, XML, HTML, iCalendar e vCard segue na branch `migration/rust-mail-parsers`.
