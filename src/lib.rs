//! # ticketprint
//!
//! Impresión de tickets en Windows dibujando directo sobre la impresora con GDI, sin PDF y sin
//! librerías de impresión de terceros. El ticket se mide primero con la fuente y el DPI reales y
//! luego se pide al driver una página de ese alto exacto, para que los tickets largos no se corten.
//!
//! El núcleo no depende de Tauri. Desde Tauri se usa con un comando (y más adelante con un plugin).
//!
//! ```no_run
//! use ticketprint::{Ticket, Style, Col, PrintOptions};
//!
//! let t = Ticket::new()
//!     .text("MI TIENDA", Style::new().size_pt(12.0).bold().center())
//!     .separator()
//!     .row([Col::left("2 x Café", 0.7), Col::right("$3.00", 0.3)])
//!     .qr("https://ejemplo.com/consulta", 30.0)
//!     .cut();
//! let reporte = ticketprint::print("POSPrinter POS-80C", &t, &PrintOptions::default())?;
//! println!("{reporte:?}");
//! # Ok::<(), ticketprint::Error>(())
//! ```

mod doc;
mod error;
pub mod escpos;
pub mod layout;
pub mod qr;
#[cfg(windows)]
mod win;

pub use doc::{Align, Col, Ecc, Item, Style, Ticket};
pub use error::{Error, Result};
pub use escpos::EscPosOptions;
pub use layout::DEFAULT_FONT;

/// Cómo se decide el alto de la página en GDI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeightStrategy {
    /// Prueba alto dinámico, luego papel largo y al final paginación.
    #[default]
    Auto,
    /// Una sola página del alto exacto del ticket (`DMPAPER_USER`).
    Dynamic,
    /// El formulario más largo que ofrece el driver con ancho de rollo.
    LongPaper,
    /// El papel configurado en el driver, cortando entre líneas, nunca dentro de una ni del QR.
    Paginate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrintOptions {
    pub job_name: String,
    pub strategy: HeightStrategy,
    /// Papel en blanco que se suma al final antes del corte, en mm.
    pub bottom_margin_mm: f32,
}

impl Default for PrintOptions {
    fn default() -> Self {
        PrintOptions {
            job_name: "Ticket".into(),
            strategy: HeightStrategy::Auto,
            bottom_margin_mm: 2.0,
        }
    }
}

impl PrintOptions {
    pub fn job_name(mut self, name: impl Into<String>) -> Self {
        self.job_name = name.into();
        self
    }
    pub fn strategy(mut self, s: HeightStrategy) -> Self {
        self.strategy = s;
        self
    }
}

/// Qué pasó al imprimir. Sirve para diagnosticar drivers.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintReport {
    pub dpi: (i32, i32),
    pub width_px: i32,
    pub content_height_px: i32,
    pub content_height_mm: f32,
    pub strategy: HeightStrategy,
    pub page_height_px: i32,
    pub pages: usize,
    pub notes: Vec<String>,
}

/// Un tamaño de papel que ofrece el driver.
#[derive(Debug, Clone, PartialEq)]
pub struct PaperForm {
    pub id: u16,
    pub name: String,
    pub width_mm: f32,
    pub length_mm: f32,
}

/// Datos de la impresora tal como los reporta su driver.
#[derive(Debug, Clone, PartialEq)]
pub struct PrinterInfo {
    pub name: String,
    pub dpi: (i32, i32),
    /// Área imprimible en puntos (ancho, alto).
    pub printable_px: (i32, i32),
    /// Página física en puntos (ancho, alto).
    pub physical_px: (i32, i32),
    /// Margen no imprimible (izquierdo, superior) en puntos.
    pub offset_px: (i32, i32),
    pub paper_id: i16,
    pub paper_mm: (f32, f32),
    pub driver_version: u16,
    pub forms: Vec<PaperForm>,
}

