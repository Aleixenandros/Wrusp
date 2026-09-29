//! Copiar imágenes del chat al portapapeles del escritorio.
//!
//! Al pulsar «Copiar imagen» en el menú del clic derecho, lo que acababa en el
//! portapapeles era la dirección del blob —`blob:https://web.whatsapp.com/…`—
//! en vez de la foto. Pegarla en cualquier sitio daba ese texto.
//!
//! Medido con banco propio contra WebKitGTK 2.52, no supuesto:
//!
//! - Escribir imágenes desde JavaScript **sí** funciona: `clipboard.write()`
//!   con un `ClipboardItem` de `image/png` o `image/jpeg` deja la imagen de
//!   verdad en el portapapeles del escritorio. Lo que falla es la acción del
//!   menú nativo cuando la imagen es un blob, que es como WhatsApp sirve todas
//!   las suyas.
//! - Traer los bytes de la página a Rust cuesta poco:
//!   `call_async_javascript_function` devolvió 160 KB en base64 en 5 ms.
//!
//! Así que Wrusp sustituye esa entrada del menú por una propia: le pide los
//! bytes a la página, los convierte en `Pixbuf` y los deja en el portapapeles
//! de GTK, que es la vía que no depende de lo que el motor sepa hacer con un
//! blob. La página tiene además un remiendo para cuando es WhatsApp —y no el
//! menú— quien copia la dirección en lugar de la imagen.

use crate::runtime::Runtime;

/// Tope de lo que se pasa por la vista: en base64 son cuatro tercios de esto
/// en un solo valor de JavaScript. Una foto de WhatsApp no se acerca.
const MAX_IMAGEN: usize = 32 * 1024 * 1024;

/// Script inyectado en las vistas de cuenta.
pub const SCRIPT: &str = r#"(function () {
  const MAX_IMAGEN = 32 * 1024 * 1024;

  // Dos caminos, por orden de fidelidad. El primero entrega los bytes
  // originales; el segundo repinta lo que ya se ve, y sirve cuando la CSP de
  // WhatsApp veta la petición o el blob ya no existe.
  async function comoBlob(uri) {
    try {
      const respuesta = await fetch(uri);
      if (respuesta.ok) return await respuesta.blob();
    } catch (e) { /* queda el lienzo */ }

    const img = Array.prototype.find.call(
      document.images, (i) => i.currentSrc === uri || i.src === uri
    );
    if (!img || !img.naturalWidth) return null;
    const lienzo = document.createElement('canvas');
    lienzo.width = img.naturalWidth;
    lienzo.height = img.naturalHeight;
    lienzo.getContext('2d').drawImage(img, 0, 0);
    return await new Promise((listo) => lienzo.toBlob(listo, 'image/png'));
  }

  function aBase64(bytes) {
    // A trozos: `String.fromCharCode` con un array entero desborda la pila.
    let bruto = '';
    for (let i = 0; i < bytes.length; i += 8192)
      bruto += String.fromCharCode.apply(null, bytes.subarray(i, i + 8192));
    return btoa(bruto);
  }

  // Lo llama Rust cuando se pulsa «Copiar imagen» (ver `clipboard.rs`).
  window.__wruspLeerImagen = async function (uri) {
    const blob = await comoBlob(uri);
    if (!blob || !blob.size || blob.size > MAX_IMAGEN) return '';
    return aBase64(new Uint8Array(await blob.arrayBuffer()));
  };

  // Remiendo para el otro camino: si la página copia como texto la dirección
  // de una imagen suya, se copia la imagen. WebKitGTK admite escribir
  // `image/png` y `image/jpeg` desde aquí (comprobado), así que no hace falta
  // molestar a Rust.
  const portapapeles = navigator.clipboard;
  if (portapapeles && portapapeles.writeText && typeof ClipboardItem === 'function') {
    const escribirTexto = portapapeles.writeText.bind(portapapeles);
    portapapeles.writeText = function (texto) {
      if (typeof texto === 'string' && texto.indexOf('blob:') === 0) {
        return comoBlob(texto).then((blob) => {
          if (!blob || !/^image\//.test(blob.type)) return escribirTexto(texto);
          return portapapeles.write([new ClipboardItem({ [blob.type]: blob })]);
        }).catch(() => escribirTexto(texto));
      }
      return escribirTexto(texto);
    };
  }
})();"#;

