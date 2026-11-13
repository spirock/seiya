# ADR0041 · calibración única Qwen con warning acotado

2026-11-11. Tras pedir retomar lo pendiente, usuario confirma explícitamente
**Autorizar calibración de Qwen**: una única prueba de `qwen3.5:9b-mlx` con pesos
existentes, servidor privado,≤120s y una respuesta≤256tokens, contexto solicitado
4096, think=false. Sin Gemma simultáneo, descargas, cambios de servicio o retries.

## Decisión

Extender el ensayo manual con selector separado `qwen-calibration`, ligado a
confirmación propia ADR0041 y solo al tag/manifiesto Qwen seleccionado. No
reutilizar el permiso consumido E2B, no repetir E2B ni ejecutar mode models.
Sin interfaces de producto, dependencias o empaquetado nuevos.

Se conservan los límites y la contabilidad conservadora de ADR0040: preflight
normal y justo antes de la petición, lease existente exclusiva, servidor
compartido vacío,≤10000ms acumulados de warning sin reset al volver a normal;
parada ante critical/desconocida/vigilancia fallida/intervalo>1s/overflow,
swap adicional>512MiB o120s. Riesgo de ralentización/OOM entre muestras declarado
y aceptado. No convertir caché o RSS en una reserva garantizada.

La política pura Rust comparte las mismas comprobaciones con identificación
ADR0041; el adaptador macOS selecciona solo Qwen. Protecciones del almacén,
HOME/entorno privados, ownership nativo PID/inicio, cleanup acotado y denegación
de escritura heredada del sandbox se mantienen. No tocar procesos ajenos.

Alternativas: ejecutar ambos tags de nuevo gasta un permiso no concedido;
reutilizar el modo E2B mezcla evidencia y autorización. Se elige un modo explícito
individual. Rollback: no usarlo; gate global/ADR0039/0040 siguen intactos.

Una respuesta válida no demuestra calidad de programación, tools, contexto lleno,
memoria total, sesiones sostenidas o concurrencia Gemma+Qwen. El permiso se
consume por el intento real, incluso si aborta; no reintentar automáticamente.
