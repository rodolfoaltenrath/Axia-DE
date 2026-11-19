# Comparação dos perfis Rust da shell

Experimento da Fase 0, iniciado em 2026-11-18. A configuração de produção em
`rust/Cargo.toml` e as flags do RPM permanecem inalteradas. Esta comparação
avalia `axia-shell` e suas dependências; não é uma reconstrução de todos os
helpers nem uma medição do workflow completo.

## Protocolo

- Retomada em container rootless Fedora 44, Rust/Cargo 1.99.1-2.fc44 e LLVM
  22.1.8, sobre host Fedora 45. Limites do container: quatro CPUs e 15 GiB RAM.
  Host Xeon E5-2667 v4, 16 CPUs lógicas, 23,4 GiB RAM e 8 GiB swap. A sessão
  física do usuário continuou aberta, sem ações de teste sobre ela.
- Fonte de referência: clone limpo de `c5e595f1`, com LF. Os três builds usam
  exatamente essa revisão, sem copiar arquivos do trabalho paralelo. Somente
  depois deles o driver e o instrumento Rust foram copiados para executar a
  integração. Nenhuma implementação de produção foi alterada.
- Dois jobs, prioridade `nice -n 19`, compilação incremental desligada,
  diretório Cargo novo por variante. Execuções sequenciais, cache do sistema
  de arquivos aquecido. Uma compilação por variante; não há distribuição
  estatística dos tempos de compilação nem comparação com boot frio.
- Flags base obtidas de `rpm --eval '%{build_rustflags}'`:
  `-Copt-level=3 -Cdebuginfo=2 -Ccodegen-units=1 -Cstrip=none -Cforce-frame-pointers=yes --cap-lints=warn`.
- `fedora`: LTO falso, uma unidade; `thin`: Thin LTO, uma unidade;
  `cgu8`: LTO falso, oito unidades. A variante `cgu8` acrescenta o override
  em `RUSTFLAGS`, porque somente mudar o perfil Cargo não supera a flag Fedora.
- A coleta Rust amostra RSS a cada 200 ms. Páginas compartilhadas podem ser
  somadas várias vezes; picos muito curtos podem não aparecer.
- O tamanho do arquivo estático inclui código e símbolos de depuração; não é
  o tamanho do pacote instalado nem uma medida do RSS da sessão.
