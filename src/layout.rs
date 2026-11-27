//! Maquetado: convierte el documento en operaciones de dibujo con posiciones en puntos del
//! dispositivo, midiendo cada texto con la fuente real. De aquí sale el alto exacto del ticket,
//! que es lo que evita los cortes.
//!
//! No depende de Windows: la medición llega por el trait [`Measurer`], que en Windows implementa
//! el backend GDI y en las pruebas un medidor de mentira.

use crate::doc::{Align, Item, Style, Ticket};
use crate::error::Result;
use crate::qr::{QrMatrix, QUIET_ZONE};

/// Fuente que se usa si ni el ticket ni el estilo dicen otra. Monoespaciada para que las columnas
/// alineen igual en cualquier equipo con Windows.
pub const DEFAULT_FONT: &str = "Consolas";

/// Separación entre columnas de una fila, en mm.
const COL_GAP_MM: f32 = 1.0;

/// Fuente ya resuelta a puntos del dispositivo.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontKey {
    /// Nombre de la fuente de Windows.
    pub face: String,
    /// Alto de la letra (em) en puntos del dispositivo.
    pub height_px: i32,
    /// Negrita.
    pub bold: bool,
}

/// Una operación de dibujo. `y` es relativa al inicio de su bloque.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawOp {
    /// Una línea de texto. `y` es el borde superior de la línea.
    Text {
        /// Posición horizontal en puntos.
        x: i32,
        /// Posición vertical en puntos, relativa al bloque.
        y: i32,
        /// Texto de la línea.
        text: String,
        /// Fuente.
        font: FontKey,
    },
    /// Rectángulo negro relleno (módulos del QR).
    Rect {
        /// Borde izquierdo en puntos.
        x: i32,
        /// Borde superior en puntos, relativo al bloque.
        y: i32,
        /// Ancho en puntos.
        w: i32,
        /// Alto en puntos.
        h: i32,
    },
}

/// Un bloque es la unidad que nunca se parte entre páginas: una línea de texto, una fila o un QR.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// Posición vertical del bloque en el ticket, en puntos.
    pub top: i32,
    /// Alto del bloque en puntos.
    pub height: i32,
    /// Lo que se dibuja dentro del bloque.
    pub ops: Vec<DrawOp>,
}

/// El ticket maquetado.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// Resolución horizontal del dispositivo.
    pub dpi_x: i32,
    /// Resolución vertical del dispositivo.
    pub dpi_y: i32,
    /// Ancho imprimible en puntos.
    pub width: i32,
    /// Alto total del contenido en puntos.
    pub height: i32,
    /// Bloques en orden de arriba abajo.
    pub blocks: Vec<Block>,
    /// El documento pide corte al final.
    pub cut: bool,
}

impl Layout {
    /// Alto total en décimas de milímetro, la unidad de `DEVMODE.dmPaperLength`.
    pub fn height_tenths_mm(&self) -> i32 {
        ((self.height as f64) * 254.0 / self.dpi_y as f64).ceil() as i32
    }

    /// Reparte los bloques en páginas de `page_height` puntos sin partir ninguno. Un bloque más alto
    /// que la página va solo en la suya (y el driver lo recortará; el reporte lo avisa).
    pub fn paginate(&self, page_height: i32) -> Vec<std::ops::Range<usize>> {
        let mut pages = Vec::new();
        let mut start = 0;
        let mut used = 0;
        for (i, b) in self.blocks.iter().enumerate() {
            if i > start && used + b.height > page_height {
                pages.push(start..i);
                start = i;
                used = 0;
            }
            used += b.height;
        }
        if start < self.blocks.len() {
            pages.push(start..self.blocks.len());
        }
        pages
    }
}

/// Mide texto en el dispositivo de destino.
pub trait Measurer {
    /// Resolución en puntos por pulgada (x, y).
    fn dpi(&self) -> (i32, i32);
    /// Ancho imprimible en puntos.
    fn printable_width(&self) -> i32;
    /// Ancho de `text` con esa fuente, en puntos.
    fn text_width(&mut self, font: &FontKey, text: &str) -> i32;
    /// Alto de una línea (ascendente + descendente + interlineado externo) en puntos.
    fn line_height(&mut self, font: &FontKey) -> i32;
}

