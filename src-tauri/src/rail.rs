//! Barra lateral, atajos de teclado y contador de no leídos, inyectados en
//! todas las vistas.
//!
//! La barra vive *dentro* de la vista visible, tanto sobre WhatsApp Web como
//! sobre la página de ajustes. Así conserva exactamente el mismo estado y
//! comportamiento en todos los perfiles y plataformas.
//!
//! No se concede IPC de Tauri a las páginas de whatsapp.com. Para hablar con
//! Rust, este script pide `wrusp://…` con `fetch`: lo atiende el esquema
//! propio registrado en `main`. Si no estuviera disponible se recurre a la
//! navegación, que el manejador intercepta y cancela (ver ADR-012).
//!
//! El script se parte en dos: `runtime_script` (la maquinaria, se inyecta una
//! vez al crear la vista y sabe *qué* vista es) y `state_script` (los datos,
//! que se reenvían por `eval` cada vez que cambian las cuentas o el tema).

use crate::config::Account;

/// Ancho de la barra en píxeles CSS.
pub const WIDTH: u32 = 60;

/// Iniciales que se muestran en el avatar de una cuenta.
fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .flat_map(|c| c.to_uppercase())
        .collect();
    if letters.is_empty() {
        "?".into()
    } else {
        letters
    }
}

/// Datos de la barra. Se evalúa en la vista cada vez que cambian.
pub fn state_script(
    accounts: &[Account],
    active: &str,
    dark: bool,
    unread: &std::collections::HashMap<String, u32>,
) -> String {
    let items: Vec<serde_json::Value> = accounts
        .iter()
        .map(|acc| {
            serde_json::json!({
                "id": acc.id,
                "name": acc.name,
                "initials": initials(&acc.name),
                "unread": unread.get(&acc.id).copied().unwrap_or(0),
            })
        })
        .collect();
    let state = serde_json::json!({
        "accounts": items,
        "active": active,
        "dark": dark,
        "width": WIDTH,
    });
    format!("window.__wruspState = {state}; window.__wruspRender && window.__wruspRender();")
}

