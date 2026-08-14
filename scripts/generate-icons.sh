#!/usr/bin/env bash
# Regenera los PNG del catálogo de iconos y los iconos del bundle a partir de
# los SVG de `ui/appicons/`.
#
#   ui/appicons/*.svg            catálogo (fuente, y lo que muestra el selector)
#   src-tauri/icons/appicons/    PNG 256 px usados en bandeja y ventanas
#   src-tauri/icons/icon.*       iconos del instalador, desde DEFAULT_ICON
#
# Requiere ImageMagick 7 (`magick`). Ejecutar desde la raíz del repositorio.
set -euo pipefail

# Debe coincidir con `DEFAULT_ICON` en src-tauri/src/config.rs.
DEFAULT_ICON="whatsapp-logo-2449-orange"

SVG_DIR="ui/appicons"
PNG_DIR="src-tauri/icons/appicons"
ICON_DIR="src-tauri/icons"

[ -d "$SVG_DIR" ] || { echo "No se encuentra $SVG_DIR (¿estás en la raíz?)" >&2; exit 1; }
mkdir -p "$PNG_DIR"

# PNG32 con -depth 8 a propósito: ImageMagick genera PNG de 16 bits por canal
# por defecto y la bandeja de Tauri los rechaza («wrong data size»).
echo "Rasterizando catálogo a 256 px…"
for svg in "$SVG_DIR"/*.svg; do
  name=$(basename "$svg" .svg)
  magick -background none -density 96 "$svg" \
    -resize 256x256 -gravity center -extent 256x256 \
    -depth 8 "PNG32:$PNG_DIR/$name.png"
done

echo "Generando iconos del bundle desde $DEFAULT_ICON…"
magick -background none -density 192 "$SVG_DIR/$DEFAULT_ICON.svg" \
  -resize 512x512 -gravity center -extent 512x512 \
  -depth 8 "PNG32:$ICON_DIR/icon.png"
magick "$ICON_DIR/icon.png" -resize 256x256 -depth 8 "PNG32:$ICON_DIR/128x128@2x.png"
magick "$ICON_DIR/icon.png" -resize 128x128 -depth 8 "PNG32:$ICON_DIR/128x128.png"
magick "$ICON_DIR/icon.png" -resize 32x32   -depth 8 "PNG32:$ICON_DIR/32x32.png"
magick "$ICON_DIR/icon.png" -define icon:auto-resize=256,128,64,48,32,16 "$ICON_DIR/icon.ico"

# ImageMagick no sabe escribir ICNS, así que se monta el contenedor a mano
# con los tipos que admiten PNG embebido (ic07/ic08/ic09).
python3 - "$ICON_DIR" <<'PY'
import struct, sys
d = sys.argv[1]
parts = []
for typ, name in ((b"ic09", "icon.png"), (b"ic08", "128x128@2x.png"), (b"ic07", "128x128.png")):
    data = open(f"{d}/{name}", "rb").read()
    parts.append(typ + struct.pack(">I", 8 + len(data)) + data)
body = b"".join(parts)
open(f"{d}/icon.icns", "wb").write(b"icns" + struct.pack(">I", 8 + len(body)) + body)
PY

# El manifest lo lee el selector de iconos de la ventana de gestión.
echo "Escribiendo $SVG_DIR/manifest.json…"
python3 - "$SVG_DIR" <<'PY'
import json, os, sys
d = sys.argv[1]
names = sorted(f[:-4] for f in os.listdir(d) if f.endswith(".svg"))
json.dump(names, open(f"{d}/manifest.json", "w"), indent=0)
print(f"{len(names)} iconos")
PY

echo "Listo."
