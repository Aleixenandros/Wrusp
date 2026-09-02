//! Volcado a disco de los medios que fallan al reproducirse.
//!
//! El registro dice *que* un vídeo falla y, desde la 0.4.8, qué códecs
//! declara; pero para saber *por qué* GStreamer lo rechaza hace falta el
//! fichero. Con `WRUSP_GUARDAR_MEDIOS_FALLIDOS` en el entorno (vacío o `1`
//! para la carpeta temporal, o una ruta de carpeta), la página guarda los
//! blobs que fallan y Rust los escribe en disco, listos para
//! `gst-discoverer-1.0` o `ffprobe`. Apagado por defecto: son mensajes del
//! usuario.

use crate::runtime::AppHandle;
use std::path::PathBuf;

const VARIABLE: &str = "WRUSP_GUARDAR_MEDIOS_FALLIDOS";
const MAX_BASE64: usize = 64 * 1024 * 1024 / 3 * 4 + 8;

/// ¿Está activado el volcado?
pub fn activo() -> bool {
    std::env::var_os(VARIABLE).is_some()
}

/// Carpeta donde se guardan: la indicada en la variable si es una carpeta, y
/// si no, una bajo la temporal.
fn carpeta() -> PathBuf {
    let valor = std::env::var(VARIABLE).unwrap_or_default();
    let ruta = PathBuf::from(&valor);
    if !valor.is_empty() && valor != "1" && ruta.is_dir() {
        ruta
    } else {
        std::env::temp_dir().join("wrusp-medios-fallidos")
    }
}

/// Script de arranque: la página solo ofrece volcados si Rust los va a
/// recoger.
pub fn init_script() -> String {
    format!("window.__wruspGuardarFallos = {};", activo())
}

/// Pide a la vista `etiqueta` el medio fallido `id` y lo escribe en disco.
#[cfg(target_os = "linux")]
pub fn guardar_medio_fallido(app: &AppHandle, etiqueta: &str, id: &str) {
    use javascriptcore::ValueExt;
    use tauri::Manager;
    use webkit2gtk::WebViewExt;

    if !activo() {
        return;
    }
    // El id lo genera nuestro script (`f1`, `f2`…); nada más se acepta.
    if id.is_empty() || id.len() > 8 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return;
    }
    let Some(vista) = app.get_webview(etiqueta) else {
        return;
    };
    let id = id.to_string();
    let resultado = vista.with_webview(move |plataforma| {
        let nativa = plataforma.inner();
        let cuerpo = format!(
            "return await window.__wruspLeerFallido({});",
            serde_json::to_string(&id).unwrap_or_else(|_| "''".into())
        );
        nativa.call_async_javascript_function(
            &cuerpo,
            None,
            None,
            None,
            None::<&webkit2gtk::gio::Cancellable>,
            move |resultado| match resultado {
                Ok(valor) if valor.is_string() => {
                    let base64 = valor.to_str();
                    if base64.is_empty() || base64.len() > MAX_BASE64 {
                        eprintln!("wrusp: medio fallido {id}: la página no lo entregó (vacío o demasiado grande)");
                        return;
                    }
                    let Some(bytes) = crate::clipboard::desde_base64(&base64) else {
                        eprintln!("wrusp: medio fallido {id}: base64 ilegible");
                        return;
                    };
                    let dir = carpeta();
                    let _ = std::fs::create_dir_all(&dir);
                    let nombre = format!(
                        "{}-{id}.mp4",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0)
                    );
                    let ruta = dir.join(nombre);
                    match std::fs::write(&ruta, &bytes) {
                        Ok(()) => eprintln!(
                            "wrusp: medio fallido guardado en {} ({} KiB); analízalo con gst-discoverer-1.0 o ffprobe",
                            ruta.display(),
                            bytes.len() / 1024
                        ),
                        Err(err) => eprintln!("wrusp: no se pudo guardar el medio fallido en {} ({err})", ruta.display()),
                    }
                }
                Ok(_) => eprintln!("wrusp: medio fallido {id}: la página no devolvió bytes"),
                Err(err) => eprintln!("wrusp: medio fallido {id}: no se pudo leer ({err})"),
            },
        );
    });
    if let Err(err) = resultado {
        eprintln!("wrusp: no se pudo pedir el medio fallido ({err})");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn guardar_medio_fallido(_app: &AppHandle, _etiqueta: &str, _id: &str) {}
