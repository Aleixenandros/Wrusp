# Registro de cambios

Todas las novedades destacables de Wrusp.

Los enlaces de descarga de cada versión están en [Releases](https://github.com/Aleixenandros/Wrusp/releases).

## [0.3.2] — 2026-08-18

### Corregido

- **Descargar un fichero de un chat vuelve a funcionar, y ahora pregunta dónde guardarlo.** WhatsApp entrega las descargas navegando a un `blob:` con atributo `download`, y el filtro de navegación de las vistas solo admitía `https`: la descarga se cancelaba antes de llegar al gestor y no pasaba absolutamente nada, fuera un PDF, una imagen o un vídeo (confirmado con arnés local: `blob:` y `data:` morían en el filtro; solo una descarga HTTP directa llegaba a guardarse). Ahora los `blob:` creados por la propia página se aceptan —los de cualquier otro origen se siguen rechazando— y WebKit los convierte en la descarga que son. Además, en vez de guardar en silencio, cada descarga abre el diálogo nativo de «guardar como», partiendo de la carpeta configurada en ajustes y del nombre que sugiere la página; cancelar el diálogo cancela la descarga, y todo queda anotado en el registro. En Windows y macOS se conserva el guardado directo en la carpeta configurada.
- **Documentación de códecs en Fedora afinada tras un caso real.** El README omitía `gstreamer1-plugin-libav` en la línea de Fedora (es el plugin que expone `avdec_h264`; sin él, `libavcodec-freeworld` no aporta nada) y, sobre todo, no avisaba de que «volver a abrir la aplicación» exige salir de verdad: cerrar la ventana deja Wrusp vivo en la bandeja, relanzar el binario solo enfoca la instancia en marcha, y un proceso ya arrancado no relee el registro de GStreamer. Caso diagnosticado: códecs instalados con la aplicación abierta — el registro tardó día y medio en reescanearse solo y el proceso seguía siendo el anterior, así que solo se reproducía el perfil baseline (un vídeo de iPhone sí; los Main/High, clavados en el primer fotograma) pese a estar ya todo instalado. Verificado que con el registro al día `avdec_h264` decodifica un High 1080p60 real sin errores.

## [0.3.1] — 2026-08-17

### Corregido

- **El visor de fotos ya no queda cortado por la barra lateral.** La barra abre su hueco con padding en `body`, pero las capas fijas a pantalla completa —el visor de medios, entre otras— se anclan al viewport y lo ignoraban: sus primeros 60 píxeles quedaban debajo de la barra. Como las clases de WhatsApp cambian en cada despliegue, la corrección no depende de ningún selector: cualquier capa fija que cubra casi todo el viewport pegada al borde izquierdo recibe el mismo hueco automáticamente. Los menús y globos pequeños no se tocan.

## [0.3.0] — 2026-08-17

### Corregido

- **Los vídeos vuelven a reproducirse: se oculta la parte de vídeo de WebCodecs.** WebKitGTK anuncia `VideoDecoder` y asegura soportar H.264, pero su decodificación real no emite un solo fotograma (240 unidades válidas → 0 fotogramas y «Decode error», medido con arnés propio). WhatsApp Web, al verse en Chrome con WebCodecs disponible, elige su reproductor moderno sobre esa API: el play no hacía nada y el fotograma no se movía aunque el tiempo corriera. Esto explica también por qué «al principio funcionaba sin tocar nada»: lo que cambió no fue Wrusp, fue WhatsApp desplegando ese reproductor. Sin la API a la vista, la página cae al reproductor clásico `<video>`/MSE, que funciona. El audio (`AudioDecoder`) se deja intacto: las notas de voz no presentaban síntomas.

### Añadido

- **Registro consultable desde ajustes.** Todo lo que la aplicación y el motor escriben —incluida la consola JavaScript de WhatsApp Web y los avisos de GStreamer— queda en `wrusp.log`, en una carpeta configurable (por defecto `~/.local/state/wrusp/logs`) con botón para abrirla desde preferencias. Rotación sencilla a los 5 MB. Pensado para que la próxima vez que un vídeo no arranque, el porqué esté escrito en un sitio que se pueda mirar.

## [0.2.2] — 2026-08-17

### Corregido

- **Revertida la desactivación del renderizador DMA-BUF de la 0.2.1**: resultó una regresión. Con esa variable, `requestVideoFrameCallback` no dispara nunca, y WhatsApp usa justo esa API para revelar el vídeo cuando llega el primer fotograma: el reproductor se quedaba clavado en el póster —siempre el mismo fotograma aunque el tiempo avanzara— y el botón de play parecía no hacer nada. Con el renderizador por defecto la reproducción, el MSE y la lectura de fotogramas funcionan; los vídeos que no arrancan son cuestión de códecs del sistema (ver README).

## [0.2.1] — 2026-08-17

### Corregido

- **Los vídeos vuelven a reproducirse y las miniaturas dejan de salir con nieve.** El renderizador DMA-BUF de WebKitGTK no deja leer los fotogramas de vídeo desde canvas ni WebGL —el primero devuelve negro y el segundo, ruido de textura sin inicializar—, y esa lectura es justo la vía por la que WhatsApp genera miniaturas y previsualizaciones: según el vídeo, salía nieve o un «no se puede reproducir el video». Wrusp lo desactiva al arrancar (solo en Linux, y respetando el valor si ya lo has fijado tú fuera).
- **El desplazamiento dentro de los chats va más fluido.** Mismo origen y mismo remedio: sin el renderizador DMA-BUF, una página tipo chat con miles de mensajes pasa de 75 a 97 FPS.
- El README recomendaba en Fedora el plugin openh264, que solo decodifica el perfil baseline de H.264: bastantes vídeos de WhatsApp usan Main o High y no arrancaban o se veían a medias. Las instrucciones apuntan ahora a `libavcodec-freeworld` (RPM Fusion), con el aviso de limpiar la caché del registro de GStreamer si los códecs llegan después que la aplicación.

## [0.2.0] — 2026-08-14

### Añadido

- **Arrastrar y soltar ficheros sobre un chat.** El motor entrega la ruta de lo que sueltas, pero no construye el fichero que la página necesita, así que soltar algo sobre un chat no hacía absolutamente nada. Ahora Wrusp recoge lo soltado y se lo entrega a WhatsApp en el punto exacto donde lo has soltado, con su nombre y su tipo. Solo se puede acceder a lo que acabas de soltar y durante unos segundos, así que la página no tiene forma de leer nada más del disco.

## [0.1.1] — 2026-08-14

### Corregido

- **El motor deja de cerrarle el portapapeles a la página.** WebKitGTK bloquea por defecto el acceso de JavaScript al portapapeles, así que WhatsApp no podía leer lo que se pega ni escribir al copiar.
- **Los recursos internos de WhatsApp dejan de abrirse en el navegador.** La vista solo admitía `web.whatsapp.com`, así que lo que WhatsApp carga dentro desde otros dominios suyos —el visor de PDF en `webtp.whatsapp.net` y el mantenimiento de caché en `flows.whatsapp.net`— se cancelaba y acababa como pestañas sueltas en el navegador cada poco rato. Ahora se cargan donde deben, y con ellos vuelven a verse los PDF dentro de la aplicación. Los enlaces de un chat y la web pública de WhatsApp se siguen abriendo fuera.

## [0.1.0] — 2026-08-14

### Añadido

- **Cliente de escritorio no oficial de WhatsApp para Linux**, escrito en Rust con Tauri 2. No reimplementa nada del protocolo: envuelve WhatsApp Web en un webview nativo, con las mismas condiciones de servicio que tendría en un navegador.
- **Multicuenta con sesiones aisladas**: cada cuenta tiene su propio perfil de webview, así que las sesiones no se mezclan y borrar una cuenta cierra la suya.
- **Una sola ventana con barra lateral**: todas las cuentas conviven en la misma ventana y cambiar entre ellas es un clic, sin recargar la sesión.
- **El vídeo se reproduce dentro del chat**, con los códecs que ya trae el sistema por GStreamer.
- **Notas de voz y cámara**, concediendo a la vista los permisos de captura que WebKitGTK trae desactivados.
- **Notificaciones de escritorio** con remitente y mensaje, recogidas de la señal nativa del motor —que cubre también las del service worker, la vía real de WhatsApp— y respetando la cuenta que tienes delante.
- **Contador de no leídos** en la barra lateral, en el icono de la aplicación y en la bandeja.
- **Icono en la bandeja del sistema**: cerrar la ventana la oculta y Wrusp sigue recibiendo mensajes.
- **Atajos de teclado**: `Ctrl`+`1`…`9` para cambiar de cuenta, `Ctrl`+`U` para añadir una, `Ctrl`+`P` para los ajustes y zoom por cuenta que se recuerda.
- **Tema claro, oscuro o el del sistema**, aplicado también a WhatsApp Web.
- **Arrastrar y soltar** un fichero sobre un chat para enviarlo, y descargas a la carpeta que elijas.
- **Instancia única**: relanzar el binario enfoca la ventana que ya está abierta.
- Paquetes para Linux (deb, rpm, AppImage y Arch), Windows (msi e instalador) y macOS (dmg), con `SHA256SUMS.txt` y attestation de procedencia.

### Notas

- **Las llamadas no funcionan en Fedora**, y no es cosa de Wrusp: su WebKitGTK está compilado sin WebRTC, así que `RTCPeerConnection` no existe y WhatsApp responde que el navegador no admite llamadas. Depende de que la distribución lo habilite.
- El vídeo necesita que el sistema traiga los códecs de GStreamer correspondientes. Wrusp no distribuye ninguno.
- En GNOME hace falta la extensión AppIndicator para ver el icono de la bandeja; es comportamiento del escritorio, no de la aplicación.
- Los binarios de Windows y macOS no van firmados.

[0.3.1]: https://github.com/Aleixenandros/Wrusp/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/Aleixenandros/Wrusp/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/Aleixenandros/Wrusp/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/Aleixenandros/Wrusp/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/Aleixenandros/Wrusp/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/Aleixenandros/Wrusp/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Aleixenandros/Wrusp/releases/tag/v0.1.0
