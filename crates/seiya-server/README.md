# seiya-server · experimental, cierre Sprint 01

El binario arranca en **discovery por defecto**. Una configuración explícita con
`runtime` activa chat JSON/SSE bufferizado mediante un worker y cola limitada. No sustituye
`seiya` ni ejecuta herramientas nativas. Se ha probado con transporte simulado y
backends HTTP sintéticos y **dos smoke tests reales aislados con OpenCode/Gemma**:
lectura, edición y tests. No se promociona todavía como proveedor operativo.

## Funciones implementadas

- CLI `serve --config PATH`, `check-config --config PATH`, help y versión.
- Configuración JSON estricta de hasta 64 KiB, con dirección explícita.
- Bind únicamente `127.0.0.1`; puerto de ejemplo **8012** (8010 reservado a Python).
- `GET /health` y `GET /v1/models`, comparados con las fixtures compartidas.
- Guardas Host/Origin, auth Bearer opcional y respuestas JSON/no-store.
- Header `x-seiya-experimental: discovery-only` o `chat-buffered-sse`, según modo.
- Cierre ordenado con Ctrl+C y SIGTERM en Unix. Ningún servicio de inicio instalado.

## Adaptador y admisión

- `runtime::RuntimeConfig`: configuración estricta e independiente del listener.
  `runtime.example.json` es un ejemplo de esa parte, con checkpoint placeholder;
  **no se pasa solo a la CLI**. `chat.example.json` muestra el campo `runtime`
  anidado en la configuración del servicio. No se carga automáticamente.
- `runtime::RuntimeTransport`: interfaz async inyectable; `HttpTransport` usa
  HTTP/JSON solo a `127.0.0.1:<puerto>/v1`, sin DNS, proxies, redirects, retries,
  credenciales URL, query ni fragment. Backend `mlx`; Ollama no admitido todavía.
- `inference::InferenceAdapter`: un worker, validación de petición/respuesta,
  techo de salida, historial/tools preservados, uso y completion pública `pegassus`.
  Nunca ejecuta las tools de OpenCode ni interpreta prosa JSON como órdenes.
- El adaptador retorna JSON completo; el backend siempre recibe `stream:false`.
  La API admite `stream:true` como SSE bufferizado, no tokens en vivo.
- Timeout 1–600 s, entrada serializada 256 KiB y respuesta de transporte 8 MiB
  máximos. HTTP limita antes de parsear y exige framing/JSON válidos.
- Un adaptador compartido y un scheduler: una generación y dos esperas FIFO,
  120000 ms por defecto. Cola llena/espera agotada: 429. Descartar un future que
  espera libera capacidad. No es un scheduler entre procesos.
- Si el consumidor desaparece, la tarea propia conserva su lease. Un error de
  transporte/timeout deja el adaptador bloqueado para futuras llamadas (503):
  no se puede asegurar que el backend haya parado. `requires_recovery` indica
  esa incertidumbre, no una medición de la GPU. Verificar que el runtime esté
  libre antes de recrear el adaptador; no reciclarlo por petición ni matar MLX.
- Una respuesta JSON recibida pero semánticamente inválida da error sin inventar
  éxito. Su recepción finalizada permite liberar la exclusión del worker.
- S2-01: `max_tokens` o `max_completion_tokens`, enteros 1–4096. Si vienen ambos
  deben coincidir; valores distintos dan 400 antes del runtime. Sin ambos se usa
  config; con uno se aplica el mínimo entre cliente y `max_output_tokens`.
  MLX recibe solo `max_tokens` efectivo. Null/booleanos/decimales son inválidos.
- S2-02: `store` solo admite false (o ausencia); true/null/otros dan 400. No
  ofrece almacenamiento de completions ni controla logs/cachés del backend o
  sesiones OpenCode. La opción no se reenvía al runtime.
- `stream_options` solo con `stream:true`: objeto con clave opcional booleana
  `include_usage`. Ausente/vacío/true conserva usage final; false lo omite de SSE.
  Extras y tipos inválidos dan 400 antes de admisión. JSON sigue incluyendo usage.
