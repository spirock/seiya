# ADR0031 · transporte HTTPS acotado del lector

S4-05 en implementación, no activación. Dependencias TLS mínimas fijadas con
Cargo; no red de lectura real ni paquetes del sistema.

Se usará TLS Rustls con raíces WebPKI, SNI y verificación de certificado por
hostname, y HTTP/1.1 sin pooling, cookies, proxies, autenticación, retries ni
redirects implícitos. La conexión se hace a una IP previamente validada y fijada;
el hostname permanece en TLS y Host. No se intercambia la URL por una IP en TLS.

Se descarta inicialmente el cliente reqwest para recibir páginas: sus límites
de cabeceras HTTP/1 no son configurables al cap64 KiB requerido antes de acumular.
Un parser HTTP/1 pequeño y conservador permitirá limitar cabeceras y cuerpos de
forma incremental. Respuestas/framing ambiguos, transferencias no soportadas y
content-encoding distinto de identity fallan cerrado. No se implementa gzip en
esta primera entrega: solicitar identity y rechazar compresión evita bombas y
no se atribuye soporte a descompresión no probada. HTML/texto UTF-8 únicamente;
rechazo de otros charsets visible, sin reemplazo silencioso.

Las redirecciones son manuales, hasta2, con validación completa y DNS nuevos por
salto bajo el mismo deadline y contadores; cada cuerpo cuenta aunque se descarte.
Se rechazan todos los resultados DNS si alguno no es público. Solo IPv4 pública
y subconjunto conservador IPv6 global2000::/3, con rangos especiales excluidos.
Permitir un destino público no demuestra la integridad de su contenido.

El transporte y parser se prueban con streams en memoria sin DNS/red real. No
abrir el endpoint de ADR0030 hasta probar guardas, slot, deadline, cancelación y
cuarentena. Un resolver del sistema puede tener trabajo bloqueante no cancelable:
si su finalización no se puede confirmar, no liberar admisión como si estuviera
limpia. Rust no convierte una llamada bloqueante cancelada en trabajo terminado.

Consecuencias: más código de framing a revisar y soporte HTTP deliberadamente
restringido frente a un cliente general. No soportar HTTP2, HTTP3 o compresión
no justifica fallback ni usar Safari. Rollback: lector desactivado, snippets
con limitación visible. Contratos OpenAI/nativo y servicios activos intactos.

## Estado implementado y frontera pendiente

Existe constructor explícito de librería `app_with_reader`, separado de CLI y
router normal. Requiere clave no vacía suministrada por host, guardas existentes,
frame acotado y slot por batch. No configura ni resuelve credenciales del usuario.
No hay extracción: cuerpo200 produce `no_useful_text`, no éxito útil fingido.
Timeout o caída del future mantiene cuarentena; los tests del router son en
memoria y no acreditan cancelación por desconexión TCP. Axum no garantiza que
desaparezca el handler JSON al desconectar el cliente: no activar este constructor
en el servicio antes de resolver explícitamente esa diferencia. Límites y
contadores se refieren a bytes HTTP entregados al parser, no a tráfico cifrado
o buffers TLS/kernel. Errores de exceso de cota abortan el batch (413) sin
producir un frame que oculte observaciones fuera de rango.

## Cancelación explícita

Se conservan respuesta JSON y códigos HTTP, con operación separada
de cancelación por run y deadline de respaldo si se pierde conexión. Esto precisa
la promesa anterior de ADR0030: desconectar el TCP no es una señal fiable para
detener el handler JSON; no se afirma cancelación inmediata por desconexión.

Nuevo `POST /internal/reader/v0.1/cancel`, con las mismas guardas, secreto y límites
de frame. Cuerpo `cancel.json` y esquema al lado de los frames0.1-draft.1: solo
versión/run_id.202 reconoce solicitud, no confirma limpieza ni deshace un run ya
completado. El read pendiente devuelve409 si observa cancelación. Si todavía no
inició DNS, se libera el slot; si había operación en vuelo, cuarentena porque la
finalización de DNS del SO puede ser incierta. No arrancar otro run en esa instancia.

Se conservan hasta128 IDs cancelados por instancia, sin expiración silenciosa;
superar capacidad devuelve503. La cancelación es idempotente y también se registra
antes de admitir un read para cubrir la carrera de upload/cancel. No es un grant
del modelo, no acepta una URL, no afecta inferencia y no mata procesos. El host
debe usar IDs nuevos por run, no reiniciar el mismo run cancelado. No se garantiza
que un cancel llegado después de completar la operación cambie su resultado.

Cliente Python/UI de S4-07 debe enviar cancel por decisión del usuario, diferenciar
202 de limpieza confirmada y conservar deadline/cuarentena como respaldo. No se
añaden retries de lectura. Ninguno de estos endpoints está activo en CLI/servicio.
