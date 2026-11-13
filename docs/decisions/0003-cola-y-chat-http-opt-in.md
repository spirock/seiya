# ADR 0003 · admisión, cola y chat JSON opt-in

2026-10-19 · S1-04. No sustituir Python ni conectar un modelo real durante las pruebas.

Evolución S1-05: ADR 0004 añade SSE bufferizado; los límites siguientes describen
la integración inicial S1-04. Se mantiene el arranque opt-in, cola y lifecycle.

## Alcance

- Config listener sin `runtime`: sigue siendo discovery; chat devuelve 501.
- `runtime` estructurado y explícito: un solo adaptador compartido y un scheduler.
  Chat JSON disponible, no SSE aún. `stream:true` se rechaza antes del backend,
  sin devolver JSON como si fuera streaming.
- Ninguna inferencia al arrancar o validar configuración. El endpoint local es
  fijo, validado y distinto del puerto real del servidor. No se pasa desde el cliente.

## Recursos y estados

- Una operación de inferencia y dos peticiones esperando; semáforo de tres slots
  y exclusión FIFO con Tokio Mutex. Espera máxima por defecto 120000 ms, configurable
  entre 1 y 120000. Errores 429 de cola/timeout mantienen las fixtures de Python.
- Antes de esperar se validan JSON, modos, mensajes y tools. Al cancelar el future
  que espera, se liberan su slot y posición. Después de entrar al worker, una tarea
  propia conserva slot/exclusión hasta terminar el adaptador, aunque se pierda el
  consumidor. No anunciar que una desconexión TCP siempre cancela su handler:
  según el servidor HTTP puede continuar hasta respuesta/timeout.
- Si el backend queda incierto, las esperas posteriores fallan con 503 sin otra
  inferencia. No reintentar, crear otro adaptador o reiniciar el runtime a escondidas.
- Cierre: detener admisión, despertar/rechazar esperas y drenar la operación activa
  incluso si el handler desapareció. Si su resultado deja el runtime incierto,
  devolver fallo de cierre, no una confirmación falsa de que MLX se ha detenido.
- Esto no es un endpoint de cancelación por sesión, scheduler entre procesos o
  implementación del protocolo nativo. No compite con Python sobre un modelo real.

## HTTP antes de la generación

- Host/Origin/auth se comprueban antes de leer. Sin CORS abierto, sin log de headers.
- Exigir un solo Content-Type JSON, con charset UTF-8 opcional; Content-Encoding
  ausente/identity. 415 si no corresponde. Diferencia explícita respecto a Python.
- Un Content-Length único de 1–262144 bytes; no Transfer-Encoding. Leer con límite
  y deadline (30000 ms por defecto; rango configurable 1–30000). Verificar tamaño
  leído y rechazar JSON/UTF-8 inválidos sin reflejar contenido. 408 por deadline.
- Hasta ocho lectores de cuerpos simultáneos antes de parsing; saturación da 429
  de capacidad de lectura. Se libera ese permiso antes de esperar inferencia.
  La cola de inferencia sigue limitada a tres slots. No hay uploads sin límite.
- Body parsing y validación no tienen acceso a red, archivos, tools o runtime.
- Errores cierran la conexión HTTP para no mantener un keep-alive drenando un
  cuerpo rechazado o incompleto sin necesidad. No se reintenta una generación.
- Health muestra ocupación real del scheduler/adaptador y estado incierto cuando
  corresponda; no afirma disponibilidad de un backend no probado.

No se añaden dependencias de producción: usar Tokio, Axum, Serde y el cliente
local existentes. Tests con transporte simulado y servidores locales sintéticos.
No se promueve el binario a proveedor de OpenCode hasta SSE y prueba controlada.
