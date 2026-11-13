# seiya-observe · interno, Sprint 03

Núcleo puro de observaciones acotadas con proveedor inyectable; módulo separado
`probes` para ejecución explícita opt-in. No carga modelos ni decide políticas
de memoria, y no se activa al arrancar el servidor.
La identidad y ownership de targets deben establecerlos el launcher y
adaptador: pasar un PID al constructor **no demuestra propiedad ni da permiso**.

- RSS KiB a bytes con overflow comprobado; cero observado no es dato faltante.
- Exited irreversible, errores transitorios recuperables, PID/inicio distintos
  y reloj regresivo terminales. Targets/snapshots limitados, sin guardar historial.
- Cada muestra conserva fase, tiempo monotónico, rol e identidad; footprint,
  MLX y Metal unsupported/null, sin sumas engañosas.
- Proveedor falso en tests: no se muestrean procesos reales ni memoria del host.

`cargo test -p seiya-observe --offline`. Contrato interno y límites:
`docs/decisions/0007-nucleo-observaciones.md`.

Preparación offline adicional: `runtime_memory::RuntimeMemoryReport::decode`
valida reportes separados≤4096 bytes del adaptador Python en proceso
`seiya.runtime_memory.RuntimeMemorySampler`. IM-02 lo conecta solo con builder
opt-in `CampaignDriver::with_runtime_memory()`, socketpair privado y wrapper
en mismo Child (ADR0035). Apagado por defecto; no cambia servicio operativo o
probes nativos. Solo contadores MLX active/cache/peak, no RSS o Metal total;
peak con resets externos desconocidos. PID esperado no autentica el canal.
Contrato/rollback: `docs/decisions/0034-contadores-runtime-offline.md`;
pruebas sintéticas en `tests/runtime_memory.rs` y
`swarm-app/tests/test_runtime_memory.py`. No sustituye los null de probes nativos.
El módulo `macos` parsea RSS, vm_stat y swap con límites/formato/unidades
explícitos. Fixtures sintéticas, sin ejecutar ps/vm_stat/sysctl. RSS no aporta
identidad de inicio ni prueba ownership; el adaptador debe verificarla aparte.
Ver `docs/decisions/0008-parsers-macos-observacion.md`.

## Adaptador S3-03

`probes::from_child(&mut child, role, deadline)` exige el handle original de un
proceso creado por el launcher. No acepta un PID suelto. En macOS, conserva
identidad nativa PID/inicio sec-usec y parentesco; descubre solo sus descendientes
(máximo 32), verifica antes/después de RSS y falla cerrado ante cambios.
Fuera de macOS: unsupported, no fallback a ps global.

`for_current_collector(deadline)` registra solo el proceso que ejecuta el código
(`std::process::id()`), para no ocultar el overhead del recolector; tampoco acepta
PIDs externos. El smoke nativo comprueba esta entrada además del Child propio.

`sample().await` entrega un proveedor consumible `Readings` y targets para el
núcleo. El driver integra altas de targets, reloj/cotas y artefactos de campaña;
eso no equivale a inferencia real validada. `sample_system()` es explícito/separado:
VM y swap whole-host no son memoria propia ni RAM disponible. Footprint/MLX/Metal
siguen unsupported. Descendant es un rol genérico, no identificación de OpenCode.

El ejecutor usa solo ps/vm_stat/sysctl con rutas/argumentos fijos, sin shell,
entorno limpio, stdin null, stderr descartado, stdout 16 KiB y deadline 1–2000 ms.
Timeout/cancelación descartan el probe propio con Tokio kill_on_drop/reaping;
el proceso medido nunca recibe señales del adaptador. No se promete interrumpir
un syscall nativo del kernel ni un deadline hard-real-time del host completo.

16 tests adicionales: tablas/ejecutor simulados, pipes en memoria, reloj virtual,
límites/cancelación y construcción de comandos sin ejecutarlos. Smoke nativo
separado, sobre auxiliar/sleep propios, sin modelos o red:

```sh
cargo test -p seiya-observe --test native_smoke native_owned_tree_and_system_probe_smoke --locked -- --ignored --nocapture
```

