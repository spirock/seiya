# Imágenes intercambiables desde el chat · experimental

Implementación Rust experimental del snapshot actual. Verificar el SHA del
paquete en BUILD-INFO.json y README. Validada
en macOS con mensajes, motores HTTP y PNG sintéticos; no con modelos reales.

## Configurar

1. Preparar por separado motores dedicados de texto e imagen que cumplan el
   contrato siguiente. Seiya no instala pesos, inicia ni adopta servicios.
2. Usar como referencia [images.example.json](../contracts/local-models/v1/images.example.json),
   **no ejecutable tal cual**: sustituir bindings/puertos/capacidades por los del
   motor. No sobrescribir una configuración existente. Mantener permisos privados.
3. Registrar hasta16 image_models, IDs únicos también frente al catálogo de texto.
   Seleccionar image o usar null para ninguna. Cada runtime declara binding exacto,
   backend openai_local, URL canónica127.0.0.1 y timeout. max_output_tokens se
   conserva por compatibilidad RuntimeConfig pero no se envía para generar imagen.
4. Para pedir imágenes mediante texto, image_chat=true, cerebro supports_tools=true
   y max_input_bytes≥8192. Es declaración, no certificación de su plantilla/tools.
   Para solo control manual, image_chat=false. Desactivarlo antes de quitar image.
5. En Configuración elegir cerebro, trabajadores y el ID de imagen registrado,
   revisar y guardar. Detener y esperar cierre confirmado antes de sustituirlo.
   CAS conserva catálogo, pesos, rutas y perfiles anteriores. Las rutas nativas
   de ejecutable/assets de imagen quedan vacías en esta entrada HTTP.

Al abrir/guardar/iniciar no hay inferencia. Encontrar Ollama tampoco prueba que
su API soporte imagen. La ruta nativa Qwen/MLX pinned tiene ahora un opt-in
chat_images separado: ver [residencia nativa](RESIDENCIA_NATIVA.md). Perfiles
anteriores continúan manuales hasta revisar; no se convierte cualquier peso en
compatible ni se amplía el presupuesto3584bytes.

## Contrato del motor de imagen

GET /v1/models debe anunciar el binding seleccionado. POST /v1/images/generations:

```json
{"model":"binding-exacto","prompt":"Un caballo azul","n":1,"size":"512x512","response_format":"b64_json","seed":42}
```

Respuesta acotada≤24MiB, JSON con model idéntico y un único data[].b64_json
(PNG≤16MiB). Sin data[].url ni descargas indirectas. Se verifican contenedor,
CRC, dimensiones512×512 y píxeles/zlib antes de confirmar. **Esta extensión de
binding de respuesta no es universal en APIs OpenAI**: si el motor omite model,
devuelve URL/JPEG o usa otro protocolo, necesita un adaptador explícito. No hay
fallback ni relajación silenciosa. No probado con motores de imagen reales.
El decodificador nativo actual solo permite anunciar generación en macOS;
no afirmar soporte de imágenes Linux/Windows por compilar el catálogo.

## Pedir y recuperar

Solicitar explícitamente una imagen en el chat. El cerebro puede proponer
image_generate con prompt; no es un detector de palabras ni una garantía de que
todos los modelos reconozcan la intención. La prosa no ejecuta herramientas.
El host valida una sola acción, sesión y argumentos, y registra un trabajo.
La respuesta de chat **no confirma generación terminada**: el panel existente
ofrece estado observado, cancelar, Ver imagen y Descargar PNG tras validación.
No se ejecuta un segundo turno automático de síntesis. Sin visión ni control PC.

El puente JS nuevo conserva los seis assets congelados y el diseño. Guarda la
referencia antes de enviar, ligada a la conversación; refrescar no reenvía.
Si el envío es incierto, conserva el mismo mensaje/ID y consulta el panel; no
borres el chat ni cambies el prompt para intentar otra generación. Un reenvío
explícito con ID ya registrado recupera el trabajo sin otra inferencia/imagen.
Una sesión ajena, acciones múltiples o argumentos URL/model/path se rechazan.
Una búsqueda o página no puede autorizar la imagen: solo primera acción del turno,
antes de recibir resultados web, nunca tool después de snippets/extractos.

### Si se perdió la confirmación del chat

El panel puede mostrar el trabajo aunque el chat haya recibido un error de
conexión. En las fuentes nuevas, **Recuperar imagen registrada** aparece cuando
una lista válida del broker coincide con la referencia pendiente de ese chat.
Al pulsarlo se consulta una lista fresca y se confirma el registro localmente:
no se reenvía el chat ni se solicita otra generación. Recuperar un trabajo en
cola/en ejecución **no significa que el PNG haya terminado**.

