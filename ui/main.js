// Wrusp — lógica de la ventana de gestión (sin frameworks).
// Habla con el backend Rust vía window.__TAURI__ (withGlobalTauri).

const { invoke } = window.__TAURI__.core;

// ── Avisos flotantes ────────────────────────────────────
// Hasta ahora, guardar una carpeta o cambiar el tema no decía nada: solo se
// veía texto cuando algo fallaba, así que no había forma de distinguir «hecho»
// de «no ha pasado nada».
const TOAST_MS = 2600;

function aviso(texto, tipo = "ok") {
  const zona = document.getElementById("toasts");
  if (!zona) return;
  const nodo = document.createElement("div");
  nodo.className = `toast ${tipo}`;
  nodo.textContent = texto;
  zona.appendChild(nodo);
  // Dos marcos para que la transición de entrada tenga de dónde salir.
  requestAnimationFrame(() => requestAnimationFrame(() => nodo.classList.add("visible")));
  setTimeout(() => {
    nodo.classList.remove("visible");
    // Se quita al acabar la transición, y también si no la hubo.
    nodo.addEventListener("transitionend", () => nodo.remove(), { once: true });
    setTimeout(() => nodo.remove(), 400);
  }, TOAST_MS);
}

const listEl = document.getElementById("account-list");
const emptyEl = document.getElementById("empty");
const formEl = document.getElementById("add-form");
const nameEl = document.getElementById("add-name");
const themeButtons = document.querySelectorAll(".theme-switch button");

// ── Menú de secciones ───────────────────────────────────
// Los ajustes eran una sola columna con todo apilado y medio escondido en
// <details>; ahora cada cosa vive en su panel y el menú decide cuál se ve. La
// elección se recuerda para que volver a ajustes no obligue a buscar otra vez.
const navButtons = document.querySelectorAll(".nav button");
const PANEL_POR_DEFECTO = "cuentas";

function showPanel(name) {
  let encontrado = false;
  for (const btn of navButtons) {
    const activo = btn.dataset.panel === name;
    encontrado = encontrado || activo;
    btn.classList.toggle("active", activo);
    btn.setAttribute("aria-selected", String(activo));
    document.getElementById(`panel-${btn.dataset.panel}`).hidden = !activo;
  }
  if (!encontrado) return showPanel(PANEL_POR_DEFECTO);
  try {
    localStorage.setItem("wrusp-panel", name);
  } catch {
    // Sin almacenamiento se pierde el recuerdo, no la navegación.
  }
}

navButtons.forEach((btn) =>
  btn.addEventListener("click", () => showPanel(btn.dataset.panel))
);

const media = window.matchMedia("(prefers-color-scheme: dark)");
let themeMode = "system";

// ── Tema ────────────────────────────────────────────────
function applyThemeLocally() {
  const effective =
    themeMode === "system" ? (media.matches ? "dark" : "light") : themeMode;
  document.documentElement.dataset.theme = effective;
  themeButtons.forEach((btn) =>
    btn.classList.toggle("active", btn.dataset.mode === themeMode)
  );
}

media.addEventListener("change", () => {
  if (themeMode === "system") applyThemeLocally();
});

themeButtons.forEach((btn) => {
  btn.addEventListener("click", async () => {
    themeMode = btn.dataset.mode;
    applyThemeLocally();
    try {
      await invoke("set_theme", { theme: themeMode });
      const comoSeLlama = { system: "automático", light: "claro", dark: "oscuro" };
      aviso(`Tema ${comoSeLlama[themeMode] || themeMode} aplicado`);
    } catch (err) {
      console.error("set_theme:", err);
      aviso("No se pudo cambiar el tema", "error");
    }
  });
});

// ── Icono de la aplicación ──────────────────────────────
const iconGrid = document.getElementById("icon-grid");
const iconSearch = document.getElementById("icon-search");
const iconCurrent = document.getElementById("icon-current");
let iconNames = [];
let selectedIcon = "";

