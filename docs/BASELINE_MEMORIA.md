# Baseline de memoria · diseño S2-04

## Preparación offline de contadores · 2026-11-11

Implementados `seiya.runtime_memory.RuntimeMemorySampler` (adaptador Python
inyectable dentro del proceso medido) y `seiya_observe::runtime_memory` (decoder
Rust puro). [ADR0034](decisions/0034-contadores-runtime-offline.md) define límites,
scope y rollback. IM-02 añade canal privado opt-in a la campaña aislada
([ADR0035](decisions/0035-canal-memoria-propio.md)); no al servicio operativo.
La implementación se comprobó primero sin importar MLX o cargar Gemma; después
se ejecutó la calibración instrumentada autorizada descrita más abajo. Los null
de la calibración anterior se conservan: no se rellenan retrospectivamente.

- `get_active_memory`, `get_cache_memory`, `get_peak_memory` se revisaron
  estáticamente en los stubs instalados de MLX0.32.4. El adaptador recibe
  `mlx.core` ya importado por el runtime; no lo importa desde el colector.
- Bytes del asignador MLX, no memoria física total ni contador Metal separado.
  Peak global desde inicio/último reset externo desconocido, no por petición.
  No se llama reset/synchronize/eval/clear_cache ni se cambia configuración.
- Muestra secuencial con inicio/fin monotónicos relativos al adaptador y secuencia;
  cero es válido, desconocido es null+razón. Excepciones no exportan sus mensajes.
  No se guardan muestras ni se crea hilo, endpoint, archivo o proceso al importar.
- Metal total/footprint siguen null/no_validated_probe. RSS sigue en el probe
  nativo con identidad Child propia. No sumar estas cantidades solapadas.

Fixture compartida:
[`runtime-memory.fixture.json`](../contracts/benchmarks/memory-baseline/v0.1.0/runtime-memory.fixture.json).
El decoder rechaza >4096 bytes, duplicados, campos desconocidos, tipos/rangos
inválidos, falta de campos, relojes intra-muestra regresivos y PID distinto del
esperado. PID/JSON no autentican ni autorizan nada. IM-02 liga el canal a Child
original/run/identidad de inicio, limita lectura y valida secuencia/reloj entre
muestras; el getter no ofrece deadline hard en Python.
No invocar este adaptador en el watchdog/colector ni en runtimes compartidos.

Pruebas offline: 9 Python del adaptador y 4 Rust del decoder, con counters
sintéticos y la misma fixture; regresión completa 284 Rust +136 Python/UI OK,
22 ignored. No se acredita semántica real MLX/Metal o compatibilidad con el
launcher restringido. Esa regresión cerró IM-01; resultados posteriores abajo.

### Canal supervisado IM-02

`CampaignDriver::with_runtime_memory()` habilita wrapper de entrada Python en
el **mismo Child**, con socketpair anónimo stdin/stdout; stdout de aplicación
se redirige a stderr descartado. No endpoint HTTP, archivos de intercambio o
PID adoptado. Fuente de adaptador+bootstrap embebida se registra literalmente
en metadata. La instrumentación añade hilo/overhead: no compararla como si fuera
idéntica a la calibración anterior sin diseñar ambos brazos equivalentes.

El padre verifica identidad nativa antes/después, secuencia exacta, relojes,
≤4096 bytes/frame, ≤1024 muestras/run y plazo≤500ms limitado al presupuesto
restante. Error/cancelación de lectura cierra canal sin reutilizar bytes parciales;
runner conserva cleanup/lease. Rehearsal previo basado en exec se rechaza: perdería
el hilo del wrapper. Por defecto todo permanece apagado.

Harness ignored de calibración: `SEIYA_CALIBRATION_RUNTIME_MEMORY=1` activa el
builder **solo** junto a la aprobación real propia y los paths/preflight existentes.
No ejecutar ese harness por disponer de la variable o pasar tests. Smoke nativo
con helper Python -I/-S propio: canal e identidad reales, getters falsos, cierre
sin force OK; no MLX/modelos. Regresión288 Rust +141 Python/UI y23 ignored OK.

### Calibración instrumentada real · 2026-11-11

