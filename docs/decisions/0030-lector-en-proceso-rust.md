# ADR 0030 · lector en proceso Rust, sin launcher por página

Estado: decidido para S4-04; implementación de transporte/endpoint pendiente de
S4-05. Sustituye **solo la elección de helper por pipes** de ADR0019/0029, no
sus límites de lectura ni la semántica de los frames. No habilita ninguna ruta.

Actualización: ADR0031 implementa componentes y constructor de librería opt-in,
y precisa cancelación explícita elegida por el usuario: pérdida de TCP no equivale
a cancelación automática de un handler JSON. Endpoint cancel autenticado y deadline
de respaldo; no activado por CLI/servicio. Ver estado/evidencia de S4-05.

## Motivo y alternativas

El ensayo con un ejecutable fijo de Cargo prueba pipes/EOF, no identidad del
binario de producción. Comprobar hash de una ruta y luego ejecutarla introduce
una carrera; comparar el PID después tampoco impide efectos previos. No se
acepta ese launcher ni se declara segura una ruta absoluta por sí sola.

Alternativas: helper instalado en una ubicación protegida por un administrador
(introduce instalación y permisos externos); FFI dentro de Python (distribución
y ownership nuevos); librería dentro del servidor Rust ya configurado. Se elige
esta última, reutilizando el transporte loopback de ADR0026, sin nuevo daemon,
FFI, permisos del SO ni ejecutables elegidos por el modelo. No se requiere que
lectura e inferencia se ejecuten simultáneamente.

## Interfaz interna decidida, aún no disponible

- `POST /internal/reader/v0.1/read` en la instancia Rust explícita del perfil,
  con el frame de petición/respuesta de `contracts/reader-ipc/v0.1-draft.1/`.
  No se monta en el router por defecto ni se anuncia `web_fetch` por documentarlo.
- Activación separada de inferencia y requiere clave compartida no vacía mediante
  variable de entorno; si falta, error de configuración. Nunca credencial en
  URL, JSON, binario, fixture o log. No reutilizar el modelo ni sus headers como
  autoridad de red. No basta `runtime_bridge` o un health HTTP200.
- El cliente solo conecta al loopback numérico y puerto configurado, sin proxies,
  redirects, fallback ni arranque implícito. Host/Origin y autenticación preceden
  a la lectura del JSON. `Content-Type: application/json`, sin compresión del
  enlace interno. La implementación rechazará framing HTTP ambiguo conforme a
  las guardas existentes. Las cotas de frame se leen incrementalmente.
- La instancia Rust arranca por decisión del operador, no por una petición de
  lectura. S4-04 no autentica el hash de su ejecutable: se confía en la instancia
  que el operador ha iniciado y en su clave, no en autoidentificación por JSON.
  No protege contra otro proceso del mismo usuario que haya robado esa clave.
  Si no se puede establecer esa confianza, el lector permanece desactivado.
- No se crea hijo para leer: identidad por pathname y reaping del helper dejan
  de ser requisitos de esta interfaz. Los hijos de las fixtures son solo ensayo
  histórico. No se da por probado un mecanismo de lanzamiento seguro.

## Run, deadline, cancelación y errores

- Python conserva el registro de búsqueda por run, asigna IDs y construye el
  frame después de validar la selección del modelo. El lector Rust valida red
  independientemente; ningún par ID/URL es un grant. La respuesta conserva IDs
  solicitados y orden; no se incorporan URLs elegidas por una página.
- Una llamada contiene todos los intentos de lectura del run (hasta3 IDs); no
  repartir el presupuesto entre invocaciones ni reiniciarlo al reintentar. Sin
  retries automáticos. Búsqueda y lectura comparten deadline monotónico≤45 s en
  Python. El resto se reduce antes del envío y Rust establece su límite local
  al admitir, nunca superior. El cliente aplica también su deadline original:
  la latencia del enlace no concede tiempo adicional al usuario.
- Una operación de lectura activa por instancia; rechazar otra con503 sin esperar
  indefinidamente. El handler posee el transporte como future cancelable; no
  spawn, threads o trabajo bloqueante desacoplado por petición. Al vencer deadline,
  desconectar o cancelar, soltar operaciones DNS/HTTP y confirmar fin antes de
  liberar el slot. Si una dependencia no permite confirmar cierre: cuarentena
  del lector, sin nueva admisión, recuperación explícita de la instancia por
  operador. No parar el runtime de inferencia ni reiniciar OpenCode.
- HTTP400: frame inválido;401/403: auth/guardas;413: frame excedido;503: lector
  desactivado, ocupado o en cuarentena;504: deadline global. Los errores externos
  no incluyen cuerpos, URL, consultas, claves ni diagnósticos internos. Fallos
  por página dentro del frame usan los estados congelados, sin texto reutilizado.
- Un frame válido solo se acepta con respuesta completa bajo cap y estado200;
  ni éxito del transporte ni cancelación del cliente prueban éxito de lectura.
  Los datos de fuente son texto no confiable; la selección de contexto/citas
  corresponde a S4-06/07, con cotas adicionales8000/16000 caracteres.

## Compatibilidad, migración y rollback

Los esquemas0.1-draft.1 quedan congelados como contrato interno experimental;
cambios de forma/semántica incompatibles requieren nueva versión. JSON Schema
es descriptivo de forma: las reglas semánticas adicionales y diferencias con el
parser están en el README del contrato. No añadir títulos/fechas/final URL en
esta versión: el host conserva la URL original para citas y fechas ausentes se
declaran desconocidas; no inferir fecha de la query o de un redirect.

El prototipo de snippets permanece default, sin migración de chats ni cambios
OpenAI/nativo. La ruta de inferencia y su binding de modelo no conceden lectura.
Rollback: desactivar exclusivamente el flag de lector y conservar el perfil de
snippets con limitación visible, nunca sustituir lecturas fallidas por éxito.
No instaladores, proveedores, dependencias o servicios activos modificados aquí.

## Evidencia y puerta de S4-05

S4-04 verifica formas/semántica de frames en Rust y Python, fixtures compartidas,
EOF con lectores fragmentados, IPC sintético y timeout/reaping de una fixture,
no seguridad de red ni HTTP real de este endpoint. Antes de activación, S4-05
debe implementar y probar auth/guardas, límites, deadline, cancelación, slot y
cuarentena junto a DNS/IP fijada, TLS, SSRF y redirects con transporte simulado.
El endpoint y todos esos controles siguen **no implementados**. S4-08/09 validan
red/modelo reales con autorización propia. S4-04 es contrato, no entrega del lector.
