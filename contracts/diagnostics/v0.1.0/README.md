# diagnostics-v1 · browser de propietario autenticado

POST `/api/diagnostics/v1`, JSON cerrado≤4096bytes, Content-Length exacto,
Host/Origin/auth antes de lectura/efectos. No es API OpenAI o herramienta de modelo.
status devuelve revisión, límites configurados, observación read-only del host y
último bloqueo registrado, sin rutas, prompts, PIDs o claves. Configure requiere
revisión CAS y documento Limits completo;400 invalidez/409 conflicto/503 disco.
Guardar nunca inicia inferencia, levanta cuarentena o confirma recursos seguros.

Rangos ADR0053: chat warning0..10000ms, swap64..2048MiB, plazo10..120s;
imagen warning siempre0, swap64..512MiB/plazo30..600s; sesión60..1800s en próximo
arranque. Critical/unknown siempre stop, no hay campos para desactivarlos. Límites
por turno se capturan al comenzar; cambiar no relaja una ejecución ya iniciada.
Cambios de revisión y snapshot atómico se confirman antes de ACK. Fallo ambiguo
no se reintenta automáticamente: status y revisión determinan qué quedó guardado.

ADR0056 añade `features {revision, features:{images:bool,multiagent:bool}}` al mismo
CAS/snapshot. Default false para compatibilidad; host evaluation inicializa ambos
true solo antes de una elección guardada. La API devuelve solicitado, instalado
y efectivo separadamente. No son grants de modelo o nuevos ejecutores, ni reset.
Solo el host autorizado con adaptadores adjuntos hace efectivos los switches.
