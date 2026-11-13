# image-jobs-v1 · browser opt-in

POST `/api/images/v1`, JSON cerrado≤16KiB, Content-Length exacto, auth/Host/Origin.
No es jobs-v1 de texto, API OpenAI o SessionRequest. Ejemplos sintéticos en
commands.json: sus IDs/capacidades no conceden acceso ni provocan ejecución.

Sesión volátil opaca por conversación; no se coloca en URLs/prompts/localStorage.
Submit usa UUID y request idéntica para reintento manual idempotente; cambiar
request con ese UUID falla. Inicial512×512, seed u32, prompt≤2048bytes sin NUL;
no rutas/modelos/URLs en el schema. Estado sin porcentajes, cancel requiere
confirmación del ejecutor. No retry automático tras error ambiguo.

Todos los controles devuelven JSON salvo artifact completado: bytes image/png,
Content-Disposition attachment con nombre fijo, nosniff/no-store. Nunca devolver
un PNG antes de validar dimensiones/CRC/zlib/filtros/hash y confirmar cleanup.
Capacidad/datos acotados. Extensión aditiva ADR0052: `open {conversation_id}`
recupera la sesión del UUID de chat tras auth; `forget {session}` borra registros
terminales propios (rechaza tareas aún vivas o cuarentena). No son model tools.
Perfil de propietario único autenticado: UUID no concede permisos ni representa
aislamiento multiusuario. Capacidades no se guardan en el navegador. Constructor
persistente opt-in escribe snapshot privado y PNG binario; `volatile:false` lo
anuncia. Sin almacén solo se recupera al recargar mientras el host siga vivo.
Reinicio nunca hace submit: queued→cancelled, activa→quarantined y pool cerrado.
PNG restaurado pasa otra vez por decoder/hash/metadata antes de servirlo.
Error de almacén→503 y nuevas ejecuciones/publicaciones bloqueadas; no borrar el
snapshot para recuperarse automáticamente. Corrupción/versión/cuotas fallan al abrir.
Disponibilidad depende del host real y decoder soportado; default false.
