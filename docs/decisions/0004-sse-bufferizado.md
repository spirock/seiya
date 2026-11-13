# ADR 0004 · SSE bufferizado y herramientas externas

2026-10-19 · S1-05.

Se conserva la inferencia no stream y la validación completa antes de enviar
headers. Alternativa descartada en este cambio: streaming incremental del
backend, que exigiría validación parcial, cancelación y errores tras headers.

Con `stream:true` se serializa una respuesta completa en dos eventos `data:`:
mensaje/delta (tool calls con índice estable) y finish/usage/swarm, seguidos de
un único `[DONE]`. ID, created y modelo son los mismos en ambos eventos. No hay
tokens fingidos, pausas, heartbeat ni conexión abierta tras DONE. La memoria del
buffer está acotada por la respuesta del transporte existente (8 MiB); no hay
una cola de eventos ilimitada ni una tarea nueva para escribir SSE.

Los fallos antes de obtener un completion válido siguen siendo HTTP JSON con
su status original, nunca 200 con un DONE falso. Auth/Host/Origin, cola y lease
siguen el camino de S1-04. La desconexión no cancela mágicamente el backend.

Seiya transmite llamadas y resultados estructurados; el cliente externo concede
permisos y ejecuta herramientas. Prosa JSON no se convierte en llamada. No se
introduce ejecutor, acceso a archivos, shell, Mail ni protocolo nativo draft.

Migración: `stream:false` y discovery no cambian; el header experimental pasa
de `chat-json-no-sse` a `chat-buffered-sse`. No promover a OpenCode operativo
antes de S1-06. Pruebas sintéticas no acreditan un modelo ni sesiones reales.
