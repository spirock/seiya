# Contratos v1 · draft

- `session-request.schema.json`: forma de la petición nativa futura.
- `examples/session-request.json`: ejemplo que se parsea desde los tests Rust.
- `tools.json`: catálogo objetivo con todos sus elementos **planned**; no pasar
  directamente a un modelo como herramientas ya disponibles.
- `tool-arguments.schema.json`: entradas estrictas por herramienta en `$defs`;
  el registro confiable selecciona el esquema por nombre exacto.
- `tool-result.schema.json`: envelope común de éxito, fallo o cancelación.
- `capabilities.example.json`: host iOS hipotético, con tools vacías y sin un
  modelo validado. No es un endpoint real ni un inventario del Mac actual.

Los tipos iniciales se implementan en `crates/seiya-contracts`. No se han
implementado aún un validador JSON Schema en el broker ni las formas completas
de `data` de cada resultado: son puertas obligatorias antes de construir
adaptadores ejecutables. Los esquemas de argumentos no comprueban permisos ni
seguridad de URLs/rutas por sí solos.
El protocolo completo, estados y errores están en `docs/PROTOCOLO_V1.md`.
No confundir estos archivos con la API `/v1/chat/completions` existente.
