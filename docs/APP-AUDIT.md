# Avaliação dos aplicativos nativos no Fedora

Análise em 2 de novembro de 2026, na branch `feature/standalone-fedora-de`,
com Axia DE `0.8.7-2.fc44` instalada no Fedora 44. O objetivo é decidir quais
aplicativos podem assumir as tarefas dos equivalentes do host antes de ocultar
entradas do launcher. Nenhum aplicativo foi ocultado ou desinstalado.

## Resultado

Há bons candidatos a aplicativos principais para tarefas locais, especialmente
Editor, Relógio, Fontes e Emoji/Símbolos. Isso ainda exige testes de uso. Discos,
Loja e Configurações têm lacunas concretas da extração do LordOS e precisam
continuar acompanhados das ferramentas do Fedora. Calculadora oferece apenas
aritmética básica. Fotos e Player dependem da estabilização do helper de mídia.

Não basta o app abrir ou sua descrição listar uma função: a avaliação distingue
o que foi encontrado no código do que foi exercitado nesta máquina.

## Equivalentes instalados e decisão por tarefa

Os nomes GNOME abaixo foram encontrados nos arquivos `.desktop` desta máquina.
Eles identificam a alternativa instalada; não representam um teste comparativo
completo desses aplicativos.

| Axia | Alternativa instalada | Avaliação para substituir |
|---|---|---|
| Editor de Texto | GNOME Text Editor | Candidato para texto e código leves. Tem abas, projeto, busca/regex, múltiplos cursores e undo. Recusa binários e arquivos acima de 8 MB. Não assume o papel do VS Code. |
| Relógio | GNOME Clocks | Candidato: fusos, alarmes, cronômetro e temporizador. Alarmes vivem no SDK mesmo com a janela fechada; comportamento durante suspensão e troca de sessão precisa de teste. |
| Fontes | GNOME Fonts | Candidato para visualizar fontes e administrar fontes do usuário; validar instalação, remoção e atualização do catálogo com outros apps abertos. |
| Emoji e Símbolos | GNOME Characters | Candidato para busca e inserção. Validar cópia para apps GTK/Qt e renderização de sequências compostas. |
| Calculadora | GNOME Calculator | Substituição apenas para contas básicas: quatro operações e parênteses. Não há modos científico, financeiro ou de programação na implementação. |
| Terminal | Ptyxis | Candidato para shell comum: PTY próprio, abas, painéis e emulação xterm-256color. Validar TUI, SSH, tmux, atalhos e fluxos de contêiner antes de dispensar o terminal atual. |
| PDF | GNOME Papers | Candidato para leitura, busca, formulários e anotações. Usa Poppler no `axia-doc`, instalado nesta máquina. Validar documentos reais, impressão, links externos e assinaturas antes de decidir. |
| Fotos | GNOME Loupe | Tem navegação, zoom, rotação, apresentação e edição salva como cópia. Ainda depende de validar codecs e o ciclo de vida do `axia-media`. |
| Player | GNOME Showtime e Decibels | Tem biblioteca, fila, legendas e picture in picture. Manter alternativa até validar áudio/vídeo, seek, formatos e recuperação de falha do helper. |
| Calendário | GNOME Calendar | Local e CalDAV implementados. A documentação registra limites na edição de ocorrências de eventos recorrentes e na criação de calendários remotos. Validar sincronização com o provedor usado. |
| Contatos | GNOME Contacts | Local, CardDAV e importação vCard implementados. Validar importar/exportar e sincronização com o provedor usado antes de migrar dados. |
| Câmera | GNOME Snapshot | Implementação V4L2 com gravação pelo helper de mídia. Hardware e gravação não foram testados nesta análise. |
| Scanner | GNOME Document Scanner | O cliente nativo é eSCL/AirScan de rede, sem SANE. Isso não cobre automaticamente um scanner USB que depende de um backend SANE. Manter alternativa conforme o equipamento. |
| Discos | GNOME Disks | Não substituir: montar, desmontar, ejetar, formatar e outras ações usam o helper administrativo que foi removido. |
| Monitor | GNOME System Monitor | Útil para métricas e processos do usuário. Não considerar equivalente completo em ações privilegiadas, controle de ventoinhas ou firewall por app. |
| Logs | GNOME Logs | Leitor próprio de journal e logs da DE. Consultas de sistema pelo SDK passam por `logs-system`, indisponível sem o helper; confirmar a cobertura necessária antes de substituir. |
| Loja | GNOME Software | Complementares: a loja nativa gerencia `.lpk` e encaminha os pacotes do sistema ao gerenciador do host. Preservar GNOME Software para o sistema. |
| Configurações | GNOME Settings | Configura o shell e possui integrações existentes, como áudio. Administração de rede/VPN, usuários e outras áreas não foi integralmente substituída por adaptadores do Fedora. Preservar as ferramentas do host. |
| Cleaner | GNOME Disk Usage Analyzer | Há sobreposição em análise de espaço, mas as funções não são idênticas. Limpeza administrativa do sistema ainda depende do backend removido. Não ocultar Baobab por equivalência presumida. |
| Remote Desk | GNOME Connections | Cliente RDP/VNC existe e tem helper instalado; conexão, clipboard e reconexão precisam de teste com um destino real. |

Arquivos oferece abas, quatro visualizações, lixeira, prévia e operações locais.
É candidato para navegação diária, mas montar/ejetar volumes ainda chama
`admin.run('disk-mount', ...)` e ações semelhantes; compartilhamentos SMB/NFS
também dependem de integração administrativa. Não foi encontrada uma entrada
do Nautilus no inventário de `.desktop` do sistema usado nesta análise.

