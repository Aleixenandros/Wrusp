//! Ficheros que entran en un chat: soltados sobre la vista o pegados.
//!
//! Son dos agujeros del motor con el mismo remedio. Medidos con banco de
//! pruebas propio (`WRUSP_TEST_URL`), no supuestos:
//!
//! - **Soltar**: WebKitGTK entrega el soltado con la ruta del fichero
//!   (`text/uri-list`) pero **no** construye el `File` que espera la página:
//!   `DROP ficheros=0 tipos=[text/uri-list|text/html]`.
//! - **Pegar**: con un PNG en el portapapeles, el evento `paste` le llega a la
//!   página vacío del todo —`tipos=[] ficheros=0 items=[]`— mientras el motor
//!   sí incrusta la imagen en el DOM como `<img src="blob:…">`. Con texto, en
//!   cambio, llega `tipos=[text/plain]`. WhatsApp escucha ese evento buscando
//!   un fichero y no encuentra nada que adjuntar.
//!
//! El puente lo pone Wrusp: Rust lee los bytes y **los empuja** a la vista con
//! `eval`, en trozos de base64, y la página arma los `File` y se los entrega a
//! WhatsApp como un soltado normal.
//!
//! Antes los bytes se servían por `wrusp://drop/…` y la página los pedía con
//! `fetch`; la CSP de WhatsApp lo bloquea (`connect-src` no admite esquemas
//! propios) y arrastrar dejó de funcionar sin decir nada. Empujar por `eval`
//! no depende de la CSP —comprobado reproduciendo la política en el banco de
//! pruebas— y de paso la página ya no puede *pedir* nada: solo recibe lo que
//! el usuario acaba de soltar o pegar.

use crate::runtime::{AppHandle, Runtime};
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::{webview::Webview, Manager};

/// Bytes por `eval`; en base64 son algo más de 1 MB de script por viaje.
const TROZO: usize = 768 * 1024;

/// Tope por fichero: pasarlo por la vista cuesta memoria en el proceso web, y
/// más allá de esto no compensa. Lo que se descarte se dice en el registro.
const MAX_FICHERO: u64 = 256 * 1024 * 1024;
const MB: u64 = 1024 * 1024;

/// Punto de entrega «donde toque»: lo pegado no tiene coordenadas, así que va
/// al elemento con el foco (la caja del chat abierto).
const DONDE_TOCA: i64 = -1;

/// Lo que se entrega a la vista: un fichero del disco o unos bytes en memoria.
enum Entrada {
    Ruta(PathBuf),
    Bytes {
        nombre: String,
        tipo: String,
        datos: Vec<u8>,
    },
}

/// Entrega a la vista los ficheros soltados sobre ella, en el punto exacto
/// donde se soltaron (en píxeles CSS).
pub fn soltar(app: &AppHandle, etiqueta: &str, rutas: Vec<PathBuf>, x: i64, y: i64) {
    // Un directorio soltado no se puede adjuntar.
    let entradas: Vec<Entrada> = rutas
        .into_iter()
        .filter(|ruta| ruta.is_file())
        .map(Entrada::Ruta)
        .collect();
    entregar(app, etiqueta, entradas, x, y);
}

/// Entrega al chat lo que haya en el portapapeles: los ficheros copiados en el
/// gestor de ficheros o, si no los hay, la imagen.
///
/// Se pide por la vía asíncrona (`request_*`) y no por `wait_for_*`: esta
/// función corre en el hilo de GTK, y la espera síncrona lo deja parado hasta
/// que conteste el dueño del portapapeles —otra aplicación, que puede tardar o
/// no contestar—. Con la interfaz parada, el gestor de ventanas da la
/// aplicación por colgada y sus botones dejan de responder.
#[cfg(target_os = "linux")]
pub fn pegar(app: &AppHandle, etiqueta: &str) {
    use gtk::gdk;

    let Some(pantalla) = gdk::Display::default() else {
        return;
    };
    let Some(portapapeles) = gtk::Clipboard::default(&pantalla) else {
        return;
    };

    let app = app.clone();
    let etiqueta = etiqueta.to_string();

    // Un fichero copiado conserva nombre y tipo, así que tiene preferencia
    // sobre la imagen que el escritorio ofrezca del mismo contenido.
    portapapeles.request_contents(
        &gdk::Atom::intern("text/uri-list"),
        move |papeles, datos| {
            let rutas: Vec<Entrada> = datos
                .uris()
                .iter()
                .filter_map(|uri| uri.parse::<tauri::Url>().ok())
                .filter_map(|url| url.to_file_path().ok())
                .filter(|ruta| ruta.is_file())
                .map(Entrada::Ruta)
                .collect();
            if !rutas.is_empty() {
                entregar(&app, &etiqueta, rutas, DONDE_TOCA, DONDE_TOCA);
                return;
            }

            papeles.request_image(move |_, imagen| {
                let Some(imagen) = imagen else {
                    return; // no había nada que Wrusp pueda adjuntar
                };
                match imagen.save_to_bufferv("png", &[]) {
                    Ok(datos) => entregar(
                        &app,
                        &etiqueta,
                        vec![Entrada::Bytes {
                            nombre: "imagen-pegada.png".to_string(),
                            tipo: "image/png".to_string(),
                            datos,
                        }],
                        DONDE_TOCA,
                        DONDE_TOCA,
                    ),
                    Err(err) => eprintln!("wrusp: no se pudo convertir la imagen pegada ({err})"),
                }
            });
        },
    );
}

