//! Prueba de concepto en una térmica real (pensada para la SAT Q22).
//!
//! ```text
//! cargo run --example prueba_q22 -- listar
//! cargo run --example prueba_q22 -- info  "POSPrinter POS-80C"
//! cargo run --example prueba_q22 -- gdi   "POSPrinter POS-80C" [auto|dinamico|papel-largo|paginar] [productos]
//! cargo run --example prueba_q22 -- raw   "POSPrinter POS-80C" [productos]
//! cargo run --example prueba_q22 -- todo  "POSPrinter POS-80C"
//! ```
//!
//! Con 110 productos (el valor por defecto) el ticket mide alrededor de un metro. Si al final sale
//! «FIN DEL TICKET», no se cortó nada.

use ticketprint::{Col, EscPosOptions, HeightStrategy, PrintOptions, Style, Ticket};

fn ticket_largo(titulo: &str, productos: usize) -> Ticket {
    let mut t = Ticket::new()
        .text("PRUEBA TICKETPRINT", Style::new().size_pt(12.0).bold().center())
        .text(titulo, Style::new().center())
        .text("Acentos: áéíóú ÁÉÍÓÚ ñÑ ü ¿? ¡!", Style::new().center())
        .separator_char('=');
    for i in 1..=productos {
        t = t.row([
            Col::left(
                format!("{i:>3} Producto de prueba con una descripción larga para forzar dos líneas"),
                0.72,
            ),
            Col::right(format!("${}.{:02}", i, i % 100), 0.28),
        ]);
    }
    t.separator_char('=')
        .row_styled(
            [Col::left("TOTAL", 0.5).bold(), Col::right("$9,999.99", 0.5).bold()],
            Style::new().size_pt(11.0),
        )
        .feed_mm(2.0)
        .text("QR de verificación", Style::new().bold().center())
        .qr(
            "https://admin.factura.gob.sv/consultaPublica?ambiente=00&codGen=00000000-0000-0000-0000-000000000000&fechaEmi=2026-11-26",
            30.0,
        )
        .text("FIN DEL TICKET", Style::new().size_pt(12.0).bold().center())
        .feed_mm(3.0)
        .cut()
}

fn estrategia(s: &str) -> HeightStrategy {
    match s {
        "dinamico" => HeightStrategy::Dynamic,
        "papel-largo" => HeightStrategy::LongPaper,
        "paginar" => HeightStrategy::Paginate,
        _ => HeightStrategy::Auto,
    }
}

fn info(impresora: &str) -> ticketprint::Result<()> {
    let i = ticketprint::printer_info(impresora)?;
    println!("Impresora: {}", i.name);
    println!("  DPI: {} x {}", i.dpi.0, i.dpi.1);
    println!("  Área imprimible: {} x {} puntos", i.printable_px.0, i.printable_px.1);
    println!(
        "  Página física: {} x {} puntos, margen {} x {}",
        i.physical_px.0, i.physical_px.1, i.offset_px.0, i.offset_px.1
    );
    println!(
        "  Papel actual: id {} ({} x {} mm), versión del driver 0x{:04X}",
        i.paper_id, i.paper_mm.0, i.paper_mm.1, i.driver_version
    );
    println!("  Tamaños de papel del driver:");
    for f in &i.forms {
        println!("    id {:>5}  {:>6.1} x {:>7.1} mm  {}", f.id, f.width_mm, f.length_mm, f.name);
    }
    Ok(())
}

fn gdi(impresora: &str, est: &str, productos: usize) -> ticketprint::Result<()> {
    let t = ticket_largo(&format!("GDI, estrategia: {est}"), productos);
    let opt = PrintOptions::default()
        .job_name(format!("Prueba GDI {est}"))
        .strategy(estrategia(est));
    let r = ticketprint::print(impresora, &t, &opt)?;
    println!("GDI {est}: {r:#?}");
    Ok(())
}

fn raw(impresora: &str, productos: usize) -> ticketprint::Result<()> {
    let t = ticket_largo("ESC/POS en trabajo RAW", productos);
    ticketprint::print_escpos(impresora, &t, &EscPosOptions::default(), "Prueba ESC/POS")?;
    println!(
        "ESC/POS: enviado ({} bytes)",
        ticketprint::escpos::encode(&t, &EscPosOptions::default())?.len()
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let impresora = args.get(1).cloned().unwrap_or_default();
    let r = match cmd {
        "listar" => ticketprint::list_printers().map(|l| {
            let pred = ticketprint::default_printer().ok().flatten();
            for p in l {
                let marca = if Some(&p) == pred.as_ref() { " (predeterminada)" } else { "" };
                println!("{p}{marca}");
            }
        }),
        "info" => info(&impresora),
        "gdi" => {
            let est = args.get(2).map(String::as_str).unwrap_or("auto");
            let n = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(110);
            gdi(&impresora, est, n)
        }
        "raw" => raw(&impresora, args.get(2).and_then(|s| s.parse().ok()).unwrap_or(110)),
        "todo" => (|| {
            info(&impresora)?;
            for est in ["dinamico", "papel-largo", "paginar"] {
                if let Err(e) = gdi(&impresora, est, 110) {
                    println!("GDI {est}: ERROR {e}");
                }
            }
            raw(&impresora, 110)
        })(),
        _ => {
            eprintln!("Uso: prueba_q22 listar | info <impresora> | gdi <impresora> [auto|dinamico|papel-largo|paginar] [productos] | raw <impresora> [productos] | todo <impresora>");
            std::process::exit(2);
        }
    };
    if let Err(e) = r {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