Este smoke **sí ejecuta probes reales**: solo invocarlo como prueba explícita.
Por defecto ambos tests de ese archivo se ignoran; no forman parte de make test.
ADR y límites: `docs/decisions/0009-adaptador-probes-propios.md`. Smoke básico
pasado en este Mac, no una prueba prolongada de carreras/cancelación nativa o
presión del host. Ese smoke no valida el driver A/B ni una inferencia medida.

## Runner de simulación S3-04

`runner::launch_simulation` ejecuta protocolo ABBA × 3 con Driver inyectado,
fases/observaciones/cuerpos/techo/resultados comprobados y artefactos en directorio
nuevo. Valida tiempo monotónico/roles/identidad, métricas requeridas y trabajo
igual; rechaza finish length y fallos sin continuar otro brazo. Solo normaliza
IDs en una copia analítica, conserva argumentos/historial literalmente.

Manifest/events JSONL/summary son mode simulation; los errores conservan eventos
parciales o marcador aborted. No sobrescribe, borra o migra archivos existentes.
Salida limitada a 32 MiB/10000 eventos, máximo 128 snapshots por fase; no retiene
histórico de métricas completo en RAM. El handle cancela cooperativamente y
la tarea drena la fase en vuelo, pide stop y registra si quedó confirmado.

Nueve tests con driver/tiempo/muestras simulados y directorios sintéticos propios.
No es una campaña HTTP/modelo ni validación de un launcher real. El runner recibe
evidencia del driver, no crea todavía los procesos A/B: ese driver nativo, su
integración con probes/reloj/puertos se añadió en S3-04b, antes de S3-05.
`launch_simulation` prohíbe expresamente drivers no simulados. ADR 0010 explica
contrato, límites y lo que no garantizan los mocks frente a crashes del host.

## Integración nativa S3-04b (en curso)

`FixtureDriver::prepare(Paths, output)` y `launch_fixture_integration` unen
coordinadores Python/Rust propios, backend fijo `seiya-observe-fixture`, proxy
de captura de cuerpos, puertos efímeros verificados mediante el Child original,
SSE acotado y probes macOS. No aceptan endpoint MLX ni ruta de pesos. No ejecutar
binarios no confiables: las rutas son configuración del operador, no un sandbox.

Artefactos `mode:fixture-integration`, `real_host_measured:true` y
`model_inference:false`: RSS y reloj del host son reales; las respuestas/tokens
del backend son sintéticos. No constituyen una baseline de inferencia ni ahorro
de RAM. Versiones/hashes SHA-256 se obtienen antes de medir, incluida la config
sin credenciales; no se garantiza inmunidad ante sustitución posterior de archivos.
El collector incluye el proxy en su RSS. No sumar RSS de varios procesos.

Un adaptador persistente por run descubre descendientes y conserva identidades;
no agrega árboles solapados. Una salida/lectura faltante impide declarar el run
válido. Si aparece un descendiente sin handle de limpieza, la parada del árbol
queda no confirmada y no se inicia otro brazo; no se señala ese PID descubierto.
El driver fixture no soporta todavía recetas con workers hijos adicionales.
VM/swap whole-host inicial/final y duración de colección quedan en diagnostics;
muestreo best-effort 1000 ms/fronteras, no peak instantáneo ni reloj hard-real-time.

Pruebas explícitas ignored, sin modelos (crear rutas nuevas, absolutas):

```sh
cargo build --workspace --locked
SEIYA_FIXTURE_OUTPUT=/ruta/nueva/campaña cargo test -p seiya-observe --test fixture_integration native_fixture_campaign --locked -- --ignored --nocapture
```

Python predeterminado `/opt/homebrew/bin/python3`, modificable mediante
`SEIYA_FIXTURE_PYTHON`. Tests separados de cancelación/arranque parcial requieren
`SEIYA_FIXTURE_CANCEL_OUTPUT` y `SEIYA_FIXTURE_FAILURE_OUTPUT` respectivamente.
No ejecutar todos con paralelismo: `--test-threads=1`. Sin instalar ni restaurar
servicios 8010/8081. ADR 0011. La sección siguiente describe la receta/driver
añadidos después; inferencia MLX y S3-05 siguen sin ejecutar.

## Preparación MLX y driver opt-in

`campaign::MlxRecipe` prepara argv fijos, no ejecuta. Ejemplo:
`contracts/benchmarks/memory-baseline/v0.1.0/mlx-recipe.example.json`. El intérprete
del operador (`python`) ejecutaría el entrypoint como primer argumento, nunca un
shell. Los paths y el umbral del ejemplo son ilustrativos, no recomendaciones
para la memoria de este Mac.

