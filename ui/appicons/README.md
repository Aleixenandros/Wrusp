# Catálogo de iconos

Los SVG de esta carpeta son la **fuente** de los iconos de Wrusp y también lo
que muestra el selector de la ventana de ajustes (se sirven tal cual como parte
del frontend).

- 96 iconos base y su variante naranja `<nombre>-orange.svg` (los verdes y
  azules del original sustituidos por `#F46623`, `#FF7300` y `#CE3C00`).
- `manifest.json` es la lista que lee el selector; lo genera el script, no se
  edita a mano.

Al añadir un icono conviene rasterizarlo y mirar que pinte algo: el catálogo
viene de una conversión externa y ya trajo catorce rotos —degradados volcados
como texto (`fill="{'type': 'linear', …}"`, que ningún navegador pinta) y logos
blancos a los que les faltaba el fondo— (ver ADR-029).

El icono por defecto de la aplicación es **`whatsapp-logo-2449-orange`**,
declarado en `DEFAULT_ICON` (`src-tauri/src/config.rs`).

## Regenerar los derivados

Tras añadir, quitar o modificar un SVG:

```bash
./scripts/generate-icons.sh
```

Eso reescribe `manifest.json`, los PNG de 256 px de
`src-tauri/icons/appicons/` (los que usan la bandeja y las ventanas) y los
iconos del instalador (`icon.png`, `icon.ico`, `icon.icns`, `32x32.png`,
`128x128.png`, `128x128@2x.png`) a partir del icono por defecto.
