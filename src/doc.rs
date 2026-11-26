//! El documento: qué se imprime, sin decir cómo. Se arma con el builder de Rust o llega como JSON
//! (por ejemplo desde Tauri). El mismo documento sirve para el backend GDI y para ESC/POS.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Estilo de un texto o de una fila.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Style {
    /// Tamaño en puntos tipográficos (1 pt = 1/72 de pulgada). Por defecto 8 pt: con Consolas da unas
    /// 47 columnas en 72 mm, casi las 48 de la fuente A de ESC/POS.
    pub size_pt: f32,
    pub bold: bool,
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
    pub fn new() -> Self {
        Self::default()
    }
    pub fn size_pt(mut self, pt: f32) -> Self {
        self.size_pt = pt;
        self
    }
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
    pub fn center(self) -> Self {
        self.align(Align::Center)
    }
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
    pub text: String,
    pub width: f32,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub bold: bool,
}

impl Col {
    pub fn left(text: impl Into<String>, width: f32) -> Self {
        Col {
            text: text.into(),
            width,
            align: Align::Left,
            bold: false,
        }
    }
    pub fn center(text: impl Into<String>, width: f32) -> Self {
        Col {
            align: Align::Center,
            ..Col::left(text, width)
        }
    }
    pub fn right(text: impl Into<String>, width: f32) -> Self {
        Col {
            align: Align::Right,
            ..Col::left(text, width)
        }
    }
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

/// Nivel de corrección de errores del QR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Ecc {
    L,
    #[default]
    M,
    Q,
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
pub enum Item {
    Text {
        text: String,
        #[serde(default)]
        style: Style,
    },
    Row {
        cols: Vec<Col>,
        /// Tamaño y fuente de la fila; la alineación y la negrita van por columna.
        #[serde(default)]
        style: Style,
    },
    Separator {
        #[serde(default = "sep_default", rename = "char")]
        ch: char,
    },
    Feed {
        mm: f32,
    },
    #[serde(rename_all = "camelCase")]
    Qr {
        data: String,
        #[serde(default = "qr_size_default")]
        size_mm: f32,
        #[serde(default)]
        ecc: Ecc,
        #[serde(default = "align_center")]
        align: Align,
    },
    /// Corte de papel. En GDI lo hace el driver al final del trabajo; en ESC/POS se manda el comando.
    Cut,
}

fn width_default() -> f32 {
    80.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticket {
    /// Ancho del papel en mm. Solo informativo para GDI (manda el ancho imprimible del driver).
    #[serde(default = "width_default")]
    pub paper_width_mm: f32,
    /// Fuente por defecto. Si es `None` se usa [`crate::DEFAULT_FONT`].
    #[serde(default)]
    pub font: Option<String>,
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
    pub fn new() -> Self {
        Self::default()
    }
    pub fn font(mut self, name: impl Into<String>) -> Self {
        self.font = Some(name.into());
        self
    }
    pub fn push(mut self, item: Item) -> Self {
        self.items.push(item);
        self
    }
    pub fn text(self, text: impl Into<String>, style: Style) -> Self {
        self.push(Item::Text { text: text.into(), style })
    }
    pub fn row(self, cols: impl Into<Vec<Col>>) -> Self {
        self.push(Item::Row {
            cols: cols.into(),
            style: Style::default(),
        })
    }
    pub fn row_styled(self, cols: impl Into<Vec<Col>>, style: Style) -> Self {
        self.push(Item::Row { cols: cols.into(), style })
    }
    pub fn separator(self) -> Self {
        self.push(Item::Separator { ch: '-' })
    }
    pub fn separator_char(self, ch: char) -> Self {
        self.push(Item::Separator { ch })
    }
    pub fn feed_mm(self, mm: f32) -> Self {
        self.push(Item::Feed { mm })
    }
    pub fn qr(self, data: impl Into<String>, size_mm: f32) -> Self {
        self.push(Item::Qr {
            data: data.into(),
            size_mm,
            ecc: Ecc::M,
            align: Align::Center,
        })
    }
    pub fn cut(self) -> Self {
        self.push(Item::Cut)
    }
}