/// Decodifica base64 estándar; devuelve `None` si aparece un carácter que no
/// pertenece al alfabeto.
pub(crate) fn desde_base64(texto: &str) -> Option<Vec<u8>> {
    // Tabla de 256 entradas en vez de buscar cada carácter en el alfabeto: un
    // volcado de 64 MB son 85 millones de caracteres (ADR-048).
    const NO: u8 = 0xff;
    const TABLA: [u8; 256] = {
        const ALFABETO: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut tabla = [NO; 256];
        let mut i = 0;
        while i < ALFABETO.len() {
            tabla[ALFABETO[i] as usize] = i as u8;
            i += 1;
        }
        tabla
    };
    let mut salida = Vec::with_capacity(texto.len() / 4 * 3);
    let mut acumulado: u32 = 0;
    let mut bits = 0u32;
    for byte in texto.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
        let valor = TABLA[byte as usize];
        if valor == NO {
            return None;
        }
        let valor = valor as u32;
        acumulado = (acumulado << 6) | valor;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            salida.push((acumulado >> bits) as u8);
        }
    }
    Some(salida)
}

/// Deja los bytes de una imagen en el portapapeles del escritorio.
/// Copia texto al portapapeles del escritorio.
///
/// Lo pide la página de ajustes para el informe de diagnóstico.
/// `navigator.clipboard` no vale aquí: WebKitGTK solo lo expone en contexto
/// seguro y con el permiso concedido, y el informe se copia de una vista
/// local. Por el mismo camino que las imágenes del chat, que ya funciona.
#[tauri::command]
#[cfg(target_os = "linux")]
pub fn copy_text(text: String) -> Result<(), String> {
    if text.len() > 256 * 1024 {
        return Err("Texto demasiado largo para el portapapeles".into());
    }
    let pantalla = gtk::gdk::Display::default().ok_or("No hay pantalla")?;
    let papeles = gtk::Clipboard::default(&pantalla).ok_or("No hay portapapeles")?;
    papeles.set_text(&text);
    papeles.store();
    Ok(())
}

#[tauri::command]
#[cfg(not(target_os = "linux"))]
pub fn copy_text(_text: String) -> Result<(), String> {
    Err("Copiar al portapapeles solo está implementado en Linux".into())
}

/// Deja en el portapapeles la imagen que la página entregó en base64.
///
/// Se descodifica en un hilo aparte: en Fedora 44 gdk-pixbuf lo hace con
/// glycin, que lanza un cargador aislado con bwrap y habla con él por D-Bus, y
/// en el hilo de GTK eso es esperar a otro proceso (ADR-028, ADR-048). Al hilo
/// de GTK solo vuelven los píxeles: en gtk-rs un `Pixbuf` no cruza de hilo.
#[cfg(target_os = "linux")]
fn al_portapapeles(base64: String) {
    let _ = std::thread::Builder::new()
        .name("wrusp-imagen".into())
        .spawn(move || {
            let Some(bytes) = desde_base64(&base64) else {
                eprintln!("wrusp: la página devolvió base64 ilegible");
                return;
            };
            if let Some(imagen) = descodificar(&bytes) {
                gtk::glib::MainContext::default().invoke(move || poner_imagen(imagen));
            }
        });
}

/// Los píxeles de una imagen ya descodificada, en lo que sí cruza de hilo.
#[cfg(target_os = "linux")]
struct Pixeles {
    bytes: gtk::glib::Bytes,
    alfa: bool,
    bits: i32,
    ancho: i32,
    alto: i32,
    paso: i32,
}

#[cfg(target_os = "linux")]
fn descodificar(bytes: &[u8]) -> Option<Pixeles> {
    use gtk::gdk_pixbuf::PixbufLoader;
    use gtk::prelude::*;

    let cargador = PixbufLoader::new();
    let cargada = cargador.write(bytes).and_then(|_| cargador.close());
    if let Err(err) = cargada {
        eprintln!("wrusp: la imagen copiada no se pudo decodificar ({err})");
        return None;
    }
    let Some(pixbuf) = cargador.pixbuf() else {
        eprintln!("wrusp: la imagen copiada no dio ningún fotograma");
        return None;
    };
    Some(Pixeles {
        bytes: pixbuf.read_pixel_bytes(),
        alfa: pixbuf.has_alpha(),
        bits: pixbuf.bits_per_sample(),
        ancho: pixbuf.width(),
        alto: pixbuf.height(),
        paso: pixbuf.rowstride(),
    })
}

#[cfg(target_os = "linux")]
fn poner_imagen(imagen: Pixeles) {
    use gtk::gdk_pixbuf::{Colorspace, Pixbuf};

    let pixbuf = Pixbuf::from_bytes(
        &imagen.bytes,
        Colorspace::Rgb,
        imagen.alfa,
        imagen.bits,
        imagen.ancho,
        imagen.alto,
        imagen.paso,
    );
    let Some(pantalla) = gtk::gdk::Display::default() else {
        return;
    };
    let Some(papeles) = gtk::Clipboard::default(&pantalla) else {
        return;
    };
    papeles.set_image(&pixbuf);
    // Que sobreviva a Wrusp donde el escritorio lo permita (en Wayland no hay
    // gestor de portapapeles y la llamada no hace nada).
    papeles.store();
    eprintln!(
        "wrusp: imagen copiada al portapapeles ({}×{})",
        pixbuf.width(),
        pixbuf.height()
    );
}

