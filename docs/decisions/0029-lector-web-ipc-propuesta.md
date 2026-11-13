# ADR 0029 · propuesta de framing para lector web

Estado: **borrador de S4-04**, no implementación ni contrato congelado. Complementa
[ADR 0019](0019-chat-estable-y-web.md) y [ADR 0026](0026-puente-http-rust-web-opt-in.md).
No modifica la API OpenAI, el protocolo nativo ni habilita `web_fetch`.

**Actualización:** [ADR0030](0030-lector-en-proceso-rust.md) sustituye la propuesta
de helper de este documento por librería en la instancia Rust configurada y
congela la forma de los frames. El framing por pipes y su fixture se conservan
como evidencia sintética, no como arquitectura de producción. Las menciones
a decisiones pendientes siguientes son el estado histórico de este borrador.

## Alternativas y dirección propuesta

Se conserva la dirección de ADR 0019: helper Rust local opt-in invocado sin shell,
con una operación por proceso y pipes privados. Un servicio HTTP adicional ampliaría
la superficie de red y requeriría autenticación; FFI añadiría gestión de vida útil
y distribución a Python. Se rechazan para este incremento, no para siempre. El
prototipo de snippets continúa como default y rollback.

Cada proceso recibe un único objeto JSON UTF-8, seguido de EOF, y devuelve un único
objeto JSON UTF-8 seguido de EOF. No se admite JSON concatenado, trailing no blanco,
campos desconocidos, claves duplicadas ni versiones desconocidas. El host comprueba
cotas incrementalmente en ambos sentidos; desborde, UTF-8 inválido, EOF prematuro,
salida incompleta o salida extra son error cerrado, no resultado parcial de lectura.
La salida estándar se reserva para el protocolo; stderr no se muestra al usuario ni
se guarda sin redacción. No registrar contenido, URL ni credenciales por defecto.

## Identidad, vida útil y presupuestos

El operador fija explícitamente una ruta absoluta de binario; no se busca en PATH,
no se interpreta como shell y la fuente de configuración no puede ser el modelo.
La implementación debe verificar identidad e integridad del ejecutable antes de
activación y resistir sustitución entre verificación y ejecución, o fallar cerrado;
una ruta absoluta sola no demuestra identidad. El mecanismo concreto y su prueba
siguen pendientes. No se debe activar el helper antes de resolverlos.

La solicitud identifica el run y hasta tres IDs únicos de resultados de búsqueda
del mismo run. El host, no el modelo, construye la correspondencia con URLs; la
transferencia debe incluir solo las URLs asociadas a esos IDs y tratarlas como
datos no confiables. Un ID fuera del run o repetido se rechaza antes de red. Las
URLs siguen sujetas a validación completa por el helper en cada salto: no existe
autorización implícita ni permiso para seguir enlaces del contenido.

El plazo monotónico global de búsqueda más lectura pertenece al host y nunca se
reinicia al lanzar el helper. El host envía el tiempo restante, limitado por el
máximo del contrato; el helper establece un deadline monotónico local más estricto
si procede. Los caps de bytes/cabeceras incluyen redirecciones y fallos; la
cancelación o deadline exige terminar y recoger el proceso propio antes de liberar
su slot. Si no se confirma la salida, cuarentena y ninguna nueva lectura en ese
slot. No matar procesos ajenos ni interpretar cierre del pipe como salida del hijo.

## Trabajo necesario antes de congelar

- Definir límites concretos del frame JSON, IDs, encoding y campos de petición y
  respuesta, incluidos errores, contadores, truncamiento y correspondencia por run.
- Crear fixtures válidas e inválidas y pruebas automáticas de compatibilidad para
  versión, EOF, claves duplicadas, límites y asociaciones entre IDs y resultados.
- Resolver identidad/ejecución sin carrera, recolección/cancelación y presupuesto
  global verificables; registrar el resultado y alternativas de migración.
