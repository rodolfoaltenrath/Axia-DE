# Fronteira do SDK dos apps .lpk

Fase 0, 2026-11-18. Referência documental: `lpk-sdk-0.8.7/ffabc1aa`.
Esse identificador é da captura: não adiciona campos ao manifesto, negociação
de versão nem uma nova API em runtime. O congelamento executável fica na Fase 7.

## Contrato que a opção A preserva

Cada app continua no processo `axia-server --app <id>`, com QuickJS, React e
renderer próprios. A sessão futura não inicializa QuickJS. Manifesto, permissões,
assinatura e formato .lpk seguem independentes dessa execução. Não converter os
apps de terceiros nem avaliar seu app.js no contexto da sessão.

[tools/lpk.mjs](../tools/lpk.mjs) externaliza React/JSX, `@lordos/runtime` e `@lordos/sdk`;
[runtime/app-host.js](../runtime/app-host.js) disponibiliza esses namespaces por
`globalThis.__lordos.require` e também o adaptador de apps compilados.
O host usa ThemeProvider/DEFAULT_TOKENS como apresentação da janela e recebe
modo, destaque e idioma do broker: manter esses dados não cria temas de terceiros.
Não chamar `startShell` dentro do host.

O SDK exportado e a capacidade concedida são fronteiras diferentes: a presença
de um componente ou função no namespace não concede `lord.admin`, filesystem
da sessão, acesso a outros apps ou serviços privados de Mail/Passwords. A coluna
isolado do [inventário](SHELL-RUST-API-INVENTORY.md) registra essa diferença.

## Resultado do grafo atual

O build em memória usou exatamente as opções de `buildAppHost` de
[tools/build-theme.mjs](../tools/build-theme.mjs): esbuild 0.24.2, IIFE, es2020, minificação,
JSX automático, produção e `__LORD_STANDALONE__=true`, com os aliases atuais.
`write:false` impediu alterar o bundle instalado ou os arquivos gerados do repo.

- **114/114 arquivos do SDK** aparecem como inputs e têm bytes no output.
- **Nenhum arquivo de sdk/ é exclusivamente da shell no grafo atual.**
- Bundle do host: **575714 bytes**; bibliotecas React/reconciler permanecem
  necessárias no processo dos apps.
- A causa é o namespace inteiro publicado em `shared`, incluindo o ciclo
  `sdk/packages.js → sdk/index.js`. Um export não usado pelo próprio AppHost
  ainda pode ser usado pelo app carregado depois; tree shaking não pode apagá-lo.

Logo, não apagar `updates.js`, `startShell`, `LocaleRoot` ou os componentes
da shell de um SDK ainda compartilhado sem antes separar os entry points e
estabelecer a fachada de compatibilidade dos .lpk. Também não manter código de
sessão legado disfarçado em outra pasta para declarar a migração concluída.

## Lista completa dos arquivos do SDK

**Papel** é uma indicação para separar responsabilidades, não uma autorização
de exclusão. Todos os arquivos abaixo entram no host de hoje. Os candidatos da
sessão podem conter exports compartilhados e efeitos de importação: inventariá-los
antes de criar um entry point do host. Bytes são a contribuição do módulo
minificado segundo o metafile, não memória residente ou tamanho do módulo isolado.

