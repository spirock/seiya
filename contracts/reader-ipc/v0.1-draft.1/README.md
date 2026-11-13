# Frames experimentales del lector

`request.json` y `response.json` son fixtures sintéticas del contrato interno de
[ADR0030](../../../docs/decisions/0030-lector-en-proceso-rust.md), que sustituye
la propuesta de helper de ADR0029 por librería en el proceso Rust. No son
una API pública, una autorización ni evidencia de lectura real.

`request.schema.json` y `response.schema.json` describen la forma JSON Draft
2020-12 del borrador. Son complementos, no un validador completo: JSON Schema
no controla claves duplicadas después de parsear, EOF, tamaño UTF-8 del frame,
longitud en bytes de la URL, IDs duplicados por campo, correspondencia con la
petición, presupuesto agregado o texto solo con espacios. Además ciertos
validadores consideran enteros a números con parte decimal cero. El parser Rust
aplica estas comprobaciones adicionales salvo las que se indican abajo; no se
debe aceptar un frame únicamente porque pase JSON Schema. Los `$id` bajo
`seiya.invalid` son identificadores, no recursos de red.

`seiya-contracts::reader_frame` valida en memoria JSON estricto, cotas de frame,
versión, IDs, plazos, correspondencia de resultados, estados y cotas básicas de
contadores/texto. El llamador debe leer incrementalmente con límite y confirmar
EOF antes de pasar `eof=true`. Alternativamente `read_request` y `read_response`
leen hasta EOF con un máximo de frame más un byte, pero son síncronos: el llamador
debe imponer un plazo efectivo y controlar el proceso, pues un `Read` bloqueado
no es interrumpido por estas funciones. Las pruebas usan lectores en memoria,
salvo un ensayo con `UnixStream::pair` y threads del mismo proceso; no son pipes
de un proceso hijo ni validan su salida, cancelación o limpieza.
`matches_host_results` puede comparar cada ID/URL y el run contra una lista que
aporta el host, rechazando IDs duplicados en esa lista. El host debe llamar esta
función con su estado propio y confiable: no se obtiene ni autentica tal estado en
esta librería. La comparación tampoco prueba que una URL sea segura: la política
de transporte debe validarla por separado.
No valida DNS, TLS, redirecciones, cómputo de contadores, autorización, identidad
del ejecutable, proceso hijo, deadline real ni cancelación. Un frame válido no
significa que se haya leído una página.

Los resultados fallidos requieren `text: null` y `text_truncated: false`; un
resultado correcto requiere texto no vacío y al menos un GET declarado. Los
contadores son totales por página, de hasta tres GET: cabeceras hasta 192 KiB
por página; el límite de 64 KiB por respuesta HTTP se aplica en transporte,
no puede demostrarse con este agregado. El validador rechaza acumulados superiores
a 64 KiB multiplicado por el número de GET, y bytes observados si no hubo GET;
esto no prueba la distribución real por respuesta. Los límites compartidos con la búsqueda y
acumulados entre invocaciones corresponden al host y todavía no se aplican aquí.
No registrar las URLs o el texto real por defecto.

Prueba offline: `cargo test -p seiya-contracts --test reader_frame --locked`.
Estas pruebas no prueban helper Python/Rust, pipes, ejecución ni red externa. Los ejemplos
están congelados para esta versión interna: no habilitar el lector ni anunciar
`web_fetch` por tener codec/schema. El endpoint de ADR0030 existe solo en el
constructor Rust explícito de librería, no en CLI/router normal o servicio activo.

`cargo test -p seiya-contracts --test reader_process --locked` arranca un hijo
sintético fijo del repositorio, sin red ni modelos, con pipes reales: casos válido,
salida concatenada, petición inválida y timeout con kill/wait del hijo propio.
No prueba el ejecutable de producción, integridad sin carrera, descendientes,
deadline de red, cuarentena ni interacción con Python; no activa el lector.

Actualización: una cuarta prueba de `reader_process` invoca `python-peer.py`
con Python -I/-S para intercambiar las fixtures por pipes y validarlas en Rust.
El codec Python `seiya.reader_frame` valida las mismas formas y cotas, sin red,
subprocesos o configuración. Sus pruebas están en `test_reader_frame.py`.
La fixture Python no es un servicio ni un launcher; el plan de producción no
crea hijos para leer páginas. Las funciones de codec no se importan en el
camino habitual de chat y no conceden permisos. El perfil `web_reader` de S4-07
sí usa estos codecs de forma opt-in; el contrato solo no acredita cancelación
HTTP, cuarentena o red. Sus implementaciones y pruebas están en S4-05/07.
`invalid-frames.json` conserva seis frames adversariales que ambos codecs
deben rechazar; los tests Rust y Python consumen el mismo corpus sin reserializar
el frame interno (incluye claves duplicadas y JSON concatenado).

Operación adicional autorizada de cancelación: `cancel.json` y `cancel.schema.json`
para POST `/internal/reader/v0.1/cancel`. Requiere autenticación del host;202 es
solicitud aceptada, no prueba de limpieza. Read cancelado devuelve409. Carreras,
capacidad128 y cuarentena se describen en ADR0031. El frame de lectura y la API
OpenAI no cambian; no es una instrucción que una página o el modelo puedan emitir.

S4-07 añade `select.json`/`select.schema.json`: POST autenticado
`/internal/reader/v0.1/select` con request/response como strings JSON estrictos
(para preservar claves duplicadas hasta su parser), y lista de IDs. Límite externo
160KiB UTF-8, internos16/128KiB y semántica del selector Rust; respuesta
`{"sources":[{"id":"…","text":"…","text_truncated":false}]}`. No realiza
red ni otro batch. El esquema externo no sustituye las validaciones internas.
El cliente solicita solo IDs exitosos; los errores permanecen en metadata visible.

`python-profile.example.json` tiene ruta de modelo sintética y ninguna clave.
Valida configuración, no presencia de modelo ni soporte/contexto efectivo.
Operación/rollback/estimador/cancelación: [guía experimental](../../../docs/LECTOR_WEB_EXPERIMENTAL.md).
