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
    crate::config::fijar_para_el_motor("GST_DEBUG", "0");
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
    let mut marcado = Vec::with_capacity(96 * 1024);
    let mut lineas = Lineas::new();
    loop {
        let leidos = match entrada.read(&mut buzon) {
            Ok(n) => n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => 0,
        };
        let hora = format!("{} ", hora_local());
        if leidos == 0 {
            // Nadie escribe ya: la aplicación terminó. Lo pendiente, al fichero.
            lineas.fin(hora.as_bytes(), &mut marcado);
            let _ = salida.write_all(&marcado);
            return;
        }
        lineas.trozo(&buzon[..leidos], hora.as_bytes(), &mut marcado);
        if salida.write_all(&marcado).is_ok() {
            escritos += marcado.len() as u64;
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

/// Ruido que no se escribe: WhatsApp pone `calc()` en atributos de SVG
/// (`r="calc(50% - 0px)"`), WebKit lo rechaza y la consola lo cuenta una vez
/// por círculo. Eran el 69 % del registro (LOG-02), y como pasan por nuestro
/// envoltorio de `setAttribute` salen atribuidos a un `user-script`.
#[cfg(unix)]
const RUIDO: &[u8] = b"CONSOLE RENDERING ERROR Error: Invalid value for <";
/// Cada cuánto, como mucho, se deja constancia de lo omitido.
#[cfg(unix)]
const RESUMEN_CADA: std::time::Duration = std::time::Duration::from_secs(600);
/// Una línea que no termina nunca se escribe tal cual al pasar de esto.
#[cfg(unix)]
const MAX_LINEA: usize = 64 * 1024;

/// Arma líneas completas con lo que sale de la tubería, les pone la hora y
/// quita el ruido.
///
/// Sin hora, un cuelgue solo se podía situar leyendo el registro desde fuera
/// mientras pasaba (ADR-048). Se pone aquí, al vaciar la tubería, y no en cada
/// `eprintln!`, porque así la llevan también las líneas del motor, de GStreamer
/// y de la consola de las páginas. Una línea puede llegar partida en dos
/// lecturas, así que el trozo sin terminar se guarda hasta la siguiente.
#[cfg(unix)]
struct Lineas {
    actual: Vec<u8>,
    /// Lo último escrito fue un trozo de una línea larga sin terminar: lo
    /// que sigue es su continuación y no lleva hora.
    continua: bool,
    omitidas: u64,
    ultimo_resumen: Option<std::time::Instant>,
}

#[cfg(unix)]
impl Lineas {
    fn new() -> Self {
        Self {
            actual: Vec::new(),
            continua: false,
            omitidas: 0,
            ultimo_resumen: None,
        }
    }

    /// Deja en `salida` lo que hay que escribir de `trozo`.
    fn trozo(&mut self, trozo: &[u8], hora: &[u8], salida: &mut Vec<u8>) {
        salida.clear();
        for pedazo in trozo.split_inclusive(|&b| b == b'\n') {
            self.actual.extend_from_slice(pedazo);
            if self.actual.last() == Some(&b'\n') || self.actual.len() > MAX_LINEA {
                self.cerrar(hora, salida);
            }
        }
    }

    /// Lo que quede al terminar: la última línea sin salto y el resumen.
    fn fin(&mut self, hora: &[u8], salida: &mut Vec<u8>) {
        salida.clear();
        if !self.actual.is_empty() {
            self.cerrar(hora, salida);
        }
        self.resumir(hora, salida, true);
    }

    fn cerrar(&mut self, hora: &[u8], salida: &mut Vec<u8>) {
        let linea = std::mem::take(&mut self.actual);
        let continua = std::mem::replace(&mut self.continua, linea.last() != Some(&b'\n'));
        if continua {
            salida.extend_from_slice(&linea);
            return;
        }
        if linea.windows(RUIDO.len()).any(|w| w == RUIDO) {
            self.omitidas += 1;
            return;
        }
        self.resumir(hora, salida, false);
        if linea != b"\n" {
            salida.extend_from_slice(hora);
        }
        salida.extend_from_slice(&linea);
    }

    fn resumir(&mut self, hora: &[u8], salida: &mut Vec<u8>, siempre: bool) {
        if self.omitidas == 0 {
            return;
        }
        let toca = self
            .ultimo_resumen
            .is_none_or(|antes| antes.elapsed() >= RESUMEN_CADA);
        if !(toca || siempre) {
            return;
        }
        salida.extend_from_slice(hora);
        salida.extend_from_slice(
            format!(
                "wrusp: registro: {} avisos de SVG de WhatsApp («RENDERING ERROR … Invalid value for <…>») omitidos\n",
                self.omitidas
            )
            .as_bytes(),
        );
        self.omitidas = 0;
        self.ultimo_resumen = Some(std::time::Instant::now());
    }
}

/// Hora local `HH:MM:SS`, sin dependencias.
pub(crate) fn hora_local() -> String {
    #[cfg(unix)]
    unsafe {
        let ahora = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&ahora, &mut tm).is_null() {
            return format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec);
        }
    }
    fecha_utc()[11..19].to_string()
}