Se limpia el compositor solo si todavía contiene exactamente el texto original;
un borrador distinto se conserva. Si falla el almacenamiento, la referencia y
el borrador se restauran. Una lista vacía, auth fallida, estado ambiguo o respuesta
tardía de otra conversación no descarta la referencia. El chat no se puede
eliminar mientras siga sin confirmar, antes de olvidar trabajos o PNG propios.
Las respuestas de registro malformadas tampoco dan por confirmado el envío.

Las pruebas `chat_image_recovery_ui.cjs` usan todos los complementos en orden de
producción y un broker simulado; las Rust ejercitan la ruta HTTP y PNG sintético.
No constituyen una generación real con el cerebro/motor configurados.

### Validación real aún pendiente

Se necesita una prueba explícita, aislada y autorizada con modelos ya preparados:
pedir una imagen512×512 desde el chat, observar el cambio de residente (si el
driver nativo lo gestiona), esperar completed y abrir/descargar el PNG. Comprobar
después recarga sin nueva generación y limpieza de procesos propios. No iniciar
otro coordinador sobre un runtime compartido, quitar guardas de presión, borrar
cuarentena, descargar pesos o adoptar servicios para superar esta prueba.
Calidad visual, reconocimiento de intención y liberación de RAM/VRAM se registran
por separado; una imagen no valida sesiones largas o todos los modelos.

El harness manual ignorado `image_web::real_chat_image::real_chat_image_once`
usa el mismo host desktop nativo y su router autenticado, con un perfil temporal
privado y datos vacíos bajo el temporal aprobado. Exige permiso explícito por
entorno y rechaza una raíz ya utilizada. No forma parte de `make test` normal.
Hace una sola petición de chat (sin forzar tools), espera el estado observado,
recupera/decodifica el PNG si termina y solicita stop/drenaje antes de reportar.
Si se bloquea o el cerebro no propone imagen, guarda el error sin retry ni
fallback al control manual. No usa el historial o configuración guardados del
usuario y no valida Safari por ejecutar HTTP en proceso. Repetir exige un nuevo
permiso y otra raíz; no borrar evidencia para volver a ejecutar la misma.

Ensayo autorizado2026-11-15: preflight e inicio del host correctos, pero la carga
del cerebro Gemma alcanzó presión crítica del host a3,527s y fue detenida por la
guarda. No se registró ni generó una imagen. Limpieza confirmada de3 procesos
propios, sin marcadores pendientes; no retry ni relajación de límites. Por tanto,
la integración sigue validada con fixtures, **no end-to-end real** en este equipo.
Hace falta resolver el presupuesto de recursos y un nuevo ensayo autorizado;
no sustituir automáticamente el modelo ni cerrar otras aplicaciones.

Regresión final de recuperación:499 pruebas Rust,196 Python y8 suites Node
correctas en ejecución serial fuera del árbol iCloud;25 ignoradas, incluido el
harness real manual. Formato/Clippy correctos. Una fixture anterior de sockets
agotó su plazo2s en el árbol original y pasó aislada y en la repetición completa;
no se modificó su deadline ni se da por corregida su causa. El intento real
fallido descrito arriba sigue siendo un resultado separado, no una prueba verde.

Los instaladores anteriores al incremento de recuperación (incluido bbe3139)
no contienen el nuevo botón ni el guard de borrado integrado; verificar el SHA
del build en README y BUILD-INFO.json antes de probar. Rollback de las fuentes: retirar el
hook de borrado y el complemento de recuperación, sin borrar historial ni jobs.

## Memoria, cancelación y límites

Texto y generación comparten exclusión y journals del catálogo. Memoria desconocida
o baja bloquea; no se presupone que quepan cerebro/trabajadores/imagen a la vez.
El motor externo debe dedicarse a Seiya y administrar su propia residencia.
No hay descarga automática de RAM/VRAM genérica: OpenAI no define load/unload
ni confirma liberación. Para eso falta elegir/contratar lifecycle de un motor.

Cancelar solicita y espera respuesta/deadline; no mata procesos ni acredita unload.
Si el broker agota su drenaje, timeout/drop/error ambiguo conserva journal y
cuarentena; no reutilizar modelo ni borrar marcadores para reintentar. Persistencia
de trabajos/PNG en image-history, sin replay al reiniciar. La calidad, recursos
reales, sesiones largas y cancelación de un motor real siguen sin validar.
