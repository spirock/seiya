# ADR 0006 · opciones explícitas del chat experimental

2026-10-19 · S2-02. Extensión Rust, no cambio del prototipo Python ni del protocolo
nativo. S1-06 observó los nombres `stream_options`, `store` y
`max_completion_tokens` además de los campos básicos. Sus registros resumen
nombres, no todos los valores de esas opciones: los casos de abajo son un
contrato sintético, no nuevas capturas de OpenCode real.

## Alternativas y decisión

Ignorar todas las opciones desconocidas mantiene compatibilidad accidental pero
puede ocultar requisitos de parada, sampling o persistencia. Reenviarlas sin
validación delega decisiones al backend y amplía la superficie. Rechazar todo
incluido `store:false` y `include_usage` impide compatibilidad útil implementable.

Se elige una lista explícita de campos raíz admitidos:
`model`, `messages`, `tools`, `temperature`, `max_tokens`, `max_completion_tokens`,
`swarm_mode`, `stream`, `stream_options`, `store`. Los contratos ya implementados
se conservan; presupuesto según ADR 0005. Campos raíz adicionales: HTTP 400
`request contains unsupported fields`, sin reflejar claves o valores del cliente.
Esta lista no añade validación completa de mensajes/tool schemas anidados.

## Opciones nuevas

| Campo | Política |
|---|---|
| `stream_options` ausente | SSE histórico: usage en el último de dos chunks |
| `stream_options` presente | Solo con `stream:true`, objeto con única clave opcional `include_usage` |
| Objeto vacío o `include_usage:true` | Incluir usage en el chunk final, sin evento extra |
| `include_usage:false` | Omitir usage de ambos chunks; conservar finish/swarm/DONE |
| `include_usage` otro tipo / opciones extra | 400 antes de admisión/backend, incluso null |
| `store` ausente o `false` | No existe almacenamiento de completions en Seiya; no reenviar la opción |
| `store` otro valor, incluido `true` o null | 400, no simular persistencia ni ignorar una petición de almacenamiento |

`store:false` se refiere solo al contrato del servidor Seiya: no desactiva la
persistencia de sesiones OpenCode, logs del backend o cachés MLX. No es garantía
de privacidad/retención de otros procesos. JSON no stream conserva usage.

`tool_choice`, `response_format`, `logprobs`, `parallel_tool_calls` mantienen sus
rechazos existentes. `stop`, `seed`, `top_p`, `n`, `user`, `metadata`, `service_tier`,
etc. no se soportan por estar presentes en otra API; se rechazan por la lista.
No se anuncian storage, logprobs, sampling adicional o ejecución paralela.

## Migración y pruebas

Clientes que enviaban campos ignorados deben retirarlos o solicitar soporte en
un incremento separado. No se modifica silenciosamente la configuración del
usuario para ocultar errores. Default SSE y golden Python se conservan; estas
reglas tienen fixtures Rust separadas. Las opciones aceptadas no pasan al MLX,
que sigue recibiendo la petición explícita con `stream:false`.

Pruebas de transporte/router y SSE con backend simulado; regresión combinada
del cuerpo observado y presupuesto corresponde a S2-03. No hay prueba real nueva
ni promoción del proveedor. SSE sigue bufferizado, no token streaming.
