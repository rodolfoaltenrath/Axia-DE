# Photos nativo em Rust

`apps/photos/` contém somente arquivos `.rs`. Metadados, ícone, nós da interface,
biblioteca, seleção, atalhos, menus, gestos, edição, animação e processamento de
imagens são compilados em `axia-shell`. O registro está em `rust/native_apps.rs`.
Não existe entrypoint JSX nem manifest JSON do Photos.

A janela continua usando os serviços compartilhados do desktop para menus,
impressão e tela cheia. O SDK apenas transporta fatos e efeitos; não calcula a
interface, os gestos, miniaturas ou pixels do Photos. O estado serializado em Rust
fica nos argumentos da janela para sobreviver à remontagem ao entrar e sair de
tela cheia; fecha junto com a janela e não cria arquivos de configuração.

## Caminho da imagem

- PNG, JPEG, GIF, WebP e BMP são decodificados pelo `image` Rust já vendorizado.
  A orientação EXIF é aplicada antes da construção dos níveis de resolução.
- HEIF/HEIC/AVIF usam a ABI opcional de `libheif.so.1`; TIFF usa
  `libtiff.so.6`, com as oito orientações aplicadas em Rust. Estes codecs de
  terceiros são dependências nativas, não implementações reescritas em Rust.
  A ausência deles produz um erro traduzido e não impede abrir os outros formatos.
- O Photos não abre fontes em `axia-media`, não usa `MediaView` nem o renderer
  `mview.c`. Esses componentes continuam disponíveis aos demais apps de mídia.
- O compositor, a janela, a fonte de texto e a impressão são infraestrutura
  compartilhada. Esta migração não pretende converter suas dependências C.

## Trabalho e memória

Cada janela inicia dois workers somente quando seu conteúdo é montado; desenhar
um ícone não cria threads. O worker da biblioteca tem prioridade reduzida e
atende uma miniatura visível por vez. Tanto as fotos quanto as pastas da barra
lateral são virtualizadas. Uma miniatura de GIF lê apenas o primeiro quadro.
Não há varredura recursiva, decodificação ou codificação de imagem no fluxo de
entrada da interface.

O viewer mantém pirâmides ARGB pré-multiplicadas, escolhe o nível de redução e
pré-calcula coordenadas por eixo. A interpolação usa canais empacotados em
inteiros, e brilho/contraste usam tabelas de consulta. Só o viewport é rasterizado:
um zoom de 32× não aloca uma imagem 32× maior. Quadros opacos prontos são movidos
para a cena por `NativeApp::take_raster`; a interface não os copia nem converte.
A troca invalida os níveis derivados do quadro anterior e preserva referências
compartilhadas à imagem antiga.

Movimentos acumulados atualizam a posição desejada; o próximo pedido usa essa
posição. Navegação e prévias cancelam trabalhos obsoletos, com verificação a cada
oito linhas no rasterizador. Se um quadro filtrado ultrapassar 14 ms, os gestos
seguintes usam uma prévia por amostragem simples. Após 100 ms sem movimento, o
worker produz novamente o quadro filtrado. Isso mantém os buffers no tamanho do
viewport e evita acrescentar uma etapa de ampliação na thread da interface.
A recuperação final usa o timer de atualização; 100 ms não é garantia de prazo
para terminar o processamento em máquinas sobrecarregadas.

Limites por janela:

| Recurso | Limite |
| --- | --- |
| Arquivo de entrada | 128 MiB |
| Imagem decodificada | 40 milhões de pixels, lado até 32.768 |
| Cache do viewer | 8 fotos, até 256 MiB de pixels/pirâmides |
| GIF animado | 1.200 quadros, até 192 MiB de pirâmides |
| Miniaturas | 128 quadros de 192×152, cerca de 14,3 MiB RGBA |
| Varredura | 100.000 entradas, 20.000 fotos, profundidade 4 |
| Quadro do viewport | Até 8,5 milhões de pixels |

Esses limites não representam um teto de RSS do processo. Decodificadores,
edição/codificação, um quadro em produção e o quadro atualmente exibido precisam
de memória temporária adicional. O cache de miniaturas é em memória; esta versão
não reutiliza o cache persistente do helper antigo.

## Edição e gravação

Rotação, brilho e contraste são pré-visualizados no worker; o recorte fica em
coordenadas da imagem orientada e acompanha zoom e redimensionamento. A cópia
salva é JPEG para entradas JPEG e PNG para os demais formatos. Editar um GIF
exporta o primeiro quadro como PNG. O arquivo original é preservado.

O arquivo de origem é revalidado por identidade, tamanho e data ao abrir.
A cópia é codificada antes da publicação, escrita em um temporário exclusivo e
sincronizada. `linkat` publica um nome disponível sem substituir arquivos ou
links existentes. O diretório permanece aberto durante a operação, evitando
seguir um novo link de diretório durante a gravação. Ao terminar, o viewer abre
a nova cópia. “Atualizar” revalida também a foto atualmente aberta.

## Validação

Testes cobrem geometria/zoom ancorado, recorte e rotação, transparência e níveis
ímpares/estreitos, interpolação inteira comparada com referência, cancelamento,
transferência de pixels sem cópia, propriedade de referências e falha de alocação,
cópias exclusivas, mudança da origem, links em varreduras, GIF, TIFF nas oito
orientações, virtualização de 20.000 entradas, estado ao remontar e refinamento
de prévias. O teste do host monta a foto na cena e encaminha pinch e ponteiro.

A integração `native_apps_runtime` abre biblioteca e foto no compositor headless,
verifica conteúdo nativo, remontagem e edição, e grava uma cópia. Ela falha se o
Photos chamar o viewer de mídia legado ou enviar seus pixels pelo renderer JS.

```sh
cargo test --manifest-path rust/Cargo.toml -p axia-shell --lib photos -- --test-threads=1
cargo test --release --manifest-path rust/Cargo.toml -p axia-shell --lib \
  photos_render_benchmark -- --ignored --nocapture --test-threads=1
cargo test --manifest-path rust/Cargo.toml -p axia-shell --test native_apps_runtime \
  -- --ignored --test-threads=1
```

O último comando requer `axia-server` recompilado e esbuild local. Um teste manual
opcional abre uma imagem real indicada por `AXIA_PHOTOS_CODEC_FIXTURE`:
`cargo test ... photos_system_codec_fixture -- --ignored --nocapture`.

Na máquina de desenvolvimento, Xeon E5-2667 v4, uma foto sintética de 24 MP com
pirâmide de 122,1 MiB levou aproximadamente 9–10 ms por quadro filtrado a
1280×720, contra 25–29 ms na primeira implementação desta migração. As prévias
a 3840×2074 ficaram entre 11–15 ms. As médias usam 30 quadros filtrados e 10
prévias por escala (ajustar, 100% e 1600%). São medições
do rasterizador aquecido em release, sem decodificação, composição de tela ou
concorrência de outros aplicativos; não são uma promessa de FPS do desktop.