Vale manter e melhorar essa base, priorizando navegação e operações comuns.
Na sessão Fedora avaliada, faltava a configuração XDG das pastas pessoais, e
Documentos e outras pastas padrão ainda não existiam. A primeira melhoria local
prioriza Home e pastas pessoais na barra lateral, corrige caminhos XDG absolutos
e aninhados e inicializa contas sem configuração com a ferramenta do Fedora,
preservando os diretórios existentes. Isso melhora a navegação sem encerrar a
avaliação de montagem de volumes, arquivos externos e demais operações.

Notas e Lembretes têm funções próprias; não há motivo para ocultar um app de
outra categoria por ter aparência parecida. Backups, Gravador e Capturas de Tela
precisam de testes de restauração, gravação/exportação e captura, respectivamente.
Passwords exige avaliar desbloqueio, integração com a sessão e recuperação do
cofre antes de qualquer substituição de outro gerenciador de credenciais.
Sobre, Sobre o App, Ajuda, Forçar Encerramento e Informações do Sistema são
utilitários da DE. Sandbox Probe é um aplicativo de diagnóstico, não substituto
de uma ferramenta do Fedora.

Mail tem IMAP/SMTP e cache local, mas OAuth está desabilitado na implementação
(`imap.c` retorna `oauth-disabled`). Não é uma substituição geral para clientes
que precisam autenticar em provedores via OAuth. Calendário, Contatos e
Lembretes compartilham o backend `axia-maild`, portanto sua sincronização merece
validação conjunta.

## Limitações encontradas na adaptação

- `server/src/platform/admin.c` devolve indisponível para as operações
  administrativas. Botões que ainda chamam essas operações não comprovam que a
  função está disponível no Fedora.
- `sdk/disks.js` ainda decide disponibilidade de formatos por marcadores em
  `/usr/lib/lordos/disk-formats.*`. Esses marcadores não representam os drivers
  e pacotes instalados pelo Fedora e podem produzir indicações incorretas.
- O pacote força `GPU=0`. Nesta sessão o diagnóstico de decodificação também
  informou `noDriver`; isso não comprova, por si só, que falta um driver do host.
- Foi encontrado um `axia-media` zumbi, filho do compositor. A causa não foi
  determinada; não se deve atribuir toda falha de reprodução a codecs.
- Os helpers registram ausência de delegação de cgroups. O isolamento por
  Landlock/seccomp continua sendo registrado, mas os escopos e limites de
  recursos previstos não estão funcionando nesta sessão.

## Verificação executada

- Build dos temas `default` e `classic`, app-host e aplicativos isolados concluído.
- Suíte do driver de touchpad: **57/57 verificações**, incluindo canto direito,
  canto esquerdo, preferência invertida e toque com dois dedos.
- Oito suítes existentes passaram: `files_model_test`, `dock_model_test`,
  `desktop_model_test`, `session_model_test`, `sysinfo_model_test`,
  `smart_parse_test`, `snap_test` e `scrollbar_model_test`.
- Inicialização headless com configurações e dados temporários: **14 janelas**
  confirmaram o commit inicial, sem `ReferenceError`, `TypeError` ou
  `SyntaxError`: Calculadora, Relógio, Editor, Arquivos, PDF, Fotos, Notas,
  Fontes, Configurações, Loja, Monitor, Logs, Discos e Backups.

O teste headless usou cópias dos binários instalados e os bundles recém-compilados.
Não valida input físico, interação com as janelas, impressão, dispositivos,
operações de arquivo, sincronização de contas ou restauração de backups. Um
commit inicial de janela é evidência de inicialização, não de equivalência
funcional. A sessão física e a instalação RPM não foram reiniciadas ou alteradas.

## Próximos critérios para escolher os padrões

### Validação após a primeira atualização pelo Copr

Na sessão iniciada com `0.8.7-3.20261103013626190773.git2784bdcf5ff3.fc44`,
o usuário confirmou que pressionar o canto inferior direito do touchpad voltou
a abrir menus de contexto, incluindo em aplicativos externos. Isso complementa
os testes simulados com uma verificação no dispositivo físico.

O compositor e os helpers de cofre e Mail estavam em execução, sem unidades de
usuário com falha. O timer `axia-update.timer` estava habilitado e ativo. Essas
verificações confirmam o início da sessão e a ativação das atualizações; não
validam as tarefas de cada aplicativo nem as integrações administrativas abaixo.

### Avaliação funcional pendente

Testar primeiro Editor, Relógio, Fontes e Emoji/Símbolos com tarefas reais. Depois,
Terminal, PDF e Arquivos. Para cada candidato: abrir dados representativos,
editar quando aplicável, salvar/exportar, reabrir o resultado e verificar a
integração com os outros aplicativos. Só então decidir se o equivalente do host
deve sair da apresentação principal do launcher.

Discos, Loja e administração do sistema continuam acessíveis por ferramentas do
Fedora até que existam adaptadores funcionais. Ocultar uma entrada no launcher
e remover seu pacote são decisões distintas; nenhuma delas foi aplicada aqui.

Fontes da avaliação: [separação do sistema](STANDALONE.md),
[Editor](EDITOR.md), [Terminal](TERMINAL.md), [Documentos](DOCUMENTS.md),
[Mail e sincronização](MAIL.md), [utilitários do sistema](SYSTEM.md),
[Arquivos](FILES.md), os manifestos em `apps/*/manifest.json` e os respectivos
componentes em `apps/` e `sdk/`.
