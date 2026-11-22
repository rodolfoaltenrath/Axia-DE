# Inventário da fronteira JavaScript da shell

Captura: 2026-11-18, código de referência `ffabc1aa`.
Fase 0 de [ROADMAP-SHELL-RUST.md](ROADMAP-SHELL-RUST.md). Este arquivo documenta
o contrato atual; não remove nem amplia permissões e não cria uma API de temas.

## Escopo e contagem

Foram analisados os 185 arquivos JS/JSX versionados de `sdk/`, `runtime/`,
`themes/`, `apps/` e `examples/apps/`, excluindo bundles. JSX foi convertido com
esbuild 0.24.2, sem eliminação de código, e as expressões foram lidas pela AST
Acorn 8.17.0 do Node 24.19.0. Comentários e strings não contam como chamadas.

A AST encontra **386 caminhos distintos**: 382 com acesso literal a `lord`
e quatro resolvidos pelo alias inequívoco `AW = lord.appWindows`. A tabela abaixo
tem **491 entradas**, acrescentando os destinos dos wrappers de Terminal,
áudio e mídia, as listas `reads/calls/services/dockServices` dos manifestos Rust,
o prelude de timers/console e as superfícies retornadas pelos aliases de animação
e snap. Entradas repetidas têm uma linha só. Há métodos, propriedades e testes
de capacidade; isso **não é uma contagem de funções executadas**, nem a contagem
de todas as funções C registradas. Os antigos números aproximados 314/456 usam
outros recortes e não devem ser usados para conferir esta tabela.

Não foram inferidos fluxos arbitrários de valores, aliases ambíguos, nomes
calculados de terceiros ou chamadas vindas de pacotes externos não presentes
no checkout. Os pontos dinâmicos estão registrados abaixo; continuam sujeitos
às listas de autorização dos manifestos e ao broker. A ausência de uma chamada
literal não autoriza apagar uma API.

## Critérios de classificação

- **Ativa:** responsabilidade usada pela shell, apps do projeto ou adaptadores
  atuais; migra para Rust com seus chamadores antes de retirar a ponte.
- **Usada pelos .lpk:** superfície disponível ao host isolado que deve ser
  preservada na opção A, inclusive a compatibilidade dos exports compartilhados.
  Não significa que se observou um pacote externo executar cada método.
- **Compatibilidade:** caminho antigo/indicador de capacidade ainda referenciado;
  validar seus chamadores e o contrato congelado antes de retirar.
- **Indisponível no Fedora:** namespace ausente ou implementação deliberadamente
  desativada, mesmo quando o SDK mantém um fallback.
- **Retirar:** mecanismo de temas rejeitado pela decisão do projeto; remover
  com a view Rust da Fase 4, preservando modo, destaque e papel de parede.

| Classificação | Entradas |
| --- | ---: |
| indisponível no Fedora | 15 |
| ativa | 381 |
| usada pelos .lpk | 87 |
| compatibilidade | 6 |
| retirar | 2 |

A coluna **isolado** é independente da classificação: `não` significa que o
contexto de um app comum não registra aquela capacidade. `local` não implica
I/O livre. `broker/limitada` preserva as verificações de permissão e identidade.
`próprio app compilado` e `app de sistema autorizado` não são capacidades gerais
de terceiros. Veja os registros de [server/src/ui/js.c](../server/src/ui/js.c),
[server/src/ui/js_broker.c](../server/src/ui/js_broker.c), [server/src/ui/js_brokermail.c](../server/src/ui/js_brokermail.c),
[server/src/ui/js_brokercam.c](../server/src/ui/js_brokercam.c) e [server/src/ui/js_vault.c](../server/src/ui/js_vault.c).
O provedor da tabela é o ponto de entrada da sessão ou do adaptador; no processo
isolado ele pode ser substituído pelo broker, sem mudar o nome do método.

## Decisões que não podem vir de uma busca textual

- `lord.pkg` não é registrado pelo contexto atual. `sdk/updates.js` exige
  `!standaloneDesktop`; não migrar o motor de pacotes da antiga distribuição.
- `lord.admin.available()` fica falso: [server/src/platform/admin.c](../server/src/platform/admin.c)
  retorna `admin_fd() == -1`. As ações `pkg-*`, `smart-*` e `disk-smart`
  dependentes desse helper não se tornam disponíveis ao traduzir o wrapper.
  Em `smart.js`, a detecção de `smartctl` pode funcionar, mas sua instalação,
  coleta e testes por essas ações não. Preservar os parsers Rust reaproveitáveis.
- `lord.hwinfo.scan` **continua implementado** em `js_hw.c`; não confundir a
  desativação das sugestões dependentes de `lord.pkg` com a retirada da leitura
  de hardware usada pelo relatório.
- `session.managed/lock/switchUser` retornam falso no Fedora. Energia usa
  `session.power/hostPower` e logind e segue ativa.
- `net.host/tray` são pontes Rust para NetworkManager/StatusNotifier, e os
  contadores seguem ativos. Os leitores de configuração/VPN antigos marcados
  como compatibilidade não justificam restaurar o helper `net-*` desativado.
- O namespace público do SDK não é a lista de capacidades nativas de um app:
  o bundle isolado inclui todos os 114 módulos, mas `app_context` registra
  apenas o subconjunto permitido. [runtime/app-host.js](../runtime/app-host.js) não chama
  `startShell`. A separação por arquivo está em [LPK-SDK-BOUNDARY.md](LPK-SDK-BOUNDARY.md).

## Chamadas e propriedades

Origem **literal/alias** vem da AST; **wrapper**, **manifesto** e **prelude**
explicitam expansões estáticas. Os links de consumidores incluem todas as
ocorrências por arquivo, sem repetir cada chamada do mesmo símbolo.

