# ticketprint

[![crates.io](https://img.shields.io/crates/v/ticketprint.svg)](https://crates.io/crates/ticketprint)
[![docs.rs](https://docs.rs/ticketprint/badge.svg)](https://docs.rs/ticketprint)
[![CI](https://github.com/marck2040/ticketprint/actions/workflows/ci.yml/badge.svg)](https://github.com/marck2040/ticketprint/actions/workflows/ci.yml)

Impresión de tickets en Windows dibujando directo sobre la impresora con GDI, sin PDF y sin
librerías de impresión de terceros. El ticket se mide con la fuente y el DPI reales de la impresora y
se pide una página de ese alto exacto, así que **los tickets largos no se cortan**. Incluye un backend
ESC/POS opcional y se usa igual desde Rust o desde Tauri (el ticket llega como JSON).

> *English:* print receipts on Windows thermal printers (80/58 mm) by drawing directly with GDI.
> The ticket is measured with the real printer font and DPI, then printed on a page of that exact
> height, so long receipts are never cut. Optional ESC/POS raw backend. Pure Rust + `windows-rs`.

Probado en una SAT Q22 (80 mm, 203 DPI) con tickets de más de un metro.

## Instalación

```toml
[dependencies]
ticketprint = "0.1"
```

Funciona solo en Windows. En otros sistemas compila, pero las funciones de impresión devuelven
`Error::Unsupported`; así una app de Tauri sigue compilando en macOS o Linux.

## Uso

```rust,no_run
use ticketprint::{Col, PrintOptions, Style, Ticket};

let ticket = Ticket::new()
    .text("MI TIENDA", Style::new().size_pt(12.0).bold().center())
    .text("Ticket 000123 · 26/11/2026 09:45", Style::new().center())
    .separator()
    .row([Col::left("2 x Café con leche", 0.7), Col::right("$3.00", 0.3)])
    .row([Col::left("1 x Pan francés", 0.7), Col::right("$0.25", 0.3)])
    .separator_char('=')
    .row([Col::left("TOTAL", 0.5).bold(), Col::right("$3.25", 0.5).bold()])
    .qr("https://ejemplo.com/consulta?id=123", 30.0)
    .feed_mm(3.0)
    .cut();

let reporte = ticketprint::print("POSPrinter POS-80C", &ticket, &PrintOptions::default())?;
println!("{} mm en {} página(s)", reporte.content_height_mm, reporte.pages);
# Ok::<(), ticketprint::Error>(())
```

Otras funciones:

| Función | Para qué |
| --- | --- |
| `list_printers()`, `default_printer()` | Impresoras instaladas y la predeterminada |
| `printer_info(nombre)` | DPI, área imprimible y tamaños de papel del driver |
| `print(nombre, &ticket, &opciones)` | Imprime con GDI y devuelve un `PrintReport` |
| `print_escpos(nombre, &ticket, &opciones, trabajo)` | Imprime como ESC/POS en un trabajo RAW |
| `send_raw(nombre, trabajo, &bytes)` | Manda bytes sin interpretar (corte, cajón) |

## Desde Tauri

El ticket se puede mandar como JSON desde el frontend:

```json
{
  "items": [
    { "type": "text", "text": "MI TIENDA", "style": { "sizePt": 12, "bold": true, "align": "center" } },
    { "type": "separator" },
    { "type": "row", "cols": [ { "text": "2 x Café", "width": 0.7 }, { "text": "$3.00", "width": 0.3, "align": "right" } ] },
    { "type": "qr", "data": "https://ejemplo.com/consulta", "sizeMm": 30 },
    { "type": "cut" }
  ]
}
```

```rust,ignore
#[tauri::command]
async fn imprimir_ticket(impresora: String, ticket: ticketprint::Ticket) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        ticketprint::print(&impresora, &ticket, &Default::default()).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
```

Imprimir bloquea mientras el spooler recibe el trabajo, por eso conviene `spawn_blocking`.

## Cómo evita los cortes

1. Mide el ticket en un contexto de información (`CreateICW`) de la impresora.
2. Pide una página del alto exacto (`DMPAPER_USER`) y comprueba lo que aceptó el driver.
3. Si el driver no lo acepta, usa su formulario más largo con ancho de rollo o pagina entre líneas,
   sin partir nunca una línea ni el QR (`HeightStrategy`).

El ancho también importa: muchos drivers de 80 mm reportan 80 mm de área imprimible, pero el cabezal
imprime 72. Por defecto se maqueta al ancho del papel menos 8 mm; se cambia con
`PrintOptions::printable_width_mm` y se corre el ticket con `offset_x_mm`.

## ESC/POS

`print_escpos` traduce el mismo ticket a comandos ESC/POS (página de códigos PC850 para acentos y ñ,
QR nativo `GS ( k`, corte `GS V`) y los manda como trabajo RAW por la cola del driver instalado. No hace
falta configurar la impresora como "Generic / Text Only".

## Probar una impresora

```text
cargo run --example prueba_q22 -- listar
cargo run --example prueba_q22 -- info "POSPrinter POS-80C"
cargo run --example prueba_q22 -- todo "POSPrinter POS-80C"
```

`todo` muestra los datos del driver e imprime un ticket de cerca de un metro con cada estrategia y uno
por ESC/POS. Si cada uno termina en «FIN DEL TICKET», no se cortó.

## Licencia

Bajo cualquiera de estas dos licencias, a su elección:

- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT ([LICENSE-MIT](LICENSE-MIT))

Salvo que se indique lo contrario, cualquier contribución enviada para incluirse en este crate se
licencia de la misma forma, sin términos adicionales.
