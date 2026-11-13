# ADR0052 · historial persistente y robustez sin inferencia

Autorizado: punto2 (conservar trabajos/imágenes tras recargar/reiniciar), después
punto4 (reconexión/cancelación/errores), sin rediseño, modelos o delegación.

## Persistencia opt-in Rust

Snapshot local versionado y atómico con metadata JSON acotada y blobs PNG binarios,
sin dependencias nuevas, claves API o rutas del navegador. Directorio privado
canónico0700, ficheros0600, lock exclusivo mantenido abierto; temp create_new,
sync_all/rename/sync directorio. Rechazar symlinks, archivos no regulares, corrupción,
versiones desconocidas y cuotas; nunca inicializar vacío encima de error.
Un escritor por almacén. No SQLite en esta fase: cuotas existentes pequeñas y
snapshot completo simplifican atomicidad sin instalar una dependencia. Coste:
I/O síncrona acotada al checkpoint; no promesa de alto rendimiento o cifrado.

Constructores anteriores siguen volátiles. Constructor durable separado y ruta
estable del host, no la raíz temporal de cada arranque. Snapshots de broker de
borradores e imágenes separados. Guardar antes de aceptar submit, antes de
ejecutor y antes de publicar terminal/artefacto. Fallo de disco bloquea nuevas
ejecuciones/publicación, solicita cancelación y conserva snapshot previo.
Reinicio no retoma ni reenvía: queued→cancelled, ejecución incierta→quarantined
con pool cerrado. Nunca señalizar PIDs del snapshot ni reconstruir permisos de
modelo. Cuotas originales conservadas; borrado explícito de registros terminales
por conversación, nunca purga automática ni limpieza de evidencia nativa.

## Recuperación de identidad y UI

Comando aditivo `open {conversation_id}` usa el UUID estable ya guardado en el
historial browser. Auth/Host/Origin preceden al re-enlace: solo perfil de propietario
único, no aislamiento multiusuario. UUID no concede permiso; otro usuario con la
misma autenticación tiene el mismo ámbito. Reutilizar sesión por conversación y
renovar su TTL, sin poner capacidades en localStorage/URLs/prompts. No sincroniza
conversaciones entre navegadores: necesitan el historial/ID de conversación.
`forget` exige estado terminal confirmado y no cuarentena; borra solo registros
propios del broker, no modelos/evidencia. Capacidad anuncia si persistencia activa.
Al restaurar UI solo open/list/status/artifact, nunca submit. UUID del envío
ambiguo se guarda como referencia no secreta dentro del chat antes de enviar;
si no se puede guardar, no enviar imagen. No borrar borradores nuevos o presentar
artefactos de un chat anterior por carreras de red.

Punto4 endurece peticiones de control con timeout visible, recuperación explícita
de auth/TTL, validación de estados, respuestas tardías, doble clic y Blob cleanup.
Abortar fetch no confirma cancelación del runtime. Solo cancel/status determinan
el estado. Rollback: no adjuntar almacén; conservar snapshots, no borrarlos.
Pruebas con fixtures/directorios temporales propios, sin cargar modelos/servidores.