/// En Windows y macOS el motor sí le entrega a la página lo que se pega.
#[cfg(not(target_os = "linux"))]
pub fn pegar(_app: &AppHandle, _etiqueta: &str) {}

/// Empuja las entradas a la vista y le pide que las suelte sobre la página.
fn entregar(app: &AppHandle, etiqueta: &str, entradas: Vec<Entrada>, x: i64, y: i64) {
    if entradas.is_empty() {
        return;
    }
    let Some(vista) = app.get_webview(etiqueta) else {
        return;
    };

    let mut entregados = 0;
    for entrada in entradas {
        let listo = match entrada {
            Entrada::Ruta(ruta) => empujar_fichero(&vista, entregados, &ruta),
            Entrada::Bytes {
                nombre,
                tipo,
                datos,
            } => empujar(&vista, entregados, &nombre, &tipo, &mut datos.as_slice()),
        };
        if listo {
            entregados += 1;
        }
    }
    if entregados == 0 {
        eprintln!("wrusp: no se pudo preparar ningún fichero para la vista");
        return;
    }
    eprintln!("wrusp: {entregados} fichero(s) empujados a la vista ({x}, {y})");
    let _ = vista.eval(format!(
        "window.__wruspEntregar && window.__wruspEntregar({x}, {y})"
    ));
}

/// Lee un fichero del disco y lo empuja a la vista.
fn empujar_fichero(vista: &Webview<Runtime>, indice: usize, ruta: &Path) -> bool {
    let tamano = std::fs::metadata(ruta).map(|m| m.len()).unwrap_or_default();
    if tamano > MAX_FICHERO {
        eprintln!(
            "wrusp: {} ocupa {} MB y no se adjunta (tope {} MB)",
            ruta.display(),
            tamano / MB,
            MAX_FICHERO / MB
        );
        return false;
    }
    let mut fichero = match std::fs::File::open(ruta) {
        Ok(fichero) => fichero,
        Err(err) => {
            eprintln!("wrusp: no se pudo abrir {} ({err})", ruta.display());
            return false;
        }
    };
    let nombre = ruta
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "fichero".to_string());
    empujar(
        vista,
        indice,
        &nombre,
        &mime_por_extension(ruta),
        &mut fichero,
    )
}

/// Manda el contenido en trozos de base64 y cierra el fichero en la página.
fn empujar(
    vista: &Webview<Runtime>,
    indice: usize,
    nombre: &str,
    tipo: &str,
    origen: &mut dyn Read,
) -> bool {
    let mut buffer = vec![0u8; TROZO];
    loop {
        let leidos = match origen.read(&mut buffer) {
            Ok(0) => break,
            Ok(leidos) => leidos,
            Err(err) => {
                eprintln!("wrusp: no se pudo leer «{nombre}» ({err})");
                return false;
            }
        };
        // El base64 solo tiene caracteres seguros dentro de una cadena de JS.
        let trozo = base64(&buffer[..leidos]);
        if vista
            .eval(format!(
                "window.__wruspTrozo && window.__wruspTrozo({indice}, \"{trozo}\")"
            ))
            .is_err()
        {
            return false;
        }
    }
    let nombre = serde_json::to_string(nombre).unwrap_or_else(|_| "\"fichero\"".to_string());
    let tipo =
        serde_json::to_string(tipo).unwrap_or_else(|_| "\"application/octet-stream\"".into());
    vista
        .eval(format!(
            "window.__wruspFichero && window.__wruspFichero({indice}, {nombre}, {tipo})"
        ))
        .is_ok()
}

