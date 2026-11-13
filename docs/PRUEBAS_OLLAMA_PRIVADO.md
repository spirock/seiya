# Caracterización secuencial · Ollama privado

## Alcance autorizado

Confirmación explícita registrada en `plans/2026-11-11-trabajos-chat.md`:
una prueba por tag, Gemma E2B y después Qwen9B MLX, nunca simultáneos;
≤120s/prueba, una respuesta≤256tokens, contexto candidato4096, thinking=false.
Sin retries tras fallo, descarga, instalación o cambio del servicio activo.
No activa el perfil jobs real. Un smoke sin modelos no acredita esta campaña.
Reanudación posterior autorizada: excepción **solo para este ensayo** ADR0039,
descrita abajo; el gate global de2GiB libres no se cambia.

## Preparación y decisiones

- Usar el ejecutable instalado0.40.14 mediante ruta absoluta, no otro runtime.
- HOME, cwd, TMPDIR y almacén privados bajo temporal OpenCode. Entorno construido
  desde allowlist, sin proxies/credenciales/configuración del usuario;
  OLLAMA_NO_CLOUD=1, OLLAMA_NOPRUNE=1, un modelo/una petición como máximo,
  contexto4096 y keep_alive=0. No-cloud no equivale a sandbox de red.
- Tomar el flock existente `seiya-host-model-coordination/seiya-mlx-baseline.lock`;
  no borrarlo/recrearlo ni sustituirlo por otro para eludir exclusión.
- Reservar loopback efímero; Ollama no hereda el socket: verificar el PID del
  listener contra el Popen original antes de HTTP. Si pierde la carrera, abortar,
  no adoptar ni señalizar el ocupante. No puertos11434/8010.
- Conservar el Popen original, sesión/grupo propios y plazo. Antes de terminar,
  comprobar ausencia de descendientes; SIGTERM solo al hijo original vivo,
  wait acotado y verificar salida, grupo y puerto. Incertidumbre bloquea cargas;
  no liberar ficticiamente la lease para pasar al siguiente modelo.
- Smoke vacío: solo GET version/tags/ps; sin generate/chat/pull, ni descarga de
  modelos residentes. No modificar ni señalizar Ollama compartido/OpenCode/preview.

Alternativas descartadas: usar servidor compartido no da ownership; apuntar
OLLAMA_MODELS al almacén original o enlazar todo su árbol deja capacidad de
escritura/pruning sobre pesos ajenos. Preparar un almacén acotado con protección
de escritura efectiva sigue siendo requisito antes de cargas, no una garantía
obtenida por NOPRUNE. No copiar16GB de pesos para resolverlo sin evaluar disco.
Estas decisiones son del ensayo, no cambian interfaces públicas del producto.

## Almacén observado, sin inferencia

Fuente oficial v0.40.14: `envconfig/config.go`, `manifest/paths.go` en
https://github.com/ollama/ollama/tree/v0.40.14 . Models() usa OLLAMA_MODELS o
HOME/.ollama/models; manifests-v2 y blobs se resuelven debajo de esa raíz.
El almacén local observado está en HOME/.ollama, distinto del default. No se
ha leído el entorno/configuración del proceso compartido para atribuir la causa.

Dos enlaces `manifests-v2/ollama.com/library/<modelo>/<tag>` resuelven a blobs:

| Tag | SHA256 del manifiesto | Capas | Bytes referenciados |
|---|---|---:|---:|
| gemma4:e2b-mlx | f4a8449d17818601a45b32b22b90520b5c1c0327a27374bfb84e1974bb74104c |2072|7476595162|
| qwen3.5:9b-mlx |203e30078279db51132b9e026ceb7bb21330e5b1af67ef190671b375c9770404|768|8903014758|

Lectura de manifiestos y stat de referencias: cero ausentes/discrepancias de
tamaño. Solo hash del manifiesto, **no hash íntegro de pesos** ni compatibilidad
MLX demostrada. Sin leer claves, backups, conversaciones o documentos personales.

## Puertas antes de cada carga