/// Maquinaria de la barra para la vista `own` (`settings` o el id de cuenta).
pub fn runtime_script(own: &str) -> String {
    let own = serde_json::to_string(own).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(function () {{
  if (window.__wruspReady) return;
  window.__wruspReady = true;
  window.__wruspSelf = {own};

  // Las órdenes viajan como petición de red al esquema propio `wrusp://`, no
  // como navegación: dos navegaciones en el mismo instante se pisan y se
  // perdían avisos (notificación y contador llegan juntos). Si el esquema no
  // estuviera disponible, se recurre a la navegación, que Rust también
  // intercepta.
  const go = (path) => {{
    const url = 'wrusp://' + path;
    try {{
      fetch(url, {{ method: 'POST', mode: 'cors', keepalive: true }}).catch(() => {{
        window.location.href = url;
      }});
    }} catch (e) {{
      window.location.href = url;
    }}
  }};
  const st = () => window.__wruspState || {{ accounts: [], active: '', dark: false, width: 60 }};

  // ── Barra lateral ───────────────────────────────────────────
  function ensureStyle() {{
    const s = st();
    let style = document.getElementById('wrusp-rail-style');
    if (!style) {{
      style = document.createElement('style');
      style.id = 'wrusp-rail-style';
      (document.head || document.documentElement).appendChild(style);
    }}
    style.textContent = `
      #wrusp-rail {{
        position: fixed; top: 0; left: 0; bottom: 0; width: ${{s.width}}px;
        z-index: 2147483647;
        background: ${{s.dark ? '#10151a' : '#ededed'}};
        border-right: 1px solid ${{s.dark ? '#2b343d' : '#d8dcdb'}};
        display: flex; flex-direction: column; align-items: center;
        gap: 8px; padding: 10px 0;
        font-family: system-ui, -apple-system, 'Segoe UI', sans-serif;
        box-sizing: border-box; overflow-y: auto; overflow-x: hidden;
      }}
      #wrusp-rail::-webkit-scrollbar {{ width: 0; }}
      .wrusp-btn {{
        position: relative;
        width: 40px; height: 40px; flex: 0 0 40px; border-radius: 50%;
        border: 2px solid transparent; cursor: pointer; display: flex;
        align-items: center; justify-content: center;
        font-size: 14px; font-weight: 700; padding: 0;
        background: ${{s.dark ? '#1a2129' : '#ffffff'}};
        color: ${{s.dark ? '#e6ebe8' : '#3b4a42'}};
        transition: border-color .12s, transform .12s;
      }}
      .wrusp-btn:hover {{ transform: scale(1.06); }}
      .wrusp-btn.active {{ border-color: #f46623; }}
      .wrusp-badge {{
        position: absolute; top: -3px; right: -3px; min-width: 17px; height: 17px;
        border-radius: 9px; background: #f46623; color: #fff;
        font-size: 10px; line-height: 17px; font-weight: 700;
        padding: 0 4px; box-sizing: border-box;
      }}
      .wrusp-sep {{ flex: 1 1 auto; }}
      /* Deja hueco a la barra sin depender del maquetado de WhatsApp: si el
         selector de su raíz cambiara, la barra solo taparía ese margen. */
      html {{ box-sizing: border-box; }}
      body {{ padding-left: ${{s.width}}px !important; box-sizing: border-box !important; }}
      #app {{ left: ${{s.width}}px !important; width: calc(100vw - ${{s.width}}px) !important; }}
    `;
  }}

  function button(label, title, onClick, active, badge) {{
    const b = document.createElement('button');
    b.className = 'wrusp-btn' + (active ? ' active' : '');
    b.textContent = label;
    b.title = title;
    b.addEventListener('click', onClick);
    if (badge) {{
      const n = document.createElement('span');
      n.className = 'wrusp-badge';
      n.textContent = badge > 99 ? '99+' : String(badge);
      b.appendChild(n);
    }}
    return b;
  }}

  function render() {{
    const s = st();
    ensureStyle();
    let rail = document.getElementById('wrusp-rail');
    if (!rail) {{
      rail = document.createElement('div');
      rail.id = 'wrusp-rail';
      // Cuelga de <html>, no de <body>: WhatsApp reescribe el body al
      // renderizar y se llevaría la barra por delante.
      document.documentElement.appendChild(rail);
    }}
    rail.replaceChildren();

    s.accounts.forEach((acc, i) => {{
      const title = acc.name + (i < 9 ? '  (Ctrl+' + (i + 1) + ')' : '');
      rail.appendChild(
        button(acc.initials, title, () => go('switch/' + acc.id), acc.id === s.active, acc.unread)
      );
    }});
    rail.appendChild(button('+', 'Añadir cuenta  (Ctrl+U)', () => go('add')));
    const sep = document.createElement('div');
    sep.className = 'wrusp-sep';
    rail.appendChild(sep);
    rail.appendChild(
      button('⚙', 'Ajustes  (Ctrl+P)', () => go('settings'), s.active === 'settings')
    );
  }}

  window.__wruspRender = render;

  // Este script corre ANTES de que exista el documento: en ese instante no hay
  // ni <html>, así que pintar aquí lanzaba un TypeError. Costó verlo, porque la
  // barra aparecía igual —el estado llega después por `eval` y vuelve a
  // pintarla—, pero el fallo dejaba a medias todo lo que va detrás.
  const arrancar = () => {{
    render();
    // WhatsApp es una SPA y puede vaciar el árbol al arrancar; si la barra
    // desaparece, se vuelve a poner.
    new MutationObserver(() => {{
      if (!document.getElementById('wrusp-rail')) render();
    }}).observe(document.documentElement, {{ childList: true }});
  }};

  if (document.documentElement) {{
    arrancar();
  }} else {{
    document.addEventListener('DOMContentLoaded', arrancar, {{ once: true }});
  }}

  // ── Capas a pantalla completa ───────────────────────────────
  // El hueco de la barra se abre con padding en `body` y ajustando `#app`,
  // pero las capas `position: fixed` se anclan al viewport y lo ignoran: el
  // visor de fotos quedaba con sus primeros 60 px debajo de la barra. Sus
  // clases cambian en cada despliegue, así que no hay selector del que fiarse:
  // se detectan por geometría —capa fija que cubre (casi) todo el viewport
  // pegada al borde izquierdo— y se les hace el mismo hueco. Los menús y
  // globos pequeños no pasan el filtro de tamaño y no se tocan.
  function corregirCapas() {{
    const w = st().width;
    const cand = document.querySelectorAll(
      'body > *, body > * > *, #app > *, #app > * > *, [data-animate-media-viewer]'
    );
    for (const el of cand) {{
      if (el.dataset.wruspAjustado) continue;
      if (el.id === 'wrusp-rail') continue;
      let cs;
      try {{ cs = getComputedStyle(el); }} catch (e) {{ continue; }}
      if (cs.position !== 'fixed') continue;
      const r = el.getBoundingClientRect();
      if (r.left > 1 || r.width < innerWidth * 0.9 || r.height < innerHeight * 0.85) continue;
      el.dataset.wruspAjustado = '1';
      el.style.setProperty('left', w + 'px', 'important');
      el.style.setProperty('width', 'calc(100vw - ' + w + 'px)', 'important');
    }}
  }}
  let correccionPendiente = false;
  const programarCorreccion = () => {{
    if (correccionPendiente) return;
    correccionPendiente = true;
    requestAnimationFrame(() => {{
      correccionPendiente = false;
      try {{ corregirCapas(); }} catch (e) {{ /* mejor sin corrección que sin barra */ }}
    }});
  }};
  const vigilarCapas = () => {{
    if (!document.body) return;
    programarCorreccion();
    new MutationObserver(programarCorreccion).observe(document.body, {{
      childList: true,
      subtree: true,
    }});
  }};
  if (document.body) vigilarCapas();
  else document.addEventListener('DOMContentLoaded', vigilarCapas, {{ once: true }});

  // ── Atajos de teclado ───────────────────────────────────────
  // Se capturan en fase de captura porque WhatsApp Web se come muchas teclas.
  document.addEventListener('keydown', (ev) => {{
    const ctrl = ev.ctrlKey || ev.metaKey;
    if (ev.key === 'F5') {{ ev.preventDefault(); location.reload(); return; }}
    if (!ctrl) return;

    if (ev.key >= '1' && ev.key <= '9') {{
      const acc = st().accounts[Number(ev.key) - 1];
      if (acc) {{ ev.preventDefault(); go('switch/' + acc.id); }}
      return;
    }}
    switch (ev.key.toLowerCase()) {{
      case 'p': ev.preventDefault(); go('settings'); break;
      case 'u': ev.preventDefault(); go('add'); break;
      case 'w': ev.preventDefault(); go('hide'); break;
      case 'q': ev.preventDefault(); go('quit'); break;
      case '+': case '=': ev.preventDefault(); go('zoom/in'); break;
      case '-': ev.preventDefault(); go('zoom/out'); break;
      case '0': ev.preventDefault(); go('zoom/reset'); break;
    }}
  }}, true);

  // ── Notificaciones ──────────────────────────────────────────
  // Las notificaciones **no** se interceptan aquí: las entrega WebKit por su
  // señal nativa, que cubre tanto las que nacen en la página como las del
  // service worker (la vía real de WhatsApp, a la que este script no llega).
  // Lo único que hace falta desde JavaScript es que la página crea que tiene
  // permiso, para que no se quede esperando a pedirlo.
  // El permiso no se puede falsear (`Notification.permission` es de solo
  // lectura) y, sin permiso real, WebKit descarta la notificación antes de
  // entregárnosla. Además
  // **exige un gesto del usuario**: pedirlo al cargar
  // devuelve «denied» sin llegar siquiera a consultar a la aplicación. Por eso
  // se pide en la primera interacción real; Rust lo concede sin preguntar, así
  // que el usuario no ve ningún diálogo.
  if (window.__wruspSelf !== 'settings' && window.Notification) {{
    const quitar = () => {{
      document.removeEventListener('click', pedirPermiso, true);
      document.removeEventListener('keydown', pedirPermiso, true);
    }};
    const pedirPermiso = () => {{
      try {{
        if (Notification.permission !== 'default') {{ quitar(); return; }}
        const r = Notification.requestPermission();
        if (r && r.then) {{
          r.then(() => {{ if (Notification.permission !== 'default') quitar(); }});
        }}
      }} catch (e) {{ quitar(); }}
    }};
    if (Notification.permission === 'default') {{
      document.addEventListener('click', pedirPermiso, true);
      document.addEventListener('keydown', pedirPermiso, true);
    }}
  }}

  // ── Contador de no leídos ───────────────────────────────────
  // WhatsApp Web antepone «(3)» al título del documento. Solo se avisa a Rust
  // cuando el número cambia, así que el canal se usa muy de vez en cuando.
  if (window.__wruspSelf !== 'settings') {{
    let last = -1;
    const report = () => {{
      const m = /^\(?(\d+)\)/.exec(document.title || '');
      const n = m ? Number(m[1]) : 0;
      if (n === last) return;
      last = n;
      // Pinta ya la insignia propia; Rust confirmará el estado global después.
      const s = window.__wruspState;
      const mine = s && s.accounts.find((a) => a.id === window.__wruspSelf);
      if (mine) {{ mine.unread = n; render(); }}
      go('unread/' + window.__wruspSelf + '/' + n);
    }};
    // Este script corre ANTES de que se analice el HTML, así que aquí todavía
    // no existe <title>: hay que engancharse cuando aparezca (y de nuevo si la
    // SPA lo reemplaza). El sondeo es la red de seguridad.
    let observed = null;
    const attach = () => {{
      const node = document.querySelector('title');
      if (node && node !== observed) {{
        observed = node;
        new MutationObserver(report).observe(node, {{
          childList: true,
          characterData: true,
          subtree: true,
        }});
      }}
    }};
    const tick = () => {{ attach(); report(); }};
    document.addEventListener('DOMContentLoaded', tick);
    setInterval(tick, 1500);
  }}
}})();"#,
        own = own
    )
}
