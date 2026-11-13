# ADR0056 · opciones reales de imágenes y multiagente

Configuración permite activar imágenes/multiagente en el host de evaluación
manual y revisar límites de RAM. No concede generación automática, instalación,
shell/archivos/red externos o respuesta simulada. Mantener diseño y datos.

Persistir Features separado de Limits con CAS/snapshot: images y multiagent,
false en perfiles previos. Perfil nuevo --evaluation-web inicializa ambos true
solo si no había elección guardada; respeta elecciones siguientes. Cambios no
conceden nuevos permisos: host adjunta implementaciones autorizadas y el estado
muestra solicitado vs efectivo/ocupado/fallo. No switches de funciones inexistentes.
Multiagente inicial significa Gemma coordinador y Qwen auxiliar de borradores
por turnos, no dos modelos residentes ni ejecución de código. Ambos se preparan
desde blobs existentes verificados; lease/slot único, runtime privado cerrado
antes del siguiente modelo. Qwen no carga al habilitar: solo ante trabajo explícito.
Imagen sigue control manual existente, no tool conversacional inventada.

El host no ejecuta warmup/smoke ni prompts del agente. Interfaces de tools se
anuncian a Gemma según opción efectiva en cada petición; revocación durante turno
no ejecuta una tool obsoleta. Desactivar no finge cancelar una ejecución ya
iniciada: status/cancel siguen accesibles. Mantener el historial browser del perfil
Gemma manual. Configuración deja revisar RAM sin reset de cuarentena o tolerancia
silenciosa a critical/unknown. Techo de límites existente, modificable en el panel;
no interpretar «yo reviso RAM» como un valor concreto o apagado de guardas.

Alternativa descartada: habilitar booleanos sin ejecutores o cargar el par residente
que ya llegó a critical. Cancel de borrador nativo espera fin/cleanup del turno y
descarta texto tardío; no libera ficticiamente GPU. Rollback: volver al perfil
Gemma o desactivar opciones, conservando settings/historial/evidencias/modelos.
