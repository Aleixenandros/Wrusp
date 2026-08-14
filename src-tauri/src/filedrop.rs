//! Ficheros soltados sobre una vista de cuenta.
//!
//! WebKitGTK entrega el soltado con la ruta del fichero (`text/uri-list`) pero
//! **no** construye el `File` que espera la página: `dataTransfer.files` llega
//! vacío y WhatsApp no adjunta nada. Medido en el banco de pruebas:
//! `DROP ficheros=0 tipos=[text/uri-list|text/html]`.
//!
//! Aquí se hace ese puente. Tauri sí recibe el soltado con las rutas reales;
//! se guardan un momento y la vista las pide por `wrusp://drop/…` para armar
//! los ficheros y entregárselos a WhatsApp como si los hubiera soltado el
//! usuario encima.
//!
//! **La página nunca elige la ruta**: solo puede pedir lo que se acaba de
//! soltar de verdad, y durante unos segundos. Sin eso, un script de una página
//! remota tendría por dónde leer cualquier fichero del disco.

use crate::runtime::AppHandle;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::Manager;

/// Cuánto siguen disponibles los ficheros tras soltarlos. Lo justo para que la
/// vista los recoja; pasado eso, se olvidan.
const VALIDEZ: Duration = Duration::from_secs(30);

/// Lo último que se soltó sobre una vista.
#[derive(Default)]
pub struct PendingDrop(pub Mutex<Option<Soltado>>);

pub struct Soltado {
    momento: Instant,
    rutas: Vec<PathBuf>,
}

/// Guarda las rutas recién soltadas y devuelve cuántas se aceptaron.
pub fn registrar(app: &AppHandle, rutas: Vec<PathBuf>) -> usize {
    // Solo ficheros normales: un directorio soltado no se puede adjuntar.
    let rutas: Vec<PathBuf> = rutas.into_iter().filter(|r| r.is_file()).collect();
    let total = rutas.len();
    let pendiente = app.state::<PendingDrop>();
    let mut estado = pendiente.0.lock().unwrap();
    *estado = if total == 0 {
        None
    } else {
        Some(Soltado {
            momento: Instant::now(),
            rutas,
        })
    };
    total
}

/// Rutas todavía válidas. Devuelve vacío si caducaron o no hay nada.
fn vigentes(app: &AppHandle) -> Vec<PathBuf> {
    let pendiente = app.state::<PendingDrop>();
    let mut estado = pendiente.0.lock().unwrap();
    match estado.as_ref() {
        Some(s) if s.momento.elapsed() <= VALIDEZ => s.rutas.clone(),
        Some(_) => {
            *estado = None;
            Vec::new()
        }
        None => Vec::new(),
    }
}

/// Atiende `wrusp://drop/…`. Devuelve `None` si la URL no es de este módulo,
/// para que la siga tratando `shell` como una orden más.
pub fn responder(
    app: &AppHandle,
    uri: &tauri::http::Uri,
) -> Option<tauri::http::Response<Vec<u8>>> {
    let url: tauri::Url = uri.to_string().parse().ok()?;
    if url.host_str() != Some("drop") {
        return None;
    }

    let ruta = url.path().trim_start_matches('/').to_string();
    let rutas = vigentes(app);

    let (tipo, cuerpo) = if ruta == "lista" {
        let items: Vec<serde_json::Value> = rutas
            .iter()
            .enumerate()
            .map(|(i, r)| {
                serde_json::json!({
                    "i": i,
                    "n": r.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    "t": mime_por_extension(r),
                })
            })
            .collect();
        (
            "application/json".to_string(),
            serde_json::to_vec(&items).unwrap_or_default(),
        )
    } else {
        let indice = ruta.strip_prefix("fichero/")?;
        let indice: usize = indice.parse().ok()?;
        // El índice solo puede apuntar dentro de lo que se acaba de soltar.
        let ruta = rutas.get(indice)?;
        let datos = std::fs::read(ruta).ok()?;
        (mime_por_extension(ruta), datos)
    };

    tauri::http::Response::builder()
        .status(200)
        .header("Content-Type", tipo)
        // La página se sirve desde https://web.whatsapp.com.
        .header("Access-Control-Allow-Origin", "*")
        .body(cuerpo)
        .ok()
}

/// Tipo de contenido a ojo de la extensión. Solo para que la página vea algo
/// razonable: quien decide qué hacer con el fichero es WhatsApp.
fn mime_por_extension(ruta: &Path) -> String {
    let ext = ruta
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let tipo = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "heic" => "image/heic",
        "avif" => "image/avif",
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "tar" => "application/x-tar",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        // Sin extensión conocida, WhatsApp lo trata como documento genérico.
        _ => "application/octet-stream",
    };
    tipo.to_string()
}

/// Script inyectado en las vistas de cuenta: recoge los ficheros soltados y se
/// los entrega a la página como un soltado normal.
pub const SCRIPT: &str = r#"(function () {
  // Rust llama aquí tras un soltado real, con la posición en píxeles CSS.
  window.__wruspSoltar = async function (x, y) {
    let lista;
    try {
      lista = await (await fetch('wrusp://drop/lista')).json();
    } catch (e) {
      return;
    }
    if (!lista || !lista.length) return;

    const datos = new DataTransfer();
    for (const f of lista) {
      try {
        const respuesta = await fetch('wrusp://drop/fichero/' + f.i);
        const blob = await respuesta.blob();
        datos.items.add(new File([blob], f.n, { type: f.t }));
      } catch (e) { /* si uno falla, se entregan los demás */ }
    }
    if (!datos.files.length) return;

    // Se entrega donde se soltó: WhatsApp escucha el soltado en el panel del
    // chat, no en el documento.
    const destino = document.elementFromPoint(x, y) || document.body;
    for (const tipo of ['dragenter', 'dragover', 'drop']) {
      destino.dispatchEvent(new DragEvent(tipo, {
        bubbles: true, cancelable: true, composed: true, dataTransfer: datos
      }));
    }
  };
})();"#;