Una única ejecución nuevamente autorizada, Gemma E4B existente, ADR0016,
≤120s/una respuesta≤256tokens. Completada, marcador MEMORY_BASELINE_OK verificado,
18 tokens entrada/6 salida reportados, petición bufferizada478ms. Runtime4.259ms
observados, harness16,94s con preparación/hash. **No TTFT ni aceleración probada**
frente a la prueba anterior: cambia host, cachés e instrumentación.

| Métrica | Valor observado | Límite de interpretación |
|---|---:|---|
| RSS runtime máximo muestreado | 4.176.199.680 bytes ≈3,89GiB | 6 muestras, no pico continuo |
| MLX activo máximo muestreado | 4.223.579.598 bytes ≈3,93GiB | Asignador, no RAM física total |
| Pico MLX máximo comunicado | 4.283.093.355 bytes ≈3,99GiB | Desde inicio/último reset externo desconocido |
| Caché MLX máxima muestreada | 15.069.946 bytes ≈14,37MiB | Caché del asignador, no todas las cachés del modelo |
| Metal total / footprint | null | Sin probe validado |

Seis frames propios, secuencia0–5, mismo PID que Child/identidad original y reloj
monotónico, ningún fallo de canal; lecturas secuenciales, no atómicas. **No sumar
RSS/MLX/caché/pico/Metal**; solapamiento y semánticas distintas, no desglose
pesos/KV/activaciones. RSS de coordinador≤6.701.056 bytes y colector≤37.502.976,
por separado. La fase listener-ready puede incluir carga aún en curso.

Cinco observaciones de presión normales sin stop. Las3 del preflight del driver
tenían110.067.712/104.038.400/80.281.600 bytes de páginas libres; se usó solo la
excepción experimental, no el gate normal ni una garantía de capacidad. Swap del
host305.859.133 bytes en ambos puntos frontera; no pico ni atribución al modelo.
Cleanup propio confirmado sin force, lease kernel reacquirido y archivo original
preservado; sin runtimes conocidos/listeners observados después. No inferir
exclusividad GPU universal solo por esa observación.

Resumen derivado contrastado con frames/RSS/timing/tokens/provenance y hashes:
[`gemma-calibration-instrumented-2026-11-11.json`](../contracts/benchmarks/memory-baseline/v0.1.0/gemma-calibration-instrumented-2026-11-11.json).
Originales íntegros privados en temporal aprobado, identificador
`seiya-gemma-memory-instrumented-15c35c2d77`. El manifest conserva bootstrap
literal, weight_hash exterior null y provenance nativo anidado íntegro; no se
rellena el exterior retrospectivamente. No publicación o nueva ejecución.

Autorización única consumida. Ollama cerrado por SIGTERM con permiso; servicio
de Podcast que reiniciaba su Python detenido con autorización adicional y
dejado detenido, sin borrar configuración ni reiniciarlo automáticamente.
S3-05/S4-03 siguen sin presupuesto completo, contexto4096, launcher restringido o
sesiones≥20min probados. Regresión final offline288 Rust +142 Python/UI,23 ignored,
fmt/Clippy/diff-check OK; no es otra carga. Los datos no garantizan sesiones largas.

**Baseline especificada; campaña A/B NO ejecutada (sí calibraciones separadas).**
Hay driver de campaña opt-in y rehearsal sintético, no gestor adaptativo validado.
S3-03 implementa probes opt-in de procesos propios; sus smoke tests sin modelo
tampoco son la campaña real de este plan.

**Calibración real separada completada2026-11-10**, autorizada según ADR0016,
sin cambiar el gate normal ni ejecutar ABBA. Gemma E4B dio el marcador esperado:
18 tokens de entrada/6 de salida reportados,2,060s de petición bufferizada. Máximo
RSS **muestreado** del runtime4.180.639.744 bytes (≈3,89GiB),8 muestras a fronteras/
best-effort1Hz; no pico continuo, RAM total de Gemma o pesos+KV/Metal sumables.
Proceso coordinador≤6.651.904 bytes RSS, colector≤39.714.816 bytes, por separado.
Siete observaciones de presión normales y cleanup de Child propios confirmado
sin force; lease reacquirido después. Una petición corta no acredita estabilidad.

