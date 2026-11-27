use std::fmt;

/// Errores del crate. Los mensajes están en español para poder mostrarlos tal cual al usuario.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// No existe una impresora con ese nombre.
    PrinterNotFound(String),
    /// Una llamada a Windows falló.
    Win32 {
        /// Función de Windows que falló.
        call: &'static str,
        /// Mensaje de Windows y código de error.
        detail: String,
    },
    /// El contenido del QR no cabe en un código QR.
    QrTooLong(usize),
    /// El ticket tiene algo que no se puede dibujar.
    InvalidTicket(String),
    /// La función solo existe en Windows.
    Unsupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::PrinterNotFound(n) => write!(f, "No se encontró la impresora «{n}»."),
            Error::Win32 { call, detail } => write!(f, "Windows rechazó {call}: {detail}"),
            Error::QrTooLong(n) => write!(f, "El texto del QR es demasiado largo ({n} bytes)."),
            Error::InvalidTicket(m) => write!(f, "Ticket inválido: {m}"),
            Error::Unsupported => write!(f, "La impresión solo está disponible en Windows."),
        }
    }
}

impl std::error::Error for Error {}

/// Resultado de las funciones del crate.
pub type Result<T> = std::result::Result<T, Error>;