function renderIconGrid() {
  const query = iconSearch.value.trim().toLowerCase();
  const visible = query
    ? iconNames.filter((n) => n.includes(query))
    : iconNames;
  iconGrid.replaceChildren(
    ...visible.map((name) => {
      const btn = document.createElement("button");
      btn.title = name;
      btn.classList.toggle("active", name === selectedIcon);
      const img = document.createElement("img");
      img.loading = "lazy";
      img.src = `appicons/${name}.svg`;
      img.alt = name;
      btn.appendChild(img);
      btn.addEventListener("click", async () => {
        try {
          await invoke("set_app_icon", { name });
          selectedIcon = name;
          iconCurrent.src = `appicons/${name}.svg`;
          renderIconGrid();
          aviso("Icono aplicado");
        } catch (err) {
          console.error("set_app_icon:", err);
          aviso("No se pudo aplicar el icono", "error");
        }
      });
      return btn;
    })
  );
}

async function initIconPicker() {
  try {
    iconNames = await (await fetch("appicons/manifest.json")).json();
    // El catálogo tiene que ser una lista. Si llega cualquier otra cosa,
    // `renderIconGrid` revienta fuera de este try y se lleva por delante lo
    // que el arranque hace después: carpetas, interruptores, «Acerca de» y
    // diagnóstico se quedaban sin inicializar, en silencio.
    if (!Array.isArray(iconNames)) throw new Error("el catálogo no es una lista");
    selectedIcon = await invoke("get_app_icon");
    iconCurrent.src = `appicons/${selectedIcon}.svg`;
    iconSearch.addEventListener("input", renderIconGrid);
    renderIconGrid();
  } catch (err) {
    iconNames = [];
    console.error("icon picker:", err);
  }
}

// ── Carpetas (descargas y temporales) ───────────────────
const folderError = document.getElementById("folder-error");

const FOLDER_FIELDS = [
  { id: "download-dir", command: "set_download_dir", nombre: "descargas" },
  { id: "temp-dir", command: "set_temp_dir", nombre: "temporales" },
  { id: "log-dir", command: "set_log_dir", nombre: "registros" },
];

function showFolderError(message) {
  folderError.textContent = message;
  folderError.hidden = !message;
}

async function saveFolder(field, value) {
  try {
    await invoke(field.command, { path: value.trim() });
    showFolderError("");
    aviso(`Carpeta de ${field.nombre} guardada`);
    return true;
  } catch (err) {
    showFolderError(String(err));
    aviso(`No se pudo guardar la carpeta de ${field.nombre}`, "error");
    return false;
  }
}

async function initFolders() {
  let folders;
  try {
    folders = await invoke("get_folders");
  } catch (err) {
    console.error("get_folders:", err);
    return;
  }

  const defaults = {
    "download-dir": folders.downloadDefault,
    "temp-dir": folders.tempDefault,
    "log-dir": folders.logDefault,
  };
  const values = {
    "download-dir": folders.downloadDir,
    "temp-dir": folders.tempDir,
    "log-dir": folders.logDir,
  };

  // El registro se consulta abriendo su carpeta en el gestor de ficheros.
  document.getElementById("open-logs").addEventListener("click", async () => {
    try {
      await invoke("open_log_dir");
    } catch (err) {
      showFolderError(String(err));
    }
  });

  for (const field of FOLDER_FIELDS) {
    const input = document.getElementById(field.id);
    input.value = values[field.id] || "";
    input.placeholder = defaults[field.id] || "";

    // Se guarda al salir del campo, no en cada tecla: validar rutas a medio
    // escribir solo produce errores molestos.
    input.addEventListener("blur", () => saveFolder(field, input.value));
    input.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") input.blur();
    });

    const button = document.querySelector(`[data-pick="${field.id}"]`);
    button.addEventListener("click", async () => {
      let picked;
      try {
        picked = await invoke("pick_folder");
      } catch (err) {
        console.error("pick_folder:", err);
        return;
      }
      if (!picked) return;
      input.value = picked;
      await saveFolder(field, picked);
    });
  }
}

