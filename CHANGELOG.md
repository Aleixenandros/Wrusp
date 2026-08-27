# Registro de cambios

Todas las novedades destacables de Wrusp.

Los enlaces de descarga de cada versión están en [Releases](https://github.com/Aleixenandros/Wrusp/releases).

## [0.4.1] — 2026-08-27

### Corregido

- **Rendimiento en chats multimedia sin picos de CPU ni cuelgues:**
  - Precarga perezosa (`preload="none"`) en audios y vídeos inactivos: evita que WebKitGTK arranque decenas de pipelines concurrentes de GStreamer al abrir un chat. La carga solo se inicia al reproducir.
  - Eliminado el límite restrictivo de 2 MiB en la conversión de blobs a `data:` URL, evitando la corrupción de buffers y bucles de decodificación en `avdec_aac` y `h264parse`.
  - Eliminado el recálculo forzado de estilos (`layout thrashing`) en `corregirCapas` sobre burbujas de mensajes.
  - Reducido el nivel de registro por defecto de GStreamer a `GST_DEBUG=1` para evitar saturación de E/S de disco.
- **Visualización completa de emojis:**
  - Configurado el disfraz de plataforma para que WhatsApp Web entregue su conjunto completo de imágenes de emojis estilo Apple, solventando los huecos en blanco causados por la incompatibilidad de WebKitGTK con la fuente vectorial `Noto-COLRv1.ttf` en Linux.
- **Arrastrar y soltar ficheros locales:**
  - Captura de eventos de soltado a nivel de ventana principal GTK (`WindowEvent::DragDrop`) y enrutado directo a la vista de chat activa.
- **Responsividad de los botones de la ventana:**
  - Implementada caché de estado en `icon::apply` para evitar lecturas de disco y operaciones síncronas innecesarias en el bucle principal de GTK ante cambios de no leídos.

## [0.4.0] — 2026-08-27

### Añadido

- **Personalización y gestión de cuentas:**
  - Selector de color de acento por cuenta, visible en la barra lateral y en la lista de ajustes.
  - Silenciado individual de notificaciones por cuenta con indicador visual.
  - Reordenación en caliente de cuentas (botones subir/bajar en ajustes).
  - Nuevos atajos de teclado para rotación rápida entre cuentas: `Ctrl+Tab` / `Ctrl+Shift+Tab` y `Ctrl+PageDown` / `Ctrl+PageUp`.
- **Notificaciones interactivas y privacidad:**
  - Al hacer clic sobre una notificación de escritorio, Wrusp se enfoca y abre directamente la cuenta receptora del mensaje.
  - Modo privado de notificaciones opcional para ocultar el contenido de los mensajes en los avisos del sistema.
- **Diagnóstico del sistema y multimedia:**
  - Nuevo panel de diagnóstico en ajustes con verificación en vivo de códecs GStreamer (H.264 / AAC), motor WebKitGTK y uso de disco.
  - Botón para purgar la caché de plugins de GStreamer.
- **Integración con el sistema y distribución:**
  - Opción de inicio automático al encender el equipo (`~/.config/autostart/wrusp.desktop`) con soporte de argumento `--hidden`.
  - Selector de directorios mejorado con cascada `zenity` → `kdialog` → `qarma`.
  - Manifiesto Flatpak (`distribution/flatpak/org.wrusp.Wrusp.yaml`) y metadatos AppStream.

## [0.3.12] — 2026-08-24

### Corregido

- **Las notificaciones ya aparecen en GNOME.** Wrusp recibía el aviso de WhatsApp y GNOME confirmaba la llamada D-Bus, pero el emisor se desconectaba inmediatamente después. GNOME liga la fuente de notificaciones a esa conexión y la destruía antes de llegar a pintar el banner. Ahora todos los avisos pasan por una única conexión que permanece viva mientras Wrusp está abierto. También se declara explícitamente `Wrusp.desktop`, la categoría de mensajería y el sonido estándar, para que el escritorio aplique el icono y las preferencias correctas de la aplicación.

### Añadido

- **Banco manual de notificaciones** (`cargo run --example banco_notificaciones`). Emite dos avisos consecutivos para comprobar que el escritorio los muestra y que ambos comparten el mismo emisor D-Bus; es exactamente la condición que faltaba en GNOME.

## [0.3.11] — 2026-08-24

### Corregido

- **Abrir un chat con muchos vídeos e imágenes deja de bloquear la aplicación.** El parche que evita la corrupción de MP4 grandes convertía cada vídeo a base64 nada más aparecer, aunque no se reprodujese: varios adjuntos disparaban lecturas completas simultáneas y duplicaban su tamaño en memoria. Ahora cada MP4 se materializa solo al pulsar reproducir, su memoria se libera al salir del chat y los observadores agrupan el trabajo de las miniaturas en lugar de recorrer y medir todo el documento repetidamente.

## [0.3.10] — 2026-08-24

### Corregido

- **Los chats vuelven a abrirse: la 0.3.9 dejaba el panel de conversación en blanco.** Al ocultar el anuncio de «WhatsApp para Mac», el script subía por los elementos que lo envuelven buscando la tarjeta que hay que esconder, y el único freno era su tamaño. Pero ese anuncio vive **dentro del panel de conversación**, y un elemento al que aún no se le ha calculado la posición mide cero, así que pasaba por «pequeño»: el panel entero acababa oculto y ahí se quedaba el resto de la sesión, aunque después pulsaras un chat. Reproducido con una maqueta de la estructura de WhatsApp antes de tocar nada, y verificado después. Que solo pasara en una de las cuentas es cuestión de cuál de las dos llegó a enseñar el anuncio.

  De paso se corrige algo que la 0.3.9 hacía sin que se notara: **los mensajes de la propia conversación que mencionaran «WhatsApp para Mac» —o que llevaran un enlace a la tienda— también desaparecían**. Ahora nada del contenido de los chats se toca nunca, y el anuncio se busca por texto solo donde puede estar: la pantalla sin conversación abierta y las ventanas emergentes.

  Y por si acaso: lo que se oculta se vigila. Si deja de anunciar nada, vuelve a mostrarse. Una equivocación de puntería dura una pasada, no toda la sesión.

### Añadido

- **Banco de pruebas del script del anuncio** (`cargo run --example banco_promo`). Levanta dos maquetas con la estructura de WhatsApp Web y comprueba lo que debe desaparecer y lo que no: el panel, la lista de chats, la caja de escritura y los mensajes siguen vivos; el anuncio y la emergente, no. Con el script de la 0.3.9 canta cinco fallos.

## [0.3.9] — 2026-08-24

### Corregido

- **Copiar una imagen del chat copia la imagen, no su dirección.** Con el clic derecho sobre una foto y «Copiar imagen», lo que acababa en el portapapeles era `blob:https://web.whatsapp.com/…`; pegarlo en cualquier sitio daba ese texto. Medido con banco propio contra WebKitGTK 2.52: el motor **sí** sabe copiar imágenes (escribir `image/png` o `image/jpeg` desde la página deja la foto de verdad en el portapapeles del escritorio) y traer los bytes de la página a Wrusp cuesta 5 ms para 160 KB. Lo que falla es la acción del menú nativo cuando la imagen es un blob, que es como WhatsApp sirve todas las suyas. Wrusp sustituye esa entrada del menú por una propia que pide los bytes a la página y los deja en el portapapeles. Si es WhatsApp —y no el menú— quien copia la dirección, también se corrige.
- **Los botones de minimizar, maximizar y cerrar dejan de quedarse muertos.** Los dibuja el gestor de ventanas, así que si no responden es que la aplicación no contesta. Había dos esperas síncronas en el hilo que dibuja la interfaz: la notificación de escritorio (una llamada D-Bus al servidor de notificaciones) y la lectura del portapapeles al pegar (una espera a que conteste otra aplicación). Como las notificaciones llegan cuando llegan, el bloqueo no tenía patrón. Ahora ninguna de las dos bloquea la ventana.
- **Los iconos que salían en blanco en el selector.** De los 193 del catálogo, 14 no pintaban nada visible: seis traían el degradado sin convertir —el atributo era un objeto de JavaScript volcado como texto, que ningún navegador pinta— y a ocho les faltaba el fondo, así que quedaba el logo blanco sobre nada. Reparados y verificados uno a uno. El selector pinta además cada icono sobre un tablero de contraste medio: el catálogo tiene iconos blancos y negros, y sobre un fondo liso siempre había unos cuantos que parecían celdas vacías, en el tema claro o en el oscuro.
- **Menos insistencia con «WhatsApp para Mac».** El disfraz de Chrome se quedaba corto: faltaba `window.chrome` —la comprobación más extendida para separar Chrome de Safari—, la lista de complementos del visor de PDF y un par de propiedades de `navigator` que WebKitGTK no tiene. Viéndonos como Safari, WhatsApp deducía Mac. Además, el anuncio se busca ahora también por su texto y no solo por el enlace a la tienda, y las ventanas emergentes se cierran por su propio botón.

### Cambiado

- **Los ajustes se ordenan con un menú lateral.** Estaban en una sola columna, con casi todo plegado dentro de desplegables. Ahora hay seis secciones —Cuentas, Apariencia, Carpetas, Comportamiento, Atajos y Acerca de— y se abre la última que estuvieras mirando. El tema pasa de la cabecera a Apariencia, junto al icono de la aplicación.

## [0.3.8] — 2026-08-20

### Corregido

- **Los vídeos MP4 grandes se reproducen desde el primer fotograma, sin una pantalla gris durante media reproducción.** WebKitGTK 2.52 obliga a pasar los vídeos `blob:` por `playbin3` y les aplica un búfer circular fijo de 2 MiB. Cuando el MP4 es mayor y tiene su índice al final —la forma en que WhatsApp entrega muchos vídeos—, GStreamer 1.28 recibe primero datos de la mitad y después los del inicio: el registro mostraba rangos mezclados, H.264/AAC corrupto y timestamps que retrocedían de 13,03 s a 0,06 s. Wrusp materializa únicamente esos blobs MP4 problemáticos por una ruta que evita el búfer, preservando la posición, el estado de reproducción y la promesa de `play()`. Verificado con el vídeo real que fallaba y con un MP4 de control de 5,7 MiB: 402 fotogramas decodificados, cero descartados y ningún error.

## [0.3.7] — 2026-08-20

### Corregido

- **Los vídeos dejan de quedarse clavados después de unos fotogramas en Fedora 44.** WebKitGTK 2.52 y GStreamer 1.28 negocian en el sink GL texturas External OES que después no pueden volver a mapear; el registro real mostraba `Cannot map External OES textures`, `invalid video buffer received` y finalmente `Decode error`. Wrusp usa ahora el sink de memoria normal sin apagar el renderizador de toda la vista. Verificado con un H.264 de 1024×576 leído también desde canvas, como hace WhatsApp: el camino anterior produjo 345 buffers inválidos en 345 fotogramas; el nuevo entregó los 354 fotogramas, todos los callbacks y cero errores.
- **Las notificaciones llegan cuando Wrusp está abierto pero en segundo plano.** Se estaban descartando siempre que la ventana fuese visible y la cuenta estuviese seleccionada, incluso cubierta por otra aplicación o situada en otro escritorio. Ahora solo se omiten cuando esa cuenta está seleccionada y Wrusp tiene realmente el foco.

## [0.3.6] — 2026-08-19

### Corregido

- **Los vídeos vuelven a reproducirse: la culpa era de la decodificación por hardware.** Con `mesa-va-drivers-freeworld` instalado —que el propio README recomendaba— el decodificador VA-API pasa a tener más rango que el de software y se queda con todos los vídeos… y con los de WhatsApp falla. En el registro de un caso real, los tres reproductores de la sesión usaron `vah264dec` y los tres murieron en `vaEndPicture: operation failed` y «Failed to decode data», con avisos de flujo mal formado. Con un fichero suelto ese mismo decodificador va bien, así que lo que no digiere es la forma troceada en que WhatsApp entrega el vídeo. Wrusp baja ahora el rango de los decodificadores por hardware para que ganen los de software, y respeta tu elección si ya traes la variable puesta. El README recomendaba ese paquete sin avisar de esto: corregido.

### Añadido

- **Registro de la cadena completa de notificaciones.** Hasta ahora, cuando no llegaba una notificación no había forma de saber en qué eslabón se perdía. El registro anota el permiso que se concede al arrancar, cada notificación que entrega el motor (sin su contenido: el registro no es sitio para los mensajes de nadie), y qué se hace con ella — enviada al escritorio, descartada porque tienes esa cuenta a la vista, o descartada porque las desactivaste en ajustes.

## [0.3.5] — 2026-08-19

### Corregido

- **El escritorio deja de llenarse de controles de reproducción muertos.** WebKitGTK publica una sesión de medios en el escritorio por cada audio o vídeo que suena —de ahí los «wrusp» del panel de GNOME— pero no la retira al terminar: veinte vídeos dejaban veinte entradas, y ahí se quedaban hasta cerrar la aplicación. Comprobado en el bus de la sesión, con reproductores acumulados del proceso web de Wrusp. Se desactiva esa integración del motor, verificado con la misma prueba antes y después: sin el cambio aparece una entrada nueva por reproducción, con él ninguna, y el vídeo y el audio se siguen reproduciendo igual. A cambio, las teclas de medios del teclado ya no controlan lo que suena en WhatsApp.

## [0.3.4] — 2026-08-19

### Corregido

- **Vuelven las notificaciones de escritorio.** Desde WebKitGTK 2.40 el motor decide el permiso preguntándoselo a la aplicación por una vía nueva, y además siembra el valor de `Notification.permission` **solo al lanzar el proceso web**, antes de que Tauri deje configurar la vista. El resultado, medido: la página veía el permiso en «default» pese a que Wrusp lo concedía, WhatsApp no emitía un solo aviso, y no había forma de arreglarlo desde la página porque pedir el permiso sin un clic del usuario devuelve «denegado». Ahora Wrusp contesta esa consulta y se asegura de que el proceso web arranque con el permiso ya concedido; comprobado de extremo a extremo, con la notificación llegando al escritorio. No lo rompió una versión de Wrusp, sino la actualización del motor del sistema.
- **Pegar una captura se entrega ahora como un pegado de verdad.** La 0.3.3 recuperaba la imagen del portapapeles pero se la ofrecía a WhatsApp como si la hubieras soltado, y WhatsApp no la recogía. Ahora se le entrega como el evento de pegado que espera, con el fichero dentro, apuntando a la caja de escritura donde estabas.
- **Arrastrar y soltar: la entrega llega donde tiene que llegar.** Los eventos se dirigían a un punto que podía quedar fuera del panel de conversación —y un evento solo lo ven su destino y sus padres—, y se lanzaban los tres seguidos sin dar tiempo a que WhatsApp actualizara su estado. Ahora se prueban el punto del soltado, la caja con el foco y el centro de la conversación, con un respiro entre eventos, y se comprueba si el otro lado lo acepta.

### Añadido

- **El puente de ficheros deja rastro en el registro.** Cada entrega anota cuántos ficheros se empujaron y si WhatsApp los aceptó, con nombres y destino. Las dos averías anteriores no dejaban ni una línea: se diagnosticaban a ciegas.

## [0.3.3] — 2026-08-19

### Corregido

- **Pegar una imagen en un chat vuelve a adjuntarla.** WebKitGTK no le entrega a la página lo que se pega si no es texto: medido con banco de pruebas propio, teniendo un PNG en el portapapeles el evento `paste` llega con `tipos=[] ficheros=0 items=[]`, mientras que con texto llega `tipos=[text/plain]`. El motor sí tiene la imagen —la incrusta en la caja de escritura como `<img src="blob:…">`—, pero WhatsApp busca un fichero en ese evento y no encontraba nada. Ahora, cuando el motor no le pasa nada a la página, Wrusp lee el portapapeles desde Rust y le entrega la imagen a WhatsApp como si la hubieras soltado en el chat; la imagen deja además de colarse dentro de la caja de texto. También funciona con un fichero copiado en el gestor de ficheros.
- **Arrastrar y soltar vuelve a funcionar: WhatsApp cerró la puerta que usaba el puente.** Los ficheros soltados se le servían a la página en `wrusp://drop/…` y esta los pedía con `fetch`; la política de seguridad de contenido de WhatsApp Web no admite esquemas propios en `connect-src`, así que la petición se rechazaba y el puente se rendía sin decir nada (en el registro: «Refused to connect to wrusp://drop/lista»). El mismo bloqueo afecta desde hace tiempo a la barra lateral, que sobrevive porque tiene un camino alternativo. Ahora es Rust quien **empuja** los bytes a la vista en trozos, una vía que la política de la página no gobierna, verificado reproduciendo esa misma política en el banco de pruebas: dos ficheros soltados a la vez llegan con su tamaño y su suma de control exactos. De paso la página ya no puede pedirle nada a Wrusp: solo recibe lo que acabas de soltar o pegar.

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