- Assets e helpers da integração vieram do RPM Fedora 44 do workflow
  [44422355912](https://github.com/rodolfoaltenrath/Axia-DE/actions/runs/44422355912),
  revisão `c5e595f1`. SHA-256 do ZIP `rpm`:
  `5d91dddf0b24024e6c729870c9182d46c71433d10fc8cb87820f0fc724582933`.
  O RPM foi extraído somente na instalação descartável, sem instalar no host.

`cargo xtask profile-check` exige um diretório novo e preserva JSON, log Cargo,
versões, revisão, alterações locais e a relação de artefatos novos/reutilizados.
Recusa uma comparação bem-sucedida com artefatos reutilizados.

## Reprodução

Preparar um clone LF no Linux. Executar os três comandos abaixo **em sequência**,
com `nice -n 19` e caminhos absolutos novos para os artefatos:

```text
cargo xtask profile-check --variant fedora --target-dir /var/tmp/axia-profiles/fedora --output /var/tmp/axia-results/fedora.json
cargo xtask profile-check --variant thin --target-dir /var/tmp/axia-profiles/thin --output /var/tmp/axia-results/thin.json
cargo xtask profile-check --variant cgu8 --target-dir /var/tmp/axia-profiles/cgu8 --output /var/tmp/axia-results/cgu8.json
```

Para a integração, preparar uma instalação descartável com os assets e helpers
do RPM de referência; criar o arquivo marcador `.axia-profile-stage` dentro da
raiz que contém `bin/`, `themes/` e `assets/`. O driver troca somente o compositor
dessa cópia. Não apontar para a instalação do sistema.

```text
AXIA_PROFILE_RESULTS=/var/tmp/axia-results AXIA_PROFILE_STAGE=/var/tmp/axia-stage/usr/libexec/axia cargo test --manifest-path rust/Cargo.toml -p xtask --locked --offline --test profile_runtime -- --ignored --nocapture --test-threads=1
```

O driver Rust vincula novamente o compositor por variante, conservando os
objetos C, GPU=1 e `-O2 -g -D_FORTIFY_SOURCE=3 -fstack-protector-strong`, as flags
C usadas no job desktop. A biblioteca Rust de benchmark incorporada ao
compositor também é compilada com o perfil correspondente, em `bench-c/`
separado por variante; esse build auxiliar não entra no tempo da compilação
fria da shell. Não se mistura a biblioteca sem `native-host` usada pelo C com
a biblioteca do instrumento nativo. Os helpers permanecem os do RPM de
referência. Cada sessão usa software,
HOME/XDG privados, `nobody` quando iniciada por root, rede offline, três amostras
de 8 s de aquecimento + 20 s de observação e os limites iniciais de 15 poll/s e
120 MiB. Os relatórios por variante e `runtime.json` preservam os resultados.

Antes de vincular, o driver recusa relatórios parciais, fontes modificadas,
compiladores ou flags divergentes, alvos reutilizados e telemetria incompleta.
Também confere o tamanho do arquivo medido e verifica que o vínculo e o build
do instrumento não o recompilaram.

O instrumento manual `axia-profile-ui`, escrito em Rust e não instalado, mede
layout e pintura da cena fixa de 529 nós em 1920×1080, com uma e quatro threads,
e shaping dos cinco conjuntos de texto/fontes incorporados. Cada carga tem
0,1 s de aquecimento e três amostras de 1 s. O resultado de shaping é execuções
do conjunto completo por segundo, não glifos por segundo. Os pixels precisam
coincidir entre threads e entre os três perfis, assim como as métricas dos
textos. A comparação usa os mesmos objetos C do motor; esses componentes C não
mudam com o perfil Rust.

Os testes verificam vínculo, ociosidade e vazão dos componentes do motor em
software. Não substituem as capturas da interface da shell, tempo de quadro
completo, primeira apresentação ou medições em hardware real.

## Resultados e decisão

A primeira tentativa, em Fedora WSL com Rust 1.98.1, foi interrompida para
desligamento: `fedora` cancelada e demais variantes não iniciadas. Seus dados
parciais não entram na comparação. Na retomada, foram usados três alvos novos,
o ambiente descrito acima e a mesma revisão limpa em todas as variantes.

| Perfil | Compilação | Pico de um rustc | Pico da árvore de processos | Arquivo estático |
| --- | ---: | ---: | ---: | ---: |
| Fedora, sem LTO, 1 unidade | 389,76 s | 2,520 GiB | 2,560 GiB | 529,105 MiB |
| Thin LTO, 1 unidade | 420,24 s | 2,961 GiB | 3,001 GiB | 846,851 MiB |
| Sem LTO, 8 unidades | 426,73 s | 1,677 GiB | 1,716 GiB | 796,905 MiB |

Todas terminaram com saída zero e **nenhum artefato Cargo reaproveitado**.
Comparadas ao perfil Fedora, oito unidades reduziram o pico individual de RSS
em cerca de 33,5%, ao custo de 9,5% no tempo e 50,6% no tamanho do arquivo
estático com símbolos. Thin LTO aumentou o tempo em 7,8%, o pico individual em
17,5% e o arquivo estático em 60,1%. Esse arquivo contém material de vínculo e
depuração; esses aumentos não descrevem diretamente o tamanho do executável ou
o RSS da sessão. Uma compilação por variante não estabelece ganhos universais.

Os três perfis também vincularam e executaram a sessão real privada com sucesso.
Nenhum arquivo da shell medido foi recompilado durante essa integração.

| Perfil | Executável com símbolos | Poll/s mediano | RSS mediano | Layout/pintura, 1 thread | Layout/pintura, 4 threads | Shaping, 1 thread |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Fedora | 288,584 MiB | 12,500 | 101,242 MiB | 53,410 cenas/s | 57,922 cenas/s | 782,107 conjuntos/s |
| Thin LTO | 290,921 MiB | 12,500 | 101,555 MiB | 52,139 cenas/s | 53,420 cenas/s | 760,321 conjuntos/s |
| 8 unidades | 366,059 MiB | 12,500 | 103,328 MiB | 55,655 cenas/s | 60,106 cenas/s | 812,301 conjuntos/s |

As nove amostras ociosas foram válidas, sem excesso do orçamento. O checksum
dos pixels foi idêntico entre perfis e threads (`15883224589412617081`), assim
como nós, propriedades e métricas dos cinco conjuntos de texto. As medianas
de vazão acima representam somente a cena/carga fixa do motor, sem entrada,
React, composição de tela completa ou apresentação física. Não são a taxa de
quadros percebida na sessão.

**Decisão: manter as flags Fedora de produção.** Oito unidades poupam memória
na compilação e tiveram cerca de 4% mais vazão nesta rodada curta, mas aumentam
o tempo de build, o arquivo estático, o executável com símbolos e o RSS ocioso.
O perfil atual já fica bem abaixo do orçamento de 5 GiB por compilador. A faixa
serial da própria referência foi 50,842–55,527 cenas/s; sem rodadas independentes
e controle de frequência/carga do host, não se afirma um ganho estável de 4%.
Thin LTO não apresentou vantagem nas cargas medidas. Nenhum desses resultados
justifica alterar as flags atuais. Medições de primeira apresentação e frame
completo em hardware continuam pendentes; a Fase 0 inteira não está concluída.

Os relatórios completos ficam em
`/var/tmp/axia-phase0-20261118-resume/build/validation/profile-compare-resume/`;
os três JSONs e logs Cargo estão em `results/`, e a integração conserva seus
logs separados por variante. São ignorados pelo Git e não acompanham o push.
O protocolo e as ferramentas Rust ficam versionados para permitir repetir a
coleta sem depender desse clone temporário.
