# Registro de cambios

Todas las novedades destacables de Wrusp.

Los enlaces de descarga de cada versión están en [Releases](https://github.com/Aleixenandros/Wrusp/releases).

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

[0.1.1]: https://github.com/Aleixenandros/Wrusp/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Aleixenandros/Wrusp/releases/tag/v0.1.0
