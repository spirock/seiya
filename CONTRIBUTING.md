# Desarrollo de Seiya

- Separar comportamiento implementado de especificaciones futuras.
- Mantener compatibilidad con `seiya/pegassus` y el cliente OpenCode probado.
- Contratos nuevos: versión explícita, ejemplos, pruebas y estrategia de migración.
- Núcleo nuevo en Rust. Usar adaptadores nativos para permisos y APIs del SO;
  no reescribir kernels de inferencia ni migrar todo de golpe sin mediciones.
- No copiar herramientas o dependencias sin revisar sus licencias.
- No introducir shell arbitrario, acceso global a archivos, permisos implícitos,
  telemetría de contenido o descargas automáticas de pesos.
- No usar correos, documentos o capturas reales como fixtures de pruebas.
- Ejecutar `make test`, `cargo fmt --all -- --check` y `git diff --check`.
- Las pruebas de modelos y dispositivos son explícitas; unit tests no deben
  arrancar runtimes, acceder a Mail ni cargar modelos.
- Se mantienen Python 3.10+ y la API actual durante la migración. La librería
  Rust no implica que ya exista un servidor Rust ni una app móvil.
