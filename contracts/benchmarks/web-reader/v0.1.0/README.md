# Lectura pública S4-08 · evidencia real acotada

`public-read-2026-11-09.json` conserva el reporte real del harness
`authorized_reader::authorized_three_public_sources_without_models`, ejecutado
con permiso explícito para las3 fuentes y para guardar URLs/métricas, no bodies.
La salida coincide con el reporte temporal `seiya-reader-live-41ed60cdc4.json`
salvo formato/orden JSON. Logs originales solo locales, no publicados aquí.

3/3 fuentes con transporte HTTPS real y extracción útil según heurística, total
673ms en esta muestra,3 GET, sin redirects observados/retries/modelos/Safari.
No es benchmark de calidad, latencia garantizada, estabilidad prolongada o
certificación de seguridad de todas las páginas; no prueba rutas de error reales,
compresión, redirects o cancelación que no ocurrieron en esta muestra.

El harness se ejecutó desde el snapshot publicado fuera de iCloud tras comprobar
igualdad SHA256 con la fuente de trabajo para Cargo.lock, el harness y todos los
módulos reader. Evita repetir la compilación lenta en iCloud, no cambia el servicio
o el directorio de la sesión. El snapshot contenía las fuentes del commit raíz
19cbbeb; la documentación posterior no cambia los módulos de lectura.

Procedencia de archivos usados (SHA256):

| Archivo | SHA256 |
|---|---|
| Cargo.lock | 6289d0d0051810badb909ec3cad2089703804c9ee4126b5fdc0e8e41b8b9dfaf |
| tests/authorized_reader.rs | d5efa6b6553d56c4bb72d62936b0794e72366c5c393e7d45d5d36a7887b56ea5 |
| reader/extract.rs | 9b36488c01b77e2a98a2de410aa447a4f12b79783af79e227681db11d6571460 |
| reader/mod.rs | e3759120428490ceac9e871df62c86aa9a31372cb1d11e95167b4a9845514889 |
| reader/policy.rs | ce900cbd09db314961b0167201c2c527ad3905dafe31df645ec1e79b2acd7dd7 |
| reader/selection.rs | 45df9a52dbe90a6163701acc307afe52d58ea4a099c348b3038719cbfce6a2f4 |
| reader/transport.rs | 276e45cc46bfc3afe61d4c4448e897e95a8280dc4f71db82824562c392ad314f |
| reader/wire.rs | 47493517fb90a32f77bd61558efce819deb0061e0cd80495baca4ea96a69cc42 |

Rutas de código relativas a crates/seiya-server salvo Cargo.lock. Los hashes
identifican contenido, no son firma de confianza o medición de memoria. Los
bytes contados son los entregados al parser, no TLS readahead/kernel/cifrado.
`llm_quality:null` es desconocido, nunca cero ni calidad aprobada. Cleanup por
scope/drop no demuestra por separado finalización del worker DNS del sistema.

[Procedimiento/permiso y límites](../../../../docs/PRUEBA_LECTOR_PUBLICO.md).