Lease exclusiva, ownership confirmado, ningún modelo ajeno residente, presión
normal y páginas libres≥2GiB frescas en el intento original. Liberar solo los dos tags autorizados si
están residentes; no matar procesos/apps ajenos ni bajar la guarda. Fallo detiene
campaña completa, sin retry automático. Después: validar identidad, terminación,
usage y ausencia de thinking/tools/truncamiento; guardar evidencia minimizada.
RSS/MLX/Metal no se suman. Resultado individual no acredita par simultáneo,
sesión sostenida, contexto4096 lleno o admisión automática en16/32GiB.

Rollback: cerrar únicamente instancia privada propia; conservar pesos,
configuración, conversaciones, preview y servidor activo. No instalación o
migración que revertir. La evidencia del ensayo se añade al plan, no a resultados
de modelos reales si no hubo inferencia.

## Resultado observado · 2026-11-11

Smoke operativo con Ollama real0.40.14 y almacén **vacío**: listener efímero60832
propiedad del Popen26439, GET version/tags/ps correctos (cero modelos). Sin
descendientes antes de apagar; SIGTERM al hijo original y wait exit0, grupo vacío,
puerto cerrado y lease existente readquirida. HOME/almacén privados conservados
en `seiya-ollama-empty-lpzx598v` bajo temporal OpenCode; evidence.json/server.log.
No runtime con pesos ni prueba del árbol MLX o inferencia. No sandbox de red
acreditado; no solicitudes externas realizadas por el controlador del ensayo.

Preflight11:15 +01:00: presión1,6057 páginas×16384=99.237.888bytes libres
(≈0,092GiB), mínimo2GiB; /api/ps compartido vacío, lease exclusiva. Gate falló:
abortado antes de Gemma, sin pasar a Qwen, sin retry/unload. Registro preflight.json
en el mismo directorio. Falta liberar recursos sin tocar procesos ajenos y
reanudar explícitamente; además implementar protección efectiva del almacén y
supervisión del árbol MLX antes de ninguna carga. No se entrega un harness de
inferencia listo ni se considera consumido el permiso por una inferencia inexistente.

## Reanudación ADR0039 · implementada y detenida por presión real

Se añadió un adaptador manual macOS
`local-tools-tests/seiya_ollama_private.py` y checker puro Rust
`crates/seiya-observe/src/bin/seiya-ollama-check.rs`. No integrado en servidor,
perfil jobs o instalador. No dependencias nuevas. Antes/después de cada paso:
lease exclusiva, servidor compartido vacío y ownership de Popen original con
libproc PID/inicio sec-usec/parent/grupo. Descendientes acotados32; señales solo
al árbol propio comprobado, SIGKILL como último recurso propio si SIGTERM falla.
Ante incertidumbre: campaña abortada, no siguiente modelo.

Solo excepción de calibración: presión normal, stop con warning/dato faltante o
swap>512MiB sobre preflight; deadline120s incluyendo carga/respuesta. Se registran
VM/caché sin sumarlas, no se estima RAM disponible ni se promete que el modelo
quepa. Caché recuperable no es reserva garantizada. No memory_pressure -Q como
GiB disponibles, purge, instalación o señales a servicios ajenos.

Almacén privado por modelo con manifiesto validado por hash y enlaces a sus blobs
exactos, tamaños verificados. Sandbox-exec heredado niega file-write* sobre raíz
original; fixture propia acredita denegación directa/por symlink antes de conectar
pesos. NOPRUNE adicional. HOME privado, entorno allowlist y NO_CLOUD. Sin sandbox
de red acreditado ni protección contra runtime hostil; son binarios locales
instalados, no código generado por el modelo. No se leen documentos del usuario.

Comprobación operativa previa en `seiya-ollama-adr0039-empty-01`: fixture de
protección y padre+hijo propios, Ollama vacío63360 cerrado exit0, árbol/puerto
detenidos y lease readquirida. No modelos en ese smoke. Ejecución manual requiere
`--mode empty|models --output <directorio-nuevo-temporal> --checker <binario>` y
`--approval ADR0039-sequential-120s-256tokens`; el flag documenta intención, **no
concede autorización**. No ejecutar automáticamente o repetir tras un fallo.

