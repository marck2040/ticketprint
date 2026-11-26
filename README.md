# ticketprint

Crate de Rust para imprimir tickets en Windows dibujando directo sobre la impresora con GDI, sin PDF
y sin librerías de impresión de terceros. Pensado para usarse desde Tauri (SIGE, SIGMED y otros),
pero el núcleo no depende de Tauri.

**Estado:** prueba de concepto. El objetivo de esta versión es comprobar en una térmica real (SAT Q22)
qué forma de fijar el alto de la página respeta su driver.

## Por qué

Imprimir un PDF obliga a fijar el alto de la página antes de saber cuánto mide el ticket; si el
contenido mide más, se corta. `ticketprint` hace lo contrario:

1. Mide el ticket con la fuente y el DPI reales de la impresora.
2. Pide al driver una página de ese alto exacto (`DMPAPER_USER`).
3. Dibuja con GDI.

Si el driver no acepta el alto pedido, usa el formulario más largo del driver o pagina entre líneas,
sin partir nunca una línea ni el QR.

## Uso

```rust
use ticketprint::{Col, PrintOptions, Style, Ticket};

let t = Ticket::new()
    .text("MI TIENDA", Style::new().size_pt(12.0).bold().center())
    .separator()
    .row([Col::left("2 x Café", 0.7), Col::right("$3.00", 0.3)])
    .qr("https://ejemplo.com/consulta", 30.0)
    .cut();

let reporte = ticketprint::print("POSPrinter POS-80C", &t, &PrintOptions::default())?;
```

El mismo ticket llega como JSON desde el frontend:

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

También hay un backend ESC/POS (`print_escpos`) que manda bytes como trabajo RAW por la misma cola
del driver instalado; no hace falta una impresora "Generic / Text Only".

## Prueba en la impresora

```text
cargo run --example prueba_q22 -- listar
cargo run --example prueba_q22 -- info "POSPrinter POS-80C"
cargo run --example prueba_q22 -- todo "POSPrinter POS-80C"
```

`todo` muestra los datos del driver e imprime un ticket de cerca de un metro con cada estrategia
(`dinamico`, `papel-largo`, `paginar`) y uno por ESC/POS. Si cada uno termina en «FIN DEL TICKET»,
no se cortó. El ejecutable ya compilado sale como artefacto `prueba_q22` en cada ejecución de CI.

## Estructura

| Módulo | Qué hace |
| --- | --- |
| `doc` | El documento: `Ticket`, `Item`, `Style`, `Col` (serde) |
| `layout` | Maquetado con medidas reales; calcula el alto y pagina sin partir bloques |
| `qr` | QR con `qrcodegen`, módulos de puntos enteros |
| `escpos` | Traducción a comandos ESC/POS (PC850, QR nativo, corte) |
| `win` | GDI, DEVMODE, lista de impresoras y trabajos RAW (solo Windows) |

Más adelante se agregará `tauri-plugin-ticketprint` como segundo crate del workspace.
