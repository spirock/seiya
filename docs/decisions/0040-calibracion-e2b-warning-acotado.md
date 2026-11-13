# ADR0040 · calibración única E2B con warning acotado

2026-11-11. Calibración acotada autorizada: una carga de `gemma4:e2b-mlx`,≤120s, una
respuesta≤256tokens, contexto solicitado4096, think=false. Sin Qwen ni retries.

El intento ADR0039 fue interrumpido por nuestra guarda al aparecer warning, no
demostró imposibilidad de completar inferencia. La calibración E4B anterior no
sirve de presupuesto para este tag. No modificar el gate normal ni ADR0039.

## Excepción experimental, no perfil de producción

- Presión normal en preflight y justo antes de la petición, lease exclusiva y
  Ollama compartido vacío. Protección de pesos/ownership/cleanup de ADR0039.
- Solo durante esta petición se toleran **hasta10000ms acumulados de warning**.
  Contabilidad conservadora: cargar el intervalo completo entre dos muestras si
  cualquiera de sus extremos es warning. No resetear el acumulado al volver a
  normal. No prometer detectar transiciones ocurridas entre muestras.
- Presión crítica/desconocida, vigilancia faltante/intervalo>1000ms, overflow,
  crecimiento swap>512MiB sobre preflight o deadline120s abortan a cleanup propio.
  Inicio con swap ya usado no equivale a memoria libre. Lecturas best-effort;
  no garantía hard-real-time, prevención de OOM o ausencia de ralentización.
- Selector manual separado `gemma-calibration` con confirmación propia, solo
  E2B; no parámetro de HTTP/modelo ni habilitación de jobs. Acumulación/política
  pura en Rust, adaptador macOS aporta relojes/mediciones y aplica decisiones.

Alternativas: repetir con stop inmediato reproduce el bloqueo conocido; aceptar
warning indefinido borra un control importante. Se elige ventana corta explícita
y swap/deadline/critical independientes. Mantener original y evidencia fallida;
una carga completada no acredita estabilidad sostenida, tools o concurrencia.

Rollback: no usar este modo; los modos anteriores siguen rechazando warning.
Sin configuración, pesos, dependencias o servidor compartido modificados.
El permiso se consume por el intento real, también si termina en parada.