### Intento real único · 11:50 +01:00

Gemma `gemma4:e2b-mlx`, manifiesto f4a8449d1781: Ollama privado63368, runner
MLX/Metal63374. Log acredita motor0.32.4-31-g065ec46 y carga de tensores, no una
respuesta de inferencia completada. Request sintético «SEIYA_OK», think=false,
num_ctx4096/num_predict256/keep_alive0. Contexto efectivo no validado.

12 muestras: presión inicial1, **warning2 a3,224s**, stop inmediato del ensayo.
Swap observado770.053.242bytes sin crecimiento en esas muestras. RSS máximo
muestreado servidor32832KiB, descendiente2366736KiB, **no sumables ni peak total**;
MLX activo/Metal total/footprint desconocidos. Archivo caché pasó de5.797.429.248 a
2.735.882.240bytes en muestras; el cambio no mide exactamente memoria recuperada
ni toda la memoria del modelo. No respuesta válida, tokens de salida o latencia
de generación medidos. El cliente terminó tras cancelación, HTTP499 en log.

Cleanup propio confirmado: servidor exit0, dos procesos observados detenidos,
PIDs ausentes y puertos de servidor/runner cerrados; lease readquirida después.
Ollama compartido sigue vacío, presión volvió a1 en muestra posterior y preview8010
permanece sana/simulada. **No Qwen ni retry**: se respeta abortar al primer fallo.
Se consumió el intento Gemma, no se hizo el de Qwen. Nueva campaña exige permiso
específico y condiciones revisadas; no subir umbrales para ocultar el warning.

Originales privados `seiya-ollama-adr0039-models-01/run-0/{server.log,evidence.json}`;
resumen mínimo contrastado en
`contracts/benchmarks/background-jobs/v0.1.0/sequential-attempt.json`. No claves,
logs o HOME privado versionados. No se borran pesos ni directorios de evidencia.

Regresión final:308 Rust+160 Python/UI,24 ignored; fmt/Clippy locked/offline y
diff-check OK.180 archivos código/tests/JSON contrastados con copia probada.
Son pruebas de software con fixtures, no nuevas cargas reales o aprobación del
modelo. No reconstrucción de paquete necesaria: no se cambió su empaquetado.

## Calibración E2B completada · ADR0040

Nuevo permiso explícito del usuario: una única carga E2B con preflight normal,
≤10s acumulados de warning, stop ante critical/fallo de vigilancia/intervalo>1s,
swap+512MiB o120s; una respuesta≤256tokens, contexto solicitado4096, think=false.
Modo manual separado `--mode gemma-calibration` requiere
`--approval ADR0040-gemma-warning10s-120s-256tokens`; solo E2B, no Qwen/retries.
La cadena CLI no es un permiso por sí misma. Ruta normal y ADR0039 conservan
stop inmediato ante warning. Contador Rust cobra todo intervalo si alguno de
sus extremos es warning; acumula sin reset, redondea hacia arriba. No muestreo
hard-real-time ni prevención garantizada de OOM entre muestras.

Intento12:05 +01:00 en `seiya-e2b-adr0040-calibration-01/run-0`: respuesta
**SEIYA_OK**, stop,22tokens entrada/5salida, sin thinking/tools en respuesta.
Petición4,416s observados, runtime total4,362s/carga4,214s/prefill0,037s/decode0,110s.
16 muestras, warning en4,1431ms contabilizados (<10s), final/póstcleanup normal.
Swap creció145.752.064bytes≈139MiB, por debajo del límite512MiB. Preflight tuvo
2.555.035.648bytes libres: estado del host distinto al intento anterior; no
atribuir éxito únicamente a tolerar warning. Log carga draft interno incluido
Gemma4AssistantForCausalLM, no otro tag descargado/cargado por el harness.

Máximos RSS servidor32336KiB y descendiente2276752KiB≈2,17GiB por separado:
**no son memoria total del modelo**, pico real o suma MLX/Metal. Contadores
MLX/Metal/footprint ausentes; falta perfil completo para admitir pool. Contexto
efectivo/ventana llena, sesiones largas, herramientas y par simultáneo no probados.

