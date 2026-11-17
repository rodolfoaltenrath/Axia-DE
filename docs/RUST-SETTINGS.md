# Settings em Rust

`apps/settings/` contém somente Rust: manifesto, navegação, ícone, controles,
23 páginas, estado, validação, eventos e testes. O registro compilado substitui
o manifesto JSON e a entrada JSX. A janela conserva 880 × 600 e mínimo 700 × 460.

`mod.rs` organiza as consultas e os efeitos; os módulos por assunto desenham a
cena nativa e decidem as ações. `ui.rs` conserva os controles agrupados, campos,
pop-ups, toggles, sliders e seletores do aplicativo anterior. As traduções e os
catálogos de temas existentes continuam compartilhados com o desktop.

O shell ainda está em migração. `runtime/native_system_services.js` encaminha
leituras, chamadas e eventos às APIs existentes; não implementa páginas de
Settings. `runtime/native_file_picker.js` monta o diálogo de arquivos Rust
compartilhado. As políticas de autorização, os serviços do sistema e o diálogo
de confirmação/reversão de monitores permanecem nas respectivas APIs do shell.

Cuidados cobertos por testes Rust:

- Senhas mascaradas na cena, operações silenciosas e descarte dos rascunhos ao
  trocar de conta ou sair da página; proteção do último administrador.
- Respostas de configuração de rede vinculadas à geração e interface consultada;
  falhas de leitura não habilitam Aplicar.
- Escritas compostas em um efeito, preservação de preferências enquanto chegam
  snapshots antigos e serialização dos resultados dos efeitos.
- Coalescência de alterações rápidas de volume, horários cruzando meia-noite,
  validação estrita das respostas de firmware e recarga de traduções.
- Renderização de todas as páginas, traduções existentes e handlers dos controles.

```sh
cargo test --manifest-path rust/Cargo.toml -p axia-shell --lib settings_app
cargo test --manifest-path rust/Cargo.toml -p axia-shell --lib
npm run build
make -C server GPU=1
cargo test --manifest-path rust/Cargo.toml -p axia-shell \
  --test native_apps_runtime -- --ignored --nocapture
```

O último teste executa o compositor com HOME temporário, abre Settings,
troca aparência e navega para Clipboard. Os testes não alteram contas, firmware,
impressoras ou conexões reais. Validação física desses dispositivos continua
necessária no ambiente em que o desktop será utilizado.
