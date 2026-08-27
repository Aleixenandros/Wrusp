//! Registro consultable de la aplicación.
//!
//! Todo lo que Wrusp y el motor escriben por stdout/stderr acaba en
//! `wrusp.log` dentro de la carpeta de registros: los `eprintln` propios, los
//! avisos de GStreamer y la consola JavaScript de las vistas (ver
//! `permissions`). La carpeta por defecto sigue XDG
//! (`~/.local/state/wrusp/logs`) y puede cambiarse en ajustes; el cambio se
//! aplica al reiniciar, porque la redirección de los descriptores ocurre antes
//! de arrancar el motor y los procesos hijos del webview la heredan de ahí.
//!
//! **Nadie escribe al disco desde el hilo que dibuja la ventana.** Los
//! descriptores no apuntan al fichero sino a una tubería, y un hilo aparte la
//! vacía. Un vídeo que se rompe deja a GStreamer soltando miles de líneas por
//! segundo —medido: 2127 en un solo segundo—, y con la escritura en medio eso
//! bastaba para que la barra de título dejara de responder.

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
    // GStreamer callado por defecto. Con `1` seguía escupiendo miles de líneas
    // por segundo cuando un vídeo llega corrupto, y eso es E/S y trabajo que
    // no ayudan a nadie. Para diagnosticar, `GST_DEBUG=2 wrusp` desde consola.
    if std::env::var_os("GST_DEBUG").is_none() {
        std::env::set_var("GST_DEBUG", "0");
    }
    redirect(&path);
}

#[cfg(unix)]
fn redirect(path: &std::path::Path) {
    use std::os::unix::io::{FromRawFd, IntoRawFd};

    let Ok(fichero) = fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let escritos = fichero.metadata().map(|m| m.len()).unwrap_or(0);

    // Una tubería en vez del fichero: escribir en ella es copiar a un búfer del
    // núcleo, no tocar el disco. Quien escriba —el hilo de GTK, los hilos de
    // GStreamer o los procesos del webview, que heredan estos descriptores— no
    // espera a nada.
    let mut extremos = [0 as libc::c_int; 2];
    if unsafe { libc::pipe(extremos.as_mut_ptr()) } != 0 {
        // Sin tubería, al fichero directamente: mejor un registro que puede
        // frenar que ningún registro.
        let fd = fichero.into_raw_fd();
        unsafe {
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
            libc::close(fd);
        }
        cabecera();
        return;
    }
    let (lectura, escritura) = (extremos[0], extremos[1]);
    // Búfer holgado para absorber las ráfagas sin que nadie llegue a esperar.
    // Si el núcleo no lo concede, se queda con el suyo y no pasa nada. Es cosa
    // de Linux: en el resto de Unix no existe esta opción y vale el tamaño por
    // defecto, que ya absorbe bastante.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::fcntl(escritura, libc::F_SETPIPE_SZ, 1 << 20)
    };
    unsafe {
        libc::dup2(escritura, 1);
        libc::dup2(escritura, 2);
        libc::close(escritura);
    }

    let destino = path.to_path_buf();
    let entrada = unsafe { fs::File::from_raw_fd(lectura) };
    // Si este hilo muriera, la tubería se llenaría y la aplicación entera se
    // quedaría esperando a escribir: por eso aquí dentro no hay un solo
    // `unwrap` y ningún error interrumpe el bucle.
    let _ = std::thread::Builder::new()
        .name("wrusp-registro".into())
        .spawn(move || volcar(entrada, fichero, escritos, destino, MAX_BYTES));

    cabecera();
}

/// Vacía la tubería al fichero, rotando cuando se pasa del tope.
#[cfg(unix)]
fn volcar(
    mut entrada: fs::File,
    mut salida: fs::File,
    mut escritos: u64,
    destino: PathBuf,
    tope: u64,
) {
    use std::io::{Read, Write};

    let mut buzon = vec![0u8; 64 * 1024];
    loop {
        let leidos = match entrada.read(&mut buzon) {
            Ok(0) => return, // nadie escribe ya: la aplicación terminó
            Ok(n) => n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        };
        if salida.write_all(&buzon[..leidos]).is_ok() {
            escritos += leidos as u64;
        }
        if escritos <= tope {
            continue;
        }
        // Rotación en caliente: lo escrito pasa a «anterior» y se sigue en un
        // fichero limpio. Si algo falla, se sigue con el que había.
        let Some(dir) = destino.parent() else {
            continue;
        };
        if fs::rename(&destino, dir.join(PREVIOUS_FILE)).is_err() {
            escritos = 0;
            continue;
        }
        match fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&destino)
        {
            Ok(nuevo) => {
                salida = nuevo;
                escritos = 0;
            }
            Err(_) => return, // sin fichero al que escribir, se deja de vaciar
        }
    }
}

