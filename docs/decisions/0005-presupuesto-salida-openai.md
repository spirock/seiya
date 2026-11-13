# ADR 0005 · presupuesto de salida del cliente OpenAI

2026-10-19 · S2-01. Extensión del servidor Rust experimental; no cambia el
protocolo nativo draft ni el comportamiento del prototipo Python.

## Problema y alternativas

OpenCode 2.0.32 envía `max_completion_tokens`; Rust lo ignoraba y usaba config.
Un límite solicitado menor podía perderse aunque el smoke de 2048 pasara.

- Priorizar uno de los dos campos: oculta un techo contradictorio del cliente.
- Tomar el mínimo de ambos: conservador, pero disimula configuración ambigua.
- Rechazar siempre ambos: innecesariamente incompatible si son iguales.
- **Elegido:** validar ambos; aceptar iguales, rechazar valores diferentes.

## Contrato de esta extensión

- Cada campo presente debe ser un entero JSON entre 1 y 4096. Null, booleanos,
  strings, decimales, objetos/arrays y fuera de rango: HTTP 400 antes del runtime.
- Solo `max_tokens` o solo `max_completion_tokens`: ese es el techo solicitado.
- Ambos iguales: un único techo. Ambos distintos: 400
  `max_tokens and max_completion_tokens must match when both are provided`.
  Se comparan antes de limitar por config, incluso si ambos exceden ese límite.
- Sin ambos: usar `runtime.max_output_tokens`.
- Presupuesto efectivo: mínimo entre techo solicitado y config. Config es un
  límite del operador, no puede aumentarse desde la petición.
- Backend MLX recibe **solo `max_tokens`** efectivo, nunca ambos aliases.
- No modifica usage, respuesta, truncación o estimación de contexto. Si el backend
  termina por length sigue siendo error, no se recorta ni maquilla como éxito.

## Migración y evidencia

Clientes con `max_tokens` conservan su contrato/mensajes de validación. Clientes
que enviaban `max_completion_tokens` ahora obtienen validación y respeto al techo;
los que envían valores contradictorios deben corregirlos, sin precedencia oculta.
Python sigue ignorando este alias: fixtures Rust de extensión separadas de las
golden compartidas de Python v0.1.0. No se cambia el proveedor operativo.

Tests de transporte sintético y router HTTP demuestran el valor enviado y cero
generaciones ante inválidos. No es una nueva prueba real de OpenCode/MLX ni
aceptación de otras opciones (`store`, `stream_options`, etc., S2-02).
