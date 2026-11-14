# ADR0069 · lifecycle de modelos nativos y cierre durable

Estado: implementado y probado con fixtures/helper propio; alcance macOS Ollama
y pipeline nativo. Sin validación de modelos/VRAM reales ni instalación/publicación.

## Decisión y alternativas

Reutilizar los adaptadores existentes: un proceso Ollama propio por turno de
texto/cerebro/trabajador y un proceso generador propio por imagen. Comparten
exclusión de sesión y lease estable/legacy. No adoptar11434, descargar pesos,
iniciar al abrir ni introducir Docker. Catálogo HTTP externo sigue independiente
y sin unload gestionado. Los pins Gemma/Qwen y binarios/assets actuales no se
convierten en modelos arbitrarios por este incremento.

Driver Rust de lifecycle: preparación, carga, ejecución, descarga/cierre,
inactivo confirmado o cuarentena. Un slot hasta confirmar cierre, incluidos
errores y cancelación. Cierre significa fin verificado del proceso y su árbol
propio; no una cifra de VRAM/RAM libre ni auditoría de todas las cachés del SO.
Alternativa descartada: keep_alive=0 y/o inventario vacío como única evidencia de
liberación. No controla procesos/VRAM ajenos y no sustituye OwnedChild.close.

Antes de spawn, con lease exclusivo ya adquirido, persistir native-model.inflight
en todos los dominios de coordinación. Se elimina con fsync y comprobación de
identidad solo después de cleanup confirmado. Drop/crash mantiene intención;
acquire posterior falla incluso si no hay cuarentena previa. Fallar al persistir
no inicia proceso. Fallar al confirmar mantiene bloqueo; no borrar archivos
externos/cambiados ni recuperar automáticamente. El marcador no concede permiso.
Los hosts actuales reconocerán este journal, pero binarios antiguos no: rollback
solo tras revisión de procesos/operaciones pendientes. No migrar ni borrar pesos.

## Imagen desde chat nativo

Perfil v1 añade chat_images opcional false. Exige adaptador de imagen y features
images=true. Revisión nueva con rol imagen seleccionado puede activarlo con aviso
explícito; perfil anterior sin revisar sigue manual. No alterar assets congelados;
reusar puente JS/broker de ADR0068. Diagnostics puede desactivar imágenes y la
tool desaparece, sin fingir liberar una generación en curso.

Presupuesto native3584bytes permanece. La primera propuesta image_generate se
procesa tras terminar/cerrar Ollama; solo entonces el broker inicia la imagen.
Cada request completo con schemas se valida antes de spawn: si no cabe, error
context_exhausted, sin recortar historial ni relajar el límite. No segundo turno
automático del cerebro tras generar PNG. Cancelación/timeout no confirman unload;
el adaptador de procesos y el driver conservan estado incierto cuando corresponde.

## Entrega y límites

Fixtures de orden/carga/cleanup/cancel/drop y journal entre dominios; validación
del perfil y chat con runtime falso y límite nativo real de serialización. No
modelos/inferencia/VRAM ni GUI/instalación reales. Sin herramientas nuevas de PC,
publicación, servicios compartidos o garantías de aceleración/ahorro de pesos.
