//! Barra lateral nativa (GTK), fuera de las páginas. Solo en Linux.
//!
//! Hasta la 0.4.24 la barra era HTML inyectado en cada vista: `#app` de
//! WhatsApp se desplazaba 60 px con CSS, un observador sobre todo el `body`
//! recolocaba las capas a pantalla completa y `innerWidth` se falseaba para
//! que los menús flotantes no se saliesen (ADR-051). Todo lo que WhatsApp
//! mide contra su ventana salía 60 px desplazado, y cada mutación del DOM
//! pasaba por nuestro observador.
//!
//! Aquí la barra es un widget GTK a la izquierda de las vistas. La ventana
//! de tao trae una caja vertical donde wry empaqueta los webviews (uno
//! visible cada vez, que se queda con todo el espacio); se saca esa caja de
//! la ventana, se mete en una horizontal junto a la barra y se vuelve a
//! colgar. Las páginas no saben que existe: su `innerWidth` es su anchura.
//!
//! Solo se toca GTK desde el hilo principal: el estado vive en un
//! `thread_local` y `refrescar` se llama desde `shell::refresh_rails`, que
//! siempre llega por ese hilo. Si algo falla al instalarla, `activa()`
//! devuelve `false` y las vistas vuelven a dibujar la barra inyectada; con
//! `WRUSP_BARRA_INYECTADA=1` en el entorno se fuerza ese modo.

use crate::config::ConfigState;
use crate::runtime::AppHandle;
use gtk::prelude::*;
use std::cell::RefCell;
use tauri::Manager;

/// Ancho de la barra en píxeles lógicos (el mismo que la inyectada).
const ANCHO: i32 = crate::rail::WIDTH as i32;

struct Barra {
    caja: gtk::Box,
    estilo: gtk::CssProvider,
}

thread_local! {
    static BARRA: RefCell<Option<Barra>> = const { RefCell::new(None) };
}

/// ¿Está instalada la barra nativa? Solo tiene sentido en el hilo principal.
pub fn activa() -> bool {
    BARRA.with(|b| b.borrow().is_some())
}

/// Cuelga la barra a la izquierda de las vistas. Antes de crear ningún
/// webview y en el hilo principal.
pub fn instalar(
    app: &AppHandle,
    window: &tauri::Window<crate::runtime::Runtime>,
) -> Result<(), String> {
    if std::env::var_os("WRUSP_BARRA_INYECTADA").is_some() {
        return Err("WRUSP_BARRA_INYECTADA en el entorno".into());
    }
    if activa() {
        return Ok(());
    }
    let vbox = window.default_vbox().map_err(|e| e.to_string())?;
    // La caja vertical cuelga directamente de la ventana (tao, `window.rs`).
    let padre = vbox
        .parent()
        .and_then(|p| p.downcast::<gtk::Container>().ok())
        .ok_or("la caja de la ventana no cuelga de un contenedor")?;

    let caja = gtk::Box::new(gtk::Orientation::Vertical, 8);
    caja.set_widget_name("wrusp-barra");
    caja.set_size_request(ANCHO, -1);
    let estilo = gtk::CssProvider::new();
    let pantalla = gtk::gdk::Screen::default().ok_or("sin pantalla GDK")?;
    gtk::StyleContext::add_provider_for_screen(
        &pantalla,
        &estilo,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    padre.remove(&vbox);
    hbox.pack_start(&caja, false, false, 0);
    hbox.pack_start(&vbox, true, true, 0);
    padre.add(&hbox);
    hbox.show_all();

    #[cfg(debug_assertions)]
    if std::env::var_os("WRUSP_PRUEBA_BARRA").is_some() {
        // Para verificar la geometría sin mirar la pantalla (perfil
        // desechable): dónde queda cada caja a los tres segundos.
        let (caja_p, vbox_p) = (caja.clone(), vbox.clone());
        gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            let (a, b) = (caja_p.allocation(), vbox_p.allocation());
            eprintln!(
                "wrusp: prueba de la barra: barra x={} y={} {}×{} · vistas x={} y={} {}×{}",
                a.x(),
                a.y(),
                a.width(),
                a.height(),
                b.x(),
                b.y(),
                b.width(),
                b.height()
            );
        });
    }

    BARRA.with(|b| *b.borrow_mut() = Some(Barra { caja, estilo }));
    refrescar(app);
    Ok(())
}

/// Color de acento válido para CSS (`#rrggbb`), o el verde de WhatsApp.
fn color_css(color: Option<&str>) -> &str {
    match color {
        Some(c)
            if c.len() == 7
                && c.starts_with('#')
                && c[1..].chars().all(|ch| ch.is_ascii_hexdigit()) =>
        {
            c
        }
        _ => "#25d366",
    }
}

/// Vuelve a pintar la barra con el estado actual: cuentas, vista activa, no
/// leídos y tema. Hilo principal.
pub fn refrescar(app: &AppHandle) {
    BARRA.with(|b| {
        let b = b.borrow();
        let Some(barra) = b.as_ref() else { return };
        pintar(app, barra);
    });
}