| Caminho | Classificação | Isolado | Provedor | Consumidores / origem |
| --- | --- | --- | --- | --- |
| `lord.admin` | indisponível no Fedora | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/suggestions.js](../sdk/suggestions.js), [sdk/updates.js](../sdk/updates.js); literal |
| `lord.admin.available` | indisponível no Fedora | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/admin.js](../sdk/admin.js), [sdk/sessionrestore.jsx](../sdk/sessionrestore.jsx), [sdk/video.js](../sdk/video.js); literal |
| `lord.admin.request` | indisponível no Fedora | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/admin.js](../sdk/admin.js); literal |
| `lord.anim` | ativa | não | [server/src/ui/js_anim.c](../server/src/ui/js_anim.c) | [sdk/anim.js](../sdk/anim.js); literal |
| `lord.anim.clear` | ativa | não | [server/src/ui/js_anim.c](../server/src/ui/js_anim.c) | [sdk/anim.js](../sdk/anim.js); namespace por alias |
| `lord.anim.dockPoof` | ativa | não | [server/src/ui/js_anim.c](../server/src/ui/js_anim.c) | [sdk/anim.js](../sdk/anim.js); namespace por alias |
| `lord.anim.dockVisibility` | ativa | não | [server/src/ui/js_anim.c](../server/src/ui/js_anim.c) | [sdk/anim.js](../sdk/anim.js); namespace por alias |
| `lord.anim.minimize` | ativa | não | [server/src/ui/js_anim.c](../server/src/ui/js_anim.c) | [sdk/anim.js](../sdk/anim.js); namespace por alias |
| `lord.anim.slide` | ativa | não | [server/src/ui/js_anim.c](../server/src/ui/js_anim.c) | [sdk/anim.js](../sdk/anim.js); namespace por alias |
| `lord.appearance` | ativa | não | [server/src/ui/js_files.c](../server/src/ui/js_files.c) | [sdk/theme.js](../sdk/theme.js); literal |
| `lord.appearance.request` | ativa | não | [server/src/ui/js_files.c](../server/src/ui/js_files.c) | [sdk/theme.js](../sdk/theme.js), [themes/default/components/Dock.jsx](../themes/default/components/Dock.jsx); literal |
| `lord.appendChild` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.appId` | usada pelos .lpk | local | [server/src/ui/js_broker.c](../server/src/ui/js_broker.c) | [runtime/app-host.js](../runtime/app-host.js), [runtime/nativeapps.js](../runtime/nativeapps.js), [sdk/dock.js](../sdk/dock.js), [sdk/sessionrestore.jsx](../sdk/sessionrestore.jsx), [sdk/textfocus.js](../sdk/textfocus.js), [sdk/windowmemory.js](../sdk/windowmemory.js), [sdk/windows.js](../sdk/windows.js); literal |
| `lord.apps` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/apps.js](../sdk/apps.js), [sdk/packages.js](../sdk/packages.js); literal |
| `lord.apps.actions` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/dock.js](../sdk/dock.js); literal |
| `lord.apps.generation` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/apps.js](../sdk/apps.js); literal |
| `lord.apps.inspectPackage` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/packages.js](../sdk/packages.js), [apps/store/manifest.rs](../apps/store/manifest.rs); literal, reads |
| `lord.apps.installPackage` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/packages.js](../sdk/packages.js), [apps/store/manifest.rs](../apps/store/manifest.rs); literal, calls |
| `lord.apps.lastError` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/apps.js](../sdk/apps.js); literal |
| `lord.apps.launch` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/apps.js](../sdk/apps.js); literal |
| `lord.apps.launchAction` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/dock.js](../sdk/dock.js); literal |
| `lord.apps.launchPackage` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx), [sdk/apps.js](../sdk/apps.js), [sdk/packages.js](../sdk/packages.js); literal |
| `lord.apps.launchUri` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/nativefiles.js](../sdk/nativefiles.js), [sdk/rustfiles.js](../sdk/rustfiles.js); literal |
| `lord.apps.launchWith` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx), [sdk/nativefiles.js](../sdk/nativefiles.js), [sdk/rustfiles.js](../sdk/rustfiles.js), [sdk/sysopen.js](../sdk/sysopen.js); literal |
| `lord.apps.linux` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/apps.js](../sdk/apps.js); literal |
| `lord.apps.loadPackage` | compatibilidade | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/packages.js](../sdk/packages.js); literal |
| `lord.apps.packageFile` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/app-host.js](../runtime/app-host.js), [sdk/packages.js](../sdk/packages.js); literal |
| `lord.apps.packages` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/packages.js](../sdk/packages.js), [apps/store/manifest.rs](../apps/store/manifest.rs); literal, reads |
| `lord.apps.removePackage` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/packages.js](../sdk/packages.js), [apps/store/manifest.rs](../apps/store/manifest.rs); literal, calls |
| `lord.appWindows` | usada pelos .lpk | local | [server/src/ui/js_app_host.c](../server/src/ui/js_app_host.c) | [runtime/app-host.js](../runtime/app-host.js); literal |
| `lord.appWindows.attach` | usada pelos .lpk | local | [server/src/ui/js_app_host.c](../server/src/ui/js_app_host.c) | [runtime/app-host.js](../runtime/app-host.js); alias |
| `lord.appWindows.close` | usada pelos .lpk | local | [server/src/ui/js_app_host.c](../server/src/ui/js_app_host.c) | [runtime/app-host.js](../runtime/app-host.js); alias |
| `lord.appWindows.open` | usada pelos .lpk | local | [server/src/ui/js_app_host.c](../server/src/ui/js_app_host.c) | [runtime/app-host.js](../runtime/app-host.js); alias |
| `lord.appWindows.setTitle` | usada pelos .lpk | local | [server/src/ui/js_app_host.c](../server/src/ui/js_app_host.c) | [runtime/app-host.js](../runtime/app-host.js); alias |
| `lord.backup` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.cancel` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.config` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.configure` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.list` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.lock` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.ls` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.path` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.pause` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.remove` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.restore` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.runNow` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.status` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.backup.unlock` | ativa | não | [server/src/ui/js_backup.c](../server/src/ui/js_backup.c) | [sdk/backup.js](../sdk/backup.js); literal |
| `lord.broker` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/dock.js](../sdk/dock.js), [sdk/mail.js](../sdk/mail.js), [sdk/packages.js](../sdk/packages.js), [sdk/powerbox.jsx](../sdk/powerbox.jsx), [sdk/privacy.js](../sdk/privacy.js), [sdk/theme.js](../sdk/theme.js), [sdk/wayland.js](../sdk/wayland.js), [sdk/windows.js](../sdk/windows.js); literal |
| `lord.broker.device` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.broker.fdClose` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [apps/mail/index.jsx](../apps/mail/index.jsx), [apps/passwords/index.jsx](../apps/passwords/index.jsx), [runtime/nativeapps.js](../runtime/nativeapps.js), [sdk/mail.js](../sdk/mail.js), [sdk/powerbox.jsx](../sdk/powerbox.jsx); literal |
| `lord.broker.fdRead` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [apps/mail/index.jsx](../apps/mail/index.jsx), [sdk/powerbox.jsx](../sdk/powerbox.jsx); literal |
| `lord.broker.fdSize` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/mail.js](../sdk/mail.js); literal |
| `lord.broker.fdWrite` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [apps/passwords/index.jsx](../apps/passwords/index.jsx), [sdk/powerbox.jsx](../sdk/powerbox.jsx); literal |
| `lord.broker.menuClick` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/wayland.js](../sdk/wayland.js); literal |
| `lord.broker.menus` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/wayland.js](../sdk/wayland.js), [sdk/windows.js](../sdk/windows.js); literal |
| `lord.broker.micLevel` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.broker.micStart` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.broker.micStop` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.broker.openApp` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/windows.js](../sdk/windows.js); literal |
| `lord.broker.openArgs` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/dock.js](../sdk/dock.js), [sdk/packages.js](../sdk/packages.js); literal |
| `lord.broker.pick` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [apps/passwords/index.jsx](../apps/passwords/index.jsx), [runtime/nativeapps.js](../runtime/nativeapps.js), [sdk/mail.js](../sdk/mail.js), [sdk/powerbox.jsx](../sdk/powerbox.jsx); literal |
| `lord.broker.pickResult` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/powerbox.jsx](../sdk/powerbox.jsx); literal |
| `lord.broker.publish` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js), [sdk/theme.js](../sdk/theme.js); literal |
| `lord.broker.quit` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/dock.js](../sdk/dock.js); literal |
| `lord.broker.request` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.broker.resolve` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.broker.state` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [runtime/app-host.js](../runtime/app-host.js); literal |
| `lord.broker.textFocus` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/textfocus.js](../sdk/textfocus.js); literal |
| `lord.browserFrame` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/browserframe.js](../sdk/browserframe.js); literal |
| `lord.browserFrame.apply` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/browserframe.js](../sdk/browserframe.js); literal |
| `lord.browserFrame.chromiumRunning` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/browserframe.js](../sdk/browserframe.js); literal |
| `lord.browserFrame.state` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/browserframe.js](../sdk/browserframe.js); literal |
| `lord.browserFrame.tidy` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/browserframe.js](../sdk/browserframe.js); literal |
| `lord.bt.answer` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.bt.connect` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.bt.disconnect` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.bt.discover` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.bt.forget` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.bt.pair` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.bt.power` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.bt.state` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.clipboard` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [runtime/nativeapps.js](../runtime/nativeapps.js); literal |
| `lord.clipboard.get` | usada pelos .lpk | broker/limitada | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [apps/terminal/Panes.jsx](../apps/terminal/Panes.jsx), [apps/terminal/state.js](../apps/terminal/state.js), [sdk/clipboard.js](../sdk/clipboard.js), [sdk/components.jsx](../sdk/components.jsx), [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.clipboard.historyKind` | usada pelos .lpk | broker/limitada | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/clipboard.js](../sdk/clipboard.js); literal |
| `lord.clipboard.historyRequest` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/clipboard.js](../sdk/clipboard.js); literal |
| `lord.clipboard.historySuppress` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/clipboard.js](../sdk/clipboard.js); literal |
| `lord.clipboard.set` | usada pelos .lpk | broker/limitada | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [apps/passwords/index.jsx](../apps/passwords/index.jsx), [sdk/clipboard.js](../sdk/clipboard.js), [sdk/components.jsx](../sdk/components.jsx), [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.clipboard.state` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [apps/mail/BodyEditor.jsx](../apps/mail/BodyEditor.jsx), [sdk/clipboard.js](../sdk/clipboard.js); literal |
| `lord.clipboard.useImage` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/clipboard.js](../sdk/clipboard.js); literal |
| `lord.clock.request` | ativa | não | [server/src/ui/js_nativeview.c](../server/src/ui/js_nativeview.c) | [sdk/alarms.js](../sdk/alarms.js); literal |
| `lord.code.status` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/codesig.js](../sdk/codesig.js), [apps/about/manifest.rs](../apps/about/manifest.rs); literal, services |
| `lord.code.takeRefused` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/codesig.js](../sdk/codesig.js); literal |
| `lord.createNode` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.css` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.destroy` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.disks` | ativa | não | [server/src/ui/js_disk.c](../server/src/ui/js_disk.c) | [sdk/disks.js](../sdk/disks.js), [sdk/diskwatch.js](../sdk/diskwatch.js), [sdk/smart.js](../sdk/smart.js); literal |
| `lord.disks.list` | ativa | não | [server/src/ui/js_disk.c](../server/src/ui/js_disk.c) | [sdk/diskwatch.js](../sdk/diskwatch.js); literal |
| `lord.disks.tools` | ativa | não | [server/src/ui/js_disk.c](../server/src/ui/js_disk.c) | [sdk/smart.js](../sdk/smart.js); literal |
| `lord.display.apply` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.display.arrange` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.display.brightness` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/brightness.jsx](../sdk/brightness.jsx); literal |
| `lord.display.list` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.display.save` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.display.setCaptureAll` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.display.setIdle` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.display.setMain` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.display.setMirror` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.display.setTemperature` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.display.sun` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.display.temperature` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.doc` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.doc.arcCreate` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/archive.js](../sdk/archive.js); literal |
| `lord.doc.arcExtract` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/archive.js](../sdk/archive.js); literal |
| `lord.doc.arcList` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/archive.js](../sdk/archive.js); literal |
| `lord.doc.available` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.doc.bind` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.doc.blobRelease` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.doc.blobSave` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.doc.cancel` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.doc.close` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/archive.js](../sdk/archive.js), [sdk/pdf.js](../sdk/pdf.js); literal |
| `lord.doc.openBlob` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/pdf.js](../sdk/pdf.js); literal |
| `lord.doc.openPdf` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/pdf.js](../sdk/pdf.js); literal |
| `lord.doc.pending` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.doc.poll` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.doc.release` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.doc.request` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.doc.savePdf` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/pdf.js](../sdk/pdf.js); literal |
| `lord.doc.util` | ativa | não | [server/src/ui/js_doc.c](../server/src/ui/js_doc.c) | [sdk/doc.js](../sdk/doc.js); literal |
| `lord.fan` | ativa | não | [server/src/ui/js_fan.c](../server/src/ui/js_fan.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx), [sdk/cooling.jsx](../sdk/cooling.jsx); literal |
| `lord.fan.caseConfig` | ativa | não | [server/src/ui/js_fan.c](../server/src/ui/js_fan.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx), [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/cooling.jsx](../sdk/cooling.jsx), [sdk/suggestions.js](../sdk/suggestions.js), [apps/about/manifest.rs](../apps/about/manifest.rs); literal, services |
| `lord.fan.force` | ativa | não | [server/src/ui/js_fan.c](../server/src/ui/js_fan.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/cooling.jsx](../sdk/cooling.jsx); literal |
| `lord.fan.setCurve` | ativa | não | [server/src/ui/js_fan.c](../server/src/ui/js_fan.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/cooling.jsx](../sdk/cooling.jsx); literal |
| `lord.fan.setProfile` | ativa | não | [server/src/ui/js_fan.c](../server/src/ui/js_fan.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/cooling.jsx](../sdk/cooling.jsx); literal |
| `lord.fan.status` | ativa | não | [server/src/ui/js_fan.c](../server/src/ui/js_fan.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/cooling.jsx](../sdk/cooling.jsx); literal |
| `lord.files.request` | ativa | não | [server/src/ui/js_files.c](../server/src/ui/js_files.c) | [sdk/nativefiles.js](../sdk/nativefiles.js), [sdk/rustfiles.js](../sdk/rustfiles.js); literal |
| `lord.fonts.list` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [apps/terminal/Preferences.jsx](../apps/terminal/Preferences.jsx), [runtime/nativeapps.js](../runtime/nativeapps.js); literal |
| `lord.fs.copy` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js), [sdk/printdialog.jsx](../sdk/printdialog.jsx); literal |
| `lord.fs.du` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.emptyTrash` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.free` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.home` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [apps/terminal/Chrome.jsx](../apps/terminal/Chrome.jsx), [apps/terminal/index.jsx](../apps/terminal/index.jsx), [apps/terminal/state.js](../apps/terminal/state.js), [sdk/fs.js](../sdk/fs.js), [sdk/printdialog.jsx](../sdk/printdialog.jsx), [sdk/rustfiles.js](../sdk/rustfiles.js); literal |
| `lord.fs.jobCancel` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.jobForget` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.jobs` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.list` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [apps/terminal/Panes.jsx](../apps/terminal/Panes.jsx), [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.mkdir` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js), [sdk/rustfiles.js](../sdk/rustfiles.js); literal |
| `lord.fs.read` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.recent` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.removeFromTrash` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.rename` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.restore` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.search` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.stat` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js), [sdk/printdialog.jsx](../sdk/printdialog.jsx); literal |
| `lord.fs.trash` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.trashList` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.volumes` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fs.write` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/fs.js](../sdk/fs.js); literal |
| `lord.fw` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [sdk/firewall.js](../sdk/firewall.js), [sdk/notify.js](../sdk/notify.js); literal |
| `lord.fw.addList` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.fw.addRule` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.fw.blocks` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [sdk/notify.js](../sdk/notify.js); literal |
| `lord.fw.lists` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [apps/monitor/manifest.rs](../apps/monitor/manifest.rs); literal, services |
| `lord.fw.poll` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [apps/monitor/manifest.rs](../apps/monitor/manifest.rs); literal, services |
| `lord.fw.removeList` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.fw.removeRule` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.fw.rules` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); reads |
| `lord.fw.setAppRule` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.fw.setList` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.fw.setMode` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.fw.setRestricted` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.fw.setTrusted` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.fw.state` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [apps/monitor/manifest.rs](../apps/monitor/manifest.rs); literal, services |
| `lord.fw.status` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [apps/monitor/manifest.rs](../apps/monitor/manifest.rs); literal, services |
| `lord.fw.updateLists` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.fw.updateRule` | ativa | não | [server/src/ui/js_fw.c](../server/src/ui/js_fw.c) | [apps/monitor/manifest.rs](../apps/monitor/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.getLayout` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [apps/disks/index.jsx](../apps/disks/index.jsx), [apps/editor/EditorView.jsx](../apps/editor/EditorView.jsx), [apps/mail/BodyEditor.jsx](../apps/mail/BodyEditor.jsx), [apps/mail/HtmlView.jsx](../apps/mail/HtmlView.jsx), [apps/mail/index.jsx](../apps/mail/index.jsx), [apps/pdf/index.jsx](../apps/pdf/index.jsx), [apps/pdf/sidebar.jsx](../apps/pdf/sidebar.jsx), [apps/player/index.jsx](../apps/player/index.jsx), [apps/screenshots/index.jsx](../apps/screenshots/index.jsx), [apps/terminal/Chrome.jsx](../apps/terminal/Chrome.jsx), [apps/terminal/Panes.jsx](../apps/terminal/Panes.jsx), [apps/terminal/Preferences.jsx](../apps/terminal/Preferences.jsx), [apps/terminal/index.jsx](../apps/terminal/index.jsx), [examples/apps/fractal/index.jsx](../examples/apps/fractal/index.jsx), [runtime/events.js](../runtime/events.js), [sdk/components.jsx](../sdk/components.jsx), [sdk/controls.jsx](../sdk/controls.jsx), [sdk/cooling.jsx](../sdk/cooling.jsx), [sdk/layers.js](../sdk/layers.js), [sdk/menu.jsx](../sdk/menu.jsx), [sdk/nativetitlebar.jsx](../sdk/nativetitlebar.jsx), [sdk/netstatus.jsx](../sdk/netstatus.jsx), [sdk/network.jsx](../sdk/network.jsx), [sdk/rustfiles.js](../sdk/rustfiles.js), [sdk/rustlauncher.jsx](../sdk/rustlauncher.jsx); literal |
| `lord.greeter` | indisponível no Fedora | não | [server/src/ui/js.c](../server/src/ui/js.c) | [sdk/diskwatch.js](../sdk/diskwatch.js), [sdk/dock.js](../sdk/dock.js), [sdk/sessionrestore.jsx](../sdk/sessionrestore.jsx), [sdk/suggestions.js](../sdk/suggestions.js), [sdk/updates.js](../sdk/updates.js), [sdk/windowmemory.js](../sdk/windowmemory.js); literal |
| `lord.help.request` | ativa | não | [server/src/ui/js_help.c](../server/src/ui/js_help.c) | [sdk/help.js](../sdk/help.js); literal |
| `lord.hwinfo` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/suggestions.js](../sdk/suggestions.js); literal |
| `lord.hwinfo.scan` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/suggestions.js](../sdk/suggestions.js); literal |
| `lord.image` | usada pelos .lpk | local | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/markdown.jsx](../sdk/markdown.jsx); literal |
| `lord.image.size` | usada pelos .lpk | local | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/markdown.jsx](../sdk/markdown.jsx); literal |
| `lord.ime` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_input_services.js](../runtime/native_input_services.js), [sdk/keyboard.js](../sdk/keyboard.js); literal |
| `lord.ime.reset` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/ime.js](../sdk/ime.js); literal |
| `lord.ime.set` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/keyboard.js](../sdk/keyboard.js); literal |
| `lord.ime.setContext` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/ime.js](../sdk/ime.js); literal |
| `lord.input` | ativa | não | [server/src/ui/js_input.c](../server/src/ui/js_input.c) | [sdk/trackpad.js](../sdk/trackpad.js); literal |
| `lord.input.configure` | ativa | não | [server/src/ui/js_input.c](../server/src/ui/js_input.c) | [sdk/trackpad.js](../sdk/trackpad.js); literal |
| `lord.input.devices` | ativa | não | [server/src/ui/js_input.c](../server/src/ui/js_input.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/trackpad.js](../sdk/trackpad.js), [apps/settings/manifest.rs](../apps/settings/manifest.rs); literal, dockServices |
| `lord.input.hasKeyboard` | ativa | não | [server/src/ui/js_input.c](../server/src/ui/js_input.c) | [sdk/osk.js](../sdk/osk.js); literal |
| `lord.input.keyLabels` | ativa | não | [server/src/ui/js_input.c](../server/src/ui/js_input.c) | [sdk/osk.js](../sdk/osk.js); literal |
| `lord.input.tap` | ativa | não | [server/src/ui/js_input.c](../server/src/ui/js_input.c) | [sdk/osk.js](../sdk/osk.js); literal |
| `lord.insertBefore` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.keyboard` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/keyboard.js](../sdk/keyboard.js); literal |
| `lord.keyboard.current` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/keyboard.js](../sdk/keyboard.js); literal |
| `lord.keyboard.fallbackLayout` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/keyboard.js](../sdk/keyboard.js); literal |
| `lord.keyboard.list` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/keyboard.js](../sdk/keyboard.js); literal |
| `lord.keyboard.reloadCompose` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.keyboard.set` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/keyboard.js](../sdk/keyboard.js); literal |
| `lord.keyboard.setGroup` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/keyboard.js](../sdk/keyboard.js); literal |
| `lord.keyring` | ativa | não | [server/src/ui/js_vault.c](../server/src/ui/js_vault.c) | [sdk/vault.js](../sdk/vault.js); literal |
| `lord.keyring.request` | usada pelos .lpk | broker/limitada | [server/src/ui/js_vault.c](../server/src/ui/js_vault.c) | [sdk/vault.js](../sdk/vault.js); literal |
| `lord.lan` | ativa | não | [server/src/ui/js_lan.c](../server/src/ui/js_lan.c) | [sdk/lan.js](../sdk/lan.js); literal |
| `lord.lan.connect` | ativa | não | [server/src/ui/js_lan.c](../server/src/ui/js_lan.c) | [sdk/lan.js](../sdk/lan.js); literal |
| `lord.lan.connectPoll` | ativa | não | [server/src/ui/js_lan.c](../server/src/ui/js_lan.c) | [sdk/lan.js](../sdk/lan.js); literal |
| `lord.lan.connectTrust` | ativa | não | [server/src/ui/js_lan.c](../server/src/ui/js_lan.c) | [sdk/lan.js](../sdk/lan.js); literal |
| `lord.lan.disconnect` | ativa | não | [server/src/ui/js_lan.c](../server/src/ui/js_lan.c) | [sdk/lan.js](../sdk/lan.js); literal |
| `lord.lan.listPoll` | ativa | não | [server/src/ui/js_lan.c](../server/src/ui/js_lan.c) | [sdk/lan.js](../sdk/lan.js); literal |
| `lord.lan.listShares` | ativa | não | [server/src/ui/js_lan.c](../server/src/ui/js_lan.c) | [sdk/lan.js](../sdk/lan.js); literal |
| `lord.lan.poll` | ativa | não | [server/src/ui/js_lan.c](../server/src/ui/js_lan.c) | [sdk/lan.js](../sdk/lan.js); literal |
| `lord.lan.start` | ativa | não | [server/src/ui/js_lan.c](../server/src/ui/js_lan.c) | [sdk/lan.js](../sdk/lan.js); literal |
| `lord.lan.stop` | ativa | não | [server/src/ui/js_lan.c](../server/src/ui/js_lan.c) | [sdk/lan.js](../sdk/lan.js); literal |
| `lord.launcher` | usada pelos .lpk | local | [server/src/ui/js_launcher.c](../server/src/ui/js_launcher.c) | [sdk/smartparse.js](../sdk/smartparse.js); literal |
| `lord.launcher.request` | usada pelos .lpk | local | [server/src/ui/js_launcher.c](../server/src/ui/js_launcher.c) | [sdk/apps.js](../sdk/apps.js), [sdk/dockmodel.js](../sdk/dockmodel.js), [sdk/oskmodel.js](../sdk/oskmodel.js), [sdk/sessionmodel.js](../sdk/sessionmodel.js), [sdk/smartparse.js](../sdk/smartparse.js); literal |
| `lord.loadApp` | usada pelos .lpk | local | [server/src/ui/js_broker.c](../server/src/ui/js_broker.c) | [runtime/app-host.js](../runtime/app-host.js); literal |
| `lord.locale.available` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.locale.catalog` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/apps.js](../sdk/apps.js), [sdk/emoji.jsx](../sdk/emoji.jsx), [sdk/i18n.js](../sdk/i18n.js), [sdk/search.js](../sdk/search.js); literal |
| `lord.locale.get` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/i18n.js](../sdk/i18n.js); literal |
| `lord.locale.installPack` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.locale.languages` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/i18n.js](../sdk/i18n.js); literal |
| `lord.locale.set` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.log` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/console |
| `lord.logs` | ativa | não | [server/src/ui/js_logs.c](../server/src/ui/js_logs.c) | [sdk/logs.js](../sdk/logs.js); literal |
| `lord.logs.files` | ativa | não | [server/src/ui/js_logs.c](../server/src/ui/js_logs.c) | [sdk/logs.js](../sdk/logs.js); literal |
| `lord.logs.follow` | ativa | não | [server/src/ui/js_logs.c](../server/src/ui/js_logs.c) | [sdk/logs.js](../sdk/logs.js); literal |
| `lord.logs.query` | ativa | não | [server/src/ui/js_logs.c](../server/src/ui/js_logs.c) | [sdk/logs.js](../sdk/logs.js); literal |
| `lord.logs.sources` | ativa | não | [server/src/ui/js_logs.c](../server/src/ui/js_logs.c) | [sdk/logs.js](../sdk/logs.js); literal |
| `lord.logs.tail` | ativa | não | [server/src/ui/js_logs.c](../server/src/ui/js_logs.c) | [sdk/logs.js](../sdk/logs.js); literal |
| `lord.logs.take` | ativa | não | [server/src/ui/js_logs.c](../server/src/ui/js_logs.c) | [sdk/logs.js](../sdk/logs.js); literal |
| `lord.mail` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [sdk/calendar.js](../sdk/calendar.js), [sdk/mail.js](../sdk/mail.js); literal |
| `lord.mail.allowImages` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [apps/mail/index.jsx](../apps/mail/index.jsx); literal |
| `lord.mail.appMode` | compatibilidade | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [sdk/mail.js](../sdk/mail.js); literal |
| `lord.mail.attachmentFd` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [apps/mail/index.jsx](../apps/mail/index.jsx); literal |
| `lord.mail.available` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [sdk/calendar.js](../sdk/calendar.js), [sdk/mail.js](../sdk/mail.js); literal |
| `lord.mail.bind` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [apps/mail/HtmlView.jsx](../apps/mail/HtmlView.jsx), [sdk/contacts.js](../sdk/contacts.js), [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.mail.cacheDir` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [apps/mail/index.jsx](../apps/mail/index.jsx); literal |
| `lord.mail.config` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [sdk/mail.js](../sdk/mail.js); literal |
| `lord.mail.openAttachment` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [apps/mail/index.jsx](../apps/mail/index.jsx); literal |
| `lord.mail.poll` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [sdk/mail.js](../sdk/mail.js); literal |
| `lord.mail.release` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [apps/mail/HtmlView.jsx](../apps/mail/HtmlView.jsx), [runtime/nativeapps.js](../runtime/nativeapps.js), [sdk/contacts.js](../sdk/contacts.js); literal |
| `lord.mail.request` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [sdk/mail.js](../sdk/mail.js); literal |
| `lord.mail.saveConfig` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [sdk/mail.js](../sdk/mail.js); literal |
| `lord.mail.start` | ativa | app de sistema autorizado | [server/src/ui/js_mail.c](../server/src/ui/js_mail.c) | [sdk/mail.js](../sdk/mail.js); literal |
| `lord.measureText` | usada pelos .lpk | local | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [apps/editor/EditorView.jsx](../apps/editor/EditorView.jsx), [apps/mail/BodyEditor.jsx](../apps/mail/BodyEditor.jsx), [apps/terminal/Chrome.jsx](../apps/terminal/Chrome.jsx), [sdk/menu.jsx](../sdk/menu.jsx), [sdk/rustfiles.js](../sdk/rustfiles.js); literal |
| `lord.media` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [apps/screenshots/index.jsx](../apps/screenshots/index.jsx), [sdk/media.js](../sdk/media.js); literal |
| `lord.media.annotate` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.audioRecord` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.camera` | usada pelos .lpk | broker/limitada | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.cameraRecord` | usada pelos .lpk | broker/limitada | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.cameras` | usada pelos .lpk | broker/limitada | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.close` | usada pelos .lpk | broker/limitada | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.control` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.copyImage` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.crop` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.edit` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.hwaccel` | usada pelos .lpk | broker/limitada | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.hwDecode` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/video.js](../sdk/video.js); literal |
| `lord.media.image` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.info` | usada pelos .lpk | broker/limitada | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.photo` | usada pelos .lpk | broker/limitada | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.play` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/nativefiles.js](../sdk/nativefiles.js), [sdk/media.js](../sdk/media.js); literal, wrapper literal |
| `lord.media.poster` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.probe` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.protectedCount` | usada pelos .lpk | broker/limitada | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.saveImage` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.screenRecord` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.screenshot` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.stop` | usada pelos .lpk | broker/limitada | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.take` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.thumb` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.transcode` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.view` | usada pelos .lpk | broker/limitada | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.media.waveform` | ativa | não | [server/src/ui/js_media.c](../server/src/ui/js_media.c) | [sdk/media.js](../sdk/media.js); wrapper literal |
| `lord.mime` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/defaults.js](../sdk/defaults.js), [sdk/sysopen.js](../sdk/sysopen.js); literal |
| `lord.mime.get` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/defaults.js](../sdk/defaults.js); literal |
| `lord.mime.guess` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/nativefiles.js](../sdk/nativefiles.js), [sdk/rustfiles.js](../sdk/rustfiles.js), [sdk/sysopen.js](../sdk/sysopen.js); literal |
| `lord.mime.resolve` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/defaults.js](../sdk/defaults.js); literal |
| `lord.mime.set` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/defaults.js](../sdk/defaults.js); literal |
| `lord.mime.setTerminal` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/defaults.js](../sdk/defaults.js); literal |
| `lord.mime.terminal` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/defaults.js](../sdk/defaults.js); literal |
| `lord.mime.unset` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/defaults.js](../sdk/defaults.js); literal |
| `lord.monitor.diskTemps` | ativa | não | [server/src/ui/js_monitor.c](../server/src/ui/js_monitor.c) | [sdk/diskwatch.js](../sdk/diskwatch.js); literal |
| `lord.monitor.netTotals` | ativa | não | [server/src/ui/js_monitor.c](../server/src/ui/js_monitor.c) | [sdk/netstatus.jsx](../sdk/netstatus.jsx); literal |
| `lord.monitor.watchDisks` | ativa | não | [server/src/ui/js_monitor.c](../server/src/ui/js_monitor.c) | [sdk/diskwatch.js](../sdk/diskwatch.js); literal |
| `lord.nativeApps` | ativa | próprio app compilado | [server/src/ui/js_native_apps.c](../server/src/ui/js_native_apps.c) | [sdk/appcontrol.js](../sdk/appcontrol.js); literal |
| `lord.nativeApps.create` | ativa | próprio app compilado | [server/src/ui/js_native_apps.c](../server/src/ui/js_native_apps.c) | [apps/pdf/model.js](../apps/pdf/model.js), [apps/sysinfo/checks.js](../apps/sysinfo/checks.js), [apps/sysinfo/model.js](../apps/sysinfo/model.js), [apps/sysinfo/perf.js](../apps/sysinfo/perf.js), [apps/terminal/keys.js](../apps/terminal/keys.js), [apps/terminal/profiles.js](../apps/terminal/profiles.js), [runtime/native_session_widgets.js](../runtime/native_session_widgets.js), [sdk/nativeapps.js](../sdk/nativeapps.js); literal |
| `lord.nativeAsync` | ativa | adaptador JS compartilhado | [runtime/nativeapps.js](../runtime/nativeapps.js) | [apps/passwords/index.jsx](../apps/passwords/index.jsx), [runtime/nativeapps.js](../runtime/nativeapps.js); literal |
| `lord.nativeAsync.events` | ativa | adaptador JS compartilhado | [runtime/nativeapps.js](../runtime/nativeapps.js) | [apps/logs/manifest.rs](../apps/logs/manifest.rs); reads |
| `lord.nativeAsync.start` | ativa | adaptador JS compartilhado | [runtime/nativeapps.js](../runtime/nativeapps.js) | [apps/backup/manifest.rs](../apps/backup/manifest.rs), [apps/calendar/manifest.rs](../apps/calendar/manifest.rs), [apps/camera/manifest.rs](../apps/camera/manifest.rs), [apps/contacts/manifest.rs](../apps/contacts/manifest.rs), [apps/fonts/manifest.rs](../apps/fonts/manifest.rs), [apps/logs/manifest.rs](../apps/logs/manifest.rs), [apps/notes/manifest.rs](../apps/notes/manifest.rs), [apps/photos/manifest.rs](../apps/photos/manifest.rs), [apps/recorder/manifest.rs](../apps/recorder/manifest.rs), [apps/reminders/manifest.rs](../apps/reminders/manifest.rs), [apps/scanner/manifest.rs](../apps/scanner/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); calls |
| `lord.nativeAsync.take` | ativa | adaptador JS compartilhado | [runtime/nativeapps.js](../runtime/nativeapps.js) | [apps/backup/manifest.rs](../apps/backup/manifest.rs), [apps/calendar/manifest.rs](../apps/calendar/manifest.rs), [apps/camera/manifest.rs](../apps/camera/manifest.rs), [apps/contacts/manifest.rs](../apps/contacts/manifest.rs), [apps/fonts/manifest.rs](../apps/fonts/manifest.rs), [apps/logs/manifest.rs](../apps/logs/manifest.rs), [apps/notes/manifest.rs](../apps/notes/manifest.rs), [apps/photos/manifest.rs](../apps/photos/manifest.rs), [apps/recorder/manifest.rs](../apps/recorder/manifest.rs), [apps/reminders/manifest.rs](../apps/reminders/manifest.rs), [apps/scanner/manifest.rs](../apps/scanner/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); reads |
| `lord.nativeViews` | ativa | próprio app compilado | [server/src/ui/js_nativeview.c](../server/src/ui/js_nativeview.c) | [runtime/events.js](../runtime/events.js); literal |
| `lord.nativeViews.action` | ativa | próprio app compilado | [server/src/ui/js_nativeview.c](../server/src/ui/js_nativeview.c) | [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.nativeViews.facts` | ativa | próprio app compilado | [server/src/ui/js_nativeview.c](../server/src/ui/js_nativeview.c) | [sdk/nativetitlebar.jsx](../sdk/nativetitlebar.jsx), [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.nativeViews.focus` | ativa | próprio app compilado | [server/src/ui/js_nativeview.c](../server/src/ui/js_nativeview.c) | [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.nativeViews.mount` | ativa | próprio app compilado | [server/src/ui/js_nativeview.c](../server/src/ui/js_nativeview.c) | [sdk/nativetitlebar.jsx](../sdk/nativetitlebar.jsx), [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.nativeViews.paste` | ativa | próprio app compilado | [server/src/ui/js_nativeview.c](../server/src/ui/js_nativeview.c) | [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.nativeViews.result` | ativa | próprio app compilado | [server/src/ui/js_nativeview.c](../server/src/ui/js_nativeview.c) | [sdk/nativefiles.js](../sdk/nativefiles.js), [sdk/nativemenubar.js](../sdk/nativemenubar.js), [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.nativeViews.take` | ativa | próprio app compilado | [server/src/ui/js_nativeview.c](../server/src/ui/js_nativeview.c) | [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.nativeViews.unmount` | ativa | próprio app compilado | [server/src/ui/js_nativeview.c](../server/src/ui/js_nativeview.c) | [sdk/nativetitlebar.jsx](../sdk/nativetitlebar.jsx), [sdk/nativeview.js](../sdk/nativeview.js); literal |
| `lord.net.config` | compatibilidade | não | [server/src/ui/js_net.c](../server/src/ui/js_net.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.net.dns` | ativa | não | [server/src/ui/js_net.c](../server/src/ui/js_net.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.net.host` | ativa | não | [server/src/ui/js_net.c](../server/src/ui/js_net.c) | [sdk/network.jsx](../sdk/network.jsx); literal |
| `lord.net.interfaces` | ativa | não | [server/src/ui/js_net.c](../server/src/ui/js_net.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.net.prepareOvpn` | compatibilidade | não | [server/src/ui/js_net.c](../server/src/ui/js_net.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.net.routes` | ativa | não | [server/src/ui/js_net.c](../server/src/ui/js_net.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.net.tray` | ativa | não | [server/src/ui/js_net.c](../server/src/ui/js_net.c) | [sdk/tray.jsx](../sdk/tray.jsx); literal |
| `lord.net.vpnList` | compatibilidade | não | [server/src/ui/js_net.c](../server/src/ui/js_net.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.net.vpnTypes` | compatibilidade | não | [server/src/ui/js_net.c](../server/src/ui/js_net.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.notifyd.action` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.notifyd.closed` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.now` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/events.js](../runtime/events.js), [runtime/index.js](../runtime/index.js), [sdk/apps.js](../sdk/apps.js), [sdk/components.jsx](../sdk/components.jsx), [sdk/osk.jsx](../sdk/osk.jsx), [sdk/packages.js](../sdk/packages.js), [sdk/rustfiles.js](../sdk/rustfiles.js), [sdk/trackpad.js](../sdk/trackpad.js), [sdk/windows.js](../sdk/windows.js); literal |
| `lord.perms` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.perms.get` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.perms.list` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.perms.names` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.perms.set` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [sdk/privacy.js](../sdk/privacy.js); literal |
| `lord.pkg` | indisponível no Fedora | não | [server/src/ui/js.c](../server/src/ui/js.c) | [sdk/appcontrol.js](../sdk/appcontrol.js), [sdk/suggestions.js](../sdk/suggestions.js), [sdk/updates.js](../sdk/updates.js); literal |
| `lord.pkg.available` | indisponível no Fedora | não | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/suggestions.js](../sdk/suggestions.js), [sdk/updates.js](../sdk/updates.js); literal |
| `lord.pkg.info` | indisponível no Fedora | não | [server/src/ui/js.c](../server/src/ui/js.c) | [sdk/suggestions.js](../sdk/suggestions.js), [sdk/updates.js](../sdk/updates.js); literal |
| `lord.pkg.lastUpdate` | indisponível no Fedora | não | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/updates.js](../sdk/updates.js); literal |
| `lord.pkg.plan` | indisponível no Fedora | não | [server/src/ui/js.c](../server/src/ui/js.c) | [sdk/suggestions.js](../sdk/suggestions.js); literal |
| `lord.pkg.rebootRequired` | indisponível no Fedora | não | [server/src/ui/js.c](../server/src/ui/js.c) | [sdk/updates.js](../sdk/updates.js); literal |
| `lord.pkg.releaseNotes` | indisponível no Fedora | não | [server/src/ui/js.c](../server/src/ui/js.c) | [sdk/updates.js](../sdk/updates.js); literal |
| `lord.pkg.upgrades` | indisponível no Fedora | não | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/appcontrol.js](../sdk/appcontrol.js), [sdk/updates.js](../sdk/updates.js); literal |
| `lord.portal` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/theme.js](../sdk/theme.js); literal |
| `lord.portal.pickResult` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/powerbox.jsx](../sdk/powerbox.jsx); literal |
| `lord.portal.set` | retirada em 2026-11-22 | não | [rust/axia-shell/src/session_appearance.rs](../rust/axia-shell/src/session_appearance.rs) | Efeito React e ponte removidos; serviço residente lê o perfil e aplica diretamente. A captura original usava sdk/theme.js. |
| `lord.power.buttons` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.power.profiles` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.power.setProfile` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.power.state` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.print` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.add` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.cancel` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.discover` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.hasCups` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.jobs` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.poll` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.printers` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.remove` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.scan` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.scanCaps` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.setDefault` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.print.submit` | ativa | não | [server/src/ui/js_print.c](../server/src/ui/js_print.c) | [sdk/print.js](../sdk/print.js); literal |
| `lord.quit` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/sessionrestore.jsx](../sdk/sessionrestore.jsx); literal |
| `lord.removeChild` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.screen.displays` | ativa | não | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.screen.height` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [apps/player/index.jsx](../apps/player/index.jsx), [apps/screenshots/index.jsx](../apps/screenshots/index.jsx), [runtime/index.js](../runtime/index.js); literal |
| `lord.screen.mirror` | ativa | não | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.screen.width` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [apps/player/index.jsx](../apps/player/index.jsx), [apps/screenshots/index.jsx](../apps/screenshots/index.jsx), [runtime/index.js](../runtime/index.js); literal |
| `lord.session` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/hardware.jsx](../sdk/hardware.jsx), [sdk/index.js](../sdk/index.js), [sdk/shortcuts.js](../sdk/shortcuts.js); literal |
| `lord.session.hostPower` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/power.jsx](../sdk/power.jsx); literal |
| `lord.session.lock` | indisponível no Fedora | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/hardware.jsx](../sdk/hardware.jsx), [sdk/index.js](../sdk/index.js), [sdk/shortcuts.js](../sdk/shortcuts.js); literal |
| `lord.session.managed` | indisponível no Fedora | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/hardware.jsx](../sdk/hardware.jsx), [sdk/index.js](../sdk/index.js), [sdk/shortcuts.js](../sdk/shortcuts.js); literal |
| `lord.session.power` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/power.jsx](../sdk/power.jsx); literal |
| `lord.session.switchUser` | indisponível no Fedora | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/index.js](../sdk/index.js); literal |
| `lord.setProp` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.setRoot` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.setStyle` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [apps/sysinfo/scene.jsx](../apps/sysinfo/scene.jsx), [runtime/index.js](../runtime/index.js), [sdk/components.jsx](../sdk/components.jsx), [sdk/cooling.jsx](../sdk/cooling.jsx); literal |
| `lord.setText` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js); literal |
| `lord.settings.glassMaterial` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/glass.js](../sdk/glass.js), [themes/default/tokens.js](../themes/default/tokens.js); literal |
| `lord.settings.importWallpaper` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.settings.load` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/settings.js](../sdk/settings.js); literal |
| `lord.settings.save` | usada pelos .lpk | broker/limitada | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/index.js](../sdk/index.js), [sdk/settings.js](../sdk/settings.js); literal |
| `lord.sign.restore` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [apps/store/manifest.rs](../apps/store/manifest.rs); calls |
| `lord.sign.share` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [apps/store/manifest.rs](../apps/store/manifest.rs); calls |
| `lord.sign.status` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [apps/store/manifest.rs](../apps/store/manifest.rs); reads |
| `lord.sign.userKey` | ativa | não | [server/src/ui/js_sec.c](../server/src/ui/js_sec.c) | [apps/store/manifest.rs](../apps/store/manifest.rs); reads |
| `lord.snap` | ativa | não | [server/src/ui/js_snap.c](../server/src/ui/js_snap.c) | [sdk/overview.jsx](../sdk/overview.jsx), [sdk/windows.js](../sdk/windows.js); literal |
| `lord.snap.assist` | ativa | não | [server/src/ui/js_snap.c](../server/src/ui/js_snap.c) | [sdk/windows.js](../sdk/windows.js); namespace por alias |
| `lord.snap.next` | ativa | não | [server/src/ui/js_snap.c](../server/src/ui/js_snap.c) | [sdk/windows.js](../sdk/windows.js); namespace por alias |
| `lord.snap.rect` | ativa | não | [server/src/ui/js_snap.c](../server/src/ui/js_snap.c) | [sdk/windows.js](../sdk/windows.js); namespace por alias |
| `lord.snap.zoneAt` | ativa | não | [server/src/ui/js_snap.c](../server/src/ui/js_snap.c) | [sdk/windows.js](../sdk/windows.js); namespace por alias |
| `lord.sound` | ativa | não | [server/src/ui/js_sound.c](../server/src/ui/js_sound.c) | [sdk/sound.js](../sdk/sound.js); literal |
| `lord.sound.meter` | ativa | não | [server/src/ui/js_sound.c](../server/src/ui/js_sound.c) | [sdk/sound.js](../sdk/sound.js); wrapper literal |
| `lord.sound.move` | ativa | não | [server/src/ui/js_sound.c](../server/src/ui/js_sound.c) | [sdk/sound.js](../sdk/sound.js); wrapper literal |
| `lord.sound.play` | ativa | não | [server/src/ui/js_sound.c](../server/src/ui/js_sound.c) | [sdk/sound.js](../sdk/sound.js); wrapper literal |
| `lord.sound.setDefault` | ativa | não | [server/src/ui/js_sound.c](../server/src/ui/js_sound.c) | [sdk/sound.js](../sdk/sound.js); wrapper literal |
| `lord.sound.setMute` | ativa | não | [server/src/ui/js_sound.c](../server/src/ui/js_sound.c) | [sdk/sound.js](../sdk/sound.js); wrapper literal |
| `lord.sound.setPort` | ativa | não | [server/src/ui/js_sound.c](../server/src/ui/js_sound.c) | [sdk/sound.js](../sdk/sound.js); wrapper literal |
| `lord.sound.setVolume` | ativa | não | [server/src/ui/js_sound.c](../server/src/ui/js_sound.c) | [sdk/sound.js](../sdk/sound.js); wrapper literal |
| `lord.sound.state` | ativa | não | [server/src/ui/js_sound.c](../server/src/ui/js_sound.c) | [sdk/sound.js](../sdk/sound.js); literal |
| `lord.stats` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx), [runtime/index.js](../runtime/index.js), [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/system.js](../sdk/system.js), [apps/about/manifest.rs](../apps/about/manifest.rs); literal, services |
| `lord.switcher.begin` | ativa | não | [server/src/ui/js_switcher.c](../server/src/ui/js_switcher.c) | [sdk/appswitcher.jsx](../sdk/appswitcher.jsx); literal |
| `lord.switcher.finish` | ativa | não | [server/src/ui/js_switcher.c](../server/src/ui/js_switcher.c) | [sdk/appswitcher.jsx](../sdk/appswitcher.jsx); literal |
| `lord.switcher.focus` | ativa | não | [server/src/ui/js_switcher.c](../server/src/ui/js_switcher.c) | [sdk/appswitcher.jsx](../sdk/appswitcher.jsx); literal |
| `lord.switcher.remove` | ativa | não | [server/src/ui/js_switcher.c](../server/src/ui/js_switcher.c) | [sdk/appswitcher.jsx](../sdk/appswitcher.jsx); literal |
| `lord.switcher.select` | ativa | não | [server/src/ui/js_switcher.c](../server/src/ui/js_switcher.c) | [sdk/appswitcher.jsx](../sdk/appswitcher.jsx); literal |
| `lord.switcher.step` | ativa | não | [server/src/ui/js_switcher.c](../server/src/ui/js_switcher.c) | [sdk/appswitcher.jsx](../sdk/appswitcher.jsx); literal |
| `lord.sysinfo` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [apps/sysinfo/performance.jsx](../apps/sysinfo/performance.jsx), [sdk/system.js](../sdk/system.js); literal |
| `lord.sysinfo.bench` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [apps/sysinfo/performance.jsx](../apps/sysinfo/performance.jsx); literal |
| `lord.sysinfo.bench.saved` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx); literal |
| `lord.sysinfo.community` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx); literal |
| `lord.sysinfo.facts` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx); literal |
| `lord.sysinfo.markdown` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx); literal |
| `lord.sysinfo.peripherals` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx); literal |
| `lord.sysinfo.probe` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [apps/sysinfo/index.jsx](../apps/sysinfo/index.jsx); literal |
| `lord.sysinfo.report` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [sdk/system.js](../sdk/system.js); literal |
| `lord.system.cpuTimes` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/system.js](../sdk/system.js); literal |
| `lord.system.info` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [apps/terminal/state.js](../apps/terminal/state.js), [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/system.js](../sdk/system.js), [sdk/windows.js](../sdk/windows.js), [apps/about/manifest.rs](../apps/about/manifest.rs), [apps/settings/manifest.rs](../apps/settings/manifest.rs); literal, services, dockServices |
| `lord.term.ackRestored` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.clear` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.close` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); literal |
| `lord.term.configure` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.copy` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.copyKey` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.copyMode` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.create` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); literal |
| `lord.term.cursorWord` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.export` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.feed` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.focus` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.highlight` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.info` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.key` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.list` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); literal |
| `lord.term.log` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.logicalText` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.mark` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.mouse` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.paste` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.replay` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.scroll` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.search` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.searchMove` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.select` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.setMatches` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.text` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.tmuxCommand` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.tmuxPane` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.tmuxReady` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.urlAt` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.wheel` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.words` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.term.write` | ativa | não | [server/src/ui/js_term.c](../server/src/ui/js_term.c) | [sdk/terminal.js](../sdk/terminal.js); wrapper literal |
| `lord.text.direction` | usada pelos .lpk | local | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/i18n.js](../sdk/i18n.js); literal |
| `lord.text.setDirection` | usada pelos .lpk | local | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/i18n.js](../sdk/i18n.js); literal |
| `lord.text.setLanguage` | usada pelos .lpk | local | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/i18n.js](../sdk/i18n.js); literal |
| `lord.textEdit` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [sdk/components.jsx](../sdk/components.jsx); literal |
| `lord.themes.list` | retirar | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/index.js](../sdk/index.js); literal |
| `lord.themes.use` | retirar | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/index.js](../sdk/index.js); literal |
| `lord.time` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/hardware.jsx](../sdk/hardware.jsx); literal |
| `lord.time.reload` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.time.zone` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/hardware.jsx](../sdk/hardware.jsx), [apps/clock/manifest.rs](../apps/clock/manifest.rs); literal, reads |
| `lord.time.zoneOffset` | ativa | não | [server/src/ui/js_help.c](../server/src/ui/js_help.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [apps/clock/manifest.rs](../apps/clock/manifest.rs); literal, reads |
| `lord.time.zones` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [apps/clock/manifest.rs](../apps/clock/manifest.rs); literal, reads |
| `lord.timedate.refresh` | ativa | não | [server/src/ui/js_hw.c](../server/src/ui/js_hw.c) | [runtime/native_system_services.js](../runtime/native_system_services.js); literal |
| `lord.timers.add` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.timers.cancel` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.timers.cancelFrame` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.timers.changed` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.timers.due` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.timers.fire` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.timers.frames` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.timers.hasFrames` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.timers.next` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.timers.requestFrame` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.timers.reset` | usada pelos .lpk | local | [server/src/ui/js_timers.c](../server/src/ui/js_timers.c) | [server/src/ui/js_prelude.h](../server/src/ui/js_prelude.h); prelude/timers |
| `lord.users` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/power.jsx](../sdk/power.jsx); literal |
| `lord.users.list` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/power.jsx](../sdk/power.jsx); literal |
| `lord.vault` | ativa | app de sistema autorizado | [server/src/ui/js_vault.c](../server/src/ui/js_vault.c) | [sdk/vault.js](../sdk/vault.js); literal |
| `lord.vault.available` | ativa | app de sistema autorizado | [server/src/ui/js_vault.c](../server/src/ui/js_vault.c) | [sdk/vault.js](../sdk/vault.js); literal |
| `lord.vault.fidoAttach` | ativa | app de sistema autorizado | [server/src/ui/js_vault.c](../server/src/ui/js_vault.c) | [apps/passwords/index.jsx](../apps/passwords/index.jsx); literal |
| `lord.vault.poll` | ativa | app de sistema autorizado | [server/src/ui/js_vault.c](../server/src/ui/js_vault.c) | [sdk/vault.js](../sdk/vault.js); literal |
| `lord.vault.promptDone` | ativa | app de sistema autorizado | [server/src/ui/js_vault.c](../server/src/ui/js_vault.c) | [sdk/vault.js](../sdk/vault.js); literal |
| `lord.vault.request` | ativa | app de sistema autorizado | [server/src/ui/js_vault.c](../server/src/ui/js_vault.c) | [sdk/vault.js](../sdk/vault.js); literal |
| `lord.version` | usada pelos .lpk | local | [server/src/ui/js.c](../server/src/ui/js.c) | [runtime/index.js](../runtime/index.js), [runtime/native_system_services.js](../runtime/native_system_services.js), [sdk/nativeview.js](../sdk/nativeview.js), [sdk/system.js](../sdk/system.js); literal |
| `lord.wayland` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/clipboard.js](../sdk/clipboard.js), [sdk/emoji.jsx](../sdk/emoji.jsx), [sdk/shortcuts.js](../sdk/shortcuts.js), [sdk/wayland.js](../sdk/wayland.js); literal |
| `lord.wayland.close` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/nativeappmodel.js](../sdk/nativeappmodel.js), [sdk/wayland.js](../sdk/wayland.js); literal |
| `lord.wayland.focus` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/wayland.js](../sdk/wayland.js); literal |
| `lord.wayland.fullscreen` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/appswitcher.jsx](../sdk/appswitcher.jsx), [sdk/dockpreviews.jsx](../sdk/dockpreviews.jsx), [sdk/overview.jsx](../sdk/overview.jsx), [sdk/shortcuts.js](../sdk/shortcuts.js); literal |
| `lord.wayland.fullscreenWindow` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/appswitcher.jsx](../sdk/appswitcher.jsx); literal |
| `lord.wayland.grabKeys` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/shortcuts.js](../sdk/shortcuts.js); literal |
| `lord.wayland.hasKeyboard` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/clipboard.js](../sdk/clipboard.js), [sdk/emoji.jsx](../sdk/emoji.jsx); literal |
| `lord.wayland.info` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/wayland.js](../sdk/wayland.js); literal |
| `lord.wayland.kill` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/nativeappmodel.js](../sdk/nativeappmodel.js); literal |
| `lord.wayland.paste` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/clipboard.js](../sdk/clipboard.js), [sdk/emoji.jsx](../sdk/emoji.jsx); literal |
| `lord.wayland.state` | ativa | não | [server/src/ui/js_sys.c](../server/src/ui/js_sys.c) | [sdk/wayland.js](../sdk/wayland.js); literal |
| `lord.windowList` | usada pelos .lpk | local | [server/src/ui/js_winmodel.c](../server/src/ui/js_winmodel.c) | [sdk/windows.js](../sdk/windows.js); literal |
| `lord.windowList.call` | usada pelos .lpk | local | [server/src/ui/js_winmodel.c](../server/src/ui/js_winmodel.c) | [sdk/windows.js](../sdk/windows.js); literal |
| `lord.winmodel` | usada pelos .lpk | local | [server/src/ui/js_winmodel.c](../server/src/ui/js_winmodel.c) | [sdk/winmodel.js](../sdk/winmodel.js); literal |
| `lord.wm` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/windows.js](../sdk/windows.js); literal |
| `lord.wm.cancel` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/windows.js](../sdk/windows.js); literal |
| `lord.wm.grab` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/windows.js](../sdk/windows.js); literal |
| `lord.wm.pointer` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/windows.js](../sdk/windows.js); literal |
| `lord.wm.policy` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/windows.js](../sdk/windows.js); literal |
| `lord.wm.preview` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/dockpreviews.jsx](../sdk/dockpreviews.jsx); literal |
| `lord.wm.previewSize` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/dockpreviews.jsx](../sdk/dockpreviews.jsx); dimensões retidas, sem captura |
| `lord.wm.script` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/windows.js](../sdk/windows.js); literal |
| `lord.wm.thumbnail` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/dockpreviews.jsx](../sdk/dockpreviews.jsx); literal |
| `lord.wm.thumbnailSurface` | ativa | não | [server/src/ui/js_wm.c](../server/src/ui/js_wm.c) | [sdk/dockpreviews.jsx](../sdk/dockpreviews.jsx); literal |

