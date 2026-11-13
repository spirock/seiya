# ADR0042 · nuevo intento Qwen con critical acotado

2026-11-11. Tras explicar el stop critical de ADR0041, usuario pide expresamente
«correlo asi con el critical». Autoriza otro intento Qwen tolerando esa condición;
no se adopta su afirmación «no pasa nada» como garantía. Se comunica riesgo de
ralentización/bloqueo de aplicaciones y se conserva un ensayo acotado.

## Decisión y límites conservados

Un único intento nuevo de `qwen3.5:9b-mlx`, privado, con pesos existentes; una
respuesta≤256tokens, contexto solicitado4096, think=false, deadline120s. Preflight
normal, lease compartida exclusiva y Ollama compartido vacío. No Gemma, descargas,
cambios de configuración/servicios o retries automáticos.

Solo este modo permite presión4 durante la petición. **Warning y critical
comparten un máximo acumulado de10000ms**, no son dos bolsas separadas. Cargar
intervalo completo si algún extremo es2 o4, sin reset al volver a normal.
Vigilancia faltante/desconocida, intervalo>1s, reloj regresivo/overflow,
swap adicional>512MiB o120s siguen abortando. No relajar automáticamente esos
límites si el intento falla. No autorización para120s de critical sostenido.

Política Rust y selector manual `qwen-critical-calibration` separados, permiso
propio ADR0042; solo Qwen. Los modos anteriores mantienen parada ante critical.
Protección de pesos, ownership nativo y cleanup de procesos propios siguen
obligatorios. No garantía de evitar OOM/jetsam o detectar todo entre muestras.

Alternativas: ignorar toda vigilancia aumenta el riesgo y excede la petición de
tolerar critical; repetir sin modificar esa guarda reproduce la parada conocida.
Se elige permitir la condición expresamente autorizada en una ventana finita,
sin cambiar gates globales ni presentar presión normal como RAM garantizada.

Rollback: no usar este modo; no hay instalación/configuración que revertir.
Conservar todos los intentos anteriores con su resultado. El nuevo permiso se
consume por el intento real, incluso si aborta; no habilita concurrencia o pool.