// ── Interruptores de comportamiento ─────────────────────
async function initToggles() {
  let toggles;
  try {
    toggles = await invoke("get_toggles");
  } catch (err) {
    console.error("get_toggles:", err);
    return;
  }

  for (const input of document.querySelectorAll("[data-toggle]")) {
    const name = input.dataset.toggle;
    input.checked = Boolean(toggles[name]);
    input.addEventListener("change", async () => {
      try {
        await invoke("set_toggle", { name, enabled: input.checked });
      } catch (err) {
        console.error("set_toggle:", err);
        // El ajuste no se guardó: la casilla vuelve a donde estaba, o
        // enseñaría un estado que no es el de verdad (UX-01).
        input.checked = !input.checked;
        aviso("No se pudo guardar el ajuste: " + err, "error");
      }
    });
  }
}

// ── Acerca de ───────────────────────────────────────────
/**
 * Compara dos versiones y dice si `a` es más nueva que `b`.
 *
 * SemVer, no tres números sueltos: una publicación de prueba como
 * `0.5.0-rc.1` es **anterior** a `0.5.0`, y comparando solo los tres primeros
 * números las dos empataban y una beta se ofrecía como si fuera la final.
 * Lo que va detrás de `+` es metadato de compilación y no cuenta para nada.
 */
function esMayor(a, b) {
  const partes = (v) => {
    const [nucleo, previa = ""] = String(v).split("+")[0].split("-", 2);
    const numeros = nucleo.split(".").map((n) => parseInt(n, 10) || 0);
    return { numeros, previa };
  };
  const va = partes(a);
  const vb = partes(b);
  for (let i = 0; i < 3; i++) {
    const na = va.numeros[i] || 0;
    const nb = vb.numeros[i] || 0;
    if (na !== nb) return na > nb;
  }
  // Mismo número: manda la etiqueta de prueba. Sin etiqueta es la definitiva,
  // y una definitiva gana a cualquier `-rc`, `-beta`…
  if (va.previa === vb.previa) return false;
  if (!va.previa) return true;
  if (!vb.previa) return false;
  // Dos etiquetas: se comparan campo a campo, los numéricos como números.
  const ca = va.previa.split(".");
  const cb = vb.previa.split(".");
  for (let i = 0; i < Math.max(ca.length, cb.length); i++) {
    const xa = ca[i];
    const xb = cb[i];
    if (xa === undefined) return false; // la más corta es la anterior
    if (xb === undefined) return true;
    if (xa === xb) continue;
    const na = /^\d+$/.test(xa);
    const nb = /^\d+$/.test(xb);
    if (na && nb) return Number(xa) > Number(xb);
    if (na !== nb) return !na; // un campo numérico va antes que uno de texto
    return xa > xb;
  }
  return false;
}

/** Arquitecturas y los nombres con que aparecen en los ficheros publicados. */
const ARQUITECTURAS = {
  x86_64: ["x86_64", "x64", "amd64"],
  aarch64: ["aarch64", "arm64"],
};

/**
 * El fichero de la versión que le sirve a este sistema, o null si no hay.
 *
 * `sufijo` lo decide Rust leyendo la distribución (ver `package_suffix`).
 * Cada versión publica nueve ficheros y varios pueden compartir extensión
 * entre arquitecturas, así que uno que anuncie una arquitectura distinta de
 * la nuestra se descarta: es peor bajar el paquete equivocado que no bajar
 * ninguno.
 */
function elegirPaquete(assets, sufijo, arquitectura) {
  if (!sufijo) return null;
  const mias = ARQUITECTURAS[arquitectura] || [];
  const ajenas = Object.entries(ARQUITECTURAS)
    .filter(([arch]) => arch !== arquitectura)
    .flatMap(([, nombres]) => nombres);
  const candidatos = assets.filter((a) => a.name && a.name.endsWith(sufijo));
  const nombra = (nombre, lista) =>
    lista.some((token) => nombre.toLowerCase().includes(token));
  return (
    candidatos.find((a) => nombra(a.name, mias)) ||
    candidatos.find((a) => !nombra(a.name, ajenas)) ||
    null
  );
}

/** Tamaño en bytes como lo lee una persona. */
function tamano(bytes) {
  if (!bytes) return "tamaño desconocido";
  const mib = bytes / (1024 * 1024);
  return `${mib.toFixed(1).replace(".", ",")} MiB`;
}

