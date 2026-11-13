# ADR0043 · Qwen con memoria solo informativa

2026-11-11. Usuario pide dejarlo sin límites y, tras acotar el alcance, confirma
**Autorizar intento sin cortes**: un intento `qwen3.5:9b-mlx` sin cortes por
presión de RAM o crecimiento de swap; mantener servidor privado, plazo120s,
una respuesta≤256tokens, contexto solicitado4096, think=false. Sin otros modelos,
descargas, cambios de servicio o retries automáticos. Excepción solo del ensayo,
no del supervisor general. Riesgo de ralentización/bloqueo/OOM comunicado.

## Decisión

Selector manual nuevo `qwen-observe-calibration`, confirmación propia ADR0043,
solo Qwen. Presión/swap/VM se registran como observaciones, **no admisión**:
sin stop por nivel, delta o tiempo acumulado de presión. Tampoco se exige presión
normal como gate de memoria inicial de este modo. Una medición de memoria fallida
se registra como ausente/error, no se inventa cero ni dispara ese stop.

Se conservan: lease compartida exclusiva, ausencia de modelos en Ollama compartido,
ownership nativo del Popen/descendientes, validación del manifiesto/pesos protegidos,
HOME/entorno privados, HTTP loopback acotado, deadline120s, límites de respuesta,
validación de identidad/finalización y cleanup propio confirmado. Fallo de
ownership, cliente, checker, deadline o cleanup sigue abortando. No se eliminan
autenticación, límites de API/archivos, permisos o protección del servicio activo.

Política de observación pura Rust separada. Adaptador macOS registra muestras
incluidos errores, sin inferir disponibilidad o sumar RSS/Metal. Todos los modos
anteriores conservan sus cortes. Monitorización/deadline best-effort, no protección
garantizada frente a OOM o bloqueo del host entre muestras. El SO conserva sus
propios límites: no se modifica macOS/Metal/Ollama para eludirlos.

Alternativas: quitar toda validación/exclusión excede la petición acotada; elevar
otra vez umbrales de memoria oculta el cambio. Se elige hacer explícitamente
informativas las métricas, con duración y operación aún limitadas.

Rollback: no usar este modo. Nada instalado/configurado/promovido; conservar
evidencia de todos los intentos. El permiso se consume por un intento real,
incluso si no completa. No garantiza respuesta ni admite el par simultáneo.
