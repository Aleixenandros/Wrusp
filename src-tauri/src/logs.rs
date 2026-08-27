//! Registro consultable de la aplicación.
//!
//! Todo lo que Wrusp y el motor escriben por stdout/stderr acaba en
//! `wrusp.log` dentro de la carpeta de registros: los `eprintln` propios, los
//! avisos de GStreamer (se activa `GST_DEBUG=2` si no viene ya del entorno) y
//! la consola JavaScript de las vistas (ver `permissions`). La carpeta por
//! defecto sigue XDG (`~/.local/state/wrusp/logs`) y puede cambiarse en
//! ajustes; el cambio se aplica al reiniciar, porque la redirección de los
//! descriptores ocurre antes de arrancar el motor y los procesos hijos del
//! webview la heredan de ahí.

use std::fs;
use std::path::PathBuf;

const LOG_FILE: &str = "wrusp.log";
const PREVIOUS_FILE: &str = "wrusp.anterior.log";
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Carpeta de registros por defecto (XDG state; si no existe, la de datos).
pub fn default_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(crate::config::APP_IDENTIFIER)
        .join("logs")
}

/// Carpeta efectiva a partir del valor configurado (vacío = la por defecto).
pub fn effective_dir(configured: &str) -> PathBuf {
    if configured.is_empty() {
        default_dir()
    } else {
        PathBuf::from(configured)
    }
}

/// Redirige stdout y stderr al fichero de registro. Se llama lo primero de
/// todo: lo que arranque después (webviews incluidos) hereda los descriptores.
pub fn init() {
    let configured = crate::config::load_from_disk()
        .map(|cfg| cfg.log_dir)
        .unwrap_or_default();
    let dir = effective_dir(&configured);
    if fs::create_dir_all(&dir).is_err() {
        return; // sin carpeta no hay registro; la app funciona igual
    }
    let path = dir.join(LOG_FILE);
    // Rotación sencilla: al superar el tope, lo escrito pasa a «anterior» y se
    // empieza limpio. Dos ficheros como mucho.
    if fs::metadata(&path)
        .map(|m| m.len() > MAX_BYTES)
        .unwrap_or(false)
    {
        let _ = fs::rename(&path, dir.join(PREVIOUS_FILE));
    }
    // Errores de GStreamer: solo errores críticos (1) por defecto para no saturar E/S.
    if std::env::var_os("GST_DEBUG").is_none() {
        std::env::set_var("GST_DEBUG", "1");
    }
    redirect(&path);
}

#[cfg(unix)]
fn redirect(path: &std::path::Path) {
    use std::io::Write;
    use std::os::unix::io::IntoRawFd;

    let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(
        file,
        "\n──── Wrusp {} · {} ────",
        env!("CARGO_PKG_VERSION"),
        fecha_utc()
    );
    let fd = file.into_raw_fd();
    // A partir de aquí, 1 y 2 son el fichero; el descriptor original ya no
    // hace falta.
    unsafe {
        libc::dup2(fd, 1);
        libc::dup2(fd, 2);
        libc::close(fd);
    }
}

#[cfg(not(unix))]
fn redirect(_path: &std::path::Path) {
    // En Windows la redirección de descriptores es otra historia; el registro
    // existe para diagnosticar los problemas de WebKitGTK en Linux.
}

/// Fecha y hora UTC sin dependencias: días civiles desde la época
/// (algoritmo de Howard Hinnant).
fn fecha_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (dias, resto) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let (h, m, s) = (resto / 3600, resto % 3600 / 60, resto % 60);
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let anno = yoe + era * 400 + i64::from(mes <= 2);
    format!("{anno:04}-{mes:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

#[cfg(test)]
mod tests {
    #[test]
    fn fecha_con_formato_iso() {
        let f = super::fecha_utc();
        assert_eq!(f.len(), "2026-08-17 12:00:00 UTC".len());
        assert!(f.ends_with(" UTC"));
        assert!(f.starts_with("20"));
    }
}
