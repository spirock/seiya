# ADR0068 · modelos de imagen sustituibles y herramienta conversacional

Estado: implementado y verificado con fixtures en la ruta de catálogo HTTP;
residencia gestionada de motores arbitrarios pendiente de contrato de motor.
ADR0069 añade después lifecycle y chat opt-in en el perfil nativo macOS, sin
promocionar motores externos arbitrarios ni aumentar presupuesto3584bytes.

## Alternativas y decisión

No sustituir el pipeline nativo Qwen pinned ni prometer kernels para pesos
arbitrarios. El catálogo local admite image_models y un ID image opcional;
cada registro declara binding exacto y API OpenAI local preparada. Selección
desde el campo imagen existente, mismo CAS; sin cambios de pesos ni perfiles
MLX. Catálogos anteriores continúan sin imágenes. El formato del modelo depende
del motor externo; registrar un nombre no hace compatibles pesos/plantillas.

API de imagen opt-in: inventario GET /v1/models y POST /v1/images/generations,
model exacto, prompt≤2048bytes, n=1, size=512x512, response_format=b64_json,
seed fijo42. Respuesta exige model coincidente y un único data[].b64_json PNG;
no URLs de descarga, rutas, redirects, proxy o fallback. Límites bytes/timeout,
PNG decodificado por el validador existente antes de éxito. Un servidor que no
cumpla este contrato necesita adaptador, no relajación silenciosa.

Texto e imágenes comparten slot/locks/journals y recursos del catálogo. Persistir
intención antes de POST; timeout/drop/error ambiguo pone en cuarentena. Cancelar
espera fin/deadline, no matar servicio ajeno ni anunciar unload. Propiedad externa:
no afirmar que están descargados los otros modelos ni medir VRAM ficticia.
OpenAI no estandariza load/unload: la residencia gestionada requiere contrato
de motor dedicado y evidencia de liberación; no enviar comandos supuestos.

## Chat y frontend

image_chat boolean opcional false; exige imagen seleccionada, cerebro con tools
y presupuesto≥8192bytes. Solo /api/chat, nunca duplicar tools de OpenCode.
image_generate acepta únicamente prompt; modelo no elige URL, archivos, ID,
sesión, semilla o resolución. Solo una acción como primera decisión, antes de
recibir datos web. Prosa no dispara imagen; rechazar tool_calls mixtas/múltiples.
El usuario activa la función al revisar el catálogo y solicita la imagen en el
chat; no generar al abrir, health, detectar, guardar ni iniciar host.

Reutilizar ImageJobs para sesión privada, request_id del navegador, progreso,
cancelación, PNG y recuperación. No pasar capabilities al modelo. El puente JS
nuevo es el alcance frontend funcional separado de la petición actual; preserva
los seis assets congelados y el diseño, usa el panel existente y no inserta HTML
del modelo. Guardar referencia de envío antes de llamar; no replay al recargar.
La respuesta del chat confirma registro, no afirma imagen completada; ese estado
lo acredita el broker/validador. Sin segundo turno automático del modelo.

## Compatibilidad y entrega

Catálogo v1 amplía campos opcionales, ejemplos opt-in y tests de migración.
Perfiles nativos anteriores mantienen ejecución/control manual; no simular
compatibilidad del perfil3584bytes con tools nuevas. Rollback desactiva image_chat
y selección de imagen, conservando historial; no usar binario viejo tras journal
incierto sin revisión operativa. Solo fixtures en este incremento; motores,
calidad de imagen, GPU/memoria/instalación reales y otros SO sin validar.
