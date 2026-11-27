//! Backend ESC/POS: traduce el mismo documento a bytes de comandos de la impresora. No tiene páginas,
//! así que no puede cortar un ticket por largo. Los bytes se mandan como trabajo RAW por el spooler
//! de Windows (ver `print_escpos`), en la misma cola del driver instalado.

use crate::doc::{Align, Ecc, Item, Ticket};
use crate::qr::{QrMatrix, QUIET_ZONE};

const ESC: u8 = 0x1B;
const GS: u8 = 0x1D;

/// Página de códigos para acentos y ñ. El número de `ESC t` es el habitual en impresoras
/// compatibles con Epson; si una impresora usa otro, se cambia en [`EscPosOptions::codepage_cmd`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodePage {
    /// PC850 (Multilingual Latin 1). `ESC t 2`.
    Pc850,
}

/// Opciones del backend ESC/POS.
#[derive(Debug, Clone, PartialEq)]
pub struct EscPosOptions {
    /// Caracteres por línea con la fuente A. 48 en papel de 80 mm, 32 en 58 mm.
    pub cols: usize,
    /// Puntos por mm (8 a 203 DPI).
    pub dots_per_mm: f32,
    /// Página de códigos para convertir el texto.
    pub codepage: CodePage,
    /// Valor de `n` en `ESC t n`.
    pub codepage_cmd: u8,
    /// Puntos que avanza antes de cortar.
    pub feed_before_cut: u8,
}

impl Default for EscPosOptions {
    fn default() -> Self {
        EscPosOptions {
            cols: 48,
            dots_per_mm: 8.0,
            codepage: CodePage::Pc850,
            codepage_cmd: 2,
            feed_before_cut: 24,
        }
    }
}

/// Convierte texto a PC850. Lo que no existe en esa página se cambia por `?`.
pub fn encode_pc850(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            c if c.is_ascii() => c as u8,
            'ü' => 0x81,
            'é' => 0x82,
            'á' => 0xA0,
            'í' => 0xA1,
            'ó' => 0xA2,
            'ú' => 0xA3,
            'ñ' => 0xA4,
            'Ñ' => 0xA5,
            'º' => 0xA7,
            'ª' => 0xA6,
            '¿' => 0xA8,
            '¡' => 0xAD,
            'Á' => 0xB5,
            'É' => 0x90,
            'Í' => 0xD6,
            'Ó' => 0xE0,
            'Ú' => 0xE9,
            'Ü' => 0x9A,
            '°' => 0xF8,
            '€' => b'E',
            _ => b'?',
        })
        .collect()
}

fn wrap_chars(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let cur_len = cur.chars().count();
            let w_len = word.chars().count();
            if cur_len > 0 && cur_len + 1 + w_len <= width {
                cur.push(' ');
                cur.push_str(word);
                continue;
            }
            if cur_len > 0 {
                lines.push(std::mem::take(&mut cur));
            }
            let mut chars = word.chars().peekable();
            while chars.peek().is_some() {
                let chunk: String = chars.by_ref().take(width).collect();
                if chunk.chars().count() == width && chars.peek().is_some() {
                    lines.push(chunk);
                } else {
                    cur = chunk;
                }
            }
        }
        lines.push(cur);
    }
    lines
}

fn pad(text: &str, width: usize, align: Align) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.chars().take(width).collect();
    }
    let space = width - len;
    match align {
        Align::Left => format!("{text}{}", " ".repeat(space)),
        Align::Right => format!("{}{text}", " ".repeat(space)),
        Align::Center => format!("{}{text}{}", " ".repeat(space / 2), " ".repeat(space - space / 2)),
    }
}

fn align_cmd(out: &mut Vec<u8>, a: Align) {
    out.extend([
        ESC,
        b'a',
        match a {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        },
    ]);
}

/// Multiplicador de tamaño (1 o 2) a partir de los puntos tipográficos del estilo.
fn mult(size_pt: f32) -> usize {
    if size_pt >= 14.0 {
        2
    } else {
        1
    }
}