Resumen sin rutas privadas/PIDs en
[`gemma-calibration-2026-11-10.json`](../contracts/benchmarks/memory-baseline/v0.1.0/gemma-calibration-2026-11-10.json).
Footprint/MLX/Metal siguen null por falta de probe validado; los2 puntos de swap
del host (≈2,57GiB ambos) no son pico ni atribución causal al modelo. El peso
SHA256 se obtiene del provenance nativo anidado; el campo weight_hash del
manifest exterior sigue null y no se ha rellenado retrospectivamente. Artefactos
originales íntegros se conservan solo localmente; este resumen es derivado y
no sustituye su provenance. No copia el checkpoint, conversación o credenciales.
Presupuesto por categorías, contexto efectivo4096, compatibilidad sin fork,
sesiones≥20min y comparación A/B **siguen pendientes**. No declarar S3-05/S4-03
cerradas o modelo disponible por haber terminado esta calibración.
Plan legible por máquina:
`contracts/benchmarks/memory-baseline/v0.1.0/plan.json`.

Implementación iniciada en Sprint 03: `seiya-observe` aporta núcleo, parsers y
adaptador explícito desde Child propio, probados con mocks y un smoke nativo.
No se conecta al servidor operativo ni convierte este diseño en baseline medida.

S3-04 valida ciclo A/B/artefactos con driver y muestras sintéticos, mode simulation
obligatorio. El plan indica `simulation_runner_implemented:true`; S3-04b añade
`runner_implemented:true` por implementación, no por haber ejecutado MLX.
No se cambia el estado `specified-not-executed` de la campaña por simularla.
La integración launcher/HTTP/probes/reloj se registra explícitamente como S3-04b,
antes de la puerta real S3-05; no se declara lista por disponer de mocks.
S3-04b incorpora un `FixtureDriver` con procesos/coordinadores/probes nativos y
backend de respuestas fijas; sus artefactos `fixture-integration` no son
`simulation`, pero tampoco inferencia real (`model_inference:false`). La receta
MLX seguía pendiente de integrar en ese incremento. Ver ADR 0011 y
`crates/seiya-observe/README.md`; no habilita cargar modelos automáticamente.
ADR 0012 añade receta/preparación MLX, inventario/hashing nativo opt-in y
preflight con lease cooperativo, probados con fixtures. ADR 0013 los integra en
`CampaignDriver`: backend model variable, provenance por brazo, lease continuo y
preflight fresco antes de cargar. Artefactos `campaign-rehearsal` separan hashes/
RSS reales de archivos/respuestas sintéticos y memoria libre inyectada. La ruta
`model-campaign` exige aprobación explícita acotada; aún no se ha ejecutado.
No se han hasheado pesos reales ni medido recursos/inferencia MLX con esa ruta.
S3-05: el intento aislado autorizado fue bloqueado por el preflight nativo
por páginas libres observadas bajo el umbral conservador (presión normal).
No se inició MLX ni se midió baseline. Ese rechazo no prueba falta de memoria
recuperable: no se modifica el criterio ni se fuerza carga para ocultarlo.
Revisión autorizada 2026-10-20 (ADR0014): `review_pressure_window` separa presión
observada de capacidad de carga. Auditoría breve con presión normal y pocas
páginas libres no permite calcular un presupuesto seguro para Gemma. El gate
actual se conserva y se etiqueta free-pages-only; no es garantía de capacidad,
incluso si supera el umbral. Siguiente requisito: presupuesto runtime y política
de admisión/monitorización explícita, no una suma de memoria supuestamente recuperable.
ADR0015 incorpora el contrato de presupuesto y vigilancia stop-only del driver,
probados con presión inyectada y ensayo ABBA sin modelo. La revisión estática del
checkpoint aporta escenarios de KV lógico, no peak ni presupuesto total validado.
Siguen faltando importes/medición del runtime; el gate de páginas libres no cambia.
ADR0016 prepara una excepción experimental separada para calibrar, no comparar:
un solo runtime/brazo Rust y petición de texto, presión vigilada y plazo acotado.
Solo ensayada con fixtures; no ejecutada con MLX ni autorizada como carga real
por aprobar su preparación. No sustituye el gate normal ni completa S3-05.

Se concreta el requisito de [`MEMORIA_ADAPTATIVA.md`](MEMORIA_ADAPTATIVA.md) sin
restaurar Colibri ni confundir RSS del coordinador con los pesos del modelo.

## Pregunta y comparabilidad

Primero medir el coste del camino Python frente a Rust con el **mismo trabajo**,
no demostrar por adelantado que Rust ahorra RAM. Después se podrá comparar una
política de memoria con su propio baseline sin optimizar varias variables a la vez.

