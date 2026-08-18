# Wrusp

**English**: [README.en.md](README.en.md)

Cliente de escritorio **no oficial** de WhatsApp, escrito en **Rust** con [Tauri 2](https://tauri.app). Pensado para Linux, con builds también para Windows y macOS.

Wrusp envuelve WhatsApp Web en un webview nativo y añade lo que la web no ofrece en el escritorio:

- **Una sola ventana con barra lateral** — todas las cuentas conviven en la misma ventana; cambias entre ellas con un clic, sin recargar la sesión.
- **Multicuenta** — cada cuenta mantiene una sesión totalmente aislada (perfiles de webview independientes).
- **Vídeo dentro del chat** — los códecs los pone el sistema a través de GStreamer.
- **Notas de voz y cámara** — la vista recibe los permisos de captura que el motor trae desactivados.
- **Notificaciones del escritorio** — con remitente y mensaje, incluidas las que WhatsApp emite desde su service worker.
- **Arrastrar y soltar** — suelta un fichero sobre un chat para enviarlo.
- **Contador de no leídos** — insignia sobre el icono de la bandeja y de la barra de tareas, y por cuenta en la barra lateral.
- **Atajos de teclado** — `Ctrl`+`1`…`9` para cambiar de cuenta, `Ctrl`+`U` para añadir, `Ctrl`+`P` para ajustes, y zoom recordado por cuenta.
- **Bandeja del sistema** — cerrar la ventana la oculta; Wrusp sigue recibiendo mensajes desde el tray.
- **Tema claro / oscuro / sistema** — aplicado tanto a la app como al propio WhatsApp Web.
- **Instancia única** — relanzar el binario enfoca la ventana existente.
- **Registro consultable** — la aplicación, el motor y la consola de WhatsApp
  Web escriben en un log con carpeta configurable desde ajustes.
- **Sin Node** — el frontend de gestión es HTML/CSS/JS estático embebido.

> ⚠️ Wrusp no está afiliado, asociado ni respaldado por WhatsApp ni Meta.
> Usa WhatsApp Web internamente: las mismas condiciones de servicio que
> aplicarían en tu navegador aplican aquí.

## Instalación

Descarga el paquete de tu distribución desde [Releases](https://github.com/Aleixenandros/Wrusp/releases):

| Sistema | Paquete |
| --- | --- |
| Debian / Ubuntu / Mint | `.deb` |
| Fedora / openSUSE | `.rpm` |
| Arch / Manjaro | `.pkg.tar.zst` |
| Otras distros Linux | `.AppImage` |
| Windows | `.msi` / instalador `.exe` |
| macOS (Apple Silicon) | `.dmg` |

Los binarios de Windows y macOS no van firmados: SmartScreen avisará en
Windows, y en macOS hay que quitar la cuarentena tras instalar
(`xattr -dr com.apple.quarantine /Applications/Wrusp.app`).

Cada release incluye `SHA256SUMS.txt` con attestation de procedencia de GitHub:

```bash
gh attestation verify SHA256SUMS.txt --repo Aleixenandros/Wrusp
sha256sum -c SHA256SUMS.txt --ignore-missing
```

> **GNOME**: para ver el icono de bandeja necesitas la extensión
> [AppIndicator](https://extensions.gnome.org/extension/615/appindicator-support/).

### Vídeo y audio

El motor reproduce lo que sepa decodificar GStreamer en tu sistema. WhatsApp
manda vídeo en H.264 con audio AAC, así que si un vídeo no arranca instala los
plugins correspondientes:

```bash
# Fedora — necesita RPM Fusion (https://rpmfusion.org/Configuration):
# el openh264 que trae Fedora solo decodifica el perfil baseline, y buena
# parte de los vídeos de WhatsApp usan Main o High. El plugin que expone los
# decodificadores es gstreamer1-plugin-libav (no siempre viene instalado);
# libavcodec-freeworld, de RPM Fusion, le pone los códecs completos.
sudo dnf install gstreamer1-plugin-libav libavcodec-freeworld gstreamer1-plugins-good

# Debian / Ubuntu
sudo apt install gstreamer1.0-libav gstreamer1.0-plugins-good
```

En Fedora, `mesa-va-drivers-freeworld` (también de RPM Fusion) añade además
decodificación por hardware en GPUs AMD.

Si instalas códecs con Wrusp abierto, sal del todo y vuelve a entrar — con
«Salir» desde el icono de la bandeja: cerrar la ventana deja la aplicación
viva, relanzarla solo enfoca la que ya corre, y un proceso en marcha no relee
los códecs—. Si aun así el vídeo no arranca, borra la caché del registro de
GStreamer y repite — GStreamer solo reescanea si cambia el fichero del
plugin, no sus librerías:

```bash
rm ~/.cache/gstreamer-1.0/registry.*.bin
```

Wrusp no distribuye códecs: los pone la distribución.

## Compilar desde fuente

Dependencias de sistema (nombres de Fedora / Ubuntu):

```bash
# Fedora
sudo dnf install gcc-c++ webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3 librsvg2-devel

# Ubuntu / Debian
sudo apt install build-essential libwebkit2gtk-4.1-dev libgtk-3-dev libappindicator3-dev librsvg2-dev
```

Compilación:

```bash
cd src-tauri
cargo build --release
./target/release/wrusp
```

## Uso

1. Abre Wrusp y añade una cuenta con un nombre (p. ej. «Personal»).
2. Se carga WhatsApp Web: escanea el QR desde el móvil
   (WhatsApp → Ajustes → Dispositivos vinculados).
3. Repite para más cuentas con el botón «+» de la barra lateral: cada una
   mantiene su sesión propia y cambias entre ellas con un clic.
4. Cierra la ventana con libertad: Wrusp queda en la bandeja del sistema.

Todos los datos se guardan en `~/.local/share/wrusp/`: `config.json` contiene
la configuración y `profiles/` las sesiones. Borrar una cuenta desde la app
elimina su perfil y cierra su sesión.

## Arquitectura (resumen)

- `src-tauri/` — backend Rust: shell de ventana única, cuentas, tray, tema y
  persistencia.
- `ui/` — página de ajustes (HTML/CSS/JS estático, sin frameworks).
- Una única ventana con varios webviews apilados (uno por cuenta más el de
  ajustes); solo uno visible a la vez. La barra lateral se inyecta en la vista
  visible para conservar el mismo estado y comportamiento en cada perfil.
- Cada cuenta usa un `data_directory` propio → contexto de webview aislado y
  persistente → sesiones de WhatsApp independientes.
- Las vistas de WhatsApp **no** tienen IPC de Tauri: hablan con Rust por un
  esquema propio que solo acepta un puñado de órdenes conocidas.
- Wrusp **no** habla con ninguna API de WhatsApp: todo ocurre dentro de
  WhatsApp Web, como en un navegador.

## Contribuir

Los PRs pasan por CI (fmt, check, test, clippy bloqueante y cargo-deny).
Antes de abrir uno:

```bash
cd src-tauri
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

## Licencia

[Apache-2.0](LICENSE)

## Limitaciones conocidas

- **Llamadas**: en Fedora, WebKitGTK viene compilado sin WebRTC, así que
  `RTCPeerConnection` no existe y WhatsApp indica que el navegador no admite
  llamadas. No es algo que Wrusp pueda habilitar: depende de la distribución.
- **Vídeo**: depende de los plugins de GStreamer instalados en el sistema (ver
  arriba).
- En GNOME hace falta la extensión
  [AppIndicator](https://extensions.gnome.org/extension/615/appindicator-support/)
  para ver el icono de la bandeja.