/// Genera los bytes ESC/POS del ticket.
pub fn encode(ticket: &Ticket, opt: &EscPosOptions) -> crate::Result<Vec<u8>> {
    let enc = |s: &str| match opt.codepage {
        CodePage::Pc850 => encode_pc850(s),
    };
    let mut out = vec![ESC, b'@', ESC, b't', opt.codepage_cmd];

    for item in &ticket.items {
        match item {
            Item::Text { text, style } => {
                let k = mult(style.size_pt);
                align_cmd(&mut out, style.align);
                out.extend([ESC, b'E', style.bold as u8, GS, b'!', if k == 2 { 0x11 } else { 0x00 }]);
                for line in wrap_chars(text, opt.cols / k) {
                    out.extend(enc(&line));
                    out.push(b'\n');
                }
                out.extend([ESC, b'E', 0, GS, b'!', 0]);
            }
            Item::Row { cols, style } => {
                if cols.is_empty() {
                    continue;
                }
                let total: f32 = cols.iter().map(|c| c.width.max(0.0)).sum();
                let total = if total > 0.0 { total } else { cols.len() as f32 };
                let mut widths: Vec<usize> = cols
                    .iter()
                    .map(|c| ((c.width.max(0.0) / total) * opt.cols as f32).floor() as usize)
                    .collect();
                let used: usize = widths.iter().sum();
                if let Some(last) = widths.last_mut() {
                    *last += opt.cols.saturating_sub(used);
                }
                let wrapped: Vec<Vec<String>> = cols
                    .iter()
                    .zip(&widths)
                    .enumerate()
                    .map(|(i, (c, w))| wrap_chars(&c.text, if i + 1 < cols.len() { w.saturating_sub(1).max(1) } else { *w }))
                    .collect();
                let bold = style.bold || cols.iter().any(|c| c.bold);
                align_cmd(&mut out, Align::Left);
                out.extend([ESC, b'E', bold as u8]);
                let n = wrapped.iter().map(Vec::len).max().unwrap_or(1);
                for row in 0..n {
                    let mut line = String::new();
                    for (i, (c, w)) in cols.iter().zip(&widths).enumerate() {
                        let cell = wrapped[i].get(row).map(String::as_str).unwrap_or("");
                        if i + 1 < cols.len() {
                            line.push_str(&pad(cell, w.saturating_sub(1), c.align));
                            line.push(' ');
                        } else {
                            line.push_str(&pad(cell, *w, c.align));
                        }
                    }
                    out.extend(enc(line.trim_end()));
                    out.push(b'\n');
                }
                out.extend([ESC, b'E', 0]);
            }
            Item::Separator { ch } => {
                align_cmd(&mut out, Align::Left);
                out.extend(enc(&ch.to_string().repeat(opt.cols)));
                out.push(b'\n');
            }
            Item::Feed { mm } => {
                let mut dots = (mm * opt.dots_per_mm).round().max(0.0) as u32;
                while dots > 0 {
                    let n = dots.min(255) as u8;
                    out.extend([ESC, b'J', n]);
                    dots -= n as u32;
                }
            }
            Item::Qr { data, size_mm, ecc, align } => {
                // La impresora genera el QR; calculamos aquí cuántos módulos tendrá para elegir el
                // tamaño de módulo que más se acerca a `size_mm`.
                let modules = QrMatrix::encode(data, *ecc)?.size() + 2 * QUIET_ZONE;
                let module = ((size_mm * opt.dots_per_mm) as i32 / modules).clamp(1, 16) as u8;
                let ecc_n = match ecc {
                    Ecc::L => 48,
                    Ecc::M => 49,
                    Ecc::Q => 50,
                    Ecc::H => 51,
                };
                let bytes = data.as_bytes();
                let len = bytes.len() + 3;
                align_cmd(&mut out, *align);
                out.extend([GS, b'(', b'k', 4, 0, 49, 65, 50, 0]); // modelo 2
                out.extend([GS, b'(', b'k', 3, 0, 49, 67, module]); // tamaño de módulo
                out.extend([GS, b'(', b'k', 3, 0, 49, 69, ecc_n]); // corrección de errores
                out.extend([GS, b'(', b'k', (len % 256) as u8, (len / 256) as u8, 49, 80, 48]); // guardar datos
                out.extend(bytes);
                out.extend([GS, b'(', b'k', 3, 0, 49, 81, 48]); // imprimir
                out.push(b'\n');
                align_cmd(&mut out, Align::Left);
            }
            Item::Cut => {
                out.extend([GS, b'V', 66, opt.feed_before_cut]);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Col, Style};

    #[test]
    fn acentos_en_pc850() {
        assert_eq!(
            encode_pc850("Año ñandú ¿Sí?"),
            vec![b'A', 0xA4, b'o', b' ', 0xA4, b'a', b'n', b'd', 0xA3, b' ', 0xA8, b'S', 0xA1, b'?']
        );
    }

    #[test]
    fn inicio_qr_y_corte() {
        let t = Ticket::new().text("Hola", Style::new()).qr("abc", 30.0).cut();
        let b = encode(&t, &EscPosOptions::default()).unwrap();
        assert_eq!(&b[..5], &[ESC, b'@', ESC, b't', 2]);
        // Guardar datos del QR: largo = 3 bytes de datos + 3.
        let store = [GS, b'(', b'k', 6, 0, 49, 80, 48, b'a', b'b', b'c'];
        assert!(b.windows(store.len()).any(|w| w == store));
        assert_eq!(&b[b.len() - 4..], &[GS, b'V', 66, 24]);
    }

    #[test]
    fn filas_de_48_columnas() {
        let t = Ticket::new().row([Col::left("Producto", 0.7), Col::right("$1.00", 0.3)]);
        let b = encode(&t, &EscPosOptions::default()).unwrap();
        let text = String::from_utf8_lossy(&b);
        let line = text.lines().find(|l| l.contains("Producto")).unwrap();
        let start = line.find("Producto").unwrap();
        assert_eq!(line[start..].chars().count(), 48);
        assert!(line.ends_with("$1.00"));
    }

    #[test]
    fn partir_por_caracteres() {
        assert_eq!(wrap_chars("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap_chars("uno dos tres", 7), vec!["uno dos", "tres"]);
    }
}