- Campos raíz permitidos: model/messages/tools/temperature, ambos aliases de
  presupuesto, swarm_mode/stream/stream_options/store. Otros campos se rechazan
  sin reflejar claves/valores; no se ignoran ni reenvían como capacidades MLX.
- Validación más estricta de role/content/finish reason y contadores enteros no
  negativos que algunas aceptaciones accidentales de Python; se documenta como
  diferencia, no como paridad de todos los valores malformados.

No competir con el coordinador Python sobre el mismo runtime. El arranque no
comprueba ni contacta al backend: las llamadas empiezan solo por una petición
de chat válida, con `runtime` configurado. No se ejecutan herramientas en el host.

## Configuración opt-in de chat

La CLI conserva `serve --config PATH`. Sin `runtime` mantiene discovery/501.
`chat.example.json` es una plantilla: editar una copia, elegir la ruta de pesos
**ya descargados** y un runtime local aislado/autorizado. No ejecutar la plantilla
contra el runtime en uso del usuario. No se configura OpenCode con este ejemplo.

- `runtime`: objeto de `runtime.example.json`; backend mlx solamente.
- `queue_wait_ms`: 1–120000, default 120000.
- `request_read_timeout_ms`: 1–30000, default 30000.
- `runtime_status`: booleano, default false; true requiere `runtime`. Solo errores/
  observaciones opt-in descritos abajo, no supervisor de memoria o carga automática.
- Listener y runtime deben tener puertos distintos; se verifica también después
  de elegir un puerto efímero. No takeover ni fallback a otro endpoint.
- El identificador de modelo es configuración confiable, no un valor arbitrario
  enviado por el cliente. El backend externo puede resolver/cargar identificadores
  según sus propias reglas: usar pesos existentes y restringir su red si se exige
  offline. Seiya no descarga pesos ni controla todas las políticas del backend.

Framing: Content-Length único de 1–262144 bytes, sin Transfer-Encoding ni
compresión. Content-Type `application/json`, con charset UTF-8 opcional. 415
por media type/encoding no admitido; 400 por framing/JSON inválido; 408 si no
termina la lectura. A diferencia de Python, se valida Content-Type de entrada.
Hay ocho lectores de cuerpo simultáneos como máximo; saturación da 429 antes
de parsear, separada de la cola de inferencia. Health `busy` mide scheduler/
adaptador, no lectores ni VRAM. La espera y el timeout del runtime son presupuestos
separados; ajustar el deadline del cliente si se usa el máximo de ambos.

Al cerrar, se detiene admisión y se rechazan las esperas, pero se drena el
trabajo activo, incluso si su consumidor desapareció. TCP cerrado no garantiza
cancelación del handler: puede seguir hasta resultado/deadline. Si el runtime
queda incierto, el cierre devuelve error y no afirma que MLX haya parado. No hay
todavía un endpoint de cancelación por sesión ni reinicio automático del backend.

## Compilar y comprobar

Desde la raíz del proyecto:

```sh
cargo test --workspace --locked
cargo build -p seiya-server --release --locked
target/release/seiya-server --help
target/release/seiya-server --version
```

El ejecutable es `target/release/seiya-server` en macOS. El crate servidor declara
Rust 1.89 mínimo por sus dependencias actuales; se prueba con Rust 1.93.1, no
con el MSRV exacto. El crate de contratos conserva su declaración 1.85.
No contiene Python ni pesos. Compilarlo en este Mac no acredita
funcionamiento en Windows, Linux, iOS u otras arquitecturas.

## Arranque manual aislado

`config.example.json` solo contiene dirección y **nombre** de variable de entorno,
no la clave. Preparar una clave local temporal sin escribirla en el JSON:

```sh
export SEIYA_RUST_API_KEY="$(openssl rand -hex 32)"
target/release/seiya-server check-config --config crates/seiya-server/config.example.json
target/release/seiya-server serve --config crates/seiya-server/config.example.json
```

En otra terminal que tenga esa variable disponible:

```sh
curl -H "Authorization: Bearer $SEIYA_RUST_API_KEY" http://127.0.0.1:8012/health
curl -H "Authorization: Bearer $SEIYA_RUST_API_KEY" http://127.0.0.1:8012/v1/models
```