async function initAbout() {
  let about;
  try {
    about = await invoke("get_about");
  } catch (err) {
    console.error("get_about:", err);
    return;
  }
  document.getElementById("about-version").textContent = about.version;

  for (const btn of document.querySelectorAll("[data-link]")) {
    btn.addEventListener("click", () => {
      invoke("open_external", { url: about[btn.dataset.link] }).catch((err) =>
        console.error("open_external:", err)
      );
    });
  }

  const estado = document.getElementById("about-update");
  const prompt = document.getElementById("update-prompt");
  const titulo = document.getElementById("update-title");
  const detalle = document.getElementById("update-detail");
  const botones = document.getElementById("update-download").parentElement;

  const abrir = (url) =>
    invoke("open_external", { url }).catch((err) => {
      estado.textContent = "· no se pudo abrir el navegador";
      estado.className = "about-update error";
      console.error("open_external:", err);
    });

  document.getElementById("update-later").addEventListener("click", () => {
    prompt.hidden = true;
  });

  document.getElementById("check-updates").addEventListener("click", async () => {
    estado.textContent = "· comprobando…";
    estado.className = "about-update";
    prompt.hidden = true;
    botones.hidden = false;
    try {
      // Se consulta desde aquí y no desde Rust para no arrastrar un cliente
      // HTTP al binario solo para esto.
      const r = await fetch(
        "https://api.github.com/repos/Aleixenandros/Wrusp/releases/latest",
        { headers: { Accept: "application/vnd.github+json" } }
      );
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      const release = await r.json();
      const ultima = release.tag_name.replace(/^v/, "");
      if (!esMayor(ultima, about.version)) {
        estado.textContent = "· estás al día";
        return;
      }
      // Hay versión nueva: se avisa y se pregunta. La descarga no empieza
      // sola, ni aquí ni en el navegador, hasta que se pulse «Descargar».
      estado.textContent = `· hay una versión ${ultima}`;
      estado.className = "about-update nueva";
      const paquete = elegirPaquete(release.assets || [], about.packageSuffix, about.arch);
      titulo.textContent = `Wrusp ${ultima} ya está disponible. ¿Quieres descargarla?`;
      detalle.textContent = paquete
        ? `Se descargará ${paquete.name} (${tamano(paquete.size)}), que es el paquete de este sistema. La descarga la hace tu navegador.`
        : "No hay un paquete reconocible para este sistema, así que se abrirá la lista de descargas de la versión.";
      const notas = release.html_url || about.releases;
      document.getElementById("update-notes").onclick = () => abrir(notas);
      document.getElementById("update-download").onclick = async () => {
        await abrir(paquete ? paquete.browser_download_url : about.releases);
        // La descarga vive ya en el navegador: aquí solo queda decir qué se
        // ha pedido y cómo termina de instalarse.
        botones.hidden = true;
        detalle.textContent = paquete
          ? `Descarga abierta en el navegador: ${paquete.name}. Cuando termine, ábrela para instalar la versión nueva.`
          : "Se ha abierto en el navegador la lista de descargas de la versión.";
      };
      prompt.hidden = false;
    } catch (err) {
      estado.textContent = "· no se pudo comprobar";
      estado.className = "about-update error";
      console.error("comprobar actualizaciones:", err);
    }
  });
}

// ── Cuentas ─────────────────────────────────────────────
const ACCOUNT_COLORS = [
  "",
  "#1fa855",
  "#0ea5e9",
  "#6366f1",
  "#a855f7",
  "#ec4899",
  "#f59e0b",
  "#ef4444",
];

function initials(name) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0].toUpperCase())
    .join("");
}

