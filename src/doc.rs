//! El documento: qué se imprime, sin decir cómo. Se arma con el builder de Rust o llega como JSON
//! (por ejemplo desde Tauri). El mismo documento sirve para el backend GDI y para ESC/POS.

use serde::{Deserialize, Serialize};

/// Alineación horizontal. En JSON: `"left"`, `"center"` o `"right"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    /// A la izquierda.
    #[default]
    Left,
    /// Centrado.
    Center,
    /// A la derecha.
    Right,
}

/// Estilo de un texto o de una fila.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Style {
    /// Tamaño en puntos tipográficos (1 pt = 1/72 de pulgada). Por defecto 8 pt: con Consolas da unas
    /// 47 columnas en 72 mm, casi las 48 de la fuente A de ESC/POS.
    pub size_pt: f32,
    /// Negrita.
    pub bold: bool,
    /// Alineación del texto. En una fila se usa la de cada columna.
    pub align: Align,
    /// Fuente de Windows. Si es `None` se usa la del ticket.
    pub font: Option<String>,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            size_pt: 8.0,
            bold: false,
            align: Align::Left,
            font: None,
        }
    }
}

impl Style {
    /// Estilo por defecto: 8 pt, normal, a la izquierda.
    pub fn new() -> Self {
        Self::default()
    }
    /// Cambia el tamaño en puntos.
    pub fn size_pt(mut self, pt: f32) -> Self {
        self.size_pt = pt;
        self
    }
    /// Pone el texto en negrita.
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    /// Cambia la alineación.
    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
    /// Atajo para `align(Align::Center)`.
    pub fn center(self) -> Self {
        self.align(Align::Center)
    }
    /// Usa otra fuente instalada en Windows.
    pub fn font(mut self, name: impl Into<String>) -> Self {
        self.font = Some(name.into());
        self
    }
}

/// Una columna de una fila. `width` es una fracción del ancho imprimible; las fracciones de una fila
/// se normalizan, así que `[1, 2]` y `[0.33, 0.66]` dan lo mismo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Col {
    /// Texto de la celda. Si no cabe, baja a la línea siguiente dentro de la columna.
    pub text: String,
    /// Ancho relativo de la columna.
    pub width: f32,
    /// Alineación dentro de la columna.
    #[serde(default)]
    pub align: Align,
    /// Negrita.
    #[serde(default)]
    pub bold: bool,
}

impl Col {
    /// Columna alineada a la izquierda.
    pub fn left(text: impl Into<String>, width: f32) -> Self {
        Col {
            text: text.into(),
            width,
            align: Align::Left,
            bold: false,
        }
    }
    /// Columna centrada.
    pub fn center(text: impl Into<String>, width: f32) -> Self {
        Col {
            align: Align::Center,
            ..Col::left(text, width)
        }
    }
    /// Columna alineada a la derecha (montos).
    pub fn right(text: impl Into<String>, width: f32) -> Self {
        Col {
            align: Align::Right,
            ..Col::left(text, width)
        }
    }
    /// Pone la columna en negrita.
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

/// Nivel de corrección de errores del QR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Ecc {
    /// Recupera cerca del 7 % de daño.
    L,
    /// Recupera cerca del 15 % (por defecto).
    #[default]
    M,
    /// Recupera cerca del 25 %.
    Q,
    /// Recupera cerca del 30 %.
    H,
}

fn sep_default() -> char {
    '-'
}
fn qr_size_default() -> f32 {
    30.0
}
fn align_center() -> Align {
    Align::Center
}

