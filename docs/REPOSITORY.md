# Repositório independente Axia-DE

Repositório: <https://github.com/rodolfoaltenrath/Axia-DE>.
A migração de Git não exige mudar o nome do pacote
`axia-de`, os dados em `~/.config/axia` ou o projeto Copr. A autoria do código
herdado está registrada em [ORIGIN.md](../ORIGIN.md) e [LICENSE](../LICENSE).

## Trabalhar no projeto

A branch `main` do Axia-DE contém a adaptação independente, as correções e o
histórico da branch `feature/standalone-fedora-de`. A licença MIT original e os
avisos de origem continuam presentes; eles não precisam ser criados novamente
pelo formulário do GitHub.

Nesta cópia local, `origin` aponta para o Axia-DE. O remoto anterior foi
preservado como `legacy`, e a antiga branch local `main` do LordOS foi preservada
como `lordos-main`. O fluxo normal de trabalho passa a ser:

```sh
git status --short
git switch main
git pull --ff-only
# editar, revisar e commitar as alterações
git push
```

`git remote -v` permite conferir os destinos. A publicação no Axia-DE preserva
o histórico; não é preciso usar force-push nem apagar o `.git`.

Para trabalhar depois em uma pasta com o nome novo:

```sh
cd ..
git clone https://github.com/rodolfoaltenrath/Axia-DE.git
cd Axia-DE
git remote add upstream https://github.com/lordbasex/LordOS.git
```

O remoto `upstream` serve para consultar a origem e trazer mudanças específicas.
Antes de publicar, confirmar que os arquivos novos e as correções locais foram
commitados; um push não inclui arquivos ainda fora do histórico.

## Integração de atualizações

Configurar `COPR_PROJECT` e `COPR_CONFIG` no **repositório novo**, seguindo
[COPR-UPDATES.md](COPR-UPDATES.md). Secrets e variáveis do repositório antigo não são
transferidos por um push Git. A CI identifica a branch padrão pelo GitHub e não
tem o endereço do fork antigo fixado no workflow.

Enquanto a conta Fedora, o projeto Copr e o secret não existirem, ainda é
possível gerar e baixar os RPMs pelos artefatos do GitHub Actions. A publicação
no Copr é pulada até que a integração seja configurada.
