# Origem e autoria do Axia DE

Axia DE é um projeto independente derivado do
[LordOS](https://github.com/lordbasex/LordOS), desenvolvido pela iPERFEX e pelos
colaboradores do LordOS. A base integrada neste fork é o LordOS 0.8.7-beta,
commit `ab8afcb835422723518308e8ba9e5fa1b736c106`.

O código original do projeto está sob a [licença MIT](LICENSE). Os avisos de
copyright e a licença do código herdado são preservados. As contribuições da
Axia são identificadas pelo histórico Git e pelos avisos de autoria aplicáveis.
O projeto não é uma versão oficial do LordOS. Marca e autoria são assuntos
distintos da licença de uso do código.

Os componentes de terceiros, fontes e dados conservam suas próprias licenças;
veja [THIRD_PARTY.md](THIRD_PARTY.md). As licenças dos helpers distribuídos também
dependem das bibliotecas com as quais são ligados; o helper de documentos usa
Poppler. Trocar a implementação do compositor não elimina automaticamente
essas dependências ou seus avisos.

## Evolução para implementações próprias

Uma implementação independente pode ter autoria da Axia. Para acompanhar a
substituição gradual dos componentes:

1. Identificar o módulo substituído, sua origem e licença.
2. Registrar a implementação nova e os testes de compatibilidade no commit.
3. Verificar se continuam existindo trechos derivados, arquivos gerados,
   recursos gráficos, documentação ou dependências herdadas.
4. Atualizar os avisos dos componentes efetivamente substituídos. Renomear ou
   refatorar código herdado não o transforma, por si só, em código independente.

O destaque dado à origem na apresentação do produto pode evoluir com o projeto.
Enquanto houver cópias ou partes substanciais do código MIT herdado, conservar
os avisos de copyright e licença correspondentes. O histórico que continua
distribuindo versões antigas conserva os avisos dessas versões. Este arquivo
registra a origem histórica; ele não substitui os textos de licença.

Referência: [texto da licença MIT](https://opensource.org/license/mit).