- Un checkpoint local exacto: manifiesto de shards/config/tokenizer/chat template,
  hashes y versiones MLX/MLX-LM/Transformers, macOS, hardware y binarios/coordinador.
  Hashing antes de la ventana medida; no descargar pesos o recuperar código archivado.
- Mismas peticiones, herramientas, historial y salida máxima efectiva **256**;
  temperatura 0, thinking desactivado y los mismos parámetros de caché/concurrencia
  enumerados en el plan. No cambiar KV bits, cuantización o contexto entre brazos.
- Cliente directo sintético para ambos: `model:pegassus`, `swarm_mode:fast`,
  `max_tokens:256`, `stream:true`. Usar el alias común `max_tokens`: Python no
  implementa el alias nuevo de Rust. No enviar store/stream_options en esta A/B;
  ambos conservan su SSE con usage por defecto. Config de ambos también limita 256.
- Registrar el cuerpo normalizado que recibe el backend y exigir igualdad de
  model/messages/tools/temperature/max_tokens/stream/chat_template_kwargs. No
  comparar si las implementaciones ejecutan distintas tareas o presupuestos.
  Para comparar historiales entre runs, normalizar solo tool call IDs mediante
  una correspondencia biyectiva para el análisis: nunca modificar el historial
  transmitido. Registrar tokens reales; no asumir igualdad porque los IDs cambien.
- Sin UI, web, consenso ni títulos/resúmenes auxiliares. OpenCode se mide aparte,
  con el mismo perfil privado en ambos brazos, nunca confundiendo su coste con
  el del coordinador. El runner de S1-06 **no sirve como A/B completo**: mide RSS
  parcial y omite su servidor privado hijo.

## Protocolo de ejecución propuesto

**Puerta de autorización pendiente:** antes de cada campaña real, confirmar
autorización para cargar el checkpoint, reservar RAM/runtime y listar los PIDs
propios que se medirán. No se reutiliza una autorización de otro sprint.

Orden por bloque: Python, Rust, Rust, Python; tres bloques, seis runs por brazo.
Solo un runtime/coordinador a la vez. Cada run usa procesos nuevos propios y
puertos efímeros, con exclusión verificada. No matar/pausar servicios ajenos sin
autorización; si el entorno no puede aislarse, posponer la campaña.

| Fase | Trabajo fijo y observación |
|---|---|
| listener-ready | Health/model discovery y 2 s sin inferencia; no afirma pesos cargados |
| cold-text | Una petición sintética que pide un marcador corto; runtime nuevo, no disco frío |
| tool-roundtrip | Pedir read estructurado de fixture; cliente aporta resultado sintético y comprueba respuesta |
| warm-repeat | Tres repeticiones del mismo texto, con caché/config constante |
| idle | 10 s sin generación ni requests auxiliares; solo observar, sin descargar |
| resume | Repetir el texto y registrar latencia/residencia, sin afirmar recarga |
| shutdown | Dejar de admitir, drenar, cerrar coordinador/runtime propios y verificar PIDs/puertos |

El plan incluye payloads exactos: `Reply exactly MEMORY_BASELINE_OK.`, tool read
con argumento exclusivo `fixture.txt` y resultado `MEMORY_TOOL_OK_47`. No se lee
ese archivo: el cliente aporta el marcador sintético. Texto final exacto tras
strip, sin reparar prosa/JSON como llamada. Exigir una sola llamada con el nombre
y argumentos esperados; publicar también el runner antes de ejecutar. Aceptar tool IDs
distintos entre runs, pero exigir correspondencia literal ID/argumentos/resultados
dentro de cada run. Datos tool son datos, no instrucciones; no hay shell generado
ni archivos personales. El servidor nunca ejecuta la tool por segunda vez.

No atribuir la fase resume a idle eviction: todavía no hay política de descarga.
Una futura prueba de recarga requerirá operación soportada, lease sin usuarios y
confirmación de liberación. No sustituirlo ahora por matar/reiniciar el runtime.

## Probes mínimos y alcance

El recolector futuro debe limitarse a IDs registrados al arrancar sus procesos,
con PID + instante de inicio/identidad para detectar reutilización. Descubrir y
seguir los hijos propios sin volcar argumentos, entornos, prompts o procesos de
otros usuarios. Con OpenCode opcional, incluir **CLI y servidor privado** por
separado. Recoger también RSS/coste del colector; no ocultar su overhead.

