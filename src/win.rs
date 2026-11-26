//! Todo lo que habla con Windows: impresoras, DEVMODE, DC de GDI y trabajos RAW.

use crate::error::{Error, Result};
use crate::layout::{layout, DrawOp, FontKey, Layout, Measurer, WidthLimit};
use crate::{EscPosOptions, HeightStrategy, PaperForm, PrintOptions, PrintReport, PrinterInfo, Ticket};
use std::collections::HashMap;
use std::ops::Range;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{GetLastError, COLORREF, POINT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Printing::*;
use windows::Win32::Storage::Xps::{
    AbortDoc, DeviceCapabilitiesW, EndDoc, EndPage, StartDocW, StartPage, DC_PAPERNAMES, DC_PAPERS, DC_PAPERSIZE, DOCINFOW,
};

// Valores de DocumentPropertiesW (fMode).
const DM_OUT_BUFFER: u32 = 2;
const DM_IN_BUFFER: u32 = 8;
// dmPaperSize para un tamaño definido por el usuario.
const DMPAPER_USER: i16 = 256;
// `dmPaperLength` es i16 en décimas de mm: el máximo que se puede pedir es 3276,7 mm.
const MAX_TENTHS: i32 = i16::MAX as i32;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn text16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn last_error(call: &'static str) -> Error {
    let code = unsafe { GetLastError() };
    Error::Win32 {
        call,
        detail: format!("{} (código {})", windows::core::Error::from(code.to_hresult()).message(), code.0),
    }
}

fn mm_to_px(mm: f32, dpi: i32) -> i32 {
    (mm as f64 / 25.4 * dpi as f64).round() as i32
}

// ---------------------------------------------------------------------------------------------
// Impresoras
// ---------------------------------------------------------------------------------------------

pub fn list_printers() -> Result<Vec<String>> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let mut needed = 0u32;
    let mut returned = 0u32;
    unsafe {
        let _ = EnumPrintersW(flags, PCWSTR::null(), 4, None, &mut needed, &mut returned);
    }
    if needed == 0 {
        return Ok(Vec::new());
    }
    // Búfer de u64 para que las estructuras queden alineadas.
    let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
    let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, needed as usize) };
    unsafe { EnumPrintersW(flags, PCWSTR::null(), 4, Some(bytes), &mut needed, &mut returned) }.map_err(|e| Error::Win32 {
        call: "EnumPrintersW",
        detail: e.message(),
    })?;
    let infos = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const PRINTER_INFO_4W, returned as usize) };
    Ok(infos.iter().filter_map(|i| unsafe { i.pPrinterName.to_string() }.ok()).collect())
}

pub fn default_printer() -> Result<Option<String>> {
    let mut len = 0u32;
    unsafe {
        let _ = GetDefaultPrinterW(None, &mut len);
    }
    if len == 0 {
        return Ok(None);
    }
    let mut buf = vec![0u16; len as usize];
    if !unsafe { GetDefaultPrinterW(Some(PWSTR(buf.as_mut_ptr())), &mut len) }.as_bool() {
        return Ok(None);
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(Some(String::from_utf16_lossy(&buf[..end])))
}

struct PrinterHandle(PRINTER_HANDLE);

impl PrinterHandle {
    fn open(name: &str) -> Result<Self> {
        let w = wide(name);
        let mut h = PRINTER_HANDLE::default();
        unsafe { OpenPrinterW(PCWSTR(w.as_ptr()), &mut h, None) }.map_err(|_| Error::PrinterNotFound(name.to_string()))?;
        Ok(PrinterHandle(h))
    }
}

impl Drop for PrinterHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = ClosePrinter(self.0);
        }
    }
}

/// DEVMODE guardado en un búfer alineado, con los datos privados del driver al final.
#[derive(Clone)]
struct DevMode(Vec<u64>);

impl DevMode {
    fn ptr(&self) -> *const DEVMODEW {
        self.0.as_ptr() as *const DEVMODEW
    }
    fn get(&self) -> &DEVMODEW {
        unsafe { &*self.ptr() }
    }
    fn get_mut(&mut self) -> &mut DEVMODEW {
        unsafe { &mut *(self.0.as_mut_ptr() as *mut DEVMODEW) }
    }
    fn paper(&self) -> (i16, i16, i16) {
        let s = unsafe { self.get().Anonymous1.Anonymous1 };
        (s.dmPaperSize, s.dmPaperWidth, s.dmPaperLength)
    }
}