- Revisar que el presupuesto de red, redirecciones y cuerpo cumpla el
  [contrato de aceptación](../CONTRATO_CHAT_ESTABLE_Y_WEB.md); después S4-05
  implementará y probará la política, sin anunciarla por existir este borrador.

No se ha ejecutado red, cargado modelos, implementado un parser, transportado una
URL ni validado seguridad por este documento.

## Esquema preliminar para revisión

Los siguientes nombres y límites son propuesta, no una interfaz pública ni un
reemplazo del contrato de red. Cotas en bytes de la representación UTF-8: petición
16 KiB, respuesta 128 KiB, incluido JSON y espacios. Strings de ID ASCII opaco
`[A-Za-z0-9_-]`, longitud 1–128 bytes; URL hasta 4096 bytes. La longitud del
texto extraído se mide en puntos de código Unicode y conserva las cotas del
contrato; UTF-8 inválido y sustituciones silenciosas se rechazan. Un límite de
frame puede obligar a un resultado explícitamente truncado, nunca a JSON parcial.

Petición, únicamente construida por el host:

```json
{"version":"0.1-draft.1","run_id":"run-1","remaining_ms":12000,
 "sources":[{"id":"source-1","url":"https://example.org/article"}]}
```

`remaining_ms` es entero positivo de hasta 45000; no concede tiempo adicional ni
permiso de red. `sources` contiene de uno a tres elementos con IDs únicos del run;
la correspondencia exacta ID/URL procede de búsqueda realizada por el host, no
del texto del modelo. No se transmiten cookies, cabeceras elegidas por el modelo,
texto de instrucciones ni autorizaciones. La validación de URL de red es separada
del framing y se aplica de nuevo en cada redirect.

Respuesta preliminar, con un resultado por ID de la petición y en el mismo orden:

```json
{"version":"0.1-draft.1","run_id":"run-1","results":[
  {"id":"source-1","status":"ok","text":"Contenido sintético.",
   "text_truncated":false,"received_bytes":128,"decoded_bytes":128,
   "header_bytes":64,"get_count":1}
]}
```

Estados propuestos: `ok`, `policy_blocked`, `timeout`, `limit_exceeded`,
`http_error`, `unsupported_format`, `no_useful_text` y `transport_error`.
En errores `text` es `null`, no se entrega ningún cuerpo como texto útil y
`text_truncated` es `false`. En `ok`, texto no vacío y truncamiento explícito;
no afirma lectura total ni soporte semántico. Los contadores son enteros no
negativos y cuentan intentos fallidos/redirects, sujetos a sus cotas por página y
run. Son observaciones del helper, no prueba externa de consumo de red. Si no es
posible producirlos íntegramente, se falla la operación, no se inventa cero.

El host verifica versión, run, conjunto/orden y unicidad de IDs, formas, cotas,
contadores y coherencia entre estado y texto antes de aceptar resultados. Rechaza
respuestas parciales o de otro run; no mezcla resultados de invocaciones. Una
respuesta sintácticamente válida tampoco prueba cumplimiento de DNS, TLS o SSRF.
Antes de congelar faltan fixtures adversariales y tests en ambos extremos, así como
una decisión de metadatos de fuentes y cancelación sin ambigüedad.

## Ensayo de compatibilidad entre procesos

Se autoriza únicamente un binario de fixture del repositorio, sin red y sin leer
archivos del usuario. El test lo identifica mediante la ruta del binario producido
por Cargo, alimenta los frames sintéticos y comprueba EOF y reaping. No es un
launcher de producción: la ruta de Cargo no resuelve la identidad sin carrera,
no autoriza ejecución de un binario elegido por el usuario y no implementa
cuarentena o cancelación de producción. El timeout del test debe detener y recoger
solo su propio hijo; no es prueba de cleanup de procesos arbitrarios. El servicio
Python y el protocolo público no cambian; no hay fallback ni activación de red.