/// Base64 estándar (RFC 4648). A mano: son seis líneas y ahorra una
/// dependencia para lo único que hace falta, escribirlo.
pub(crate) fn base64(datos: &[u8]) -> String {
    const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut salida = String::with_capacity(datos.len().div_ceil(3) * 4);
    for grupo in datos.chunks(3) {
        let n = (u32::from(grupo[0]) << 16)
            | (u32::from(grupo.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(grupo.get(2).copied().unwrap_or(0));
        let letra = |desplazamiento: u32| ALFABETO[((n >> desplazamiento) & 0x3F) as usize] as char;
        salida.push(letra(18));
        salida.push(letra(12));
        salida.push(if grupo.len() > 1 { letra(6) } else { '=' });
        salida.push(if grupo.len() > 2 { letra(0) } else { '=' });
    }
    salida
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

/// Script inyectado en las vistas de cuenta: recibe los ficheros que empuja
/// Rust, se los entrega a WhatsApp como un soltado, y avisa cuando lo pegado
/// no le ha llegado a la página.
pub const SCRIPT: &str = r#"(function () {
  const partes = new Map();   // índice de fichero → trozos recibidos
  const listos = [];

  // Rust empuja los bytes en base64. La página no puede pedirlos (la CSP de
  // WhatsApp no admite `wrusp://` en connect-src), pero sí recibirlos: lo que
  // llega por `eval` no lo gobierna la CSP.
  window.__wruspTrozo = function (i, b64) {
    const bruto = atob(b64);
    const bytes = new Uint8Array(bruto.length);
    for (let k = 0; k < bruto.length; k++) bytes[k] = bruto.charCodeAt(k);
    const trozos = partes.get(i) || [];
    trozos.push(bytes);
    partes.set(i, trozos);
  };

  window.__wruspFichero = function (i, nombre, tipo) {
    listos.push(new File(partes.get(i) || [], nombre, { type: tipo }));
    partes.delete(i);
  };

  // Sin esto no hay forma de saber por qué falla una entrega: la página es la
  // única que ve si WhatsApp la acepta.
  const anotar = (texto) => {
    if (window.__wruspOrden) window.__wruspOrden('log/?m=' + encodeURIComponent(texto));
  };
  const esperar = (ms) => new Promise((listo) => setTimeout(listo, ms));

  const transporte = (ficheros) => {
    const datos = new DataTransfer();
    for (const f of ficheros) datos.items.add(f);
    return datos;
  };

  // Un soltado no dice por sí solo si ha servido de algo: `preventDefault` lo
  // llama cualquiera que escuche `dragover` —WhatsApp el primero— para que el
  // navegador no abra el fichero en la vista. Fiarse de eso daba por bueno el
  // primer destino que se probaba, un `<p>` de un mensaje cualquiera, y ahí se
  // quedaba sin haber adjuntado nada: en el registro salía «soltado aceptado
  // en P» y el fichero no llegaba a ninguna parte. Lo que sí se ve es el
  // efecto: cuando WhatsApp acepta un adjunto abre su previsualizador, con el
  // botón de enviar.
  const hayPrevisualizador = () => {
    // El botón de enviar está siempre en el DOM: lo que cambia es si se ve.
    // Buscarlo con `querySelector` a secas daba por abierto un previsualizador
    // que estaba oculto, y con eso Wrusp se negaba a entregar nada.
    for (const nodo of document.querySelectorAll(
        '[data-icon="send"], [data-testid="send"], [data-icon="send-light"]')) {
      const caja = nodo.getBoundingClientRect();
      // `offsetParent` cubre lo que esté oculto por cualquier ancestro; la
      // medida cubre lo que va en capas fijas, que no tienen offsetParent.
      if (nodo.offsetParent !== null || (caja.width > 0 && caja.height > 0)) return true;
    }
    return false;
  };

  const esperarAdjunto = async (ms) => {
    for (let esperado = 0; esperado < ms; esperado += 100) {
      await esperar(100);
      if (hayPrevisualizador()) return true;
    }
    return false;
  };

  const soltarEn = async (destino, ficheros) => {
    for (const tipo of ['dragenter', 'dragover', 'dragover', 'drop']) {
      destino.dispatchEvent(new DragEvent(tipo, {
        bubbles: true, cancelable: true, composed: true, dataTransfer: transporte(ficheros)
      }));
      // WhatsApp decide con estado de React entre un evento y el siguiente.
      await esperar(40);
    }
  };

  const pegarEn = (destino, ficheros) => {
    const evento = new ClipboardEvent('paste', {
      clipboardData: transporte(ficheros), bubbles: true, cancelable: true, composed: true
    });
    destino.dispatchEvent(evento);
    return evento.defaultPrevented;
  };

  // Respaldo cuando el soltado no cuaja: la misma entrada de fichero que hay
  // detrás del menú de adjuntar. Es la vía más directa y no depende de dónde
  // caiga el ratón, pero va la última porque hay que acertar con la entrada.
  const porEntradaDeFichero = async (ficheros) => {
    const soloMedios = ficheros.every((f) => /^(image|video)\//.test(f.type));
    const entradas = [...document.querySelectorAll('input[type="file"]')];
    const declara = (e) => /image|video/.test((e.getAttribute('accept') || '').toLowerCase());
    // Primero las que declaren aceptar lo que traemos; después, las demás.
    const preferidas = entradas.filter((e) => (soloMedios ? declara(e) : !declara(e)));
    const resto = entradas.filter((e) => !preferidas.includes(e));
    for (const entrada of [...preferidas, ...resto]) {
      try {
        entrada.files = transporte(ficheros).files;
      } catch (e) {
        continue; // esa entrada no admite que le pongamos ficheros
      }
      entrada.dispatchEvent(new Event('change', { bubbles: true }));
      if (await esperarAdjunto(1200)) return entrada.getAttribute('accept') || 'sin accept';
    }
    return null;
  };

  // `x` negativa = viene del portapapeles y no hay punto donde soltarlo.
  window.__wruspEntregar = async function (x, y) {
    const ficheros = listos.splice(0, listos.length);
    if (!ficheros.length) return;
    const nombres = ficheros.map((f) => f.name).join(', ');
    // Con el previsualizador ya abierto no hay forma de distinguir si lo que
    // se ve es de este adjunto o del anterior, así que no se toca nada.
    if (hayPrevisualizador()) {
      anotar('hay un adjunto a medias sin enviar: no se entrega ' + nombres);
      return;
    }
    // Un evento solo lo ven el destino y sus padres, así que se prueba donde
    // más sentido tiene: el punto del soltado o el foco (la caja del chat), y
    // si no, el centro de la vista, que cae dentro del panel de conversación.
    const punto = x < 0 ? null : document.elementFromPoint(x, y);
    const foco = document.activeElement;
    const chat = document.querySelector('#main, [role="region"], [data-testid="conversation-panel-wrapper"], footer, [contenteditable="true"]');
    const centro = document.elementFromPoint(window.innerWidth / 2, window.innerHeight / 2);
    const destinos = [punto, foco, chat, centro, document.body]
      .filter((d, i, todos) => d && todos.indexOf(d) === i);

    // Lo pegado se entrega primero como pegado, que es lo que WhatsApp espera
    // de una captura; si nadie lo recoge, se intenta como soltado.
    if (x < 0) {
      for (const destino of destinos) {
        if (pegarEn(destino, ficheros) && await esperarAdjunto(1200)) {
          anotar('pegado aceptado (' + nombres + ')');
          return;
        }
      }
    }
    for (const destino of destinos) {
      await soltarEn(destino, ficheros);
      if (await esperarAdjunto(1200)) {
        anotar('soltado aceptado en ' + (destino.tagName || '?') + ' (' + nombres + ')');
        return;
      }
    }
    const entrada = await porEntradaDeFichero(ficheros);
    if (entrada) {
      anotar('adjuntado por la entrada de fichero [' + entrada + '] (' + nombres + ')');
      return;
    }
    anotar('nadie aceptó ' + ficheros.length + ' fichero(s): ' + nombres +
           ' · destinos=' + destinos.map((d) => d.tagName || '?').join('/') +
           ' · entradas=' + document.querySelectorAll('input[type=file]').length);
  };

  // Pegar: si el motor no le pasa nada a la página —ni tipos ni ficheros—, es
  // que lo del portapapeles no es texto y se lo ha guardado. Lo lee Rust.
  window.addEventListener('paste', function (evento) {
    const datos = evento.clipboardData;
    if (!datos) return;
    const tipos = datos.types || [];
    if (tipos.length || (datos.files && datos.files.length)) return;
    // Sin esto, WebKit incrusta la imagen en la caja de texto como <img blob:>.
    evento.preventDefault();
    anotar('pegado vacío: se lo pedimos a Rust');
    if (window.__wruspOrden) window.__wruspOrden('pegar');
  }, true);
})();"#;

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_con_los_vectores_del_rfc() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // Bytes altos: los que delatarían un desplazamiento con signo.
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
        assert_eq!(base64(&[0x00, 0x00, 0x00]), "AAAA");
    }
}