/// DEVMODE actual del driver para esa impresora.
fn devmode_default(h: &PrinterHandle, name: &[u16]) -> Result<DevMode> {
    let size = unsafe { DocumentPropertiesW(None, h.0, PCWSTR(name.as_ptr()), None, None, 0) };
    if size <= 0 {
        return Err(last_error("DocumentPropertiesW"));
    }
    let mut buf = DevMode(vec![0u64; (size as usize).div_ceil(8)]);
    let r = unsafe { DocumentPropertiesW(None, h.0, PCWSTR(name.as_ptr()), Some(buf.get_mut()), None, DM_OUT_BUFFER) };
    if r < 0 {
        return Err(last_error("DocumentPropertiesW"));
    }
    Ok(buf)
}

/// Pasa un DEVMODE modificado por el driver para que lo valide y devuelve lo que el driver aceptó.
fn devmode_merge(h: &PrinterHandle, name: &[u16], input: &DevMode) -> Result<DevMode> {
    let mut out = input.clone();
    let r = unsafe {
        DocumentPropertiesW(
            None,
            h.0,
            PCWSTR(name.as_ptr()),
            Some(out.get_mut()),
            Some(input.ptr()),
            DM_IN_BUFFER | DM_OUT_BUFFER,
        )
    };
    if r < 0 {
        return Err(last_error("DocumentPropertiesW"));
    }
    Ok(out)
}

fn device_caps(name: &[u16], dm: &DevMode) -> Result<(Vec<u16>, Vec<POINT>, Vec<String>)> {
    let dev = PCWSTR(name.as_ptr());
    let n = unsafe { DeviceCapabilitiesW(dev, PCWSTR::null(), DC_PAPERS, None, Some(dm.ptr())) };
    if n <= 0 {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }
    let n = n as usize;
    let mut ids = vec![0u16; n];
    let mut sizes = vec![POINT::default(); n];
    let mut names = vec![0u16; n * 64];
    unsafe {
        DeviceCapabilitiesW(dev, PCWSTR::null(), DC_PAPERS, Some(PWSTR(ids.as_mut_ptr())), Some(dm.ptr()));
        DeviceCapabilitiesW(
            dev,
            PCWSTR::null(),
            DC_PAPERSIZE,
            Some(PWSTR(sizes.as_mut_ptr() as *mut u16)),
            Some(dm.ptr()),
        );
        DeviceCapabilitiesW(dev, PCWSTR::null(), DC_PAPERNAMES, Some(PWSTR(names.as_mut_ptr())), Some(dm.ptr()));
    }
    let names = names
        .chunks(64)
        .map(|c| {
            let end = c.iter().position(|&x| x == 0).unwrap_or(64);
            String::from_utf16_lossy(&c[..end])
        })
        .collect();
    Ok((ids, sizes, names))
}

fn forms(name: &[u16], dm: &DevMode) -> Result<Vec<PaperForm>> {
    let (ids, sizes, names) = device_caps(name, dm)?;
    Ok(ids
        .iter()
        .zip(sizes.iter())
        .zip(names)
        .map(|((id, s), n)| PaperForm {
            id: *id,
            name: n,
            width_mm: s.x as f32 / 10.0,
            length_mm: s.y as f32 / 10.0,
        })
        .collect())
}

// ---------------------------------------------------------------------------------------------
// DC de GDI: mide y dibuja
// ---------------------------------------------------------------------------------------------

struct Dc {
    hdc: HDC,
    fonts: HashMap<FontKey, HFONT>,
    current: Option<FontKey>,
    original_font: Option<HGDIOBJ>,
}

impl Dc {
    /// Contexto de información: mide como la impresora pero no imprime.
    fn info(name: &[u16], dm: &DevMode) -> Result<Self> {
        let hdc = unsafe { CreateICW(PCWSTR::null(), PCWSTR(name.as_ptr()), PCWSTR::null(), Some(dm.ptr())) };
        if hdc.is_invalid() {
            return Err(last_error("CreateICW"));
        }
        Ok(Dc {
            hdc,
            fonts: HashMap::new(),
            current: None,
            original_font: None,
        })
    }

    fn printer(name: &[u16], dm: &DevMode) -> Result<Self> {
        let hdc = unsafe { CreateDCW(PCWSTR::null(), PCWSTR(name.as_ptr()), PCWSTR::null(), Some(dm.ptr())) };
        if hdc.is_invalid() {
            return Err(last_error("CreateDCW"));
        }
        Ok(Dc {
            hdc,
            fonts: HashMap::new(),
            current: None,
            original_font: None,
        })
    }

