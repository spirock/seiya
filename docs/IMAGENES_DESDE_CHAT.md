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