/// Marca de arranque, ya por el camino normal: con la redirección puesta esto
/// va a la tubería como todo lo demás.
#[cfg(unix)]
fn cabecera() {
    println!(
        "\n──── Wrusp {} · {} ────",
        env!("CARGO_PKG_VERSION"),
        fecha_utc()
    );
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
    use std::path::PathBuf;

    /// Prepara una tubería y un hilo de volcado como los de verdad, y devuelve
    /// por dónde escribir, la carpeta y el hilo.
    #[cfg(unix)]
    fn banco_de_volcado(
        tope: u64,
        nombre: &str,
    ) -> (std::fs::File, PathBuf, std::thread::JoinHandle<()>) {
        use std::os::unix::io::FromRawFd;

        let dir =
            std::env::temp_dir().join(format!("wrusp-registro-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let destino = dir.join(super::LOG_FILE);

        let mut extremos = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(extremos.as_mut_ptr()) }, 0);
        let entrada = unsafe { std::fs::File::from_raw_fd(extremos[0]) };
        let escritura = unsafe { std::fs::File::from_raw_fd(extremos[1]) };

        let salida = std::fs::File::create(&destino).unwrap();
        let ruta = destino.clone();
        let hilo = std::thread::spawn(move || super::volcar(entrada, salida, 0, ruta, tope));
        (escritura, dir, hilo)
    }

    /// Lo que separa a la interfaz del disco es este bucle: si se para, la
    /// tubería se llena y la aplicación entera se queda esperando a escribir.
    /// Se le da bastante más de lo que cabe en la tubería, que es lo que pasa
    /// cuando un vídeo roto pone a GStreamer a soltar miles de líneas por
    /// segundo.
    #[cfg(unix)]
    #[test]
    fn el_volcado_traga_mas_de_lo_que_cabe_en_la_tuberia_sin_perder_nada() {
        use std::io::Write;

        let (mut escritura, dir, hilo) = banco_de_volcado(64 * 1024 * 1024, "sin-perder");
        let trozo = vec![b'x'; 4096];
        let veces = 200; // 800 KiB, muy por encima de la tubería
        for _ in 0..veces {
            escritura.write_all(&trozo).unwrap();
        }
        escritura.write_all(b"ultima").unwrap();
        drop(escritura); // cerrar el extremo de escritura termina el bucle
        hilo.join().unwrap();

        let actual = std::fs::read_to_string(dir.join(super::LOG_FILE)).unwrap();
        assert_eq!(actual.len(), veces * 4096 + "ultima".len());
        assert!(actual.ends_with("ultima"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Y sin rotación, el registro crecería sin fin.
    #[cfg(unix)]
    #[test]
    fn el_volcado_rota_al_pasar_del_tope() {
        use std::io::Write;

        const TOPE: u64 = 4096;
        let (mut escritura, dir, hilo) = banco_de_volcado(TOPE, "rota");
        escritura.write_all(&vec![b'y'; 20_000]).unwrap();
        drop(escritura);
        hilo.join().unwrap();

        assert!(
            dir.join(super::PREVIOUS_FILE).exists(),
            "al pasar del tope, lo escrito pasa a «anterior»"
        );
        let actual = std::fs::metadata(dir.join(super::LOG_FILE)).unwrap().len();
        assert!(
            actual <= TOPE,
            "tras rotar se vuelve a empezar; mide {actual}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fecha_con_formato_iso() {
        let f = super::fecha_utc();
        assert_eq!(f.len(), "2026-08-17 12:00:00 UTC".len());
        assert!(f.ends_with(" UTC"));
        assert!(f.starts_with("20"));
    }
}