    fn cap(&self, i: GET_DEVICE_CAPS_INDEX) -> i32 {
        unsafe { GetDeviceCaps(Some(self.hdc), i) }
    }

    fn select(&mut self, f: &FontKey) {
        if self.current.as_ref() == Some(f) {
            return;
        }
        let hfont = *self.fonts.entry(f.clone()).or_insert_with(|| {
            let face = wide(&f.face);
            unsafe {
                CreateFontW(
                    -f.height_px, // negativo = alto de la letra, sin el interlineado interno
                    0,
                    0,
                    0,
                    if f.bold { FW_BOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    NONANTIALIASED_QUALITY, // en una térmica de 1 bit los grises ensucian el texto
                    (FF_DONTCARE.0 | DEFAULT_PITCH.0) as u32,
                    PCWSTR(face.as_ptr()),
                )
            }
        });
        let prev = unsafe { SelectObject(self.hdc, HGDIOBJ(hfont.0)) };
        if self.original_font.is_none() {
            self.original_font = Some(prev);
        }
        self.current = Some(f.clone());
    }
}

impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            if let Some(o) = self.original_font {
                SelectObject(self.hdc, o);
            }
            for f in self.fonts.values() {
                let _ = DeleteObject(HGDIOBJ(f.0));
            }
            let _ = DeleteDC(self.hdc);
        }
    }
}

impl Measurer for Dc {
    fn dpi(&self) -> (i32, i32) {
        (self.cap(LOGPIXELSX), self.cap(LOGPIXELSY))
    }
    fn printable_width(&self) -> i32 {
        self.cap(HORZRES)
    }
    fn text_width(&mut self, font: &FontKey, text: &str) -> i32 {
        self.select(font);
        let mut size = SIZE::default();
        unsafe {
            let _ = GetTextExtentPoint32W(self.hdc, &text16(text), &mut size);
        }
        size.cx
    }
    fn line_height(&mut self, font: &FontKey) -> i32 {
        self.select(font);
        let mut tm = TEXTMETRICW::default();
        unsafe {
            let _ = GetTextMetricsW(self.hdc, &mut tm);
        }
        tm.tmHeight + tm.tmExternalLeading
    }
}

