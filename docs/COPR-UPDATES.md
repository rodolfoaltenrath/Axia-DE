# Atualizações do Axia DE pelo Copr

O pacote continua se chamando `axia-de`; o novo repositório Git pode se chamar
`Axia-DE`. Não é necessário reinstalar o Fedora, a DE ou a ISO a cada alteração.
Atualizações substituem os arquivos do pacote e preservam as configurações em
`~/.config/axia`. A sessão carrega a nova versão ao sair e entrar novamente.

## Fluxo implementado

1. Um push inicia `.github/workflows/desktop-fedora.yml`.
2. A CI verifica builds CPU/GPU, instalação isolada, inicialização headless e
   regressões do touchpad.
3. Gera um SRPM com os bundles React compilados a partir do lockfile e um RPM
   Fedora 44 x86_64, executando também os checks do pacote.
4. Publica ambos como artefato `axia-de-fedora-44` do GitHub Actions.
5. Se o push for na branch padrão e a integração estiver configurada, envia
   **esse mesmo SRPM** ao Copr. O Copr recompila, assina e publica no repositório.
6. O DNF pode atualizar o pacote manualmente ou pelo timer opcional da Axia.

PRs e pushes em outras branches produzem os testes e artefatos, sem receber
credenciais de publicação. O envio ao Copr aguarda o resultado do build remoto;
uma falha deixa o job vermelho. Sem as variáveis de integração, a CI produz os
artefatos e pula a publicação. Não configure um webhook paralelo se usar esse
fluxo: ele produziria builds duplicadas sem o mesmo bloqueio pelos testes.

Cada SRPM recebe `Release: 3.<data UTC com microssegundos>.git<commit>.fc44`
(o número inicial vem do spec). Isso é mais novo que o `0.8.7-2.fc44` instalado,
e builds posteriores recebem releases crescentes pelo relógio do builder.
Ao lançar um pacote estável da mesma versão depois desses snapshots, aumente
o número base de `Release`; ao mudar `Version`, atualize também `package.json`.

## Preparação única das contas

1. Criar uma [conta Fedora](https://accounts.fedoraproject.org/) e entrar no
   [Copr](https://copr.fedorainfracloud.org/).
2. Criar um projeto Copr chamado `axia-de`, com o chroot
   `fedora-44-x86_64`. O identificador será `djonatanaltenrath/axia-de`.
3. Obter a configuração de API em <https://copr.fedorainfracloud.org/api/>.
   Ela contém `login`, `username` e `token` na seção `[copr-cli]`.
4. No repositório GitHub, abrir **Settings → Secrets and variables → Actions**.
   Criar o secret **`COPR_CONFIG`** com o bloco completo obtido no Copr e a
   variável **`COPR_PROJECT`** com `djonatanaltenrath/axia-de`.
5. Fazer push na branch padrão do repositório ou executar o workflow manualmente
   nessa branch. Aguardar os jobs `desktop`, `rpm` e `copr`.

Não colocar credenciais no Git nem enviá-las no chat. O token tem expiração;
renová-lo no Copr e substituir o secret quando necessário. A configuração só
precisa ficar no GitHub para esse fluxo: esta máquina não precisa guardar a
credencial de publicação.

O nome do projeto Copr é independente do nome da conta GitHub e do nome do Git.
Não precisamos configurar nenhum endereço de repositório no código-fonte.

## Habilitar as atualizações nesta máquina

Depois que a primeira build Copr terminar com sucesso:

```sh
sudo dnf copr enable djonatanaltenrath/axia-de
sudo dnf upgrade --refresh axia-de
```

Isso atualiza o RPM local existente para o pacote do Copr. A chave GPG do projeto
é apresentada ao habilitar/usar o repositório; mantenha a verificação de
assinaturas habilitada. Não é preciso trocar de sessão durante o download.
Salve seu trabalho e saia/entre na sessão para passar a usar a versão nova.
Durante a atualização, o compositor em execução conserva o código já carregado,
mas helpers e recursos no disco podem mudar; a validação de uma versão nova deve
ser feita após abrir uma sessão nova.

O pacote inclui `axia-update.timer`, **desabilitado por padrão**. Para instalar
automaticamente as atualizações da Axia a cada aproximadamente 30 minutos:

```sh
sudo systemctl enable --now axia-update.timer
```

O serviço executa `dnf5 upgrade axia-de` com confirmação automática e verificação
GPG. O pedido de atualização é só para `axia-de`; dependências necessárias podem
ser atualizadas pelo resolvedor. Ele não executa uma atualização geral do Fedora,
reinicia a máquina ou encerra sua sessão. A primeira execução é cerca de cinco
minutos após o boot ou a ativação, com um pequeno atraso aleatório.

Para executar, consultar ou desativar:

```sh
sudo systemctl start axia-update.service
systemctl list-timers axia-update.timer
journalctl -u axia-update.service -n 80
sudo systemctl disable --now axia-update.timer
```

## Build local e método alternativo do Copr

Em uma máquina de desenvolvimento Fedora com Node, npm, git, rpm-build, Cargo e rustc:

```sh
cargo xtask srpm --outdir "$PWD/build/srpm"
sudo dnf builddep packaging/rpm/axia-de.spec
rpmbuild --rebuild --define "_topdir $PWD/build/rpmbuild" build/srpm/*.src.rpm
```

Para reproduzir o source builder do Copr em um contêiner descartável:

```sh
podman run --rm --security-opt label=disable \
  -v "$PWD:/src:rw" -w /src registry.fedoraproject.org/fedora:44 \
  sh -ec 'dnf install -y make; make -f .copr/Makefile srpm outdir=/src/build/srpm'
```

O `.copr/Makefile` também permite cadastrar um pacote SCM com método
`make_srpm` no Copr. Isso é uma alternativa ao envio pela CI, não um segundo
gatilho necessário. O frontend é gerado na etapa de fontes, que pode acessar a
rede. A compilação do RPM usa os bundles do SRPM e não executa npm na rede.

Referências: [fontes e integração do Copr](https://docs.copr.fedorainfracloud.org/user_documentation.html),
[secrets no GitHub Actions](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets),
[upgrade de pacotes no DNF5](https://dnf5.readthedocs.io/en/latest/commands/upgrade.8.html).