/// Limita el ancho imprimible que reporta otro medidor. Se usa cuando el driver dice tener más ancho
/// que el que imprime el cabezal.
pub struct WidthLimit<'a> {
    inner: &'a mut dyn Measurer,
    width: i32,
}

impl<'a> WidthLimit<'a> {
    /// Envuelve `inner` y limita su ancho a `max_width` puntos.
    pub fn new(inner: &'a mut dyn Measurer, max_width: i32) -> Self {
        let width = inner.printable_width().min(max_width).max(1);
        WidthLimit { inner, width }
    }
}

impl Measurer for WidthLimit<'_> {
    fn dpi(&self) -> (i32, i32) {
        self.inner.dpi()
    }
    fn printable_width(&self) -> i32 {
        self.width
    }
    fn text_width(&mut self, font: &FontKey, text: &str) -> i32 {
        self.inner.text_width(font, text)
    }
    fn line_height(&mut self, font: &FontKey) -> i32 {
        self.inner.line_height(font)
    }
}

fn mm_to_px(mm: f32, dpi: i32) -> i32 {
    (mm as f64 / 25.4 * dpi as f64).round() as i32
}

struct Ctx<'a> {
    m: &'a mut dyn Measurer,
    default_face: String,
    dpi_x: i32,
    dpi_y: i32,
    width: i32,
}

impl Ctx<'_> {
    fn font(&self, style: &Style, bold: bool) -> FontKey {
        FontKey {
            face: style.font.clone().unwrap_or_else(|| self.default_face.clone()),
            height_px: ((style.size_pt as f64) * self.dpi_y as f64 / 72.0).round().max(1.0) as i32,
            bold,
        }
    }

    /// Parte `text` en líneas que quepan en `max_w`: por palabras y, si una palabra sola no cabe,
    /// por caracteres. Respeta los saltos de línea del texto.
    fn wrap(&mut self, font: &FontKey, text: &str, max_w: i32) -> Vec<String> {
        let mut lines = Vec::new();
        for para in text.split('\n') {
            let mut cur = String::new();
            for word in para.split_whitespace() {
                let candidate = if cur.is_empty() {
                    word.to_string()
                } else {
                    format!("{cur} {word}")
                };
                if self.m.text_width(font, &candidate) <= max_w {
                    cur = candidate;
                    continue;
                }
                if !cur.is_empty() {
                    lines.push(std::mem::take(&mut cur));
                }
                if self.m.text_width(font, word) <= max_w {
                    cur = word.to_string();
                } else {
                    for ch in word.chars() {
                        let mut next = cur.clone();
                        next.push(ch);
                        if !cur.is_empty() && self.m.text_width(font, &next) > max_w {
                            lines.push(std::mem::take(&mut cur));
                            cur.push(ch);
                        } else {
                            cur = next;
                        }
                    }
                }
            }
            lines.push(cur);
        }
        lines
    }

    fn x_for(&mut self, font: &FontKey, text: &str, align: Align, left: i32, width: i32) -> i32 {
        match align {
            Align::Left => left,
            Align::Center => left + (width - self.m.text_width(font, text)).max(0) / 2,
            Align::Right => left + (width - self.m.text_width(font, text)).max(0),
        }
    }
}