/// Un elemento del ticket. En JSON lleva `"type"`: `text`, `row`, `separator`, `feed`, `qr` o `cut`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum Item {
    /// Texto que baja de línea por palabras si no cabe.
    Text {
        /// El texto. Respeta los `\n`.
        text: String,
        /// Estilo del texto.
        #[serde(default)]
        style: Style,
    },
    /// Fila de columnas, por ejemplo descripción y monto.
    Row {
        /// Columnas de la fila.
        cols: Vec<Col>,
        /// Tamaño y fuente de la fila; la alineación y la negrita van por columna.
        #[serde(default)]
        style: Style,
    },
    /// Línea de un carácter repetido a todo el ancho.
    Separator {
        /// Carácter de la línea (en JSON `"char"`, por defecto `-`).
        #[serde(default = "sep_default", rename = "char")]
        ch: char,
    },
    /// Espacio en blanco.
    Feed {
        /// Alto del espacio en mm.
        mm: f32,
    },
    /// Código QR.
    #[serde(rename_all = "camelCase")]
    Qr {
        /// Contenido del QR, normalmente una URL.
        data: String,
        /// Tamaño máximo en mm, con el margen blanco incluido (por defecto 30).
        #[serde(default = "qr_size_default")]
        size_mm: f32,
        /// Corrección de errores.
        #[serde(default)]
        ecc: Ecc,
        /// Alineación (por defecto centrado).
        #[serde(default = "align_center")]
        align: Align,
    },
    /// Corte de papel. En GDI lo hace el driver al final del trabajo; en ESC/POS se manda el comando.
    Cut,
}

fn width_default() -> f32 {
    80.0
}

/// El ticket completo: una lista de elementos que se imprimen de arriba abajo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticket {
    /// Ancho del papel en mm (80 por defecto). En GDI se imprime en este ancho menos 8 mm, salvo que
    /// [`crate::PrintOptions::printable_width_mm`] diga otra cosa.
    #[serde(default = "width_default")]
    pub paper_width_mm: f32,
    /// Fuente por defecto. Si es `None` se usa [`crate::DEFAULT_FONT`].
    #[serde(default)]
    pub font: Option<String>,
    /// Los elementos, en orden.
    pub items: Vec<Item>,
}

impl Default for Ticket {
    fn default() -> Self {
        Ticket {
            paper_width_mm: 80.0,
            font: None,
            items: Vec::new(),
        }
    }
}

impl Ticket {
    /// Ticket vacío de 80 mm.
    pub fn new() -> Self {
        Self::default()
    }
    /// Ticket vacío para otro ancho de papel, por ejemplo 58 mm.
    pub fn with_paper_width_mm(mm: f32) -> Self {
        Ticket {
            paper_width_mm: mm,
            ..Self::default()
        }
    }
    /// Fuente por defecto del ticket.
    pub fn font(mut self, name: impl Into<String>) -> Self {
        self.font = Some(name.into());
        self
    }
    /// Agrega cualquier elemento.
    pub fn push(mut self, item: Item) -> Self {
        self.items.push(item);
        self
    }
    /// Agrega un texto.
    pub fn text(self, text: impl Into<String>, style: Style) -> Self {
        self.push(Item::Text { text: text.into(), style })
    }
    /// Agrega una fila con el estilo por defecto.
    pub fn row(self, cols: impl Into<Vec<Col>>) -> Self {
        self.push(Item::Row {
            cols: cols.into(),
            style: Style::default(),
        })
    }
    /// Agrega una fila con tamaño o fuente propios.
    pub fn row_styled(self, cols: impl Into<Vec<Col>>, style: Style) -> Self {
        self.push(Item::Row { cols: cols.into(), style })
    }
    /// Agrega una línea de guiones.
    pub fn separator(self) -> Self {
        self.push(Item::Separator { ch: '-' })
    }
    /// Agrega una línea de otro carácter, por ejemplo `'='`.
    pub fn separator_char(self, ch: char) -> Self {
        self.push(Item::Separator { ch })
    }
    /// Agrega espacio en blanco.
    pub fn feed_mm(self, mm: f32) -> Self {
        self.push(Item::Feed { mm })
    }
    /// Agrega un QR centrado con corrección M.
    pub fn qr(self, data: impl Into<String>, size_mm: f32) -> Self {
        self.push(Item::Qr {
            data: data.into(),
            size_mm,
            ecc: Ecc::M,
            align: Align::Center,
        })
    }
    /// Agrega un QR con corrección y alineación a elección.
    pub fn qr_with(self, data: impl Into<String>, size_mm: f32, ecc: Ecc, align: Align) -> Self {
        self.push(Item::Qr {
            data: data.into(),
            size_mm,
            ecc,
            align,
        })
    }
    /// Pide corte de papel al final.
    pub fn cut(self) -> Self {
        self.push(Item::Cut)
    }
}
