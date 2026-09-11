//! Banco de conexiones al bus de sesión que abre WebKitGTK por cada vídeo.
//!
//! El 11-09-2026 el bus de sesión del usuario murió dos veces con «Too many
//! open files» mientras corría `banco_memoria`, que crea 800 elementos de
//! vídeo, y con él cayó la sesión de escritorio entera. Ese bus admite 1.024
//! descriptores. WebKitGTK publica un reproductor MPRIS por cada sesión
//! multimedia (`org.mpris.MediaPlayer2.webkit.instance…`), y los bancos no
//! desactivaban esa función como hace Wrusp.
//!
//! Este banco mide, con **muy pocos** vídeos y un freno, cuántas conexiones
//! al bus abre cada patrón de uso, con la función activa y desactivada como en
//! Wrusp, y lista los identificadores reales de la función en este WebKit.
//! Si el bus crece más de lo previsto, se para en seco.
//!
//! ```sh
//! cargo run --example banco_mpris
//! ```

use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use webkit2gtk::{WebView, WebViewExt};

const N_VIDEOS: usize = 10;
/// Descriptores de más en el bus antes de parar. Con 1.024 de límite y unos
/// 200 en uso, 150 deja un margen enorme.
const FRENO: usize = 150;

fn leer(ruta: &str) -> String {
    std::fs::read_to_string(ruta).unwrap_or_default()
}

fn campo<'a>(estado: &'a str, clave: &str) -> Option<&'a str> {
    estado
        .lines()
        .find(|l| l.starts_with(clave))
        .and_then(|l| l.split_whitespace().nth(1))
}