Ambos procesos propios detenidos, servidor exit0; puertos64437/64443 cerrados,
lease readquirida, compartido vacío y preview8010 sana/simulada. Resumen derivado
en `contracts/benchmarks/background-jobs/v0.1.0/e2b-calibration.json`, con hash de
evidencia original privada. Permiso consumido; no Qwen, reintentos o promoción
del perfil por este resultado. Intento fallido ADR0039 conservado como tal.

Verificación final R4:310 Rust+163 Python/UI OK,24 ignored; fmt/Clippy locked/
offline y diff-check OK.181 archivos código/tests/JSON contrastados con copia
probada y resumen real contrastado con evidencia original; sin nueva inferencia.

## Intento Qwen · ADR0041, detenido por presión crítica

Confirmación posterior expresa: una única calibración `qwen3.5:9b-mlx` con los
límites de ADR0040, sin Gemma simultáneo o retry. Selector `qwen-calibration` y
confirmación `ADR0041-qwen-warning10s-120s-256tokens` separados del modo Gemma;
la cadena no otorga permiso por sí sola. El checker Rust comparte comprobaciones
cerradas, identificando ADR0041; tests mantienen ambos selectores ligados a su
tag/permiso y comprueban critical, unknown, overflow, reloj y swap.

Intento12:12 +01:00, preflight normal y lease exclusiva, referencia203e30078279.
Log inicia MLX0.32.4-31-g065ec46/Metal y Qwen3_5ForConditionalGeneration, carga960
tensores. Petición4096/≤256tokens/think=false/keep_alive0. A3,296s apareció
**critical4**: supervisor canceló. Warning acumulado959ms y swap+329,3MiB estaban
por debajo de sus límites10s/512MiB; esta vez no fue stop por simple warning.
No respuesta, tokens o latencia de generación validados. No atribuir un fallo
de compatibilidad al runtime: la guarda interrumpió antes de completar la carga.

12 muestras, swap inicial907.416.698bytes y máximo1.252.723.261bytes. Máximos RSS
servidor31008KiB/descendiente2847328KiB≈2,72GiB: no sumables ni memoria total.
MLX activo/Metal total/footprint no medidos. No calidad de código, tools, contexto
efectivo/lleno, sesión larga o concurrencia validados por este ensayo.

Cleanup servidor exit0, ambos procesos detenidos, puertos65462/65468 cerrados y
lease readquirida; compartido vacío, presión posterior normal y preview8010 sana.
Originales privados `seiya-qwen-adr0041-calibration-01/run-0`; resumen mínimo con
hash en `contracts/benchmarks/background-jobs/v0.1.0/qwen-calibration-attempt.json`.
Permiso consumido, no retry/Gemma/purge/cambios de servicio. Nueva campaña exige
recursos/condiciones revisados y autorización específica; no ignorar critical.

## Nuevo intento con critical autorizado · ADR0042

Tras el stop anterior, usuario pide expresamente correr Qwen tolerando critical.
Se comunica riesgo real, no garantía de «no pasa nada». Modo separado
`qwen-critical-calibration`, confirmación
`ADR0042-qwen-highpressure10s-120s-256tokens`, solo Qwen y un único intento.
Preflight normal y límites anteriores conservados: warning+critical comparten
10s acumulados, swap+512MiB, vigilancia/intervalo≤1s y plazo120s. No borrar o
relajar gates globales, ni inferir permiso de una cadena CLI o respuesta del modelo.

Intento12:17 +01:00 con5.351.391.232bytes de páginas libres en preflight y presión
normal. **Critical sí se permitió** a3,259s. A3,543s la guarda paró por crecimiento
de swap915.071.304bytes≈872,7MiB (>512MiB), de1.295.389.818 a2.210.461.122bytes.
Contador combinado1496ms (<10s): no fue parada inmediata por critical ni agotar
esa ventana. El muestreo puede observar un salto ya superior al límite; no
garantiza un techo físico de consumo o ausencia de bloqueo entre muestras.

