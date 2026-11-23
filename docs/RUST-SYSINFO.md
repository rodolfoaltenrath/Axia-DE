# Informações do Sistema em Rust

O aplicativo `sysinfo` usa manifesto, ícone, árvore visual, comandos e estado
compilados em Rust. `apps/sysinfo/native.rs` controla as abas Visão geral,
Hardware, Compatibilidade, Verificações, Desempenho e Relatório, incluindo a
prévia de compartilhamento. `native_ui.rs` monta os widgets do host nativo.
Os modelos existentes em `model.rs`, `checks.rs` e `perf.rs` continuam sendo
usados pela interface e pelos contratos históricos da ponte.

`runtime/native_sysinfo_services.js` conserva os acessos de plataforma:
coletores, helper administrativo, benchmarks, arquivos, abertura de apps e
confirmação de reinicialização. Ele entrega dados brutos e executa efeitos;
a interpretação dos resultados, prazos, seleção de abas, conteúdo dos arquivos
e consentimento pertencem ao aplicativo Rust. A montagem geral da janela e
esses transportes ainda dependem da sessão JavaScript.

## Dados privados e exportação

Números de série são consultados somente por ação explícita de um
administrador. A janela recebe proteção de captura antes da entrega dos dados;
o host nativo também protege seu mount antes de reconciliar a árvore privada,
sem depender de um commit React. Ocultar, trocar de aba ou seção, atualizar ou
fechar descarta os números. Respostas atrasadas não os republicam, e um host
antigo não pode retirar a proteção de um nó reciclado.

Periféricos e Bluetooth ficam separados do relatório. O modelo mantém apenas
os campos permitidos para as linhas visuais. A assinatura Bluetooth existe
somente enquanto a janela está montada e atualiza por eventos.

O relatório normal preserva os dados do coletor e o Markdown recebido. O
relatório compartilhável passa pela lista de campos de
`tools/hwreport/hwreport.schema.json`, incluindo limites e nomes permitidos de
propriedades dinâmicas. Números de série e nomes de periféricos não são entradas
desse construtor. A prévia mostra o JSON que será salvo. Salvar congela conteúdo
e formato; uma colisão de nomes tenta os sufixos `-2` a `-99` com os mesmos dados.
Exportar um benchmark usa o resultado exibido, inclusive um resultado recente
que ainda não tenha aparecido no histórico.

O HTML local escapa o conteúdo, não carrega scripts ou recursos externos e
mantém a política CSP. O aplicativo não envia o relatório. Como não há
repositório público configurado, a entrega continua sendo os arquivos locais
e sua abertura pelo navegador escolhido pelo usuário.

## Atividade e benchmarks

Abas ociosas não têm timer nativo. Consultas assíncronas, teste do sandbox e
benchmark em execução possuem cadências limitadas; o teste do sandbox expira
em dez segundos.

A cena do teste de composição usa widgets Rust e o script de movimento do
gerenciador de janelas. O antigo timer JSX de 16 ms escrevia `rotate`, uma
propriedade que o motor não aplica. A cena preserva a carga visual efetiva,
sem transformar esse timer inoperante em uma animação diferente.

## Fontes e validação

Não há JSX, JavaScript ou manifesto JSON em `apps/sysinfo`. Os utilitários
puros de catálogo/validação/relatórios e suas referências ficam em
`tools/hwreport`; os adaptadores de teste da API histórica ficam em
`server/tests/support/sysinfo`. Eles não entram no bundle do aplicativo.

Verificações principais:

```sh
cargo test --manifest-path rust/Cargo.toml --locked --offline -p axia-app-sysinfo -p axia-native -p axia-shell --lib
node server/tests/native_sysinfo_services_test.mjs
node server/tests/native_async_mount_test.mjs
cargo test --manifest-path rust/Cargo.toml --locked --offline -p xtask --test sysinfo_runtime -- --ignored --nocapture
```

A integração exige o compositor GPU=1 compilado e `npm ci`. Exercita os dois
estilos de aparência, claro/escuro e larguras de 760/960 px; verifica páginas,
proteção por pixels, retirada da proteção, cópia do relatório, exportação sem
dados privados e desmontagem. O CI publica as capturas em `axia-sysinfo`.
As evidências locais ficam em `build/validation/native-sysinfo`.

Em 2026-11-23 passaram 15 testes do aplicativo, 17 do suporte nativo, 720 da
shell (sete ignorados), oito contratos LPK e os oito cenários gráficos do
Sysinfo. Seis casos de compartilhamento conferem o construtor Rust contra a
saída do construtor anterior, incluindo versões diferentes, campos privados,
propriedades dinâmicas e limites de listas. As verificações históricas dos
modelos, ferramentas e ponte também passaram. Plataforma/ABI e instalação
privada passaram; oito capturas do alternador permaneceram idênticas.

Na instalação privada final, três amostras ociosas tiveram medianas de
12,400 despertares/s e 104,102 MiB, dentro dos limites de 15/s e 120 MiB.
A primeira rodada também passou (12,650/s e 104,281 MiB); os dois relatórios
foram preservados. Esses resultados não demonstram ganho causal e não
substituem as medições físicas reservadas ao encerramento do roadmap.