#[cfg(windows)]
mod api {
    use super::*;
    pub fn list_printers() -> Result<Vec<String>> {
        win::list_printers()
    }
    pub fn default_printer() -> Result<Option<String>> {
        win::default_printer()
    }
    pub fn printer_info(printer: &str) -> Result<PrinterInfo> {
        win::printer_info(printer)
    }
    pub fn print(printer: &str, ticket: &Ticket, opt: &PrintOptions) -> Result<PrintReport> {
        win::print_gdi(printer, ticket, opt)
    }
    pub fn print_escpos(printer: &str, ticket: &Ticket, opt: &EscPosOptions, job_name: &str) -> Result<()> {
        win::print_escpos(printer, ticket, opt, job_name)
    }
    pub fn send_raw(printer: &str, job_name: &str, bytes: &[u8]) -> Result<()> {
        win::send_raw(printer, job_name, bytes)
    }
}

#[cfg(not(windows))]
mod api {
    use super::*;
    pub fn list_printers() -> Result<Vec<String>> {
        Err(Error::Unsupported)
    }
    pub fn default_printer() -> Result<Option<String>> {
        Err(Error::Unsupported)
    }
    pub fn printer_info(_printer: &str) -> Result<PrinterInfo> {
        Err(Error::Unsupported)
    }
    pub fn print(_printer: &str, _ticket: &Ticket, _opt: &PrintOptions) -> Result<PrintReport> {
        Err(Error::Unsupported)
    }
    pub fn print_escpos(_printer: &str, _ticket: &Ticket, _opt: &EscPosOptions, _job_name: &str) -> Result<()> {
        Err(Error::Unsupported)
    }
    pub fn send_raw(_printer: &str, _job_name: &str, _bytes: &[u8]) -> Result<()> {
        Err(Error::Unsupported)
    }
}

/// Nombres de las impresoras instaladas (locales y conectadas).
pub fn list_printers() -> Result<Vec<String>> {
    api::list_printers()
}

/// Impresora predeterminada de Windows, si hay.
pub fn default_printer() -> Result<Option<String>> {
    api::default_printer()
}

/// DPI, área imprimible y tamaños de papel que reporta el driver.
pub fn printer_info(printer: &str) -> Result<PrinterInfo> {
    api::printer_info(printer)
}

/// Imprime el ticket con GDI.
pub fn print(printer: &str, ticket: &Ticket, opt: &PrintOptions) -> Result<PrintReport> {
    api::print(printer, ticket, opt)
}

/// Imprime el ticket como ESC/POS en un trabajo RAW, por la misma cola del driver instalado.
pub fn print_escpos(printer: &str, ticket: &Ticket, opt: &EscPosOptions, job_name: &str) -> Result<()> {
    api::print_escpos(printer, ticket, opt, job_name)
}

/// Manda bytes sin interpretar a la impresora (por ejemplo un corte o abrir el cajón después de un
/// trabajo GDI).
pub fn send_raw(printer: &str, job_name: &str, bytes: &[u8]) -> Result<()> {
    api::send_raw(printer, job_name, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_desde_json() {
        let json = r#"{
            "items": [
                { "type": "text", "text": "TIENDA", "style": { "sizePt": 12, "bold": true, "align": "center" } },
                { "type": "separator" },
                { "type": "row", "cols": [ { "text": "TOTAL", "width": 0.5 }, { "text": "$5.00", "width": 0.5, "align": "right" } ] },
                { "type": "qr", "data": "https://ejemplo.com", "sizeMm": 30 },
                { "type": "feed", "mm": 3 },
                { "type": "cut" }
            ]
        }"#;
        let t: Ticket = serde_json::from_str(json).unwrap();
        assert_eq!(t.paper_width_mm, 80.0);
        assert_eq!(t.items.len(), 6);
        assert!(matches!(&t.items[1], Item::Separator { ch: '-' }));
        assert!(matches!(&t.items[3], Item::Qr { size_mm, ecc: Ecc::M, align: Align::Center, .. } if *size_mm == 30.0));
        assert_eq!(t.items[5], Item::Cut);
        // Y vuelve a JSON sin perder nada.
        let again: Ticket = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(again, t);
    }
}