| Arquivo | Entra no host | Bytes no bundle | Papel para a separação |
| --- | --- | ---: | --- |
| [sdk/admin.js](../sdk/admin.js) | sim | 752 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/alarms.js](../sdk/alarms.js) | sim | 3178 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/anim.js](../sdk/anim.js) | sim | 215 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/appcontrol.js](../sdk/appcontrol.js) | sim | 2587 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/apps.js](../sdk/apps.js) | sim | 4216 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/appswitcher.jsx](../sdk/appswitcher.jsx) | sim | 3062 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/archive.js](../sdk/archive.js) | sim | 1756 | Base/exports compartilhados dos apps |
| [sdk/backup.js](../sdk/backup.js) | sim | 1622 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/brightness.jsx](../sdk/brightness.jsx) | sim | 1949 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/browserframe.js](../sdk/browserframe.js) | sim | 790 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/calendar.js](../sdk/calendar.js) | sim | 2928 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/clipboard.js](../sdk/clipboard.js) | sim | 4006 | Base/exports compartilhados dos apps |
| [sdk/codesig.js](../sdk/codesig.js) | sim | 596 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/components.jsx](../sdk/components.jsx) | sim | 16629 | Base/exports compartilhados dos apps |
| [sdk/contacts.js](../sdk/contacts.js) | sim | 2109 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/controls.jsx](../sdk/controls.jsx) | sim | 8996 | Base/exports compartilhados dos apps |
| [sdk/cooling.jsx](../sdk/cooling.jsx) | sim | 15200 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/defaults.js](../sdk/defaults.js) | sim | 3610 | Base/exports compartilhados dos apps |
| [sdk/desktopmodel.js](../sdk/desktopmodel.js) | sim | 58 | Base/exports compartilhados dos apps |
| [sdk/desktops.js](../sdk/desktops.js) | sim | 4012 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/disks.js](../sdk/disks.js) | sim | 595 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/diskwatch.js](../sdk/diskwatch.js) | sim | 2547 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/displaysui.jsx](../sdk/displaysui.jsx) | sim | 1445 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/doc.js](../sdk/doc.js) | sim | 3046 | Base/exports compartilhados dos apps |
| [sdk/dock.js](../sdk/dock.js) | sim | 9917 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/dockmodel.js](../sdk/dockmodel.js) | sim | 2105 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/dockpreviews.jsx](../sdk/dockpreviews.jsx) | sim | 4002 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/emoji.jsx](../sdk/emoji.jsx) | sim | 6930 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/emojidata.js](../sdk/emojidata.js) | sim | 49 | Base/exports compartilhados dos apps |
| [sdk/errors.js](../sdk/errors.js) | sim | 939 | Base/exports compartilhados dos apps |
| [sdk/filedrag.js](../sdk/filedrag.js) | sim | 680 | Base/exports compartilhados dos apps |
| [sdk/filepicker.jsx](../sdk/filepicker.jsx) | sim | 152 | Base/exports compartilhados dos apps |
| [sdk/firewall.js](../sdk/firewall.js) | sim | 1743 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/fs.js](../sdk/fs.js) | sim | 1566 | Base/exports compartilhados dos apps |
| [sdk/glass.js](../sdk/glass.js) | sim | 240 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/hardware.jsx](../sdk/hardware.jsx) | sim | 11629 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/help.js](../sdk/help.js) | sim | 735 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/i18n.js](../sdk/i18n.js) | sim | 3981 | Base/exports compartilhados dos apps |
| [sdk/ime.js](../sdk/ime.js) | sim | 2494 | Base/exports compartilhados dos apps |
| [sdk/index.js](../sdk/index.js) | sim | 12500 | Base/exports compartilhados dos apps |
| [sdk/keyboard.js](../sdk/keyboard.js) | sim | 1121 | Base/exports compartilhados dos apps |
| [sdk/lan.js](../sdk/lan.js) | sim | 1567 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/layers.js](../sdk/layers.js) | sim | 1386 | Base/exports compartilhados dos apps |
| [sdk/logo.js](../sdk/logo.js) | sim | 1182 | Base/exports compartilhados dos apps |
| [sdk/logs.js](../sdk/logs.js) | sim | 1128 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/mail.js](../sdk/mail.js) | sim | 8262 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/markdown.jsx](../sdk/markdown.jsx) | sim | 7889 | Base/exports compartilhados dos apps |
| [sdk/media.js](../sdk/media.js) | sim | 6299 | Base/exports compartilhados dos apps |
| [sdk/menu.jsx](../sdk/menu.jsx) | sim | 4317 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/nativeappmodel.js](../sdk/nativeappmodel.js) | sim | 980 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativeapps.js](../sdk/nativeapps.js) | sim | 1210 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativebanner.js](../sdk/nativebanner.js) | sim | 2677 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativedockparts.jsx](../sdk/nativedockparts.jsx) | sim | 1419 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativefiles.js](../sdk/nativefiles.js) | sim | 1705 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativelauncher.js](../sdk/nativelauncher.js) | sim | 82 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativemenu.jsx](../sdk/nativemenu.jsx) | sim | 988 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativemenubar.js](../sdk/nativemenubar.js) | sim | 728 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativetitlebar.jsx](../sdk/nativetitlebar.jsx) | sim | 1026 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativetree.js](../sdk/nativetree.js) | sim | 82 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativeview.js](../sdk/nativeview.js) | sim | 7014 | Montagem e adaptadores de apps/visões Rust |
| [sdk/nativeviewfacts.js](../sdk/nativeviewfacts.js) | sim | 1819 | Montagem e adaptadores de apps/visões Rust |
| [sdk/netmodel.js](../sdk/netmodel.js) | sim | 8199 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/netstatus.jsx](../sdk/netstatus.jsx) | sim | 6983 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/network.jsx](../sdk/network.jsx) | sim | 1757 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/notify.js](../sdk/notify.js) | sim | 5072 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/osk.js](../sdk/osk.js) | sim | 2734 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/osk.jsx](../sdk/osk.jsx) | sim | 4280 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/oskmodel.js](../sdk/oskmodel.js) | sim | 1146 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/overview.jsx](../sdk/overview.jsx) | sim | 8090 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/packages.js](../sdk/packages.js) | sim | 3399 | Base/exports compartilhados dos apps |
| [sdk/pdf.js](../sdk/pdf.js) | sim | 1327 | Base/exports compartilhados dos apps |
| [sdk/platform.js](../sdk/platform.js) | sim | 10 | Base/exports compartilhados dos apps |
| [sdk/power.jsx](../sdk/power.jsx) | sim | 3798 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/powerbox.jsx](../sdk/powerbox.jsx) | sim | 1833 | Base/exports compartilhados dos apps |
| [sdk/print.js](../sdk/print.js) | sim | 1435 | Base/exports compartilhados dos apps |
| [sdk/printdialog.jsx](../sdk/printdialog.jsx) | sim | 8405 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/privacy.js](../sdk/privacy.js) | sim | 5693 | Base/exports compartilhados dos apps |
| [sdk/rustfiles.js](../sdk/rustfiles.js) | sim | 14887 | Montagem e adaptadores de apps/visões Rust |
| [sdk/rustlauncher.jsx](../sdk/rustlauncher.jsx) | sim | 3713 | Montagem e adaptadores de apps/visões Rust |
| [sdk/rustsearch.jsx](../sdk/rustsearch.jsx) | sim | 1887 | Montagem e adaptadores de apps/visões Rust |
| [sdk/rustwidgets.js](../sdk/rustwidgets.js) | sim | 1407 | Montagem e adaptadores de apps/visões Rust |
| [sdk/scrollbar.js](../sdk/scrollbar.js) | sim | 1128 | Base/exports compartilhados dos apps |
| [sdk/search.js](../sdk/search.js) | sim | 5948 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/searchglyph.jsx](../sdk/searchglyph.jsx) | sim | 436 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/sessionmodel.js](../sdk/sessionmodel.js) | sim | 1204 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/sessionrestore.jsx](../sdk/sessionrestore.jsx) | sim | 5654 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/settings.js](../sdk/settings.js) | sim | 1297 | Base/exports compartilhados dos apps |
| [sdk/shortcutprofile.js](../sdk/shortcutprofile.js) | sim | 965 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/shortcuts.js](../sdk/shortcuts.js) | sim | 5506 | Base/exports compartilhados dos apps |
| [sdk/smart.js](../sdk/smart.js) | sim | 1859 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/smartparse.js](../sdk/smartparse.js) | sim | 637 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/sound.js](../sdk/sound.js) | sim | 711 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/statusitems.js](../sdk/statusitems.js) | sim | 419 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/store.js](../sdk/store.js) | sim | 360 | Base/exports compartilhados dos apps |
| [sdk/suggestengine.js](../sdk/suggestengine.js) | sim | 4412 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/suggestions.js](../sdk/suggestions.js) | sim | 2630 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/sysopen.js](../sdk/sysopen.js) | sim | 1362 | Base/exports compartilhados dos apps |
| [sdk/system.js](../sdk/system.js) | sim | 1203 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/terminal.js](../sdk/terminal.js) | sim | 1153 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/textfocus.js](../sdk/textfocus.js) | sim | 595 | Base/exports compartilhados dos apps |
| [sdk/theme.js](../sdk/theme.js) | sim | 2645 | Base/exports compartilhados dos apps |
| [sdk/trackpad.js](../sdk/trackpad.js) | sim | 1504 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/tray.jsx](../sdk/tray.jsx) | sim | 1421 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/updates.js](../sdk/updates.js) | sim | 1743 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/vault.js](../sdk/vault.js) | sim | 6217 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/video.js](../sdk/video.js) | sim | 851 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/wallpapers.js](../sdk/wallpapers.js) | sim | 681 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/wayland.js](../sdk/wayland.js) | sim | 5039 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/widgets.js](../sdk/widgets.js) | sim | 456 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/windowmemory.js](../sdk/windowmemory.js) | sim | 927 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/windowmenu.js](../sdk/windowmenu.js) | sim | 2406 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/windows.js](../sdk/windows.js) | sim | 10718 | Base/exports compartilhados dos apps |
| [sdk/windowshell.jsx](../sdk/windowshell.jsx) | sim | 1714 | Candidato a separar a responsabilidade da sessão; compatibilidade atual compartilhada |
| [sdk/winmodel.js](../sdk/winmodel.js) | sim | 1568 | Base/exports compartilhados dos apps |