Si se declara `api_key_env`, la variable debe existir y contener de 1 a 4096
caracteres ASCII visibles sin espacios. Si falla, el servidor no abre el puerto.
Se rechaza un campo `api_key` con el valor embebido. No hay flag CLI de secretos.

Para una prueba local sin auth, un archivo separado puede contener solamente:

```json
{"listen": "127.0.0.1:8012"}
```

Eso permite acceso a otras aplicaciones locales; no es autenticación. El modelo
y las herramientas futuras no quedan habilitados por usar este archivo.
`listen: 127.0.0.1:0` elige un puerto efímero que se muestra al arrancar.
Si el puerto elegido está ocupado, se aborta sin cerrar procesos ni buscar otro.

## Estado de endpoints y límites

| Endpoint | Comportamiento actual |
|---|---|
| `GET /health` | Estado HTTP, busy según cola/worker y runtime incierto si procede |
| `GET /v1/models` | Única identidad `pegassus`, proveedor `seiya` |
| `POST /v1/chat/completions` | JSON o SSE bufferizado con runtime explícito; 501 sin runtime |
| `/`, `/api/chat`, `/native/v1/*` | HTTP 404; no hay UI ni protocolo nativo |
| Método no admitido de una ruta existente | HTTP 405 JSON |

`/health` no garantiza que el runtime esté listo. Las fixtures JSON/SSE de chat
se prueban con el router Rust y transporte simulado; también hay pruebas TCP
contra backends sintéticos. Python sigue siendo la opción funcional para OpenCode.

No se ofrece cancelación real del modelo, control de otras instancias/procesos,
permisos del SO o broker de herramientas nativas. No publicar este servidor en
la red ni ponerlo delante de herramientas sensibles como si fuera un broker terminado.

### Disponibilidad observada opt-in · S4-02a

Configurar `"runtime_status": true` en la raíz de un perfil experimental separado
habilita `/health.runtime_status` versión1 y errores estructurados. No cambia el
servicio Python en8010, ni configura/reinicia OpenCode. No habilitar sobre un
runtime compartido sin autorización y exclusión. Default false conserva el wire.

- Health no conecta al backend: observaciones cached de último intento,
  `not_checked`, `request_in_flight`, `last_request_completed`,
  `connection_not_established` o `unconfirmed`. `model_residency` siempre unknown:
  respuesta completada no prueba que los pesos sigan residentes. No hay expiración
  ni comprobación de actualidad; no usar esta observación como lease o preflight.
- `admission`: may_attempt, blocked o closed. may_attempt solo significa que el
  camino permite intentar una petición con su cola; **no permiso de memoria**.
  `memory_admission:not_integrated` declara esa limitación de este incremento.
- Error de conexión antes del envío (no timeout):503/code runtime_unavailable y
  detención de nuevas admisiones. Sin retries internos, reconexión automática,
  sondeo de /models, reset HTTP o Retry-After inventado. Las esperas ya admitidas
  tampoco repiten el backend tras ese fallo.
- Timeout/error ambiguo después de posible envío: primer502 con code
  runtime_state_unconfirmed; nuevas peticiones503, cuarentena. `active_requests`
  null indica que no se conoce la finalización, no cero. Desconectar al consumidor
  conserva el lease y nunca equivale a cancelar MLX.
- Se requiere recuperación explícita por operador tras verificar estado del
  backend y aislamiento, recreando la instancia experimental. No interpreta
  conexión rechazada como presión de RAM ni afirma cleanup de procesos externos.
- Fixture: `contracts/openai-compat/v0.1.0/rust-runtime-status.json`; pruebas con
  mocks/HTTP efímero propio. El puente UI opt-in se describe abajo; no admisión por presión, probes o
  supervisor owned en ese flag CLI. No dar S4-02 completa por esto.

### Guardia suplementaria de presión · S4-02b.1

Solo opt-in de librería: `Scheduler::new_with_memory_guard` requiere un adapter
con runtime_status. `seiya_observe::session_guard::MemoryGuard` reutiliza el
LoadMonitor existente; el productor confiable inyecta muestras con el mismo
origen de reloj monotónico. No hay actualización de muestras por HTTP, flag JSON
de permiso de memoria, probe automático, carga o parada de procesos.