fn draw(dc: &mut Dc, layout: &Layout, pages: &[Range<usize>], job: &str, dx: i32) -> Result<()> {
    let job = wide(job);
    let di = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: PCWSTR(job.as_ptr()),
        ..Default::default()
    };
    if unsafe { StartDocW(dc.hdc, &di) } <= 0 {
        return Err(last_error("StartDocW"));
    }
    for page in pages {
        if unsafe { StartPage(dc.hdc) } <= 0 {
            let e = last_error("StartPage");
            unsafe { AbortDoc(dc.hdc) };
            return Err(e);
        }
        // Algunos drivers reinician el DC en cada página: se vuelve a elegir fuente y fondo.
        dc.current = None;
        unsafe {
            SetBkMode(dc.hdc, TRANSPARENT);
            SetTextColor(dc.hdc, COLORREF(0));
        }
        let base = layout.blocks[page.start].top;
        for b in &layout.blocks[page.clone()] {
            let y0 = b.top - base;
            for op in &b.ops {
                match op {
                    DrawOp::Text { x, y, text, font } => {
                        dc.select(font);
                        unsafe {
                            let _ = TextOutW(dc.hdc, dx + *x, y0 + y, &text16(text));
                        }
                    }
                    DrawOp::Rect { x, y, w, h } => unsafe {
                        let _ = PatBlt(dc.hdc, dx + *x, y0 + y, *w, *h, BLACKNESS);
                    },
                }
            }
        }
        if unsafe { EndPage(dc.hdc) } <= 0 {
            let e = last_error("EndPage");
            unsafe { AbortDoc(dc.hdc) };
            return Err(e);
        }
    }
    if unsafe { EndDoc(dc.hdc) } <= 0 {
        return Err(last_error("EndDoc"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Estrategias de alto
// ---------------------------------------------------------------------------------------------

/// Pide al driver una página del alto exacto. Devuelve el DC si el driver la respetó.
fn try_dynamic(
    h: &PrinterHandle,
    name: &[u16],
    base: &DevMode,
    needed_tenths: i32,
    notes: &mut Vec<String>,
) -> Result<Option<(Dc, DevMode)>> {
    if needed_tenths > MAX_TENTHS {
        notes.push(format!(
            "alto dinámico: el ticket mide {} mm y DEVMODE admite como máximo 3276 mm",
            needed_tenths / 10
        ));
        return Ok(None);
    }
    let mut dm = base.clone();
    let (_, width, _) = dm.paper();
    let width = if width > 0 { width } else { 800 };
    {
        let d = dm.get_mut();
        d.dmFields |= DM_PAPERSIZE | DM_PAPERWIDTH | DM_PAPERLENGTH;
        let p = unsafe { &mut d.Anonymous1.Anonymous1 };
        p.dmPaperSize = DMPAPER_USER;
        p.dmPaperWidth = width;
        p.dmPaperLength = needed_tenths as i16;
    }
    let merged = devmode_merge(h, name, &dm)?;
    let (size, w, len) = merged.paper();
    notes.push(format!(
        "alto dinámico: se pidieron {} mm; el driver devolvió dmPaperSize={size}, ancho={} mm, largo={} mm",
        needed_tenths / 10,
        w / 10,
        len / 10
    ));
    let dc = Dc::printer(name, &merged)?;
    let page_h = dc.cap(VERTRES);
    let need_px = mm_to_px(needed_tenths as f32 / 10.0, dc.cap(LOGPIXELSY));
    notes.push(format!("alto dinámico: página real de {page_h} puntos, se necesitan {need_px}"));
    if page_h + 2 >= need_px {
        Ok(Some((dc, merged)))
    } else {
        notes.push("alto dinámico: el driver no respetó el largo pedido".into());
        Ok(None)
    }
}

/// Elige el formulario más largo del driver con ancho de rollo (60 a 90 mm).
fn long_paper(h: &PrinterHandle, name: &[u16], base: &DevMode, notes: &mut Vec<String>) -> Result<Option<(Dc, DevMode)>> {
    let list = forms(name, base)?;
    let best = list
        .iter()
        .filter(|f| (60.0..=90.0).contains(&f.width_mm))
        .max_by(|a, b| a.length_mm.total_cmp(&b.length_mm));
    let Some(best) = best else {
        notes.push("papel largo: el driver no ofrece formularios de 60 a 90 mm de ancho".into());
        return Ok(None);
    };
    notes.push(format!(
        "papel largo: se eligió «{}» ({} × {} mm, id {})",
        best.name, best.width_mm, best.length_mm, best.id
    ));
    let mut dm = base.clone();
    {
        let d = dm.get_mut();
        d.dmFields |= DM_PAPERSIZE;
        d.dmFields &= !(DM_PAPERWIDTH | DM_PAPERLENGTH);
        d.Anonymous1.Anonymous1.dmPaperSize = best.id as i16;
    }
    let merged = devmode_merge(h, name, &dm)?;
    Ok(Some((Dc::printer(name, &merged)?, merged)))
}

pub fn print_gdi(printer: &str, ticket: &Ticket, opt: &PrintOptions) -> Result<PrintReport> {
    let name = wide(printer);
    let h = PrinterHandle::open(printer)?;
    let base = devmode_default(&h, &name)?;

    // 1. Medir con la fuente y el DPI reales. El largo del papel no cambia ni el DPI ni el ancho.
    let mut notes = Vec::new();
    let (lay, driver_width) = {
        let mut ic = Dc::info(&name, &base)?;
        let driver_width = ic.printable_width();
        let head_mm = opt.printable_width_mm.unwrap_or((ticket.paper_width_mm - 8.0).max(10.0));
        let head_px = mm_to_px(head_mm, ic.dpi().0);
        if head_px < driver_width {
            notes.push(format!(
                "ancho: el driver reporta {driver_width} puntos; se usan {head_px} ({head_mm} mm) para no salirse del cabezal"
            ));
        }
        let mut m = WidthLimit::new(&mut ic, head_px);
        (layout(ticket, &mut m)?, driver_width)
    };
    let dx = mm_to_px(opt.offset_x_mm, lay.dpi_x);
    let margin = mm_to_px(opt.bottom_margin_mm, lay.dpi_y);
    let needed_px = lay.height + margin;
    let needed_tenths = ((needed_px as f64) * 254.0 / lay.dpi_y as f64).ceil() as i32;

    // 2. Elegir el papel según la estrategia.
    let mut chosen: Option<(HeightStrategy, Dc)> = None;
    let order: &[HeightStrategy] = match opt.strategy {
        HeightStrategy::Auto => &[HeightStrategy::Dynamic, HeightStrategy::LongPaper, HeightStrategy::Paginate],
        HeightStrategy::Dynamic => &[HeightStrategy::Dynamic],
        HeightStrategy::LongPaper => &[HeightStrategy::LongPaper],
        HeightStrategy::Paginate => &[HeightStrategy::Paginate],
    };
    for s in order {
        let r = match s {
            HeightStrategy::Dynamic => try_dynamic(&h, &name, &base, needed_tenths, &mut notes)?,
            HeightStrategy::LongPaper => long_paper(&h, &name, &base, &mut notes)?,
            _ => Some((Dc::printer(&name, &base)?, base.clone())),
        };
        if let Some((dc, _)) = r {
            chosen = Some((*s, dc));
            break;
        }
    }
    let (used, mut dc) = match chosen {
        Some(c) => c,
        None => {
            // Se pidió una estrategia concreta y el driver no la aceptó: igual se imprime paginado
            // para no perder el ticket, y el reporte lo dice.
            notes.push("ninguna estrategia pedida funcionó; se imprime paginado".into());
            (HeightStrategy::Paginate, Dc::printer(&name, &base)?)
        }
    };

    // 3. Dibujar.
    let page_h = dc.cap(VERTRES);
    let pages = lay.paginate(page_h);
    if lay.blocks.iter().any(|b| b.height > page_h) {
        notes.push("hay un bloque más alto que la página; saldrá recortado".into());
    }
    draw(&mut dc, &lay, &pages, &opt.job_name, dx)?;

    Ok(PrintReport {
        dpi: (lay.dpi_x, lay.dpi_y),
        width_px: lay.width,
        driver_width_px: driver_width,
        content_height_px: lay.height,
        content_height_mm: lay.height_tenths_mm() as f32 / 10.0,
        strategy: used,
        page_height_px: page_h,
        pages: pages.len(),
        notes,
    })
}

pub fn printer_info(printer: &str) -> Result<PrinterInfo> {
    let name = wide(printer);
    let h = PrinterHandle::open(printer)?;
    let dm = devmode_default(&h, &name)?;
    let dc = Dc::info(&name, &dm)?;
    let (size, w, len) = dm.paper();
    let driver_version = dm.get().dmDriverVersion;
    Ok(PrinterInfo {
        name: printer.to_string(),
        dpi: (dc.cap(LOGPIXELSX), dc.cap(LOGPIXELSY)),
        printable_px: (dc.cap(HORZRES), dc.cap(VERTRES)),
        physical_px: (dc.cap(PHYSICALWIDTH), dc.cap(PHYSICALHEIGHT)),
        offset_px: (dc.cap(PHYSICALOFFSETX), dc.cap(PHYSICALOFFSETY)),
        paper_id: size,
        paper_mm: (w as f32 / 10.0, len as f32 / 10.0),
        driver_version,
        forms: forms(&name, &dm)?,
    })
}

// ---------------------------------------------------------------------------------------------
// RAW
// ---------------------------------------------------------------------------------------------

/// Manda bytes tal cual a la impresora (tipo de datos RAW): el driver no los interpreta.
pub fn send_raw(printer: &str, job: &str, bytes: &[u8]) -> Result<()> {
    let h = PrinterHandle::open(printer)?;
    let mut job = wide(job);
    let mut raw = wide("RAW");
    let di = DOC_INFO_1W {
        pDocName: PWSTR(job.as_mut_ptr()),
        pOutputFile: PWSTR::null(),
        pDatatype: PWSTR(raw.as_mut_ptr()),
    };
    if unsafe { StartDocPrinterW(h.0, 1, &di) } == 0 {
        return Err(last_error("StartDocPrinterW"));
    }
    let result = (|| {
        if !unsafe { StartPagePrinter(h.0) }.as_bool() {
            return Err(last_error("StartPagePrinter"));
        }
        let mut written = 0u32;
        let ok = unsafe { WritePrinter(h.0, bytes.as_ptr() as *const _, bytes.len() as u32, &mut written) }.as_bool();
        if !ok || written as usize != bytes.len() {
            return Err(last_error("WritePrinter"));
        }
        if !unsafe { EndPagePrinter(h.0) }.as_bool() {
            return Err(last_error("EndPagePrinter"));
        }
        Ok(())
    })();
    let end = unsafe { EndDocPrinter(h.0) }.as_bool();
    result?;
    if !end {
        return Err(last_error("EndDocPrinter"));
    }
    Ok(())
}

pub fn print_escpos(printer: &str, ticket: &Ticket, opt: &EscPosOptions, job: &str) -> Result<()> {
    let bytes = crate::escpos::encode(ticket, opt)?;
    send_raw(printer, job, &bytes)
}