`Provenance::collect` verifica inventarios/versiones inyectados; no los hace
reales. `NativeProvenance::inspect` es una operación opt-in **costosa** que hashea
archivos existentes aprobados y ejecuta solo shasum/Python --version, sin importar
MLX ni arrancar inferencia. Recibe los paths exactos de METADATA instalados de
mlx/mlx-lm/transformers, no escanea entornos personales. Checkpoint plano con
config, tokenizer/template e índice/shards completos, sin Python remoto. Rechaza
symlinks seleccionados; 256 assets/64 GiB y 48 MiB de metadata, deadline global
600 s. No protege frente a cambios posteriores, y METADATA no es hash del paquete
entero. En S3-06a se ejecutó sobre R1 existente solo para hashes/metadata.

Actualización ADR0017: solo `tokenizer.json`/`tokenizer.model` admiten32 MiB;
config/templates/índices y otra metadata conservan2 MiB, agregado48 MiB.
tokenizer.json debe ser objeto JSON UTF-8 completo; se valida sin construir
el árbol de vocabulario. El lector nativo mueve sus bytes, no clona todo el
tokenizador; lectura cap+1/identidad/symlinks siguen comprobadas. Esto es
provenance, no prueba de tokenización o inferencia. La verificación nativa ya
pasó sobre R1 7B existente (hashing/metadata/Python --version), sin importar MLX
o cargar tensors. No se extrapola ese hashing a ejecución ni memoria de un modelo.

`Reservation::acquire` conserva lease flock y dos sockets loopback; no arranca
procesos; el driver puede entregar sus sockets sin liberar el lease. Usar el mismo directorio
compartido de coordinación en todas las campañas, no uno nuevo por run. Un lock
no demuestra GPU libre frente a procesos externos. No borrar el archivo de lock.

`observe_host` implementa probes macOS opt-in de hw.memsize, vm_stat y presión;
`assess` valida observaciones frescas y umbral explícito del operador. Free pages
no son toda la RAM disponible segura. Datos ausentes, presión no normal, falta
de clearance o reserva bloquean readiness. Ni un readiness sin bloqueos concede
autorización de carga: `model_load_authorized` permanece false. Pruebas normales
usan salidas sintéticas, locks y sockets propios; no ejecutan estos probes.

Smoke explícito de hashes/versiones sobre fixtures nuevos propios, sin modelos:

```sh
cargo test -p seiya-observe --lib campaign::tests::native_provenance_hashes_held_synthetic_files_without_loading_runtime --locked -- --ignored --exact --nocapture
```

### CampaignDriver (ADR 0013)

Calibración experimental (ADR0016, **preparada pero no ejecutada con MLX**):
`into_calibration_rehearsal(CalibrationPolicy)` solo acepta un driver de fixtures.
`prepare_calibration_authorized` exige `CalibrationApproval` separado, por receta,
política y caducidad≤15 min. No reutiliza una aprobación normal ni la deserializa
de JSON. `launch_calibration` solo acepta esta ruta; `launch_campaign` la rechaza.
Preparar la política no constituye autorización de carga real.

Una calibración ejecuta solo Rust: listener-ready2 s, un texto≤256 tokens y
shutdown. Manifest detalla `execution` efectiva aparte del plan de referencia;
`calibration-rehearsal`/`model-calibration` nunca se presentan como baseline ABBA.
La única excepción al gate free-pages-only está en esta ruta, con ventana inicial
de3 muestras normales/frescas, reserva y clearance externo. No calcula headroom
ni garantiza capacidad. La receta normal/umbral2 GiB no cambian.

Duración por política30–300 s desde antes de spawn, independiente de hashing y
preparación previos; timers en espera de listeners/petición y watchdog antes de
resto de trabajo. Cancelación/caducidad/presión abortan a cleanup propio, nunca
otro brazo. El límite es cooperativo/best-effort, no hard-real-time ni límite de
RAM; preparación y cleanup tienen sus propios deadlines. No hay reintentos.
Test ignored `native_calibration_exception_is_single_use_and_separate_from_campaign`
requiere `SEIYA_CALIBRATION_REHEARSAL_OUTPUT` nuevo y absoluto; sin MLX ni presión
real. Aprobar preparar esta política no habilita usarla con los pesos del usuario.

