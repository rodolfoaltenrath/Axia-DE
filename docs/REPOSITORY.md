# Repositório independente Axia-DE

Nome escolhido: `Axia-DE`. A migração de Git não exige mudar o nome do pacote
`axia-de`, os dados em `~/.config/axia` ou o projeto Copr. A autoria do código
herdado está registrada em [ORIGIN.md](../ORIGIN.md) e [LICENSE](../LICENSE).

## Publicar mantendo o histórico

Criar no GitHub um repositório vazio chamado `Axia-DE`, sem README ou licença
automáticos. O código atual já contém esses arquivos e os avisos de origem.

Após revisar e commitar as alterações locais, a branch a publicar é a adaptação
`feature/standalone-fedora-de`, **não a `main` antiga do LordOS**. Por exemplo,
partindo dessa branch e sem alterações pendentes:

```sh
git status --short
git remote add axia https://github.com/rodolfoaltenrath/Axia-DE.git
git push axia HEAD:main
```

Se o remoto `axia` já existir, conferir seu endereço com `git remote -v` em vez
de substituí-lo às cegas. Esse push mantém o histórico da branch e cria `main`
no destino, sem alterar o repositório antigo. Não é preciso fazer force-push nem
apagar o `.git`.

Para trabalhar depois em uma pasta com o nome novo:

```sh
cd ..
git clone https://github.com/rodolfoaltenrath/Axia-DE.git
cd Axia-DE
git remote add upstream https://github.com/lordbasex/LordOS.git
```

O remoto `upstream` serve para consultar a origem e trazer mudanças específicas,
sem transformar novamente a Axia em uma distro LordOS. Antes de publicar,
confirmar que os arquivos novos e as correções locais foram commitados; um push
não inclui arquivos ainda fora do histórico.

## Integração de atualizações

Configurar `COPR_PROJECT` e `COPR_CONFIG` no **repositório novo**, seguindo
[COPR-UPDATES.md](COPR-UPDATES.md). Secrets e variáveis do repositório antigo não são
transferidos por um push Git. A CI identifica a branch padrão pelo GitHub e não
tem o endereço do fork antigo fixado no workflow.

Enquanto a conta Fedora, o projeto Copr e o secret não existirem, ainda é
possível gerar e baixar os RPMs pelos artefatos do GitHub Actions. A publicação
no Copr é pulada até que a integração seja configurada.
