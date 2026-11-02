Esta página explica qué versiones de LordOS reciben arreglos, cómo te llegan las actualizaciones y qué planeamos para después de la versión 1.0.

## Estado actual: beta

LordOS **0.8.0 beta** es la primera versión pública (la 0.7.0 nunca se publicó). Lo que trae cada versión está en sus notas en [GitHub Releases](https://github.com/lordbasex/LordOS/releases). Las versiones beta pueden cambiar comportamientos, ajustes y formatos de archivo entre una versión y otra, y no están pensadas para trabajo crítico. Mirá los [Términos de uso](/legal/terms/).

## Qué versiones reciben arreglos

Hasta la 1.0, **solo la última versión beta** recibe arreglos, incluidos los de seguridad. Cuando sale una beta nueva, la anterior deja de recibirlos, así que actualizá a la versión más reciente. No llevamos arreglos a betas anteriores.

## Cómo llegan las actualizaciones

LordOS tiene dos tipos de actualizaciones:

- **La base Debian.** El kernel, los drivers, las bibliotecas y todos los paquetes de Debian reciben sus actualizaciones de Debian, incluidas las de seguridad desde `security.debian.org`. Instalalas desde la pestaña Actualizaciones de la Tienda (o Ajustes → Actualizaciones), o con `apt` en la Terminal. Siguen llegando mientras Debian dé soporte a Debian 13.
- **LordOS en sí.** Las versiones nuevas de LordOS se publican en [GitHub Releases](https://github.com/lordbasex/LordOS/releases) como paquete `.deb`, ISO para PC e imagen para Raspberry Pi 5, cada una firmada (mirá la página de [Firma de código](/legal/code-signing/)). En Debian, volver a ejecutar el instalador de una línea (`curl -fsSL https://lordos.org/install.sh | sh`) actualiza LordOS; también podés instalar un `.deb` descargado desde la Tienda. Todavía no hay un actualizador propio de LordOS.

LordOS nunca busca ni descarga actualizaciones por su cuenta: vos decidís cuándo.

## Después de la 1.0 (planeado)

Este es nuestro plan, no una promesa, y puede cambiar antes de la 1.0:

- cada versión estable va a recibir arreglos, incluidos los de seguridad, hasta la siguiente versión estable, más un período de superposición para que tengas tiempo de actualizar;
- la base Debian va a seguir el ciclo de vida de Debian: cuando Debian termine el soporte de seguridad de una versión, LordOS va a pasar a la siguiente versión de Debian antes de esa fecha;
- los arreglos de seguridad críticos se van a publicar apenas estén listos, fuera del calendario normal.

## Avisos de fin de soporte

Antes de que una versión deje de recibir arreglos, lo vamos a anunciar en lordos.org y en GitHub, con la fecha y la versión a la que conviene actualizar.

## Contacto

Consultas: [nuestro formulario de contacto](https://www.iperfex.com/contacto/?lang=es). Problemas de seguridad: mirá la [Política de seguridad](/legal/security/).