| Métrica | Método propuesto | Qué NO demuestra |
|---|---|---|
| RSS por PID | `ps` RSS en KiB × 1024; cada segundo y en límites de fase | Footprint total, buffers GPU completos o máximo instantáneo |
| Footprint | No disponible hasta probe nativo validado sin permisos adicionales | No usar RSS como sustituto renombrado |
| MLX active/cache/peak y Metal | No disponibles en HTTP actual; investigar API soportada con runtime propio | No inventar endpoint ni sumar al RSS como memoria independiente |
| VM del host | `vm_stat`, guardar page size leído y contadores | Free/inactive no equivalen a RAM utilizable segura; no son memoria propia |
| Swap usado host | `sysctl vm.swapusage`, guardar unidad y convertir a bytes | Swap acumulado no es presión actual ni ahorro de memoria del modelo |

Snapshots de frontera de fase más muestreo cada 1000 ms, timestamps monotónicos,
duración y huecos de colección. Un proceso terminado no equivale a una medida
0: guardar unavailable/exited y dejar de muestrearlo; PID reciclado aborta.
Los probes planificados no son capacidades ya anunciadas por Seiya.

No sumar RSS de procesos como RAM física única ni RSS + Metal + footprint: hay
solapamiento. Reportar series y máximos **muestreados**, no peak real. Si una medida
no existe: value null, availability unsupported, razón explícita; nunca cero ni
estimación presentada como medición. Una métrica requerida no soportada bloquea
el análisis que depende de ella, no autoriza instalar helpers o pedir sudo.

## Resultados y criterios de aceptación del futuro runner

- Manifiesto por run: commit/diff/hash de binario, versiones, pesos identificados,
  configuración, orden, PIDs/roles/ownership, fase y reloj. No credenciales ni
  contenido personal. Directorio sintético nuevo; no borrar bases/sesiones previas.
- Registrar request ID, presupuesto efectivo, input/output reportados por runtime,
  status/finish/tool IDs y éxito semántico, sin usar votos como prueba de calidad.
- Latencia por request y por fase. TTFB en SSE bufferizado es espera hasta
  completion, **no primer token de inferencia**. No calcular tokens/s con tiempo
  de carga/cola como si fuese decode aislado; marcar métrica no disponible.
- Publicar todos los runs, fallos, muestras ausentes y dispersiones. Seis runs por
  brazo solo dan una comparación exploratoria; no P95 robusta ni garantía térmica.
  Para decidir política, diseñar más repeticiones y un intervalo de incertidumbre.
- Registrar estado inicial/final VM/swap y condiciones de alimentación/thermal
  disponibles sin permisos nuevos. No usar `purge`, tuning del SO, presión/OOM
  forzada, descarga de modelos ajenos o cambio oculto de contexto para mejorar cifras.
- Abortan el run: ownership/budget distintos, trabajo inesperado, tool incorrecta,
  fallo de test, runtime incierto, falta de métrica requerida o parada del usuario.
  Conservar evidencia del fallo; no reintentar ocultamente ni lanzar el otro brazo
  sobre una generación residual. No garantizar GPU libre por una desconexión TCP.

Pruebas offline previstas para el runner: proveedor de muestras inyectado (sin
comandos/red), conversiones de unidades, huecos/errores, PID reciclado, seguimiento
de hijos y cancelación, rechazos de comparación desigual y limpieza solo propia.
En S2-04 **solo se validan invariantes del plan** con fixtures/tests: no se finge
que esos tests ejercitan un recolector o prueban reducción de memoria real.

## Próximo incremento delimitado

Sprint posterior: implementar recolector mínimo desacoplado de la política,
providers sintéticos y validaciones de igualdad/ownership/artefactos, primero en
Rust para lógica nueva compartida. Adaptador macOS nativo si aporta footprint
con permiso existente; si no, unsupported. Integrar el runner A/B después.

Solo con tests offline y autorización vigente se ejecutará la campaña real. Su
resultado decide si merece la pena presupuesto/leases, trimming o idle eviction
con APIs soportadas. **No hay aún baseline real, ahorro demostrado, probes de
Metal, descarga automática, port Colibri ni experimento MoE/SSD.**