Inicialmente bloquea sin observación; una muestra normal/fresca solo deja pasar
los demás controles externos. No equivale a preflight2 GiB, reserva, clearance,
presupuesto seguro o autorización. Muestra>2 s, gap>5 s, error, reloj inválido o
presión no normal detienen admisión de forma terminal; no rearman al recuperarse.
Antes de admitir y despachar se revalida. En cola/generación hay comprobación de
caducidad cada250 ms best-effort, más aviso inmediato de stop observado.

Runtime_status con guardia es versión2, memory_admission objeto versión1 con scope
stop_only_pressure/preflight external_required. Sin guardia permanece versión1
y string not_integrated de S4-02a. Health puede caducar la observación al consultar
reloj, pero no sondea host ni confirma residencia del modelo.

Si una generación ya empezó, retiene slot/worker hasta resultado/deadline. Un
stop suprime éxito tardío y devuelve503/runtime_memory_blocked; timeout conserva
además incertidumbre del adapter. Una observación inexistente antes de despachar
devuelve503/memory_observation_unavailable. Si falla justo antes del primer poll,
no inicia una generación para poder drenarla. No retries ni Retry-After.

Este drenado **no confirma shutdown del backend/descendientes ni liberación de
lease de proceso**: el transporte HTTP no es propietario. El núcleo supervisor
de S4-02b.2 ya se ensaya con capacidades/identidades/lease inyectados, incluyendo
cola, stop y cuarentena. Lease nativo macOS ensayado aparte con archivos/flock
reales, origen/versionado explícitos y runtime falso. Child de helper auditado
leaf se probó aparte con origen/versionado3 y presión inyectada.
Eso no configura ni activa el launcher en el servidor. Puente/UI opt-in añadido
aparte; no enlaza la CLI con el supervisor. Observador macOS read-only ya probado
con helper auditado y lease real (Status4, no health). El binding restringido
posterior Status5 se describe abajo; compatibilidad MLX real aún no probada.
Default operativo intacto. Fixture: `rust-memory-guard.json`; tests solo inyectados.

### Puente Python/web opt-in · S4-02c

Cuando runtime_status está activo, `/health` añade runtime_bridge versión1,
model_binding hex-utf8-v1. El cliente exige ese handshake más el header experimental;
no basta un health200 ni otro servidor Rust antiguo. No son versiones del protocolo
nativo, chat OpenAI o snapshots del supervisor.

Header opcional `x-seiya-model-binding` = hexadecimal minúsculo del UTF-8 del
modelo backend. Host/Origin/auth preceden; binding inválido/duplicado o sin opt-in
devuelve400 antes de body/cola/backend. No contiene credenciales y no autoriza
cargar pesos. Sin header se conserva compatibilidad de los clientes existentes.

Python runtime_bridge copia tools sin ejecutarlas, no usa runtime directo como
fallback y bloquea el enlace incierto sin retries. UI consulta estado autenticado
sin generación y preserva borradores. Fixture/versiones/configs separadas en
`contracts/openai-compat/v0.1.0/runtime-bridge.json`; ADR0026 y swarm-app/README.md
describen operación/rollback. Suite normal sintética y smoke Python→Rust→runtime
falso separados; no integración con servicios activos ni perfil estable MLX.

### Chat con runtime propio restringido · librería opt-in

`BoundServer::bind_with_memory_guard(config, guard)` requiere runtime_status y
runtime configurado antes de abrir listener. Comparte el mismo MemoryGuard del
launcher/supervisor propio: no introduce campo JSON o permiso desde un cliente.
Sin observación rechaza antes de conectar; stop observado bloquea cola/dispatch
y suprime éxito tardío conservando propiedad de la inferencia hasta final/deadline.
La CLI normal sigue usando bind sin guardia/launcher. No cambia el perfil activo.