Invocación real preparada en `tests/authorized_calibration.rs`, ignored y **no
ejecutada**: exige aprobación específica actual y clearance de runtime externo,
paths absolutos del checkpoint/intérprete/entrypoint/coordinadores/METADATA y un
directorio nuevo. `SEIYA_CALIBRATION_RUNTIME_SECONDS` fija el plazo30–300 s.
Confirmaciones `SEIYA_ALLOW_MODEL_CALIBRATION` y `SEIYA_EXTERNAL_RUNTIME_CLEARED`
son interlocks operativos, no permisos otorgados por variables o por un modelo.
El operador debe autorizar y verificar aislamiento antes de invocarlo. Archivos
parciales y reporte se conservan ante fallo; nunca reintenta ni inicia ABBA.

Presupuesto/watchdog (ADR0015): `LoadBudget::evaluate` enumera categorías
planificadas disjuntas, con importe/fuente y estimate/measured; un faltante no es
cero. Nunca suma RSS+Metal+footprint, devuelve bytes disponibles o autoriza carga.
`logical_kv_bytes` calcula solo tamaño lógico K/V para shapes/batch/tokens
explícitos, sin padding/copias/allocator, no estima por sí solo peak asignado.

`LoadMonitor` es terminal ante presión no normal, fallo de probe, muestra vieja,
gap>5 s o reloj inválido. CampaignDriver consulta presión durante startup,
fronteras/idle/requests (best-effort1 s; probe fijo sysctl≤1 s). Fallos abortan a
cleanup de Child propios, no a siguiente brazo. No garantiza prevenir OOM entre
muestras ni equivale a cancelación propagada al modelo; no relaja `assess`.
Rehearsal usa presión inyectada, no presión real. Test nativo separado
`native_watchdog_stops_load_and_request_admission_with_owned_cleanup` requiere
`SEIYA_REHEARSAL_MONITOR_OUTPUT` nuevo; cubre carga y admisión de petición, no
generación MLX en vuelo. Ningún importe de Gemma se rellena automáticamente.

Revisión de admisión (ADR0014): `review_pressure_window` es **solo diagnóstico**
de3–16 observaciones, ventana≥2 s, gaps≤2 s y último dato≤5 s. Verifica reloj,
presión de toda la ventana, memoria física estable y faltantes. Nunca devuelve
bytes seguros para cargar, presupuesto validado o autorización; no sustituye
`assess`. Este último etiqueta su método free-pages-only: superar2 GiB tampoco
garantiza que quepan pesos/KV/temporales. `tests/authorized_preflight.rs` conserva
serie y diagnóstico junto al gate, sin cargar ni hashear pesos. El diagnóstico
nativo normal no acredita capacidad recuperable ni estabilidad prolongada.

`CampaignDriver::prepare_rehearsal` une preparación nativa y lifecycle con el
entrypoint de test exacto del repositorio: Python hace exec del fixture Rust, sin
importar MLX. Checkpoint/METADATA sintéticos deben estar bajo el padre de salida.
Provenance se vuelve a hashear antes de cada brazo; diferencias abortan antes de
iniciar otro runtime. La memoria de preflight es inyectada y se etiqueta como tal.
`runner::launch_campaign` genera `campaign-rehearsal`, no simulation/model-campaign.
Model ID backend viene del path de checkpoint; identidad pública sigue pegassus.

`prepare_authorized` requiere `ModelLoadApproval` del operador para esa receta y
plazo (máximo 1 h). No deserializable de HTTP/modelos ni equivalente a readiness;
confirmarlo solo tras autorización real de carga y clearance de runtimes externos.
La ruta usa `NativeProvenance`/`observe_host` y `model-campaign`. Es implementación,
**no validación de MLX**; no se ha ejecutado ni autorizado S3-05. No hay CLI de
carga automática ni instalación. La interfaz actual es librería Rust.

Lease retenido toda la campaña, sockets entregados justo antes de spawn y Child/
listener verificados; no adopción/reintentos. Startup ≤720 s, hashing ≤600 s,
requests siguen acotadas independientemente. Cancelación revisada después de
preparación y antes de spawn/nuevas requests; trabajo en vuelo se drena. Errores
conservan preflight/provenance y requests parciales antes de limpiar. Descendientes
sin handles de cierre siguen impidiendo declarar cleanup confirmado.