/// Hora local «HH:MM» de un instante (la del «No molestar» del menú).
pub(crate) fn hora_minutos(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    #[cfg(unix)]
    unsafe {
        let instante = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&instante, &mut tm).is_null() {
            return format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
        }
    }
    let resto = secs.rem_euclid(86400);
    format!("{:02}:{:02} UTC", resto / 3600, resto % 3600 / 60)
}

/// La próxima vez que den las `hora` en punto, en hora local.
pub(crate) fn proxima_hora(hora: i32) -> std::time::SystemTime {
    #[cfg(unix)]
    unsafe {
        let ahora = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&ahora, &mut tm).is_null() {
            let ya_paso = tm.tm_hour >= hora;
            tm.tm_hour = hora;
            tm.tm_min = 0;
            tm.tm_sec = 0;
            tm.tm_isdst = -1; // que `mktime` decida si hay horario de verano
            if ya_paso {
                tm.tm_mday += 1; // `mktime` normaliza fin de mes y de año
            }
            let t = libc::mktime(&mut tm);
            if t > 0 {
                return std::time::UNIX_EPOCH + std::time::Duration::from_secs(t as u64);
            }
        }
    }
    std::time::SystemTime::now() + std::time::Duration::from_secs(12 * 3600)
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

/// Cada cuánto se comprueba que el hilo de la ventana atiende, y a partir de
/// cuánto se anota que no lo hacía.
const LATIDO: std::time::Duration = std::time::Duration::from_secs(2);