## Pontos dinâmicos e direção da migração

| Ponto | Como se determina o alvo | Destino |
| --- | --- | --- |
| `sdk/hardware.jsx: lord[name]` | Teste de capacidade; os nomes display/power/bt e os métodos literais estão na tabela | Serviços Rust observáveis; não criar polling novo |
| `sdk/terminal.js: lord.term[name]` | Nomes literais de `pass(...)`, expandidos acima | Preservar o backend VT e migrar o app na Fase 5 |
| `sdk/sound.js: lord.sound[fn]` | Nomes literais de `call(...)`, expandidos acima | Eventos do serviço de áudio para Rust |
| `sdk/media.js: api()[name]` | `api()` devolve lord.media; nomes literais dos wrappers expandidos acima | Adaptadores de mídia tipados; manter o subconjunto de câmera do broker |
| `nativeAnim()` e `snapApi()` | Namespaces anim/snap; superfície registrada incluída acima | Reutilizar anim/snap Rust sem serializar árvores |
| `sdk/nativeappmodel.js`, `sdk/nativeview.js`: `lord[effect.service][effect.method]` | `reads/calls` do manifesto; rejeita métodos não listados | Serviço Rust com efeitos tipados e checagem equivalente |
| `sdk/nativeview.js: services()` | `services/serviceIntervals`, inclusive `$root`; amostragem de 1 s quando há consumidor | Eventos/revisões do dono Rust |
| `sdk/nativeapps.js`: `dockServices` | Métodos do manifesto usados para ações da Dock | Catálogo compartilhado e eventos, sem clonagem por leitura |
| `runtime/nativeapps.js`: `lord.nativeAsync` | Instala adaptador JS; `asyncServices` limita cada lease/janela | Transporte assíncrono Rust; manter isolamento e descarte |
| `js_prelude.h`: `native = lord.timers` | Métodos de js_timers.c; callbacks JS e agendamento Rust | Manter no host dos .lpk, retirar da sessão somente na Fase 6 |

O catálogo não congela novos privilégios nem transforma APIs internas dos apps
Rust em API pública de terceiros. O congelamento/versionamento executável do
host é a Fase 7. Preservar os pacotes conhecidos e testar o broker antes de
reduzir exports; nenhum acesso acima autoriza mover um .lpk para dentro da sessão.

## Evidências e atualização

Os dados completos e o helper da captura ficam em
`build/validation/shell-phase0/inventory.json`, `inventory.cjs` e
`app-host-metafile.json` (ignorados pelo Git). Reprodução local da captura:
`node --expose-internals build/validation/shell-phase0/inventory.cjs`.
O uso do Acorn interno é só deste levantamento, não uma dependência do produto.
Revisar a tabela e o grafo depois de cada mudança de exports, registro de contexto
ou manifesto. Não comparar contagens de recortes diferentes como regressão.