Regresiones nativas ignored en `tests/campaign_rehearsal.rs`: ABBA completo,
cancelación/lease, preflight bloqueado antes de hijos, provenance cambiada entre
brazos y arranque parcial. Requieren directorios absolutos nuevos mediante
`SEIYA_REHEARSAL_OUTPUT`, `SEIYA_REHEARSAL_CANCEL_OUTPUT`,
`SEIYA_REHEARSAL_BLOCKED_OUTPUT`, `SEIYA_REHEARSAL_CHANGE_OUTPUT` y
`SEIYA_REHEARSAL_FAILURE_OUTPUT`. Ejecutarlas explícitamente y en serie
(`--test-threads=1`), nunca confundir sus tokens sintéticos con inferencia real.

## Evidencia de sesiones · S4-01

`session_evidence` es una librería opt-in separada del runner ABBA. Tipos estrictos
`chat-evidence/0.1.0`, solo modo synthetic, métricas null+motivo, resumen sin textos
personales e índice derivado. No integra el chat activo ni carga un runtime.
Workload congelado: diez turnos de memoria y diez casos de resumen con fuentes
ficticias; son rúbrica/entrada, no evaluación de un modelo ejecutada.

`Store::create` exige directorio nuevo absoluto; `publish` sincroniza un temporal
y publica por hard-link sin overwrite. Directorio0700, archivos0600 en Unix.
Una escritura interrumpida conserva temporal contabilizado y reportado; un error
IO después de publicar puede dejar un registro válido: consultar índice por ID,
no reintentar con nuevo ID para esconder ambigüedad. Sin garantías de atomicidad
de un filesystem que no soporte hard-links; en tal caso retorna error.

Cotas:32 registros/128 eventos por registro/256 KiB por archivo/8 MiB agregados,
temporales incluidos. `rebuild_index` deriva índice en memoria; `Store::read_index`
permite recuperación read-only explícita, sin obtener writer sobre datos existentes.
Rechaza symlinks finales/hijos, archivos no regulares, extras y directorios compartidos.
No es sandbox frente a un proceso hostil del mismo UID que cambie ancestros.
No escanea disco, limpia evidencia previa o almacena URLs/prompts automáticamente.

Datos/contrato: [chat-session/0.1.0](../../contracts/benchmarks/chat-session/v0.1.0/README.md).
Catorce tests sintéticos de formato, privacidad, cuotas, recuperación, interrupción
y conflicto de commit. S4-02/03 integrarán readiness y medición real por separado.

## Guardia de presión de sesión · S4-02b.1

`session_guard::MemoryGuard` comparte LoadMonitor y sus límites2 s/5 s con la ruta
de campañas. Scope stop_only_pressure, nunca permiso de cargar o asignar memoria.
Inicial awaiting_observation; observaciones confiables con reloj monotónico propio
o Clock inyectado. El módulo no deserializa datos del cliente ni lee host/modelos.
Warning/critical/unknown/error/caducidad/reloj regresivo paran terminalmente.
`check` verifica antigüedad y `wait_blocked` recibe stop o comprueba cada250 ms
mientras es sondeado; no hay daemon autónomo. `status` usa reloj, no probes.

Consumidor nuevo: constructor opt-in de scheduler Rust. Esa dependencia es interna
de workspace, sin paquetes externos nuevos. Preflight/presupuesto/reserva y
ownership del proceso continúan externos; no se habilita el camino de campañas
reales o una excepción de calibración. Binding nativo de supervisor/cleanup pendiente;
el núcleo por inyección se describe a continuación.

## Supervisor de capacidades propias · S4-02b.2

`session_supervisor::Supervisor::start_injected` posee un runtime y un lease
suministrados como capacidades Rust confiables, no IDs/órdenes del modelo. Modo
fijo injected-supervisor/model_inference false para runtime/reaping, sin runtime
nativo, CLI o
endpoint de control. Este módulo no carga, descubre ni mata procesos reales.

Actualización: el launcher de fixture nativa descrito al final sí posee un Child
real de helper auditado; no es el constructor inyectado ni un launcher de modelos.