/// Pide a la página los bytes de `uri` y los copia.
#[cfg(target_os = "linux")]
fn copiar_imagen(vista: &webkit2gtk::WebView, uri: &str) {
    use javascriptcore::ValueExt;
    use webkit2gtk::WebViewExt;

    let cuerpo = format!(
        "return await window.__wruspLeerImagen({});",
        serde_json::to_string(uri).unwrap_or_else(|_| "''".into())
    );
    vista.call_async_javascript_function(
        &cuerpo,
        None,
        None,
        None,
        None::<&webkit2gtk::gio::Cancellable>,
        move |resultado| match resultado {
            Ok(valor) if valor.is_string() => {
                let base64: String = valor.to_str().into();
                if base64.is_empty() {
                    eprintln!("wrusp: la página no pudo entregar la imagen que se copiaba");
                    return;
                }
                if base64.len() > MAX_IMAGEN / 3 * 4 + 8 {
                    eprintln!("wrusp: imagen demasiado grande para copiarla");
                    return;
                }
                al_portapapeles(base64);
            }
            Ok(_) => eprintln!("wrusp: la página no devolvió la imagen que se copiaba"),
            Err(err) => eprintln!("wrusp: no se pudo leer la imagen que se copiaba ({err})"),
        },
    );
}

/// Sustituye en el menú del clic derecho la acción de copiar imagen del motor
/// por la de Wrusp.
///
/// El resto del menú se deja intacto: solo se toca la entrada que no hace lo
/// que dice. Si el clic no cae sobre una imagen, esto no se entera.
#[cfg(target_os = "linux")]
pub fn configure(webview: &tauri::webview::Webview<Runtime>) {
    use webkit2gtk::{
        gio, ContextMenuAction, ContextMenuExt, ContextMenuItem, ContextMenuItemExt,
        HitTestResultExt, WebViewExt,
    };

    let resultado = webview.with_webview(|plataforma| {
        let nativa = plataforma.inner();
        nativa.connect_context_menu(|vista, menu, _evento, golpe| {
            if !golpe.context_is_image() {
                return false; // menú normal
            }
            let Some(uri) = golpe.image_uri() else {
                return false;
            };

            // Fuera la que copia la dirección creyendo copiar la imagen; se
            // recuerda su sitio para poner la nuestra donde el usuario la
            // espera.
            let mut posicion = -1;
            for (i, item) in menu.items().iter().enumerate() {
                if item.stock_action() == ContextMenuAction::CopyImageToClipboard {
                    posicion = i as i32;
                    menu.remove(item);
                    break;
                }
            }

            let accion = gio::SimpleAction::new("wrusp-copiar-imagen", None);
            let vista = vista.clone();
            let uri = uri.to_string();
            accion.connect_activate(move |_, _| copiar_imagen(&vista, &uri));
            let item = ContextMenuItem::from_gaction(&accion, "Copiar imagen", None);
            menu.insert(&item, posicion);
            false
        });
    });
    if let Err(err) = resultado {
        eprintln!("wrusp: no se pudo enganchar el menú de copiar imagen ({err})");
    }
}

/// En Windows y macOS el motor copia bien las imágenes del chat.
#[cfg(not(target_os = "linux"))]
pub fn configure(_webview: &tauri::webview::Webview<Runtime>) {}

#[cfg(test)]
mod tests {
    use super::desde_base64;

    #[test]
    fn base64_con_los_vectores_del_rfc() {
        assert_eq!(desde_base64("").unwrap(), b"");
        assert_eq!(desde_base64("Zg==").unwrap(), b"f");
        assert_eq!(desde_base64("Zm8=").unwrap(), b"fo");
        assert_eq!(desde_base64("Zm9v").unwrap(), b"foo");
        assert_eq!(desde_base64("Zm9vYg==").unwrap(), b"foob");
        assert_eq!(desde_base64("Zm9vYmE=").unwrap(), b"fooba");
        assert_eq!(desde_base64("Zm9vYmFy").unwrap(), b"foobar");
        // Bytes altos: los que delatarían un desplazamiento con signo.
        assert_eq!(desde_base64("//79").unwrap(), vec![0xff, 0xfe, 0xfd]);
        assert_eq!(desde_base64("AAAA").unwrap(), vec![0x00, 0x00, 0x00]);
    }

    #[test]
    fn base64_rechaza_lo_que_no_es_base64() {
        assert!(desde_base64("no es base64 ni de lejos ****").is_none());
        assert!(desde_base64("Zm9v\n").is_some()); // los espacios sí se saltan
    }

    #[test]
    fn base64_ida_y_vuelta_con_todos_los_bytes() {
        let original: Vec<u8> = (0..=255u8).collect();
        let texto = crate::filedrop::base64(&original);
        assert_eq!(desde_base64(&texto).unwrap(), original);
    }
}