13 muestras, sin respuesta/tokens/latencia de generación validados. RSS máximo
servidor30944KiB/runner3472416KiB≈3,31GiB por separado, no memoria total o pico
real; contadores MLX/Metal/footprint ausentes. Log acredita carga de tensores e
inicialización de salida estructurada, cancelación HTTP499 por la guarda.

Root exit0,3 procesos propios observados/detenidos y grupo vacío; puertos49627/
49633 cerrados y lease readquirida, presión posterior1, compartido vacío y
preview8010 sana/simulada. Evidencia privada
`seiya-qwen-adr0042-calibration-01/run-0`; resumen con hash en
`contracts/benchmarks/background-jobs/v0.1.0/qwen-critical-calibration-attempt.json`.
Permiso consumido; no retry automático, Gemma simultáneo, purge o modificación
del servidor activo. Qwen/calibración completa y pool siguen bloqueados.

Regresión final R5/R6:312 Rust+168 Python/UI OK,24 ignored; fmt/Clippy locked/
offline y diff-check OK.183 archivos código/tests/JSON iguales a copia probada;
ambos resúmenes reales contrastados con hash y métricas de originales privados.
Sin ResourceWarning observado en la suite. No dependencias o empaquetado nuevos.

## Qwen completado con memoria informativa · ADR0043

Usuario confirma intento nuevo sin cortes de memoria, conservando servidor
privado,120s, una respuesta≤256tokens/contexto solicitado4096/think=false, sin
otros modelos, descargas, cambios de servicio o retries. Selector manual
`qwen-observe-calibration` con confirmación
`ADR0043-qwen-memory-observe-120s-256tokens`. No activa el perfil ni elimina
límites del supervisor general. La cadena CLI nunca concede autorización sola.

Solo en este modo presión/swap/VM son informativos, incluso en preflight; errores
de probe de memoria se registran como null/error, no como cero o gatillo de stop.
La política Rust devuelve continue, memory_stop_enabled=false y not_admission,
no un gate positivo de producción o memoria disponible. Aislamiento/lease,
compartido vacío, ownership, frame/identidad/finalización, deadline y cleanup
siguen siendo obligatorios; un fallo de esos controles sí aborta.

Intento12:31 +01:00: Qwen203e30078279 respondió **SEIYA_OK**, stop,24tokens
entrada/4salida, sin thinking/tools observados. Petición4,859s, runtime4,822s,
carga4,008s/prefill0,478s/decode0,301s.17 muestras,4 warning2 y ninguna critical;
final/póstcleanup normal. Swap2.178.091.581→2.671.708.733bytes, delta493.617.152
≈470,75MiB. Preflight con más páginas libres que antes y delta inferior al antiguo
límite512MiB: no aislar causalidad entre cambios del supervisor/estado del host.

RSS máximo servidor30768KiB/runner4329968KiB≈4,13GiB, no sumables o memoria total;
MLX activo/Metal total/footprint ausentes. Una respuesta breve no valida ventana
efectiva/llena, calidad de código/tools, sesión larga o concurrencia Gemma/Qwen.

Cleanup root exit0, árbol/grupo vacío, puertos50900/50906 cerrados y lease
readquirida; compartido vacío y preview8010 sana/simulada. Originales privados
`seiya-qwen-adr0043-calibration-01/run-0`; resumen con hash en
`contracts/benchmarks/background-jobs/v0.1.0/qwen-observe-calibration.json`.
Permiso consumido; no nueva inferencia/activación automática. Evidencia abortada
anterior conservada y distinta de esta calibración completada.

Regresión final R7:313 Rust+171 Python/UI OK,24 ignored; fmt/Clippy locked/offline,
diff-check y enlaces OK.184 archivos código/tests/JSON iguales a copia probada,
resumen contrastado con originales privados. Se acotó a2s el socket de un test
antiguo que dejó la primera suite esperando hasta timeout240s; sin cambio de
parser/protocolo. Test focalizado y suite completa posteriores OK, sin atribuir
causa no demostrada al atasco. No nuevas cargas durante estas comprobaciones.
