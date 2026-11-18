# Comparação dos perfis Rust da shell

Experimento da Fase 0, iniciado em 2026-11-18. A configuração de produção em
`rust/Cargo.toml` e as flags do RPM permanecem inalteradas. Esta comparação
avalia `axia-shell` e suas dependências; não é uma reconstrução de todos os
helpers nem uma medição do workflow completo.

## Protocolo

- Fedora 44 no WSL, Rust/Cargo 1.98.1, oito CPUs lógicas, 7,7 GiB de RAM e
  2 GiB de swap. Checkout Linux com LF, separado do checkout Windows.
- Fonte de referência: `80aa4856`, acrescida do comando de medição e do driver
  de integração desta entrega. Nenhuma implementação de produção foi alterada.
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
C usadas no job desktop. O arquivo do benchmark incorporado ao compositor e os
helpers não são alvo da comparação de flags da shell. Cada sessão usa software,
HOME/XDG privados, `nobody` quando iniciada por root, rede offline, três amostras
de 8 s de aquecimento + 20 s de observação e os limites iniciais de 15 poll/s e
120 MiB. Os relatórios por variante e `runtime.json` preservam os resultados.

Esses testes verificam vínculo e ociosidade; não substituem paridade visual,
tempo de quadro completo, primeira apresentação ou medições em hardware real.

## Resultados e decisão

Em 2026-11-18, o experimento foi interrompido a pedido do usuário para desligar
o computador. A compilação `fedora` foi cancelada antes de produzir a shell;
`thin` e `cgu8` não foram iniciadas. Não há resultados completos de tempo,
tamanho ou runtime para comparar e nenhuma flag de produção foi alterada.

Passaram 43 testes Rust; o driver manual `profile_runtime` compilou e ficou
ignorado por padrão, sem executar a integração real. O comando de cancelamento
encerrou os processos do experimento e preservou os registros parciais.

Para retomar, repetir as três compilações e a integração acima. Usar diretórios
de artefatos novos: o comando recusa reutilizar o alvo parcialmente compilado.
Os relatórios locais em `build/validation/profile-compare/` são ignorados pelo
Git e não acompanham o push. O protocolo e as ferramentas Rust estão versionados,
permitindo reproduzir a coleta mesmo sem o clone temporário ou os logs locais.
