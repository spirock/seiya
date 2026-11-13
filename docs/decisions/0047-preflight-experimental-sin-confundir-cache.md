# ADR0047 · corregir preflight del ensayo web

Usuario pide resolver el preflight y terminar la prueba, y confirma «continua».
Autoriza esta corrección y reanudación del ensayo ya aprobado, no cerrar apps,
purge, descargar pesos, otra imagen, cambiar diseño o quitar controles operativos.
El host anterior expiró normalmente, sin modelos cargados; no se interrumpe un
servicio compartido ni se hereda una excepción de las calibraciones individuales.

## Problema y alternativas

`Pages free>=2GiB` confunde páginas sin contenido con capacidad de un sistema que
mantiene cachés. La muestra rechazada tenía presión normal y128MiB libres, no una
prueba de que faltara toda la RAM necesaria. Sumar inactive/purgeable/file-backed
duplica categorías solapadas; tampoco acredita cuánto es recuperable. El SDK
`os/proc.h` declara `os_proc_available_memory` **no disponible en macOS** y describe
límite dirty por proceso, no presupuesto del host. `memory_pressure -Q` es solo
consulta en el modo utilizado: su porcentaje no equivale a RAM sin usar ni a
presupuesto del par. No se multiplica ese porcentaje por RAM para inventar bytes.

Eliminar todo preflight sería inseguro. Purge/cerrar aplicaciones sin permiso o
reemplazar por Qwen solo cambia alcance. El cálculo de capacidad de producción
sigue pendiente: ninguna métrica de host aislada demuestra que ambos quepan.

## Decisión acotada

Solo para host experimental ADR0046 explícitamente autorizado: reemplazar el
floor de páginas libres por una ventana observada de presión normal y swap
estable,9 muestras a250ms cubriendo≥2s (máximo4s, gaps≤1s), crecimiento de swap
desde mínimo previo≤32MiB, datos completos. Host físico conocido≥16GiB y disco
≥4GiB, versión/modelos/store/lease/ownership privados siguen obligatorios.
Páginas libres quedan como dato diagnóstico, no gate ni capacidad disponible.
Esto admite **un experimento supervisado**, no producción o garantía de RAM.

Se conservan sin cambios: stop inmediato al observar warning/critical/unknown,
swap+512MiB desde baseline anterior a carga, monitor250ms, límites de tiempo,
ownership/listener, disco/log y cleanup propio; no retries automáticos. Si una
guarda operacional falla, registrar y no continuar cargando el segundo modelo.
Los gates generales de campaña/servidor no cambian. Pruebas sintéticas cubren
free bajo con ventana válida, presión/swap/datos/edad inválidos denegados.

### Configuración solicitada después por el usuario

El usuario pide poder colocar el límite en configuración. Se añade fichero JSON
local del operador, flag CLI `--limits`, y ejemplo en
`contracts/benchmarks/background-jobs/v0.1.0/web-trial-limits.example.json`.
No se leen límites del prompt, job o respuesta del modelo. Defaults: floor libre0
(se usa ventana normal), crecimiento swap preflight32MiB, operativo512MiB,
warning0ms (stop inmediato), sesión1800s. Opcional floor0–8192MiB,
swap preflight0–128MiB, operativo64–2048MiB, warning acumulado0–10000ms,
sesión60–1800s. Critical/unknown y datos viejos siempre paran, no configurables.
Presupuestos de warning no se reinician al volver a normal. JSON cerrado, tipos,
extras/duplicados/tamaño≤4096 y rangos comprobados antes de crear recursos.
Toda configuración efectiva se registra en evidencia, sin credenciales.
No se selecciona automáticamente un límite más permisivo si falla una prueba.
El primer nuevo arranque usa los defaults conservadores, no excepciones previas.

Decisión posterior explícita del usuario: **«Warning hasta10 segundos»** para un
nuevo intento; warning acumulado10000ms, critical/unknown inmediata, swap512MiB.
Se prepara perfil privado03 con esos valores, sin modificar defaults o ejemplo.
`web_port` del operador permite elegir otro listener loopback (1024–65535,
excluye8010/11434, bind ocupado falla sin adoptar). Perfil03 usa8012 para no
interrumpir la UI02 bloqueada, ya sin runtime/lease; no coordinadores simultáneos
sobre modelos. No clave/endpoints remotos en config ni fallback de puerto.

Tras resultado03 (warning1008ms, swap+685,94MiB superó512), el siguiente intento
acotado admite **hasta2GiB**. Perfil04 fija swap2048MiB,
warning10000ms, critical/unknown inmediatos y puerto8013; no modifica defaults
ni afirma que ambos modelos quepan. No extrapolar este permiso a imágenes.

Después se realiza **un nuevo arranque** autorizado, solo si ningún host anterior
retiene runtime/lease. Exigir warmups/residencia del par y tool-use real antes de
anunciar web lista. Si falla esa parte, no dar el problema por terminado ni
mostrar respuestas simuladas. Rollback: no usar este entrypoint y conservar
evidencias/versiones anteriores; sin borrar pesos/configuración/historial.

Referencias: SDK macOS `/usr/include/os/proc.h` (API_UNAVAILABLE(macos)); Apple
Activity Monitor «Check if your Mac needs more RAM»; consultas locales read-only
de vm_stat/sysctl y fuente Apple system_cmds/memory_pressure. Sin tests de estrés
o asignaciones artificiales de memoria.