El actor alimenta MemoryGuard incluso en idle (intervalo1 s, probe con deadline),
acota duración y cleanup, y cierra guardia antes de limpiar. Drop del consumidor
pide stop sin abortar el actor. El manifiesto Ownership está limitado a32 miembros
con identidad fuerte y sin PIDs duplicados. Cleanup requiere raíz original,
miembros reaped exactos y cobertura completa: PID reciclado, miembros omitidos,
duplicados o descendientes sin control impiden liberar lease.

Solo después de esa prueba llama OwnedLease::release. Fallos/plazo/resultado
incierto dejan estado quarantined y recursos retenidos. Un dueño puede solicitar
cleanup de nuevo, máximo3 intentos totales, canal1; nunca retry de inferencia.
Si reaping ya fue confirmado y solo falló release, no vuelve a señalar procesos.
Release debe ser idempotente/cancellation-safe y no suspender tras desbloquear
antes de informar éxito; nunca unlink del lockfile. No son garantías de un
adaptador de OS por tener traits: el núcleo inicial se prueba con capacidades falsas;
el lease nativo ensayado posteriormente se distingue abajo.

**Límite de seguridad:** si el actor pierde todos los consumidores permanece
esperando con recursos retenidos, sin busy loop. Ante pánico/teardown del executor,
el resguardo RAII cierra guardia sin consultar reloj y retiene deliberadamente
capacidades sin ejecutar Drop que pudiera desbloquear. Se pierde recuperación
en ese último caso y pueden consumirse recursos hasta salida del proceso. No es
durabilidad ante crash; no reiniciar para saltarse la cuarentena. Los tests de
esa retención usan objetos falsos acotados, sin Child/FD/lock nativo filtrado.

El manifiesto de este núcleo es fijo; no soporta registrar nuevos descendientes
dinámicos. El binding nativo deberá resolver esa cobertura/identidad/handles antes
de activarse, sin sustituirla por HTTP health o bool. Lease real probado aparte.
No basta un root reaped si quedan hijos desconocidos. No hay reset de adapter
HTTP: incluso tras prueba de cierre sigue incierto hasta recuperación explícita.

Fixture `supervisor-status.json` en chat-session/0.1.0. Catorce tests del núcleo y
un ensayo scheduler/guardia/supervisor/lease con cierre completo e incompleto,
todos inyectados. No validan shutdown de MLX, procesos reales, presión del host,
perfil estable,2 GiB disponibles o menor consumo. El núcleo por sí solo no cierra
S4-02; actualización del binding restringido al final de este documento.

### Lease nativo macOS · actualización 2026-10-26

`Reservation::into_session_lease` transfiere la reserva original a
`session_lease::NativeSessionLease` sin abrir otro lock ni liberar puertos. No es
permiso de cargar y no cambia acquire/rearm/Drop de campañas. `verify_binding`
comprueba dev/inode de directorio/file/FD, tipo regular, sin links extra/symlink,
UID efectivo, lock privado y directorio sin escritura de grupo/otros. Un binding
fallido al convertir no se re-vincula automáticamente; preparar/verificar antes
de cualquier carga. Un lease ya liberado no pasa verify_binding.

`OwnedLease::release` solo se invoca por el actor después de reaping válido.
Comprueba flock LOCK_UN y descarta reserva/puertos en el mismo poll sin await;
idempotente, nunca unlink/truncate ni señales. Ante error conserva la reserva.
Cancelar future sin poll no tiene efecto. Repetir release tras obtener otra
reserva no toca el descriptor nuevo. El supervisor conserva este wrapper en
cuarentena con el mismo resguardo RAII; los tests de teardown siguen usando fakes
para no filtrar FD nativos deliberadamente.

Status con lease nativo es versión2/lease_origin native_coordination_lock; mocks
conservan versión1 sin campo nuevo. Runtime/reaping continúan inyectados, por eso
mode injected-supervisor y model_inference false siguen presentes. No confundir
ese Status con health HTTP runtime_status versión2: son contratos distintos.

Siete tests nativos de filesystem/flock sobre directorios NUEVOS bajo target:
exclusión/release, idempotencia, future sin poll, sustitución de archivo/directorio,
symlink/hardlink/permisos, binding inicial desconocido y cuarentena/recuperación
del actor con runtime falso. El archivo persiste durante toda la exclusión; la
limpieza de fixtures solo lo elimina tras cerrar/liberar todas sus reservas.
No usan directorio de coordinación operativo, MLX, Child, procesos personales,
probes de memoria o red externa. Fixture native-lease-status.json guarda alcance.

