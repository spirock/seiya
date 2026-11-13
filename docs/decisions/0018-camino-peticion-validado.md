# ADR 0018 · reducir trabajo redundante del camino de chat Rust

2026-10-24. Revisión de velocidad solicitada por usuario; una tarea offline
verificable, no cambio del servicio activo ni de inferencia/modelos.

Hallazgo: HTTP, scheduler y adapter validan y serializan de nuevo el mismo Value.
Se construye además una copia profunda del historial al crear la petición MLX.
Cliente HTTP Rust ya persiste/reutiliza pool; no se crea por petición.

- Nuevo testigo interno ValidatedInput, construcción privada tras validación
  completa/tamaño. HTTP valida una vez después de guardas/lectura; scheduler y
  adapter consumen el testigo sin repetir serialización/validación.
- Entradas públicas directas Scheduler::complete/InferenceAdapter::complete
  conservan validación completa y límites; no expone bypass ni testigo mutable.
- Historial se mueve desde body al request interno; no se modifica contenido,
  presupuesto, tool IDs/argumentos o decisiones del scheduler. Sin cachear respuestas,
  truncar contexto o cambiar workers. Cola/FIFO/cuotas/lease/cuarentena intactas.
- Microbenchmark HTTP en proceso con runtime inmediato falso,4 KiB/240 KiB,
 5 tandas de100 tras calentamiento. Evidencia antes/después, no extrapolar a
  velocidad MLX ni medir tokens/s con este test. No modelos, sockets o probes.

Alternativa de eliminar validaciones a ciegas se rechaza: cada entrada pública
necesita una frontera validada. La optimización no debe permitir entradas inválidas
o costosas adquirir slots. Cambiar transporte Python/SSE/consenso a la vez se
pospone para conservar un cambio lógico y comparación atribuible.

La revisión también identifica costes mayores fuera de esta tarea: consenso
Python hace N propuestas+N votos y puede recargar modelos. Web usa worker con
tools, Rust no ofrece consenso aún. No paralelizar modelos ni eliminar votos para
afirmar enjambre más rápido, ni presentar SSE bufferizado como tokens en vivo.

## Evidencia y límites

Implementado: testigo interno de propiedad, validación una vez por entrada y
movimiento del historial al request MLX. Entradas públicas siguen validadas;
regresión directa del scheduler ampliada con fixtures inválidas y exceso de tamaño.
Compatibilidad de tools/JSON/SSE, timeout, consumidor desconectado y cuarentena
cubierta por la suite existente:159 Rust y90 Python OK, fmt/Clippy/diff OK.

Microbenchmark release explícito (runtime falso, no inferencia): medianas de
5 tandas de100 peticiones tras100 de calentamiento, antes/después:

| Body | Antes µs/petición | Después µs/petición |
|---|---:|---:|
| 4184 bytes | 35,57 | 26,42 |
| 245848 bytes | 246,97 | 112,85 |

Artefactos `seiya-overhead-before-0018.json` y `seiya-overhead-after-0018.json`
en temporal OpenCode aprobado; log `seiya-s3-07-request-validation.log`.
Ensayo secuencial corto, sin ABBA, asignaciones/RSS ni control de temperatura:
orientativo, no promesa de porcentaje estable ni mejora demostrada del chat real.
Sin runtime/modelo cargado, cambio del proveedor activo o instalación. Rollback:
conservar servidor Python/configuración actuales; Rust continúa opt-in.