fn pids() -> Vec<u32> {
    std::fs::read_dir("/proc")
        .map(|d| {
            d.flatten()
                .filter_map(|e| e.file_name().to_string_lossy().parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// El `dbus-broker` de la sesión de este usuario.
fn pid_bus() -> Option<u32> {
    let yo = leer("/proc/self/status");
    let uid = campo(&yo, "Uid:")?.to_string();
    pids().into_iter().find(|pid| {
        let st = leer(&format!("/proc/{pid}/status"));
        campo(&st, "Name:") == Some("dbus-broker") && campo(&st, "Uid:") == Some(uid.as_str())
    })
}

fn fds(pid: u32) -> usize {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .map(|d| d.count())
        .unwrap_or(0)
}

/// Este proceso y todos sus descendientes (los procesos de WebKit).
fn descendientes() -> HashSet<u32> {
    let pares: Vec<(u32, u32)> = pids()
        .into_iter()
        .filter_map(|pid| {
            let ppid = campo(&leer(&format!("/proc/{pid}/status")), "PPid:")?
                .parse()
                .ok()?;
            Some((pid, ppid))
        })
        .collect();
    let mut mios: HashSet<u32> = [std::process::id()].into();
    loop {
        let antes = mios.len();
        for (pid, ppid) in &pares {
            if mios.contains(ppid) {
                mios.insert(*pid);
            }
        }
        if mios.len() == antes {
            return mios;
        }
    }
}

/// Conexiones al bus de sesión de este banco y sus nombres MPRIS.
fn contar_bus() -> (usize, usize) {
    let Ok(salida) = std::process::Command::new("busctl")
        .args(["--user", "--no-pager", "list", "--no-legend"])
        .output()
    else {
        return (0, 0);
    };
    let mios = descendientes();
    let (mut conexiones, mut mpris) = (0, 0);
    for linea in String::from_utf8_lossy(&salida.stdout).lines() {
        let mut c = linea.split_whitespace();
        let (Some(nombre), Some(pid)) = (c.next(), c.next()) else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        if !mios.contains(&pid) {
            continue;
        }
        if nombre.starts_with(':') {
            conexiones += 1;
        }
        if nombre.starts_with("org.mpris.MediaPlayer2") {
            mpris += 1;
        }
    }
    (conexiones, mpris)
}

/// Identificadores de funciones que contienen «MediaSession» y, si se pide,
/// desactiva la que se llame exactamente «MediaSession», que es lo que hace
/// `permissions::apagar_sesion_multimedia` en Wrusp. Devuelve si la encontró.
fn funciones(settings: &webkit2gtk::Settings, apagar: &[&str]) -> (Vec<String>, Vec<String>) {
    use std::ffi::CStr;
    use webkit2gtk::glib::translate::ToGlibPtr;
    #[repr(C)]
    struct WebKitFeature {
        _o: [u8; 0],
    }
    #[repr(C)]
    struct WebKitFeatureList {
        _o: [u8; 0],
    }
    extern "C" {
        fn webkit_settings_get_all_features() -> *mut WebKitFeatureList;
        fn webkit_feature_list_get_length(l: *mut WebKitFeatureList) -> usize;
        fn webkit_feature_list_get(l: *mut WebKitFeatureList, i: usize) -> *mut WebKitFeature;
        fn webkit_feature_list_unref(l: *mut WebKitFeatureList);
        fn webkit_feature_get_identifier(f: *mut WebKitFeature) -> *const std::os::raw::c_char;
        fn webkit_settings_set_feature_enabled(
            s: *mut webkit2gtk::ffi::WebKitSettings,
            f: *mut WebKitFeature,
            activa: webkit2gtk::glib::ffi::gboolean,
        );
    }
    let mut vistos = Vec::new();
    let mut apagadas = Vec::new();
    unsafe {
        let lista = webkit_settings_get_all_features();
        if lista.is_null() {
            return (vistos, apagadas);
        }
        for i in 0..webkit_feature_list_get_length(lista) {
            let f = webkit_feature_list_get(lista, i);
            if f.is_null() {
                continue;
            }
            let id = webkit_feature_get_identifier(f);
            if id.is_null() {
                continue;
            }
            let id = CStr::from_ptr(id).to_string_lossy().to_string();
            if id.contains("MediaSession") {
                if apagar.contains(&id.as_str()) {
                    webkit_settings_set_feature_enabled(
                        settings.to_glib_none().0,
                        f,
                        webkit2gtk::glib::ffi::GFALSE,
                    );
                    apagadas.push(id.clone());
                }
                vistos.push(id);
            }
        }
        webkit_feature_list_unref(lista);
    }
    (vistos, apagadas)
}

fn script_video() -> String {
    let fuente = include_str!("../src/browser.rs");
    let inicio = fuente
        .find("pub fn fix_large_mp4_blobs_script() -> String {")
        .expect("la función cambió de nombre");
    let cuerpo = &fuente[inicio..];
    let abre = cuerpo.find("r#\"").expect("sin literal") + 3;
    let cierra = cuerpo.find("\"#").expect("sin cierre");
    cuerpo[abre..cierra].to_string()
}

/// Las líneas de `busctl` de las conexiones de este banco: quién es el dueño y
/// cómo se describe la conexión.
fn quienes() -> Vec<String> {
    let Ok(salida) = std::process::Command::new("busctl")
        .args(["--user", "--no-pager", "list", "--no-legend"])
        .output()
    else {
        return vec![];
    };
    let mios = descendientes();
    String::from_utf8_lossy(&salida.stdout)
        .lines()
        .filter(|l| {
            let mut c = l.split_whitespace();
            let nombre = c.next().unwrap_or("");
            let pid = c.next().and_then(|p| p.parse::<u32>().ok());
            nombre.starts_with(':') && pid.is_some_and(|p| mios.contains(&p))
        })
        .map(|l| {
            let nombre = l.split_whitespace().next().unwrap_or("").to_string();
            let proceso = l.split_whitespace().nth(2).unwrap_or("").to_string();
            let objetos = std::process::Command::new("busctl")
                .args(["--user", "--no-pager", "tree", "--list", &nombre])
                .output()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            format!("{nombre} {proceso} objetos=[{objetos}]")
        })
        .collect()
}

fn base64(datos: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(datos.len().div_ceil(3) * 4);
    for g in datos.chunks(3) {
        let n = (u32::from(g[0]) << 16)
            | (u32::from(*g.get(1).unwrap_or(&0)) << 8)
            | u32::from(*g.get(2).unwrap_or(&0));
        s.push(A[(n >> 18 & 63) as usize] as char);
        s.push(A[(n >> 12 & 63) as usize] as char);
        s.push(if g.len() > 1 {
            A[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        s.push(if g.len() > 2 {
            A[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    s
}

const PAGINA: &str = r#"<!doctype html><meta charset="utf-8">
<script>window.__wruspOrden = () => {};</script>
<script>SCRIPT_WRUSP</script>
<div id="caja"></div>
<script>
  const N = N_VIDEOS;
  const bruto = atob(MP4_BASE64);
  const bytes = new Uint8Array(bruto.length);
  for (let i = 0; i < bruto.length; i++) bytes[i] = bruto.charCodeAt(i);
  const espera = (ms) => new Promise((l) => setTimeout(l, ms));
  const caja = document.getElementById('caja');
  const urls = [];
  const conFuente = () => {
    const v = document.createElement('video');
    v.playsInline = true;
    v.style.width = '120px';
    const u = URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' }));
    urls.push(u);
    v.src = u;
    return v;
  };
  const fase = async (n) => { document.title = 'FASE:' + n; await espera(2000); };
  // Una ráfaga de memoria efímera de `mb` megas, en trozos de uno.
  const rafaga = (mb) => { for (let k = 0; k < mb; k++) { let a = new Array(131072).fill(k); a = null; } };
  (async () => {
    await espera(1000);
    await fase('base');
    let dentro = [];
    for (let i = 0; i < N; i++) { const v = conFuente(); caja.appendChild(v); dentro.push(v); await espera(120); }
    await fase('con fuente');
    for (const v of dentro) { v.removeAttribute('src'); v.load(); v.remove(); }
    dentro.length = 0;
    dentro = null;
    for (const u of urls.splice(0)) URL.revokeObjectURL(u);
    await espera(3000);
    await fase('soltados');
    let total = 0;
    for (const mb of [4, 8, 16, 32, 64]) {
      rafaga(mb);
      total += mb;
      await espera(1500);
      await fase('tras ' + total + ' MB');
    }
    document.title = 'FIN';
  })();
</script>"#;

fn correr(nombre: &str, apagar: &[&str], pagina: &str, bus: u32, base: usize) -> bool {
    let ventana = gtk::Window::new(gtk::WindowType::Toplevel);
    ventana.set_default_size(700, 400);
    let vista = WebView::new();
    let mut vistos = Vec::new();
    let mut apagadas = Vec::new();
    if let Some(ajustes) = WebViewExt::settings(&vista) {
        (vistos, apagadas) = funciones(&ajustes, apagar);
    }
    ventana.add(&vista);
    ventana.show_all();

    let fases: Rc<RefCell<Vec<String>>> = Default::default();
    let frenado = Rc::new(Cell::new(false));

    let anotadas = fases.clone();
    vista.connect_title_notify(move |v| {
        let Some(t) = v.title() else { return };
        if let Some(f) = t.strip_prefix("FASE:") {
            let (c, m) = contar_bus();
            let d = fds(bus).saturating_sub(base);
            anotadas
                .borrow_mut()
                .push(format!("{f}: bus +{d} fds, {c} conexiones, {m} MPRIS"));
            if f == "con fuente" {
                for q in quienes() {
                    anotadas.borrow_mut().push(format!("      {q}"));
                }
            }
        } else if t == "FIN" {
            gtk::main_quit();
        }
    });
    vista.load_html(pagina, Some("http://localhost/"));

    let freno = frenado.clone();
    let vigia = gtk::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        let d = fds(bus).saturating_sub(base);
        if d > FRENO {
            println!("   FRENO: el bus ha crecido {d} descriptores; se para la prueba");
            freno.set(true);
            gtk::main_quit();
            return gtk::glib::ControlFlow::Break;
        }
        gtk::glib::ControlFlow::Continue
    });
    let plazo = gtk::glib::timeout_add_seconds_local(180, || {
        gtk::main_quit();
        gtk::glib::ControlFlow::Break
    });
    gtk::main();
    vigia.remove();
    let _ = plazo;
    vista.load_html("", None);
    unsafe { ventana.destroy() };
    let hasta = std::time::Instant::now() + std::time::Duration::from_secs(4);
    while std::time::Instant::now() < hasta {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let (c, m) = contar_bus();
    println!("── {nombre}");
    println!("   identificadores con «MediaSession»: {vistos:?}");
    println!("   apagadas: {apagadas:?}");
    for f in fases.borrow().iter() {
        println!("   {f}");
    }
    println!(
        "   tras cerrar la vista: bus +{} fds, {c} conexiones, {m} MPRIS",
        fds(bus).saturating_sub(base)
    );
    !frenado.get()
}

fn main() {
    let Some(ruta) = std::fs::read_dir("/tmp/wrusp-banco-faststart")
        .ok()
        .and_then(|d| {
            d.flatten().map(|e| e.path()).find(|p| {
                p.to_string_lossy().contains("libx264") && p.extension().is_some_and(|x| x == "mp4")
            })
        })
    else {
        eprintln!("hace falta un vídeo de prueba: cargo run --example banco_faststart lo genera");
        std::process::exit(1);
    };
    let mp4 = std::fs::read(&ruta).expect("vídeo de prueba");
    let pagina = PAGINA
        .replace("N_VIDEOS", &N_VIDEOS.to_string())
        .replace("MP4_BASE64", &format!("'{}'", base64(&mp4)));
    let bus = pid_bus().expect("no encuentro el dbus-broker de la sesión");
    gtk::init().expect("no hay sesión gráfica");
    let base = fds(bus);
    println!("bus de sesión: pid {bus}, {base} descriptores al empezar (límite blando 1024)");
    let pagina = pagina.replace("SCRIPT_WRUSP", "");
    let _ = script_video; // el script de Wrusp no cambia nada aquí (medido)
    correr(
        "MediaSession apagada, como en Wrusp",
        &["MediaSession"],
        &pagina,
        bus,
        base,
    );
}