fn pintar(app: &AppHandle, barra: &Barra) {
    let (accounts, mode) = {
        let cfg = app.state::<ConfigState>();
        let cfg = cfg.0.lock().unwrap();
        (cfg.accounts.clone(), cfg.theme)
    };
    let active = app
        .state::<crate::shell::ActiveView>()
        .0
        .lock()
        .unwrap()
        .clone();
    let unread = app
        .state::<crate::shell::Unread>()
        .0
        .lock()
        .unwrap()
        .clone();
    let dark = crate::theme::is_dark(app, mode);

    // Los mismos colores que tenía la barra inyectada.
    let (fondo, borde, boton, texto, hover) = if dark {
        ("#10151a", "#2b343d", "#1f2a30", "#e9edef", "#2b3942")
    } else {
        ("#ededed", "#d8dcdb", "#ffffff", "#333333", "#f5f6f6")
    };
    let mut css = format!(
        "#wrusp-barra {{ background-color: {fondo}; border-right: 1px solid {borde}; padding: 10px 0; }}\n\
         #wrusp-barra button {{ min-width: 36px; min-height: 36px; padding: 0; margin: 0 9px; \
           border-radius: 20px; border: 2px solid transparent; background-image: none; \
           background-color: {boton}; color: {texto}; font-weight: 700; font-size: 14px; \
           box-shadow: none; outline-width: 0; }}\n\
         #wrusp-barra button:hover {{ background-color: {hover}; }}\n\
         #wrusp-barra button.activa {{ border-color: #25d366; }}\n\
         #wrusp-barra label.insignia {{ background-color: #25d366; color: #ffffff; border-radius: 9px; \
           min-width: 9px; min-height: 13px; padding: 1px 4px; margin: 0 2px 0 0; \
           font-size: 10px; font-weight: 700; }}\n\
         #wrusp-barra label.silenciada {{ background-color: #8696a0; border-radius: 4px; \
           min-width: 8px; min-height: 8px; margin: 0 7px 0 0; }}\n"
    );

    let caja = &barra.caja;
    for hijo in caja.children() {
        caja.remove(&hijo);
    }

    for (i, acc) in accounts.iter().enumerate() {
        let color = color_css(acc.color.as_deref());
        let es_activa = acc.id == active;
        let nombre = format!("wrusp-cuenta-{i}");
        if es_activa {
            css.push_str(&format!(
                "#wrusp-barra #{nombre} {{ border-color: {color}; }}\n"
            ));
        } else {
            css.push_str(&format!("#wrusp-barra #{nombre} {{ color: {color}; }}\n"));
        }
        css.push_str(&format!(
            "#wrusp-barra #wrusp-insignia-{i} {{ background-color: {color}; }}\n"
        ));

        let boton = gtk::Button::with_label(&crate::rail::initials(&acc.name));
        boton.set_widget_name(&nombre);
        if es_activa {
            boton.style_context().add_class("activa");
        }
        let atajo = if i < 9 {
            format!("  (Ctrl+{})", i + 1)
        } else {
            String::new()
        };
        boton.set_tooltip_text(Some(&format!(
            "{}{}{atajo}",
            acc.name,
            if acc.muted { " (Silenciada)" } else { "" }
        )));
        // El foco se queda en la página: los atajos de teclado viven allí.
        boton.set_can_focus(false);
        boton.set_focus_on_click(false);
        let (app_clic, id) = (app.clone(), acc.id.clone());
        boton.connect_clicked(move |_| {
            if let Err(err) = crate::shell::show_account(&app_clic, &id) {
                eprintln!("wrusp: no se pudo cambiar de cuenta: {err}");
            }
        });

        let capa = gtk::Overlay::new();
        capa.add(&boton);
        let n = unread.get(&acc.id).copied().unwrap_or(0);
        if n > 0 {
            let insignia = gtk::Label::new(Some(&if n > 99 {
                "99+".to_string()
            } else {
                n.to_string()
            }));
            insignia.set_widget_name(&format!("wrusp-insignia-{i}"));
            insignia.style_context().add_class("insignia");
            insignia.set_halign(gtk::Align::End);
            insignia.set_valign(gtk::Align::Start);
            capa.add_overlay(&insignia);
            capa.set_overlay_pass_through(&insignia, true);
        }
        if acc.muted {
            let punto = gtk::Label::new(None);
            punto.style_context().add_class("silenciada");
            punto.set_tooltip_text(Some("Silenciada"));
            punto.set_halign(gtk::Align::End);
            punto.set_valign(gtk::Align::End);
            capa.add_overlay(&punto);
            capa.set_overlay_pass_through(&punto, true);
        }
        caja.pack_start(&capa, false, false, 0);
    }

    let alta = gtk::Button::with_label("+");
    alta.set_tooltip_text(Some("Añadir cuenta  (Ctrl+U)"));
    alta.set_can_focus(false);
    alta.set_focus_on_click(false);
    let app_alta = app.clone();
    alta.connect_clicked(move |_| crate::shell::abrir_alta(&app_alta));
    caja.pack_start(&alta, false, false, 0);

    let hueco = gtk::Box::new(gtk::Orientation::Vertical, 0);
    hueco.set_vexpand(true);
    caja.pack_start(&hueco, true, true, 0);

    let ajustes = gtk::Button::with_label("⚙");
    ajustes.set_tooltip_text(Some("Ajustes  (Ctrl+P)"));
    ajustes.set_can_focus(false);
    ajustes.set_focus_on_click(false);
    if active == crate::shell::SETTINGS_VIEW {
        ajustes.style_context().add_class("activa");
    }
    let app_ajustes = app.clone();
    ajustes.connect_clicked(move |_| crate::shell::show_settings(&app_ajustes));
    caja.pack_start(&ajustes, false, false, 0);

    if let Err(err) = barra.estilo.load_from_data(css.as_bytes()) {
        eprintln!("wrusp: la hoja de estilo de la barra no se aplicó ({err})");
    }
    caja.show_all();
}
