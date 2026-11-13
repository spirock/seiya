# S4-08 · prueba pública aislada del lector

## Ejecución registrada

2026-11-09:3/3 fuentes leídas y texto extraído,673ms total; sin modelos, cookies,
Safari, retries, redirects observados o cambio del servicio. Reporte y procedencia:
[evidencia real acotada](../contracts/benchmarks/web-reader/v0.1.0/README.md).
Esto no acredita resúmenes útiles o estabilidad prolongada. La prueba normal del
reporte histórico no vuelve a consultar Internet. S4-08 cerrada en ese alcance;
una repetición real vuelve a requerir permiso vigente y comprobaciones previas.

## Procedimiento

Harness `crates/seiya-server/tests/authorized_reader.rs`, ignored en la suite
normal. El ensayo quedó limitado a tres URLs fijadas en código: example.org/, tutorial
Python3 e instalación del libro Rust. Hasta3 páginas,2 redirects por página,
45s totales y caps del contrato. Sin modelos, navegador, cookies, login, proxies
de entorno, otra fuente elegida por modelo o descarga persistente de documentos.

Precondiciones: permiso vigente para esa lista y revisión de memoria/procesos;
no competir con servicios ni cierre automático de apps. No se necesita cargar
MLX, lease GPU o abrir un nuevo servidor: se invoca la librería Rust en proceso.
La autorización de red no permite bajar gates de cargas de modelos ni acredita
ausencia de OOM. Sin presión normal no ejecutar compilaciones pesadas.

Pruebas offline primero:

```sh
cargo test -p seiya-server --test authorized_reader --locked
```

La prueba real solo se ejecuta tras aprobación y con destino absoluto nuevo bajo
el temporal OpenCode aprobado o directorio privado seleccionado por el operador:

```sh
SEIYA_ALLOW_READER_LIVE=1 SEIYA_READER_LIVE_OUTPUT=/ruta/privada/nueva.json \
  cargo test -p seiya-server --test authorized_reader --locked \
  authorized_three_public_sources_without_models -- --ignored --exact --nocapture
```

No copiar literalmente la ruta de ejemplo, no convertir la variable de entorno
en permiso del modelo y no lanzar automáticamente desde `make test`. Destino
create_new y0600 en Unix, sin reemplazar evidencia previa. Escribe un marcador
incompleto antes de red; después sustituye solo su propio contenido por el reporte.
Una interrupción puede dejar marcador/JSON inválido: no tratarlo como éxito ni
completar métricas con ceros. Las operaciones fallidas conservan contadores
observados y estado cerrado, sin mensajes crudos del servidor, IPs, cuerpos,
consultas personales o cabeceras. Las URLs públicas se guardan por consentimiento.

Los bytes son HTTP entregado al parser, no tráfico cifrado/TLS readahead/kernel.
Registro de métricas/estados no es almacenamiento de artículos. La cota de texto
20000/página y30000 total se aplica al conteo de texto disponible en este ensayo.
Un timeout deja cuarentena y se omiten las fuentes restantes: no crear otra
instancia para eludirlo. No hay retries ocultos ni fallback a curl/webfetch.

**Que el test pase solo acredita que escribió el reporte.** Revisar cada estado:
ok implica transporte/extracción de texto útil según la heurística, no calidad
de síntesis, actualidad, soporte de citas, todas las webs o cancelación real.
No se afirma cierre independiente del worker DNS del sistema; ante incertidumbre
no se reutiliza. El proceso de prueba termina, sin servicio persistente o perfil
de Seiya/OpenCode modificado. Ejecución real y fallos se registran en el sprint.