Límite: cooperación en filesystem local, no sandbox frente al mismo UID cambiando
rutas entre syscalls o creando otro lockfile. Detectar reemplazo retiene el inode
original, no impide la creación de un lock paralelo por un actor no cooperativo.
La operación síncrona no tiene garantía hard-real-time. Child/descendientes,
observador nativo, bridge/UI y estabilidad con modelos siguen pendientes.

### Child nativo de helper auditado · actualización 2026-10-26

`session_native_fixture::start` es un launcher de prueba, no MLX: requiere binario
del repositorio elegido por operador y lease nativo ya preparado. Argumentos
cerrados, entorno vacío, pipes propios; no shell/HTTP/model commands. Valida tipo,
UID/permisos/nombre del ejecutable, no autentica su procedencia ni resuelve carreras
de un actor hostil del mismo UID. No pasar contenido del modelo a este constructor.

Helper nuevo sin fork/red/modelos, handshake fijo por pipe no bloqueante64 bytes/
1 s, EOF cooperativo y variante ignora EOF para probar SIGTERM. Autolímite10 s
implementado como resguardo del helper; no sustituye watchdog ni aprobación.
PID/start marker proceden de Child/proc_pidinfo, no del texto impreso. Inspección
de árbol mediante probes::inspect_targets no ejecuta RSS/VM/otros comandos.

Stop: verificar Child original, cerrar stdin, recoger salida con try_wait; si
tras100 ms sigue vivo, identidad coincide y no hay hijos no controlados, SIGTERM
solo al original aún no recogido. Nunca killpg, PID libre o repetir señal tras
reaping. Coverage desconocida queda terminal y no libera lease. Las instantáneas
no prueban ausencia histórica de fork: Complete solo es válido para este helper
auditado leaf. No se ha probado contención de árboles nativos arbitrarios.

Preparación fallida intenta cerrar/recoger recursos propios. Si no se confirma o
se cancela bootstrap, retiene Child/lease fail-closed, pudiendo consumir FDs hasta
salida del padre; no hay recuperación automática de ese resguardo. Antes de uso
operativo hace falta resolver esa recuperación y binding del runtime real.

Status versión3/native-fixture-supervisor con runtime_origin native_fixture_child,
pressure_origin injected_fixture, lease_origin explícito y model_inference false.
No es health HTTP ni prueba de presión del host. Constructores inyectados v1/v2
siguen iguales. Fixture native-child-status.json y un test normal nuevo de árbol
con raíz reciclada/sin comandos; smoke de procesos separado ignored:

```sh
SEIYA_NATIVE_FIXTURE_OUTPUT=/absolute/new/fixture-output cargo test -p seiya-observe --test native_session_fixture record_owned_fixture_child_and_lease_without_models --locked -- --ignored --exact --nocapture
```

Ejecutado explícitamente con salida nueva aprobada: EOF, SIGTERM único, deadline
idle2 s, handshake inválido y ejecutable ausente/symlink rechazados. Child reaped
y reacquisición de lock comprobados, presión inyectada, sin GPU/modelos/servicios
operativos. No acredita MLX, observador real, sesiones largas o ahorro de RAM.

### Productor de presión macOS · actualización2026-10-26

`session_pressure::sample` lee solo el selector fijo de sysctlbyname, i32 de tamaño
exacto, sin shell/subprocess. Error/tamaño inválido falla; valor desconocido no se
convierte en normal. Timestamp del mismo guardia **anterior** al syscall; no renovar
una muestra lenta. La llamada kernel es síncrona: no se promete plazo preemptivo
si el SO la bloquea. Sigue siendo stop-only, no presupuesto/preflight/autorización.

`session_native_fixture::start_with_native_pressure` conecta esa lectura al helper
auditado, verificando identidad/árbol antes/después. Status4, pressure_origin
native_macos_sysctl, runtime_origin native_fixture_child, model_inference false.
Fixture native-pressure-status.json; no cambia health, Python o CLI de servidor.
ADR0027 documenta alternativas y límites. Tests normales solo resultados inyectados.

