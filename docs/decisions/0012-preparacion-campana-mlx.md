# ADR 0012 · receta y preparación MLX sin inferencia

S3-04b, 2026-10-19. El driver fixture no se convierte en driver MLX cambiando
su etiqueta o sustituyendo su binario. Este incremento añade preparación
comprobable y no contiene una entrada que arranque MLX.

## Decisión

- `campaign` valida una receta de paths locales absolutos del operador y genera
  argumentos MLX fijos compatibles con el plan versionado: loopback, concurrencia
  1/1, prefill 256, cache 1/128MB, salida 256, thinking false. No shell, flags
  adicionales, downloads, trust_remote_code o endpoints aportados por el modelo.
- Provenance mediante proveedor inyectable: inventario acotado de checkpoint,
  hashes SHA-256 reales aportados por el mecanismo (nunca placeholders), config,
  tokenizer/template, shards y versiones de Python/MLX/MLX-LM/Transformers. Validar
  índices y cobertura de shards antes de aceptar el inventario. El mecanismo
  nativo de lectura/hash de pesos queda separado y no se invoca en tests normales.
- Preflight puro sobre observaciones explícitas: memoria libre observada frente
  a umbral elegido por operador, presión no crítica conocida, snapshot fresco,
  clearance externo explícito y puertos realmente reservados. No deducir seguridad
  de RAM de RSS, tamaño de pesos o free/inactive; no sumar métricas solapadas.
- Lease host cooperativo mediante flock no bloqueante en archivo fijo de un
  directorio de coordinación compartido elegido por operador. Retener handle
  abierto durante toda la campaña; nunca quitar el archivo para liberar lock.
  Ante lock ocupado, symlink/error, rechazar, no matar procesos ni borrar locks.
- El lock solo excluye participantes que usan ese mismo directorio/archivo; no
  prueba ausencia de MLX externo. Hace falta clearance/preflight vigente del
  operador, y listener/PID del Child propio después del arranque. Ni un booleano
  en JSON ni un resultado de fixtures concede autorización de carga.

## Alternativas y consecuencias

Buscar globalmente procesos/argumentos para matar o adoptar MLX se rechaza: mezcla
propiedad y observación y puede afectar conversaciones ajenas. Un archivo
create_new sin flock deja locks huérfanos tras crash; borrar ese archivo abre
carreras de inode. flock libera por cierre del descriptor/crash, conservando
archivo/inode. El uso de un directorio distinto sigue siendo una limitación
cooperativa visible, no exclusión universal de GPU.

## Migración y alcance

Mecanismo nativo implementado y opt-in: checkpoint por descriptor, enumeración
fdopendir/readdir duplicada, openat/nofollow para assets seleccionados, shasum por
stdin desde archivos abiertos y metadatos antes/después. 256 assets/64 GiB,
metadata 2 MiB/archivo y 16 MiB total; deadline 120 s/hash y 600 s global. Lee
METADATA exactos suministrados por operador y Python --version, sin imports MLX.
Smoke nativo solo con archivos sintéticos propios; no hashea pesos existentes.
Versiones/METADATA no acreditan hash de todo el código de paquetes ni protegen
frente a sustitución posterior. Probes preflight nativos implementados, aún
probados solo mediante fixtures de salida; no se ejecutaron sobre el host.

No cambia el wire de Seiya, el driver fixture ni `runner_implemented:false`.
Provenance y readiness preparadas no bastan para lanzar: faltan unir el ejecutor
nativo/provenance/lease al driver de campaña, identidad de modelo variable en las
validaciones de cuerpos, deadlines de carga, limpieza y regresiones de esa ruta.
S3-04b sigue en curso y S3-05 necesita autorización posterior específica. Tests
con inventarios/lecturas sintéticos y locks/sockets de prueba propios; sin
inspeccionar pesos externos, importar MLX ni consultar recursos reales del host.
