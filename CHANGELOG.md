# Cambios

## 0.1.0 (sin publicar)

Primera versión.

- Documento declarativo (`Ticket`, `Item`, `Style`, `Col`) con builder y JSON (serde).
- Backend GDI: mide el ticket con la fuente y el DPI reales y pide una página del alto exacto
  (`DMPAPER_USER`), con alternativas de papel largo y paginación sin partir líneas ni el QR.
- Ancho imprimible limitado al cabezal (papel menos 8 mm por defecto) y corrimiento horizontal.
- QR generado con `qrcodegen` y dibujado con módulos de puntos enteros.
- Backend ESC/POS en trabajo RAW: PC850, filas por columnas, QR nativo, corte.
- `list_printers`, `default_printer`, `printer_info`, `send_raw`.
- Probado en SAT Q22 (80 mm, 203 DPI).
