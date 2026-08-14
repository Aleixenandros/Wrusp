//! Insignia de no leídos dibujada sobre el icono de la aplicación.
//!
//! Se compone a mano sobre el búfer RGBA en vez de usar una biblioteca de
//! dibujo: hacen falta un círculo y como mucho dos cifras, y eso no justifica
//! arrastrar un rasterizador de fuentes al binario.

use tauri::image::Image;

/// Rojo de aviso. **No** el naranja del icono: con el mismo color la insignia
/// se confunde con el logotipo y deja de leerse como un aviso.
const BADGE_RGB: [u8; 3] = [0xD3, 0x2F, 0x2F];
const TEXT_RGB: [u8; 3] = [0xFF, 0xFF, 0xFF];
/// Anillo que separa la insignia del icono, sea cual sea el icono elegido.
const RING_RGB: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// Dígitos en una retícula de 3×5. Cada `u8` es una fila de 3 bits.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111], // 0
    [0b010, 0b110, 0b010, 0b010, 0b111], // 1
    [0b111, 0b001, 0b111, 0b100, 0b111], // 2
    [0b111, 0b001, 0b111, 0b001, 0b111], // 3
    [0b101, 0b101, 0b111, 0b001, 0b001], // 4
    [0b111, 0b100, 0b111, 0b001, 0b111], // 5
    [0b111, 0b100, 0b111, 0b101, 0b111], // 6
    [0b111, 0b001, 0b010, 0b010, 0b010], // 7
    [0b111, 0b101, 0b111, 0b101, 0b111], // 8
    [0b111, 0b101, 0b111, 0b001, 0b111], // 9
];

/// Mezcla un color sobre el píxel `(x, y)` con la cobertura dada (0.0–1.0).
fn blend(rgba: &mut [u8], width: u32, x: i64, y: i64, color: [u8; 3], coverage: f32) {
    if x < 0 || y < 0 || coverage <= 0.0 {
        return;
    }
    let idx = ((y as u32 * width + x as u32) * 4) as usize;
    if idx + 3 >= rgba.len() {
        return;
    }
    let a = coverage.clamp(0.0, 1.0);
    for c in 0..3 {
        let dst = rgba[idx + c] as f32;
        rgba[idx + c] = (dst + (color[c] as f32 - dst) * a).round() as u8;
    }
    // El icono puede ser transparente ahí debajo: la insignia es opaca.
    let dst_a = rgba[idx + 3] as f32;
    rgba[idx + 3] = (dst_a + (255.0 - dst_a) * a).round() as u8;
}

/// Devuelve el icono con una insignia si hay mensajes sin leer.
///
/// Con más de 99 se dibuja solo el círculo: dos cifras es lo que cabe legible
/// en un icono de bandeja.
pub fn with_unread(icon: &Image<'_>, unread: u32) -> Option<Image<'static>> {
    if unread == 0 {
        return None;
    }
    let (w, h) = (icon.width(), icon.height());
    if w < 16 || h < 16 {
        return None;
    }
    let mut rgba = icon.rgba().to_vec();

    // Círculo abajo a la derecha, con su anillo por fuera.
    let ring = (w.min(h) as f32 * 0.03).max(1.0);
    let radius = (w.min(h) as f32) * 0.22;
    let outer = radius + ring;
    let cx = w as f32 - outer - 1.0;
    let cy = h as f32 - outer - 1.0;

    let y0 = (cy - outer - 1.0).max(0.0) as i64;
    let y1 = (cy + outer + 1.0).min(h as f32) as i64;
    let x0 = (cx - outer - 1.0).max(0.0) as i64;
    let x1 = (cx + outer + 1.0).min(w as f32) as i64;

    for y in y0..y1 {
        for x in x0..x1 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            // Cobertura suavizada en los bordes para que no queden dentados.
            blend(
                &mut rgba,
                w,
                x,
                y,
                RING_RGB,
                (outer - d + 0.5).clamp(0.0, 1.0),
            );
            blend(
                &mut rgba,
                w,
                x,
                y,
                BADGE_RGB,
                (radius - d + 0.5).clamp(0.0, 1.0),
            );
        }
    }

    if unread <= 99 {
        draw_number(&mut rgba, w, unread, cx, cy, radius);
    }

    Some(Image::new_owned(rgba, w, h))
}

/// Dibuja el número centrado en el círculo, con píxeles cuadrados.
fn draw_number(rgba: &mut [u8], width: u32, number: u32, cx: f32, cy: f32, radius: f32) {
    let text = number.to_string();
    let digits: Vec<usize> = text
        .chars()
        .filter_map(|c| c.to_digit(10).map(|d| d as usize))
        .collect();

    // Escala para que las cifras ocupen ~70 % del diámetro.
    let cols = digits.len() as f32 * 3.0 + (digits.len() as f32 - 1.0);
    let scale = ((radius * 2.0 * 0.72) / cols.max(5.0)).floor().max(1.0);

    let text_w = cols * scale;
    let text_h = 5.0 * scale;
    let start_x = cx - text_w / 2.0;
    let start_y = cy - text_h / 2.0;

    for (i, digit) in digits.iter().enumerate() {
        let glyph = DIGITS[*digit];
        let ox = start_x + i as f32 * 4.0 * scale;
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..3 {
                if bits & (1 << (2 - col)) == 0 {
                    continue;
                }
                let px = ox + col as f32 * scale;
                let py = start_y + row as f32 * scale;
                for sy in 0..scale as i64 {
                    for sx in 0..scale as i64 {
                        blend(rgba, width, px as i64 + sx, py as i64 + sy, TEXT_RGB, 1.0);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Genera muestras del icono con insignia para revisarlas a ojo.
    /// `cargo test -- --ignored genera_muestras`
    #[test]
    #[ignore]
    fn genera_muestras() {
        let base = Image::from_path("icons/icon.png").expect("icono base");
        for (n, escala) in [
            (1u32, 512u32),
            (7, 512),
            (42, 512),
            (99, 512),
            (150, 512),
            (3, 32),
        ] {
            let src = if escala == 512 {
                base.clone()
            } else {
                Image::from_path("icons/32x32.png").expect("icono 32")
            };
            let con = with_unread(&src, n).expect("insignia");
            let ruta = format!("/tmp/badge-{n}-{}.png", con.width());
            let file = std::fs::File::create(&ruta).unwrap();
            let mut enc =
                png::Encoder::new(std::io::BufWriter::new(file), con.width(), con.height());
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()
                .unwrap()
                .write_image_data(con.rgba())
                .unwrap();
            println!("escrito {ruta}");
        }
    }

    #[test]
    fn sin_no_leidos_no_hay_insignia() {
        let base = Image::from_path("icons/icon.png").expect("icono base");
        assert!(with_unread(&base, 0).is_none());
    }
}
