# ADR 0011 · integración de procesos/HTTP/probes sin cargar modelos

S3-04b · 2026-10-19. Primero validar el mecanismo nativo contra un runtime
sintético propio. No arrancar/restaurar servicios cancelados 8010/8081.

- Nuevo driver concreto `FixtureDriver`: ejecutables/rutas configurados por el
  operador, no por modelo/HTTP. Runtime solo acepta el binario fixture de este
  proyecto, con handshake --version, sin modo MLX o descarga de pesos.
- Python/Rust se arrancan como hijos propios en puertos efímeros separados,
  configs y HOME temporales bajo directorio nuevo. Runtime fixture, coordinador
  y proxy padre no usan endpoints operativos. No confiar en un puerto libre:
  comprobar listener de su Child con lsof acotado antes de hacer HTTP.
- Proxy loopback propio captura el cuerpo efectivo antes de reenviarlo al
  runtime hijo verificado. Límites/Host/Origin/lectura, sin DNS/proxies/redirects/
  retries. Una solicitud adicional o simultánea invalida la fase; no fabricar
  trazas con el cuerpo que el driver esperaba enviar.
- Probes desde Child originales y collector actual; identidades antes/después,
  roles desde handles conocidos, descendientes conservados sin duplicar RSS.
  Reloj monotónico y snapshots de frontera/durante requests/idle. Huellas GPU no
  disponibles siguen unsupported. Ninguna señal a procesos ajenos.
- Versiones reales (--version) y SHA-256 de binarios/fuentes relevantes antes
  de medir usando shasum fijo, limitado y sin shell. Hashes no inventados, clave
  Bearer solo en entorno de hijos, sin secretos en archivos/artifacts.
- Constructor/entrada de runner concretos separados de launch_simulation:
  artefactos `fixture-integration`, real_host_measured true pero model_inference
  false. El modo simulation no se reetiqueta como medición real.
- Cancelar drena la fase; timeout/error impide otro brazo hasta cerrar/reapear
  hijos propios. Stop conserva detalles de cierre forzado si se necesitó; no
  equivale a confirmar que una inferencia terminó correctamente.

Pruebas normales: HTTP loopback sintético, parsers/ejecutor/clock inyectados,
sin ejecutar procesos/probes del host. Smoke nativo ignored explícito con los
binarios ya compilados y Python existente; sin modelos. Validar este driver no
completa una baseline real de MLX. S3-05 requiere receta de modelo/autorización,
identidad de pesos y preflight exclusivos, no heredar permisos de S1-06.

Axum/Reqwest/UUID son dependencias ya fijadas en el workspace; no cliente de
shell ni SDK OpenCode añadido. La API nueva es interna de benchmark, no catálogo
de herramientas ni cambio del servidor operativo.

Límites de este incremento: receta exclusivamente fixture, no ejecución MLX ni
checkpoint. Hashes describen archivos inspeccionados, no un almacenamiento
inmutable. Adaptador collector persistente por run para conservar altas/salidas;
si hay descendientes sin handles de cierre, cleanup no confirmado y se detiene
la campaña, sin señales a esos PIDs. Solo raíces directas fixture/coordinador y
proxy tienen cierre confirmado. No habilitar recetas multiproceso generalizadas
sin resolver su ownership/lifecycle. VM/swap inicial/final whole-host se conserva
separado; tiempos de colección y muestreo best-effort no equivalen a peak real.