## Runtime trazido pelo host

- [runtime/events.js](../runtime/events.js)
- [runtime/index.js](../runtime/index.js)
- [runtime/native_file_picker.js](../runtime/native_file_picker.js)
- [runtime/native_async.js](../runtime/native_async.js)
- [runtime/native_doc_services.js](../runtime/native_doc_services.js)
- [runtime/native_font_services.js](../runtime/native_font_services.js)
- [runtime/native_scan_services.js](../runtime/native_scan_services.js)
- [runtime/native_fs_services.js](../runtime/native_fs_services.js)
- [runtime/native_media_services.js](../runtime/native_media_services.js)
- [runtime/native_backup_services.js](../runtime/native_backup_services.js)
- [runtime/native_window_services.js](../runtime/native_window_services.js)
- [runtime/native_settings_services.js](../runtime/native_settings_services.js)
- [runtime/native_system_services.js](../runtime/native_system_services.js)
- [runtime/native_clipboard_services.js](../runtime/native_clipboard_services.js)
- [runtime/native_input_services.js](../runtime/native_input_services.js)
- [runtime/native_session_widgets.js](../runtime/native_session_widgets.js)
- [runtime/native_pim_services.js](../runtime/native_pim_services.js)
- [runtime/nativeapps.js](../runtime/nativeapps.js)
- [runtime/app-host.js](../runtime/app-host.js)