Smoke ignored separado `native_pressure_stops_only_owned_leaf_or_reaches_deadline`
requiere SEIYA_NATIVE_PRESSURE_OUTPUT absoluto/nuevo. Solo lee el host y maneja su
helper propio; no induce presión, toca procesos del usuario o carga modelos.
Ejecutado: host warning, guardia detuvo helper por EOF, reaped y lease liberado/
reacquirido. No acredita cierre MLX, árbol arbitrario, Metal o capacidad de carga.

### Runtime propio restringido · S4-02 · ADR0028

`session_runtime` (solo macOS) añade el binding opt-in, separado de los helpers
anteriores y del servicio activo. `/usr/bin/sandbox-exec` aplica perfil fijo deny
process-fork; `seiya-session-bootstrap` comprueba EPERM en fork y posix_spawn antes
de reemplazar su propia imagen mediante exec. Child/PID/start marker originales
se conservan; si la política falta/falla, no hay fallback sin ella. Comprobada
también herencia tras exec. La restricción no es sandbox de datos/red/XPC/Metal.
Runtime/código del operador deben ser confiables; no órdenes de modelo/HTTP.
Herramienta del SO deprecated: fallo cerrado si deja de funcionar, no soporte iOS.

`native_owned_socket.c` es shim libproc mínimo sobre headers del SDK: consulta
solo FDs acotados del PID propio para probar listener TCP127.0.0.1/puerto esperado.
No lsof/shell/helpers nuevos para sondeo ni structs de ABI adivinados en Rust.
Build con cc/ar del SDK existente (arm64/x86_64 seleccionados, solo este host
ejecutado); sin deps Cargo/paquetes nuevos. Puerto ocupado ajeno no acredita ready.

Presión/guardia antes y durante arranque, identidad/árbol antes/después de sondeos;
handshake limitado y startup acotado. SIGTERM solo al original aún no reaped, una
vez; sin SIGKILL automático. Proof completo solo con restricción confirmada y
salida recogida, sin hijo inesperado. Timeout conserva lease en cuarentena;
recuperación explícita acotada, no retry de inferencia. Cancelar bootstrap sin
proof puede retener capacidades hasta salida del padre, como ADR0023; no crash
durability ni recuperación automática de ese caso.

`start_rehearsal`/`start_rehearsal_with_behavior` aceptan solo binarios del repo y
opciones cerradas para runtime sintético, no rutas de pesos. `start_mlx(MlxLaunch,
guard, policy)` es ruta real **no ejecutada**: exige ModelLoadApproval de receta
exacta/vigente y clearance/revisión humana, presupuesto completo ligado al SHA256
del índice de pesos, provenance nativa, ventana normal/preflight2 GiB, total
planeado con margen≤páginas libres y reserva original. No prueba capacidad segura;
context_tokens es plan, no límite de tokens aplicado por este launcher. Aprobación
vencida detiene también guardia compartida mediante stop_at_millis (solo acorta).
No generar aprobación real por haber pasado tests o usar modelos personales aquí.

`Supervisor` Status5/native-restricted-supervisor distingue origen fixture/MLX,
presión nativa/inyección y containment macos_process_fork_denied_v1. Fixtures
model_inference false; MLX null/no observado, no true por arrancar. Tipo Rust
Option<bool>, JSON1–4 intacto. Fixture restricted-runtime-status.json. No es
health HTTP. `BoundServer::bind_with_memory_guard` integra ese guardia en chat por
librería; CLI/config default no lanzan runtime ni lo activan al instalar fuentes.

Smokes explícitos sin modelos: fork/spawn/exec, socket propio/frente a ajeno,
presión inyectada/cancelación/idle/plazo de host, lectura nativa normal, y helper
ignora SIGTERM → cuarentena → autolímite propio3 s → retry explícito/reaping/release.
Chat real HTTP con runtime falso conserva texto/tools y tras stop devuelve503 sin
nueva inferencia; lease se reacquiere solo después. No tools ejecutadas. Tests
normales bloquean autorización/presupuesto/presión/ejecutables antes de empezar.

No se validó compatibilidad MLX/Metal bajo esa política, presupuesto/contexto,
rendimiento, Safari o sesión sostenida. Si MLX necesita subprocess, perfil
incompatible: no quitar política. S4-02 se cierra solo en alcance experimental;
S4-03 y campaña real siguen con sus puertas de permiso/recursos/budget/exclusión.
