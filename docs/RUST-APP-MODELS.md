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

## Limites com a frente Mail

Esta frente não altera `rust/axia-mail`, `rust/axia-codecs`, arquivos Cargo,
`server/src/mail`, `server/mk/mail.mk` ou o Makefile do servidor. A migração de
MIME, XML, HTML, iCalendar e vCard segue na branch `migration/rust-mail-parsers`.
