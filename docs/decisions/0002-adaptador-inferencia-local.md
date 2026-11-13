# ADR 0002 · adaptador local, sin promoción del servidor

Actualización S1-04: la integración HTTP JSON opt-in se define en ADR 0003.
El alcance siguiente describe la decisión original de S1-03; no se promovió
el servidor a producción ni se conectó un modelo real por esa integración.

- Fecha: 2026-10-19; alcance S1-03, experimental.
- El adaptador es una librería interna del crate servidor. No se conecta todavía
  a la ruta HTTP pública: chat sigue devolviendo 501 hasta integrar payloads,
  scheduler y escritura de respuestas en S1-04/S1-05.

## Decisión

- Reqwest 0.12 con features por defecto desactivadas y solo JSON. No TLS,
  compresión, proxy automático, retries ni redirects: este backend solo acepta HTTP
  a `127.0.0.1` con puerto explícito y path `/v1` (o `/v1/`). Sin DNS,
  credenciales URL, query o fragment. HTTPS/remoto requieren otro contrato.
- UUID v4 para IDs de completion compatibles con las fixtures, no contadores
  reutilizados entre procesos. Versiones fijadas en Cargo.lock.
- Configuración `RuntimeConfig` separada de la del listener. Backend `mlx`
  únicamente; Ollama necesita unload y pruebas propias antes de anunciarse.
- Interfaz de transporte async con future boxed, sin otra macro async-trait.
  El transporte simulado permite comparar requests/responses con las fixtures.
- No abrir conexiones al construir el adaptador. POST solo cuando su consumidor
  invoque explícitamente `complete`; tests reales de transporte solo contra
  servidores sintéticos en loopback y puertos efímeros.
- Validar peticiones antes del transporte. Preservar mensajes/tools y strings
  de argumentos; no usar los tipos del protocolo nativo para chat completions.
- Worker único, salida acotada por configuración, timeout 1–600 s y respuesta
  máxima de 8 MiB comprobada también durante la lectura, no solo por headers.
- Payloads de entrada del adaptador: máximo 256 KiB. Su validación no sustituye
  al guard HTTP antes de leer/parsing, que se integra en S1-04.

## Vida del trabajo y fallos

Una invocación mantiene un estado compartido exclusivo. Abandonar el future
del consumidor no debe liberar ficticiamente una generación: el trabajo propio
continúa hasta respuesta/error/timeout y sigue ocupando el adaptador.

Tras error de transporte/timeout o pérdida del trabajo se desconoce si el
runtime sigue generando. El adaptador queda bloqueado para nuevas generaciones
(`requires_recovery`), sin matar ni reiniciar el runtime. Volver a crearlo solo
después de verificar que el backend está libre. Los errores de estructura de
una respuesta JSON ya recibida no implican generación en curso.

Esto es una protección conservadora, no cancelación del modelo ni el scheduler
completo. No crear un adaptador por petición, no usar instancias independientes
para saltarse el estado bloqueado y no afirmar que detener el proceso Rust
detuvo MLX. El cierre/drenaje integrado y la cola se trabajan en S1-04.

## Compatibilidad y límites

- Retornar el envelope completo de completion y errores redactados. No publicar
  URL/model path, respuesta HTTP del backend, prompts o argumentos en errores.
- Preservar estructura/uso/IDs de tools, sin ejecutar nada. Una llamada inválida
  falla; JSON escrito como prosa se conserva como texto.
- El adaptador produce JSON completo, no SSE. `stream` se valida pero el backend
  recibe false; la decisión de emitir SSE pertenece a la capa HTTP de S1-05.
- Consenso explícito se rechaza, incluso con tools. Nunca afirmar mayoría.
- Validación más estricta que aceptaciones accidentales de Python: backend role
  assistant, content texto/null coherente con llamadas, usage no negativo y
  finish reason soportado. No se copia una aceptación malformada como requisito.
- No se ha probado un modelo real ni integración nueva de OpenCode en este paso.

Reqwest 0.12.28 usa MIT OR Apache-2.0 y UUID 1.28.0 Apache-2.0 OR MIT, verificado
contra el metadata resuelto. El servidor declara Rust 1.89 mínimo porque UUID
lo requiere y las ICU transitivas requieren 1.88; `seiya-contracts` conserva
su declaración 1.85. La compilación se prueba con 1.93.1, no se ha ejecutado
una prueba de MSRV exacta con 1.89.
El adaptador no demuestra ahorro de memoria, performance o paridad Ollama.
