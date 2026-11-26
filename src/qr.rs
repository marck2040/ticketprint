//! Generación del QR en memoria con `qrcodegen` (no imprime nada) y cálculo de su tamaño en puntos.

use crate::doc::Ecc;
use crate::error::{Error, Result};
use qrcodegen::{QrCode, QrCodeEcc};

/// Margen blanco obligatorio alrededor del QR, en módulos.
pub const QUIET_ZONE: i32 = 4;

/// Matriz de módulos del QR, sin margen.
#[derive(Debug, Clone)]
pub struct QrMatrix {
    size: i32,
    dark: Vec<bool>,
}

impl QrMatrix {
    pub fn encode(data: &str, ecc: Ecc) -> Result<Self> {
        let ecl = match ecc {
            Ecc::L => QrCodeEcc::Low,
            Ecc::M => QrCodeEcc::Medium,
            Ecc::Q => QrCodeEcc::Quartile,
            Ecc::H => QrCodeEcc::High,
        };
        let qr = QrCode::encode_text(data, ecl).map_err(|_| Error::QrTooLong(data.len()))?;
        let size = qr.size();
        let mut dark = Vec::with_capacity((size * size) as usize);
        for y in 0..size {
            for x in 0..size {
                dark.push(qr.get_module(x, y));
            }
        }
        Ok(QrMatrix { size, dark })
    }

    /// Módulos por lado, sin margen.
    pub fn size(&self) -> i32 {
        self.size
    }

    pub fn is_dark(&self, x: i32, y: i32) -> bool {
        self.dark[(y * self.size + x) as usize]
    }

    /// Tramos horizontales de módulos negros: `(fila, columna inicial, largo)`. Dibujar por tramos
    /// en vez de módulo por módulo reduce mucho las llamadas a GDI.
    pub fn runs(&self) -> Vec<(i32, i32, i32)> {
        let mut out = Vec::new();
        for y in 0..self.size {
            let mut x = 0;
            while x < self.size {
                if self.is_dark(x, y) {
                    let start = x;
                    while x < self.size && self.is_dark(x, y) {
                        x += 1;
                    }
                    out.push((y, start, x - start));
                } else {
                    x += 1;
                }
            }
        }
        out
    }

    /// Puntos por módulo para que el QR, con su margen, no pase de `target_px`. Siempre un número
    /// entero: un módulo de 5,5 puntos sale deforme en una térmica y algunos lectores no lo leen.
    pub fn module_px(&self, target_px: i32) -> i32 {
        (target_px / (self.size + 2 * QUIET_ZONE)).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_y_modulo() {
        let qr = QrMatrix::encode("https://ejemplo.com/verificar?id=123", Ecc::M).unwrap();
        assert!(qr.size() >= 21);
        // 30 mm a 203 DPI = 240 puntos.
        let m = qr.module_px(240);
        assert!(m >= 1 && m * (qr.size() + 8) <= 240);
    }

    #[test]
    fn tramos_cubren_los_modulos_negros() {
        let qr = QrMatrix::encode("hola", Ecc::L).unwrap();
        let en_tramos: i32 = qr.runs().iter().map(|r| r.2).sum();
        let negros = (0..qr.size())
            .flat_map(|y| (0..qr.size()).map(move |x| (x, y)))
            .filter(|&(x, y)| qr.is_dark(x, y))
            .count();
        assert_eq!(en_tramos as usize, negros);
    }

    #[test]
    fn demasiado_largo() {
        let largo = "x".repeat(5000);
        assert!(matches!(QrMatrix::encode(&largo, Ecc::H), Err(Error::QrTooLong(_))));
    }
}