/// Maqueta el ticket con las medidas reales del dispositivo.
pub fn layout(ticket: &Ticket, m: &mut dyn Measurer) -> Result<Layout> {
    let (dpi_x, dpi_y) = m.dpi();
    let width = m.printable_width();
    let mut cx = Ctx {
        m,
        default_face: ticket.font.clone().unwrap_or_else(|| DEFAULT_FONT.to_string()),
        dpi_x,
        dpi_y,
        width,
    };
    let mut blocks: Vec<Block> = Vec::new();
    let mut y = 0;
    let mut cut = false;
    let mut push = |blocks: &mut Vec<Block>, height: i32, ops: Vec<DrawOp>| {
        blocks.push(Block { top: y, height, ops });
        y += height;
    };

    for item in &ticket.items {
        match item {
            Item::Text { text, style } => {
                let font = cx.font(style, style.bold);
                let lh = cx.m.line_height(&font);
                // Una línea por bloque: un párrafo largo sí se puede repartir entre páginas.
                for line in cx.wrap(&font, text, cx.width) {
                    let x = cx.x_for(&font, &line, style.align, 0, cx.width);
                    push(
                        &mut blocks,
                        lh,
                        vec![DrawOp::Text {
                            x,
                            y: 0,
                            text: line,
                            font: font.clone(),
                        }],
                    );
                }
            }
            Item::Row { cols, style } => {
                if cols.is_empty() {
                    continue;
                }
                let total: f32 = cols.iter().map(|c| c.width.max(0.0)).sum();
                let total = if total > 0.0 { total } else { cols.len() as f32 };
                let gap = mm_to_px(COL_GAP_MM, cx.dpi_x);
                let mut left = 0;
                let mut lines_per_col = Vec::with_capacity(cols.len());
                let mut lh = 0;
                for (i, c) in cols.iter().enumerate() {
                    let last = i + 1 == cols.len();
                    let w = if last {
                        cx.width - left
                    } else {
                        ((c.width.max(0.0) / total) as f64 * cx.width as f64).round() as i32
                    };
                    let inner = if last { w } else { (w - gap).max(1) };
                    let font = cx.font(style, style.bold || c.bold);
                    lh = lh.max(cx.m.line_height(&font));
                    let lines = cx.wrap(&font, &c.text, inner);
                    lines_per_col.push((left, inner, font, lines, c.align));
                    left += w;
                }
                let n = lines_per_col.iter().map(|c| c.3.len()).max().unwrap_or(1);
                for row in 0..n {
                    let mut ops = Vec::new();
                    for (left, inner, font, lines, align) in &lines_per_col {
                        if let Some(text) = lines.get(row) {
                            if !text.is_empty() {
                                let x = cx.x_for(font, text, *align, *left, *inner);
                                ops.push(DrawOp::Text {
                                    x,
                                    y: 0,
                                    text: text.clone(),
                                    font: font.clone(),
                                });
                            }
                        }
                    }
                    push(&mut blocks, lh, ops);
                }
            }
            Item::Separator { ch } => {
                let font = cx.font(&Style::default(), false);
                let lh = cx.m.line_height(&font);
                let one = cx.m.text_width(&font, &ch.to_string()).max(1);
                let mut text: String = std::iter::repeat_n(*ch, (cx.width / one).max(1) as usize).collect();
                while text.chars().count() > 1 && cx.m.text_width(&font, &text) > cx.width {
                    text.pop();
                }
                let x = cx.x_for(&font, &text, Align::Center, 0, cx.width);
                push(&mut blocks, lh, vec![DrawOp::Text { x, y: 0, text, font }]);
            }
            Item::Feed { mm } => {
                push(&mut blocks, mm_to_px(*mm, cx.dpi_y).max(0), Vec::new());
            }
            Item::Qr { data, size_mm, ecc, align } => {
                let qr = QrMatrix::encode(data, *ecc)?;
                let target = mm_to_px(*size_mm, cx.dpi_x).min(cx.width);
                let module = qr.module_px(target);
                let side = module * (qr.size() + 2 * QUIET_ZONE);
                let x0 = match align {
                    Align::Left => 0,
                    Align::Center => (cx.width - side).max(0) / 2,
                    Align::Right => (cx.width - side).max(0),
                } + QUIET_ZONE * module;
                let y0 = QUIET_ZONE * module;
                let ops = qr
                    .runs()
                    .into_iter()
                    .map(|(row, col, len)| DrawOp::Rect {
                        x: x0 + col * module,
                        y: y0 + row * module,
                        w: len * module,
                        h: module,
                    })
                    .collect();
                push(&mut blocks, side, ops);
            }
            Item::Cut => cut = true,
        }
    }

    Ok(Layout {
        dpi_x,
        dpi_y,
        width,
        height: y,
        blocks,
        cut,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::doc::Col;

    /// Térmica de 80 mm a 203 DPI: 576 puntos de ancho. Cada carácter mide 0,6 × alto de la fuente.
    pub struct FakeThermal;

    impl Measurer for FakeThermal {
        fn dpi(&self) -> (i32, i32) {
            (203, 203)
        }
        fn printable_width(&self) -> i32 {
            576
        }
        fn text_width(&mut self, font: &FontKey, text: &str) -> i32 {
            (text.chars().count() as f64 * font.height_px as f64 * 0.6).round() as i32
        }
        fn line_height(&mut self, font: &FontKey) -> i32 {
            (font.height_px as f64 * 1.2).round() as i32
        }
    }

    fn largo(lineas: usize) -> Ticket {
        let mut t = Ticket::new().text("TIENDA", Style::new().size_pt(12.0).bold().center()).separator();
        for i in 0..lineas {
            t = t.row([
                Col::left(format!("{i} x Producto con una descripción bastante larga que no cabe"), 0.7),
                Col::right("$1.00", 0.3),
            ]);
        }
        t.separator().qr("https://ejemplo.com/consulta?codigo=ABC", 30.0).feed_mm(3.0).cut()
    }

    #[test]
    fn el_alto_es_la_suma_de_los_bloques() {
        let l = layout(&largo(10), &mut FakeThermal).unwrap();
        assert_eq!(l.height, l.blocks.iter().map(|b| b.height).sum::<i32>());
        for w in l.blocks.windows(2) {
            assert_eq!(w[0].top + w[0].height, w[1].top);
        }
        assert!(l.cut);
    }

    #[test]
    fn un_ticket_largo_mide_mas_de_un_metro() {
        let l = layout(&largo(400), &mut FakeThermal).unwrap();
        assert!(l.height_tenths_mm() > 10_000, "alto: {} décimas de mm", l.height_tenths_mm());
    }

    #[test]
    fn el_texto_largo_se_parte_y_nada_se_sale_del_ancho() {
        let mut m = FakeThermal;
        let l = layout(&largo(3), &mut m).unwrap();
        for b in &l.blocks {
            for op in &b.ops {
                match op {
                    DrawOp::Text { x, text, font, .. } => assert!(x + m.text_width(font, text) <= 576, "se sale: {text}"),
                    DrawOp::Rect { x, w, .. } => assert!(x + w <= 576),
                }
            }
        }
    }

    #[test]
    fn la_descripcion_larga_ocupa_varias_lineas() {
        let t = Ticket::new().row([Col::left("palabra ".repeat(30), 0.7), Col::right("$1.00", 0.3)]);
        let l = layout(&t, &mut FakeThermal).unwrap();
        assert!(l.blocks.len() > 1);
        // El precio va solo en la primera línea.
        assert_eq!(l.blocks[0].ops.len(), 2);
        assert_eq!(l.blocks[1].ops.len(), 1);
    }

    #[test]
    fn paginar_no_parte_bloques_ni_pierde_ninguno() {
        let l = layout(&largo(200), &mut FakeThermal).unwrap();
        let page = 2000; // unos 250 mm
        let pages = l.paginate(page);
        assert!(pages.len() > 1);
        let mut next = 0;
        for p in &pages {
            assert_eq!(p.start, next);
            next = p.end;
            let alto: i32 = l.blocks[p.clone()].iter().map(|b| b.height).sum();
            assert!(alto <= page || p.len() == 1);
        }
        assert_eq!(next, l.blocks.len());
    }

    #[test]
    fn el_limite_de_ancho_mete_el_precio_dentro_del_cabezal() {
        let t = Ticket::new().row([Col::left("Producto", 0.7), Col::right("$110.10", 0.3)]);
        let mut fake = FakeThermal;
        let mut m = WidthLimit::new(&mut fake, 500);
        let l = layout(&t, &mut m).unwrap();
        assert_eq!(l.width, 500);
        let mut fake = FakeThermal;
        for op in &l.blocks[0].ops {
            if let DrawOp::Text { x, text, font, .. } = op {
                assert!(x + fake.text_width(font, text) <= 500, "se sale: {text}");
            }
        }
        // Nunca agranda el ancho del driver.
        assert_eq!(WidthLimit::new(&mut fake, 9999).printable_width(), 576);
    }

    #[test]
    fn qr_centrado_y_cuadrado() {
        let l = layout(&Ticket::new().qr("hola", 30.0), &mut FakeThermal).unwrap();
        let b = &l.blocks[0];
        let min_x = b
            .ops
            .iter()
            .map(|o| if let DrawOp::Rect { x, .. } = o { *x } else { i32::MAX })
            .min()
            .unwrap();
        let max_x = b
            .ops
            .iter()
            .map(|o| if let DrawOp::Rect { x, w, .. } = o { x + w } else { 0 })
            .max()
            .unwrap();
        assert!((min_x - (576 - max_x)).abs() <= 1, "no está centrado: {min_x} .. {max_x}");
        assert!(b.height <= 240);
    }
}