`runtime/index.js` e `runtime/events.js` continuam no host dos apps após
saírem da sessão. Os adaptadores `native_*` atendem também os apps de sistema
isolados; preservá-los até haver um caminho equivalente para cada operação.
Não renomear os namespaces usados pelos pacotes existentes como parte da retirada
do React da shell.

## Trabalho posterior, sem alterar o contrato agora

1. Separar a inicialização da sessão dos exports dos apps, com inventário de
   efeitos de importação. Manter os nomes e assinaturas consumidos pelos .lpk.
2. Fazer bundle do host por um entry point dedicado e comparar o grafo/exports
   com esta captura, usando os exemplos e apps isolados como consumidores reais.
3. Declarar e documentar a versão suportada da API, inclusive retornos de
   indisponibilidade, timers, callbacks de revogação, janelas e atualização de
   idioma/aparência. Registrar como tratar pacotes anteriores antes de retirar exports.
4. Manter os testes de permissões, broker, várias janelas, powerbox, keyring e
   assinaturas; a retirada de uma ponte da sessão não retira sua variante do app.

Evidências locais: `build/validation/shell-phase0/app-host-metafile.json`,
`inventory.json` e `app-host.snapshot.js`. Nenhum SDK, manifesto ou pacote
foi modificado para produzir este levantamento.