function accountRow(account, index, allAccounts) {
  const li = document.createElement("li");
  li.className = "account";

  const reorderDiv = document.createElement("div");
  reorderDiv.className = "reorder-btns";

  const upBtn = document.createElement("button");
  upBtn.type = "button";
  upBtn.className = "icon-btn reorder-btn";
  upBtn.title = "Subir";
  upBtn.disabled = index === 0;
  upBtn.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M18 15l-6-6-6 6"/></svg>';
  upBtn.addEventListener("click", async () => {
    const newOrder = allAccounts.map((a) => a.id);
    [newOrder[index - 1], newOrder[index]] = [newOrder[index], newOrder[index - 1]];
    try {
      await invoke("reorder_accounts", { ids: newOrder });
      await refresh();
    } catch (err) {
      console.error("reorder_accounts:", err);
    }
  });

  const downBtn = document.createElement("button");
  downBtn.type = "button";
  downBtn.className = "icon-btn reorder-btn";
  downBtn.title = "Bajar";
  downBtn.disabled = index === allAccounts.length - 1;
  downBtn.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>';
  downBtn.addEventListener("click", async () => {
    const newOrder = allAccounts.map((a) => a.id);
    [newOrder[index + 1], newOrder[index]] = [newOrder[index], newOrder[index + 1]];
    try {
      await invoke("reorder_accounts", { ids: newOrder });
      await refresh();
    } catch (err) {
      console.error("reorder_accounts:", err);
    }
  });
  reorderDiv.append(upBtn, downBtn);

  const avatar = document.createElement("div");
  avatar.className = "avatar";
  avatar.textContent = initials(account.name) || "?";
  if (account.color) {
    avatar.style.backgroundColor = account.color;
  }

  const name = document.createElement("div");
  name.className = "name";
  name.textContent = account.name;
  name.title = "Doble clic para renombrar";

  const actions = document.createElement("div");
  actions.className = "actions";

  // Botón para rotar color de acento
  const colorBtn = document.createElement("button");
  colorBtn.type = "button";
  colorBtn.className = "icon-btn color-btn";
  colorBtn.title = "Cambiar color de acento";
  colorBtn.style.backgroundColor = account.color || "var(--accent)";
  colorBtn.addEventListener("click", async () => {
    const currentIdx = ACCOUNT_COLORS.indexOf(account.color || "");
    const nextColor = ACCOUNT_COLORS[(currentIdx + 1) % ACCOUNT_COLORS.length];
    try {
      await invoke("set_account_color", { id: account.id, color: nextColor || null });
      await refresh();
    } catch (err) {
      console.error("set_account_color:", err);
    }
  });

  // Botón para silenciar
  const muteBtn = document.createElement("button");
  muteBtn.type = "button";
  muteBtn.className = "icon-btn mute-btn" + (account.muted ? " active" : "");
  muteBtn.title = account.muted ? "Desactivar silencio" : "Silenciar notificaciones";
  muteBtn.innerHTML = account.muted
    ? '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M11 5L6 9H2v6h4l5 4V5zM23 9l-6 6M17 9l6 6"/></svg>'
    : '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M11 5L6 9H2v6h4l5 4V5zM19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07"/></svg>';
  muteBtn.addEventListener("click", async () => {
    try {
      await invoke("set_account_muted", { id: account.id, muted: !account.muted });
      await refresh();
    } catch (err) {
      console.error("set_account_muted:", err);
    }
  });

  const openBtn = document.createElement("button");
  openBtn.type = "button";
  openBtn.className = "open";
  openBtn.textContent = "Abrir";
  openBtn.addEventListener("click", async () => {
    try {
      await invoke("open_account", { id: account.id });
    } catch (err) {
      console.error("open_account:", err);
    }
  });

  // Borrado en dos pasos para no depender de diálogos nativos.
  const delBtn = document.createElement("button");
  delBtn.type = "button";
  delBtn.className = "danger";
  delBtn.textContent = "Borrar";
  let armed = false;
  delBtn.addEventListener("click", async () => {
    if (!armed) {
      armed = true;
      delBtn.textContent = "¿Seguro?";
      setTimeout(() => {
        armed = false;
        delBtn.textContent = "Borrar";
      }, 3000);
      return;
    }
    try {
      await invoke("remove_account", { id: account.id });
    } catch (err) {
      console.error("remove_account:", err);
    }
    await refresh();
  });

  // Renombrar con doble clic sobre el nombre.
  name.addEventListener("dblclick", () => {
    const input = document.createElement("input");
    input.type = "text";
    input.className = "rename";
    input.maxLength = 40;
    input.value = account.name;
    li.replaceChild(input, name);
    input.focus();
    input.select();

    const commit = async () => {
      const value = input.value.trim();
      if (value && value !== account.name) {
        try {
          await invoke("rename_account", { id: account.id, name: value });
        } catch (err) {
          console.error("rename_account:", err);
        }
      }
      await refresh();
    };
    input.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") input.blur();
      if (ev.key === "Escape") {
        input.value = account.name;
        input.blur();
      }
    });
    input.addEventListener("blur", commit, { once: true });
  });

  actions.append(colorBtn, muteBtn, openBtn, delBtn);
  li.append(reorderDiv, avatar, name, actions);
  return li;
}