/// Anota en el registro cuándo el hilo de GTK deja de atender más de dos
/// segundos y cuánto dura. Es el que dibuja la ventana (ADR-028): si se para,
/// toda la aplicación parece colgada, y desde fuera un bloqueo dentro de una
/// llamada síncrona se ve igual que el reposo (ADR-048). Un hilo aparte le
/// manda un encargo vacío y mide cuánto tarda en correr. Los diálogos modales
/// no cuentan: su bucle anidado también atiende los encargos.
pub fn watch_main_thread(app: &crate::runtime::AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("wrusp-latido".into())
        .spawn(move || loop {
            std::thread::sleep(LATIDO);
            let (hecho, atendido) = std::sync::mpsc::sync_channel(1);
            let desde = hora_local();
            let enviado = std::time::Instant::now();
            if app
                .run_on_main_thread(move || {
                    let _ = hecho.send(());
                })
                .is_err()
            {
                return;
            }
            if atendido.recv_timeout(LATIDO).is_ok() {
                continue;
            }
            // Se espera a que corra para saber cuánto duró; si el encargo se
            // descarta, la aplicación está cerrando.
            if atendido.recv().is_err() {
                return;
            }
            eprintln!(
                "wrusp: el hilo de la ventana estuvo {:.1} s sin atender (desde {desde})",
                enviado.elapsed().as_secs_f64()
            );
        });
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
        // Sin saltos de línea es una sola línea: lleva la hora una vez.
        assert_eq!(
            actual.len(),
            "HH:MM:SS ".len() + veces * 4096 + "ultima".len()
        );
        assert!(actual.ends_with("ultima"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// La hora va delante de cada línea con algo, no de las vacías, y una
    /// línea que llega partida en dos lecturas la lleva una sola vez.
    #[cfg(unix)]
    #[test]
    fn cada_linea_lleva_su_hora() {
        let mut lineas = super::Lineas::new();
        let mut salida = Vec::new();
        let mut todo = Vec::new();
        for trozo in [&b"uno\n\ndos pa"[..], b"rtida\ntres\n", b"sin salto"] {
            lineas.trozo(trozo, b"12:00:00 ", &mut salida);
            todo.extend_from_slice(&salida);
        }
        lineas.fin(b"12:00:01 ", &mut salida);
        todo.extend_from_slice(&salida);
        assert_eq!(
            String::from_utf8(todo).unwrap(),
            "12:00:00 uno\n\n12:00:00 dos partida\n12:00:00 tres\n12:00:01 sin salto"
        );
    }

    /// El ruido de SVG de WhatsApp no se escribe, pero queda contado.
    #[cfg(unix)]
    #[test]
    fn el_ruido_de_svg_se_omite_y_se_cuenta() {
        let ruido = "user-script:12:918:23: CONSOLE RENDERING ERROR Error: Invalid value for <circle> attribute r=\"calc(50% - 0px)\"\n";
        let mut lineas = super::Lineas::new();
        let mut salida = Vec::new();
        let mut todo = Vec::new();
        let entrada = format!("{ruido}{ruido}wrusp: algo que importa\n{ruido}");
        lineas.trozo(entrada.as_bytes(), b"12:00:00 ", &mut salida);
        todo.extend_from_slice(&salida);
        lineas.fin(b"12:00:05 ", &mut salida);
        todo.extend_from_slice(&salida);
        let texto = String::from_utf8(todo).unwrap();
        assert!(!texto.contains("calc(50%"), "{texto}");
        assert!(
            texto.contains("12:00:00 wrusp: algo que importa\n"),
            "{texto}"
        );
        // Las dos primeras, antes de la línea buena; la última, al terminar.
        assert!(
            texto.starts_with("12:00:00 wrusp: registro: 2 avisos"),
            "{texto}"
        );
        assert!(
            texto.contains("12:00:05 wrusp: registro: 1 avisos"),
            "{texto}"
        );
    }

    #[test]
    fn las_ocho_siguientes_caen_en_las_proximas_24_horas() {
        let ahora = std::time::SystemTime::now();
        let ocho = super::proxima_hora(8);
        let falta = ocho.duration_since(ahora).expect("en el futuro");
        assert!(
            falta <= std::time::Duration::from_secs(25 * 3600),
            "{falta:?}"
        );
        #[cfg(unix)]
        assert_eq!(super::hora_minutos(ocho), "08:00");
    }

    #[test]
    fn hora_local_con_formato() {
        let h = super::hora_local();
        assert_eq!(h.len(), "12:00:00".len());
        assert_eq!(h.as_bytes()[2], b':');
    }

    /// Y sin rotación, el registro crecería sin fin.
    #[cfg(unix)]
    #[test]
    fn el_volcado_rota_al_pasar_del_tope() {
        use std::io::Write;

        const TOPE: u64 = 4096;
        let (mut escritura, dir, hilo) = banco_de_volcado(TOPE, "rota");
        let linea = format!("{}\n", "y".repeat(99));
        escritura.write_all(linea.repeat(200).as_bytes()).unwrap();
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
