//! Tipos concretos del runtime del navegador.
//!
//! Wrusp usa el runtime propio de Tauri (Wry), que en Linux es WebKitGTK. Los
//! códecs los pone el sistema a través de GStreamer, así que el vídeo y el
//! audio de WhatsApp se reproducen dentro de la aplicación.

pub type Runtime = tauri::Wry;
pub type AppHandle = tauri::AppHandle<Runtime>;