async function refresh() {
  try {
    const accounts = await invoke("list_accounts");
    listEl.replaceChildren(...accounts.map((acc, idx) => accountRow(acc, idx, accounts)));
    emptyEl.hidden = accounts.length > 0;
  } catch (err) {
    console.error("list_accounts:", err);
  }
}

// ── Diagnóstico ─────────────────────────────────────────
function formatBytes(bytes) {
  if (!bytes || bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return (bytes / Math.pow(k, i)).toFixed(1) + " " + sizes[i];
}

async function loadDiagnostics() {
  try {
    const diag = await invoke("get_diagnostics");
    const h264El = document.getElementById("diag-h264");
    const h264DetailEl = document.getElementById("diag-h264-detail");
    if (diag.hasH264Decoder) {
      h264El.textContent = "Disponible";
      h264El.className = "diag-status ok";
      h264DetailEl.textContent = diag.h264DecoderName;
    } else {
      h264El.textContent = "No disponible";
      h264El.className = "diag-status error";
      h264DetailEl.textContent = "Instala gstreamer1-plugin-libav / gstreamer1.0-libav";
    }

    const aacEl = document.getElementById("diag-aac");
    if (diag.hasAacDecoder) {
      aacEl.textContent = "Disponible";
      aacEl.className = "diag-status ok";
    } else {
      aacEl.textContent = "Integrado o nativo";
      aacEl.className = "diag-status ok";
    }

    document.getElementById("diag-webkit").textContent = diag.webkitVersion;
    document.getElementById("diag-os").textContent = diag.osInfo;
    document.getElementById("diag-profiles").textContent = formatBytes(diag.profilesSize);
    document.getElementById("diag-logs").textContent = formatBytes(diag.logSize);
  } catch (err) {
    console.error("get_diagnostics:", err);
  }
}

// ── Visor del registro ──────────────────────────────────
// Antes había que salir de Wrusp, abrir la carpeta y buscar `wrusp.log` en un
// editor. Se leen solo las últimas líneas: el fichero llega a varios megas.
const LOG_LINEAS = 300;

/** Qué deja pasar cada filtro. Se comparan en minúsculas. */
const LOG_FILTROS = {
  todo: () => true,
  wrusp: (l) => l.includes("wrusp:"),
  medios: (l) =>
    /v[íi]deo|medio|gstreamer|qtdemux|h264|avdec|codec|c[óo]dec|blob|data:|mediaload/.test(l),
  red: (l) => /websocket|csp|content security|network|http|connect|tls/.test(l),
  errores: (l) => /error|fallo|critical|warning|panic|refused/.test(l),
};

let logCrudo = "";

function pintarRegistro() {
  const salida = document.getElementById("log-lines");
  const resumen = document.getElementById("log-summary");
  const filtro = LOG_FILTROS[document.getElementById("log-filter").value] || LOG_FILTROS.todo;
  const busqueda = document.getElementById("log-search").value.trim().toLowerCase();

  const todas = logCrudo ? logCrudo.split("\n") : [];
  const mostradas = todas.filter((linea) => {
    const minuscula = linea.toLowerCase();
    return filtro(minuscula) && (!busqueda || minuscula.includes(busqueda));
  });
  // `textContent`, nunca `innerHTML`: el registro trae texto de WhatsApp y de
  // la consola de la página, y no tiene por qué interpretarse como marcado.
  salida.textContent = mostradas.join("\n") || "(nada que coincida)";
  salida.scrollTop = salida.scrollHeight;
  resumen.textContent = todas.length
    ? `Mostrando ${mostradas.length} de las últimas ${todas.length} líneas.`
    : "El registro está vacío.";
}

async function cargarRegistro() {
  const resumen = document.getElementById("log-summary");
  resumen.textContent = "Leyendo…";
  try {
    logCrudo = await invoke("get_log_tail", { lines: LOG_LINEAS });
    pintarRegistro();
  } catch (err) {
    logCrudo = "";
    resumen.textContent = "No se pudo leer el registro: " + err;
    console.error("get_log_tail:", err);
  }
}

function initDiagnostics() {
  document.getElementById("refresh-diagnostics").addEventListener("click", async () => {
    await loadDiagnostics();
    aviso("Diagnóstico actualizado");
  });
  const statusEl = document.getElementById("diag-action-status");
  document.getElementById("clear-gst-cache").addEventListener("click", async () => {
    statusEl.textContent = "Limpiando…";
    try {
      await invoke("clear_gstreamer_cache");
      statusEl.textContent = "Caché de GStreamer borrada con éxito.";
      statusEl.style.color = "var(--accent)";
      aviso("Caché de GStreamer borrada");
      await loadDiagnostics();
    } catch (err) {
      statusEl.textContent = "Error: " + err;
      statusEl.style.color = "var(--danger)";
      aviso("No se pudo borrar la caché", "error");
    }
  });

  document.getElementById("copy-report").addEventListener("click", async () => {
    try {
      const informe = await invoke("diagnostic_report");
      await invoke("copy_text", { text: informe });
      aviso("Informe copiado: pégalo en la incidencia");
    } catch (err) {
      aviso("No se pudo copiar el informe", "error");
      console.error("diagnostic_report:", err);
    }
  });

  // El registro se lee al desplegarlo, no al abrir ajustes: leer megas de
  // fichero para algo que casi nadie mira es trabajo por nada.
  const visor = document.getElementById("log-viewer");
  visor.addEventListener("toggle", () => {
    if (visor.open && !logCrudo) cargarRegistro();
  });
  document.getElementById("log-refresh").addEventListener("click", cargarRegistro);
  document.getElementById("log-filter").addEventListener("change", pintarRegistro);
  document.getElementById("log-search").addEventListener("input", pintarRegistro);
  document.getElementById("log-copy").addEventListener("click", async () => {
    try {
      await invoke("copy_text", { text: document.getElementById("log-lines").textContent });
      aviso("Registro copiado al portapapeles");
    } catch (err) {
      aviso("No se pudo copiar el registro", "error");
      console.error("copy_text:", err);
    }
  });
}

// Lo llama Rust cuando se pulsa «+» en la barra lateral. El campo puede estar
// en un panel que no se ve, así que primero se enseña el suyo. Si la vista se
// acaba de crear, la orden llega antes que este script y solo deja la marca
// `__wruspQuiereAlta`, que se atiende al arrancar (ver `init`).
window.__wruspFocusAdd = () => {
  window.__wruspQuiereAlta = false;
  showPanel("cuentas");
  nameEl.focus();
  nameEl.select();
};

formEl.addEventListener("submit", async (ev) => {
  ev.preventDefault();
  const name = nameEl.value.trim();
  if (!name) return;
  try {
    await invoke("add_account", { name });
    nameEl.value = "";
    await refresh();
  } catch (err) {
    console.error("add_account:", err);
  }
});

// ── Arranque ────────────────────────────────────────────
(async function init() {
  let recordado = null;
  try {
    recordado = localStorage.getItem("wrusp-panel");
  } catch {
    recordado = null;
  }
  showPanel(recordado || PANEL_POR_DEFECTO);
  if (window.__wruspQuiereAlta) window.__wruspFocusAdd();

  try {
    themeMode = await invoke("get_theme");
  } catch {
    themeMode = "system";
  }
  applyThemeLocally();
  await initIconPicker();
  await initFolders();
  await initToggles();
  await initAbout();
  initDiagnostics();
  await loadDiagnostics();
  await refresh();
})();