`seiya_observe::session_runtime` añade perfil macOS sin fork (ADR0028), original
Child+lease y listener propio verificado por libproc. Snapshot supervisor5 no es
health.runtime_status5: HTTP mantiene versión2 con guardia y residencia unknown.
Una prueba nativa separada unió runtime HTTP sintético restringido, texto/tools,
guardia/scheduler, stop por presión, reaping y release/reacquisición del lease;
HTTP posterior503/runtime_memory_blocked sin Retry-After o reinicio. Cero tools
ejecutadas. Tests normales comprueban opt-in/guardia antes de backend. Esto no
prueba MLX/Metal, memoria segura, contexto o sesión sostenida: S4-03 sigue pendiente.

## Pruebas y rollback

- Tests config/CLI: defaults explícitos, errores redactados, claves ausentes,
  campos desconocidos y validación sin abrir sockets.
- Tests de router: discovery/errores de guardas usan los JSON de Python;
  se comprueban headers duplicados, origen cruzado y endpoints no disponibles.
- Lifecycle: listener loopback efímero, HTTP real, cierre y puerto ocupado.
- Los tests no acceden al runtime/modelos, Internet, Mail ni archivos personales.
- Adaptador: fixtures de chat/tools/validación contra un transporte simulado.
  Cliente HTTP: backend sintético efímero, forwarding, redirects rechazados,
  límites de respuesta, error HTTP/JSON y timeout. No es prueba de inferencia real.
- Cancelación del consumidor conserva exclusión; deadline y fallo de transporte
  bloquean reentrada. No se simula éxito de una cancelación del modelo.
- Cola FIFO, saturación/timeout, liberación de slots y cierre con trabajo activo.
- Read timeout, ocho uploads detenidos y rechazo del noveno, framing/media types
  inválidos y no ejecución de backend en guardas fallidas. Config runtime explícita.
- SSE: dos chunks y un DONE, IDs/finish/uso/tool indices, múltiples llamadas,
  Unicode/newlines sin inyectar frames, errores JSON antes de headers y recorrido
  TCP con cliente simulado que aporta resultados sintéticos, sin ejecutar tools.

### SSE bufferizado

`stream:true` espera una respuesta completa y válida, luego devuelve
`text/event-stream; charset=utf-8`: un evento con role/content/tool_calls, otro
con finish reason/usage/swarm y un único `data: [DONE]`. Usage se omite si se
solicita `stream_options.include_usage:false`. Cada llamada lleva su
índice y conserva ID/nombre/argumentos. El cuerpo es finito, sin temporizadores
ni tareas de envío; no simula tokens ni mantiene abierta la conexión de eventos.
ID/created/model son iguales en los dos chunks. Errores de guardas, admisión o
backend conservan HTTP status y JSON; no se abre un SSE exitoso en caso de fallo.
El cliente sigue siendo responsable de autorizar/ejecutar herramientas y enviar
los mensajes tool con sus IDs. Seiya no añade un segundo ejecutor.

Parar con Ctrl+C. No hay migraciones, archivos de usuario modificados o modelos
que descargar. El rollback es dejar de usar el perfil/binario experimental;
Python y sus datos no se migran. Si Python estaba apagado, no se arranca solo.
No instalar, copiar a `/usr/local/bin` ni cambiar OpenCode automáticamente.

Dependencias y alternativas: `docs/decisions/0001-servidor-rust-experimental.md`.
Decisiones del adaptador: `docs/decisions/0002-adaptador-inferencia-local.md`.
Admisión/cierre: `docs/decisions/0003-cola-y-chat-http-opt-in.md`.
SSE: `docs/decisions/0004-sse-bufferizado.md`.
Prueba real y límites: `docs/PRUEBA_RUST_OPENCODE.md`.
Operación/rollback y pendientes: `docs/OPERACION_RUST_EXPERIMENTAL.md`.
Presupuesto de salida: `docs/decisions/0005-presupuesto-salida-openai.md`.
S2-01 probado con mocks/router HTTP; no se repitió la prueba real para ese alias.
Opciones adicionales: `docs/decisions/0006-opciones-chat-explicitas.md`.
S2-03 prueba la combinación por TCP con cliente/backend sintéticos propios:
JSON/SSE, tools, presupuestos y errores/truncados. No nueva ejecución real de
OpenCode/modelos. S2-04 especifica la baseline de memoria en
`docs/BASELINE_MEMORIA.md`; runner y campaña aún no implementados/ejecutados.
