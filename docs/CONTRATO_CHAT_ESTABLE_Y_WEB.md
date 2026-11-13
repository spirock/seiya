# Contrato de aceptación · chat estable y lectura web

**Versión:** `0.1-draft.1` · **Fecha:** 2026-10-24.
**Estado:** implementación parcial offline: S4-01 workload/evidencia sintéticos
probados, S4-02a disponibilidad/error y S4-02b.1 guardia de presión Rust probadas
con mocks; núcleo supervisor inyectado y lease nativo macOS probados con fixtures;
Child nativo auditado y observador macOS read-only probados por separado;
bridge/UI opt-in y binding macOS restringido sin fork probados con runtime HTTP
sintético. Compatibilidad MLX/Metal, Safari real, perfil estable y lector pendientes.
Actualización posterior: S4-04/05/06 aportan contrato interno, transporte seguro y
extracción/selección en librerías con pruebas offline; el lector no está habilitado
en el servicio/CLI habitual. S4-07 integra chat/UI en perfil separado opt-in y
prueba10 casos con páginas/modelo falsos, selección Rust y presupuesto estimado,
no tokenización verificada. Perfil/modelo, Safari y red/calidad reales siguen
pendientes. [Operación, evidencia y rollback](LECTOR_WEB_EXPERIMENTAL.md).
S4-08 posterior:3 fuentes públicas leídas con HTTPS/extracción reales en un ensayo
aislado autorizado, sin modelo o Safari. No demuestra resumen útil, fechas,
estabilidad o calidad de citas. Evidencia en `contracts/benchmarks/web-reader/`.
Complementa [CONTRATO_SEIYA.md](CONTRATO_SEIYA.md), sin modificar el protocolo
nativo draft ni la API OpenAI existente. Plan de ejecución:
[hoja de ruta pública](../ROADMAP.md).

## 1. Resultado esperado y alcance

Seiya, con identidad pública Pegassus (`pegassus`, `seiya/pegassus`), debe:

1. Mantener una conversación local dentro de un perfil de recursos medido.
2. Buscar fuentes, leer artículos públicos permitidos y sintetizar su contenido
   con citas. Los enlaces son respaldo, no sustituyen el resumen solicitado.
3. Explicar bloqueos, fuentes no leídas, resultados parciales y paradas del runtime.
4. Guardar evidencia comparable para revisar mejoras, sin entrenamiento ni cambios
   autónomos de modelos, permisos o políticas.

Primera entrega: Mac Apple Silicon de 16 GiB, un worker y un modelo ya instalado.
El perfil se valida por checkpoint/runtime/dispositivo, no por familia comercial.
No incluye consenso nuevo, streaming de tokens, ejecución de archivos/comandos,
Mail, visión, remoto, nuevos pesos, KV cuantizada, MoE o carga dinámica de expertos.

## 2. Punto de partida y evidencia

| Elemento | Evidencia actual | Lo que no acredita |
|---|---|---|
| Chat Python/web | Operativo, historial local y búsqueda de fragmentos | Lectura de artículos o paridad Rust |
| Rust | Cola, presupuestos, transporte y runner probados con mocks | Estabilidad sostenida de MLX |
| Gemma manual | Respuesta «Cuatro.» en ~3,1 s, con presión normal al finalizar | Baseline A/B, peak completo o sesión larga |
| Monitor manual | Detectó presión `2`, envió SIGTERM y confirmó salida `-15` | Causa del aumento, cancelación MLX o límite duro de RAM |
| Captura del usuario | Una búsqueda devolvió enlaces/portadas, sin resumen útil | Lectura del cuerpo de las páginas o verificación de noticias |

Artefactos manuales bajo el temporal OpenCode aprobado:
`seiya-chat-load-smoke-20261024.json` y `seiya-chat-mlx-guarded-20261024.log`.
No son ejecuciones `model-campaign` ni `model-calibration`; conservar su procedencia.
El aumento de presión es del host: sin muestras correlacionadas no se atribuye
exclusivamente al modelo, al historial o a otra aplicación.

## 3. Fronteras de arquitectura y migración

- Política compartida nueva, presupuestos, lectura segura, extracción y tipos
  internos: Rust. Inferencia permanece en MLX; no reescribir sus kernels.
- Python sigue siendo el prototipo operativo; su adaptación será mínima y opt-in.
  El bridge, dependencias/licencias y esquema interno versionado deben tener ADR
  y fixtures antes de implementarse. Ver [ADR 0019](decisions/0019-chat-estable-y-web.md).
- `seiya-contracts` no pasa a ser broker por añadir tipos. `web_fetch` continúa
  `planned` en el catálogo hasta superar sus puertas; no anunciarlo por compilar.
- La web solo puede leer recursos seleccionados por la política del host. Ningún
  texto del modelo, búsqueda o página otorga permisos ni cambia límites.
- `/v1` no recibe nuevas tools del sistema implícitas: OpenCode sigue ejecutando
  sus herramientas una sola vez. El cambio se limita al camino web autorizado.
- No cambia `seiya.chats.v1`, ni migra/borrar chats. `localhost` y `127.0.0.1`
  siguen siendo almacenes distintos; no forzar un cambio de origen.
- Activación mediante perfil separado y flag explícito; preservar originales.
  Rollback: desactivar el perfil/lector y volver al prototipo de fragmentos,
  anunciando su limitación. Sin instalaciones o reinicios del servicio compartido
  por aprobar este documento.

## 4. Contrato de memoria y continuidad

### Admisión

- Un runtime/modelo activo, una inferencia activa, dos esperas como máximo y
  espera máxima 120 s. No ejecutar consenso o resúmenes auxiliares en paralelo.
- Preflight fresco con identidad del host, puertos, procesos propios, lease
  cooperativo compartido y clearance explícito de runtimes externos. Un puerto
  libre o RSS pequeño no prueba exclusividad GPU.
- Conservar gate normal: presión normal, observaciones disponibles/frescas,
  reserva y al menos 2 GiB de páginas libres. Usar las caducidades de la política
  vigente; no reutilizar muestras antiguas tras una espera. Este gate no garantiza
  capacidad de carga ni sustituye el presupuesto runtime.
- Presupuesto completo medido antes de aprobar un perfil estable. Componentes
  desconocidos se registran como `null` con motivo, nunca cero. Sin presupuesto
  suficiente, solo calibración explícita, no promoción del chat.
- [ADR 0016](decisions/0016-calibracion-experimental-separada.md) sigue siendo una
  ruta separada, acotada y con aprobación propia; no habilita sesiones prolongadas
  ni sirve de bypass automático cuando falla el gate normal.

### Contexto y parada

- Probar primero perfiles aislados de contexto efectivo hasta 4096 tokens y
  salida hasta 256, como candidatos, no valores seguros demostrados. Verificar
  soporte y aplicación real en el runtime antes de registrarlos como efectivos.
- Cambiar una variable por experimento: contexto, caché o salida. No modificar
  cuantización de pesos/KV, modelo o plataforma para ocultar una regresión.
- Presupuesto de entrada incluye sistema, historial, tools, fuentes, salida y
  margen. Sin tokenizador fiable, declarar estimación y no afirmar ajuste exacto.
- No cortar silenciosamente mensajes/esquemas. Si no cabe: resultado parcial,
  reducción explícita de fuentes o `context_exhausted`; no bucle de compactación.
- Presión warning/critical/desconocida, watchdog sin muestras o deadline vencido:
  detener admisión y limpiar solo recursos propios. Monitor best-effort, no
  garantía de prevenir OOM. No matar apps ajenas ni reiniciar modelos en bucle.
- Confirmar salida de runtime e hijos propios y liberación del lease. Si no puede
  confirmarse: estado incierto/cuarentena, sin nueva carga. La desconexión HTTP
  no prueba que la inferencia haya terminado.

### Estado visible

Separar salud del coordinador, runtime accesible, modelo cargado y capacidad de
aceptar otra petición. Una respuesta de `/health` no acredita esos cuatro estados.
La UI debe distinguir cola, búsqueda, lectura, síntesis, límite y runtime detenido;
no mostrar actividad inventada, progreso porcentual o razonamiento privado.
Tras una parada, conservar chat/borrador y explicar cómo reintentar. Un refresh
no debe reenviar automáticamente una petición en curso.

Caso observado también desde OpenCode: coordinador8010 responde, runtime8081
apagado produce `local model endpoint failed: URLError` y el cliente reintenta.
El camino nuevo deberá devolver un error de disponibilidad estructurado y
redactado, con causa conocida o «conexión no disponible», no prosa del modelo ni
éxito vacío. Propuesta de categoría interna `runtime_unavailable`, HTTP503 para
indisponibilidad confirmada antes de generar; el mapeo compatible se congelará
con fixtures/ADR antes de cambiar la API. No inventar un plazo de recuperación
con Retry-After ni diagnosticar presión si solo se observó conexión rechazada.
Seiya no reintentará una inferencia con estado incierto ni arrancará el runtime
implícitamente. No controla los retries de OpenCode: documentar cancelación de
la petición y recuperación explícita, sin cambiar la configuración de ese cliente.

S4-02a materializa ese mapeo de disponibilidad en Rust tras ADR0021 y fixtures,
solo con `runtime_status:true`; default false conserva el contrato anterior.
Health nuevo es observación cached, sin probes o admisión por presión. Integrado
después en Python/UI solo opt-in (ADR0026), no habilitado en el servidor operativo. El resto del
contrato de continuidad y su validación con modelos siguen pendientes.

S4-02b.1 añade guardia stop-only de librería con muestras inyectadas y monitor de
caducidad en cola/generación, sin preflight/probes automáticos. Health de ese
constructor usa runtime_status versión2. Puede bloquear/drenar trabajo, pero no
posee ni apaga el runtime HTTP; su binding nativo ownership/cleanup/lease se
implementa aparte en b.2. No es
admisión positiva de carga ni se activa por el flag runtime_status de CLI.

S4-02b.2 aporta el núcleo supervisor inyectado, con manifiesto de miembros,
evidencia de reaping, lease retenido en cuarentena y recuperación explícita acotada.
El núcleo por sí solo no aporta binding nativo Child/lock o cobertura de descendientes dinámicos;
ningún ensayo simulado prueba shutdown real. El binding restringido posterior se
describe abajo; un perfil para sesiones con modelo sigue requiriendo S4-03.

Actualización2026-10-26: NativeSessionLease retiene/libera la Reservation original
con verificación de binding y flock macOS probado en archivos nuevos propios.
Status del supervisor distingue lease nativo (versión2/origen explícito) de runtime/
reaping todavía inyectados. No usa coordinación operativa ni aporta cierre real de
Child/descendientes. No cambia preflight, autoriza cargas o valida estabilidad.

Child nativo ya ensayado únicamente con helper del repositorio auditado sin fork:
identidad del Child original, EOF/SIGTERM, reaping y release del lease. Status v3
native-fixture-supervisor distingue origen nativo de presión aún inyectada y
model_inference false. Inspecciones puntuales no prueban ausencia histórica de
hijos para un programa arbitrario; no convertir ese smoke en soporte MLX o
contención de árboles. Modelo y prueba sostenida siguen pendientes. Actualización:
productor read-only sysctl nativo ya ensayado con ese helper, Status4/origen
native_macos_sysctl, sin carga de modelos o inducción de presión (ADR0027).

S4-02c: puente Python→Rust opt-in con handshake runtime_bridge versión1 y binding
hex-utf8-v1, runtime_status1/2; UI muestra observación, residencia desconocida y
guardia no integrada donde corresponda. Prueba conjunta con runtime sintético:
texto/tools, binding incorrecto sin dispatch y desconexión incierta sin reentrada.
No ejecuta tools /v1 ni habilita lectura de páginas. Default/servicio operativo
intactos; CLI Rust todavía no conecta supervisor/guardia nativos. No cerrar
S4-02b/S4-02 o afirmar recuperación MLX por el puente. S4-04 parcial (ADR0026):
  esquema/framing del lector web sigue pendiente antes de S4-05.

S4-02b/c completadas en alcance experimental tras ADR0028: nuevo launcher macOS
de librería exige sandbox deny process-fork, bootstrap comprueba fork/posix_spawn
denegados y exec conserva Child/PID/start marker. Listener loopback debe pertenecer
al original por libproc. No snapshots como prueba histórica ni killpg. Runtime
sintético nativo probado: presión, cancelación, idle, deadline, puerto ajeno y
SIGTERM ignorado/cuarentena/recovery; reaping precede al unlock. Guardia compartida
con BoundServer probado por HTTP con texto/tools y error503 tras stop. Nuevo
Status5 distingue orígenes/restricción y model_inference null para ruta MLX no
observada; fixtures false. Wire1–4 no cambia; Rust interno usa Option<bool>.

Ruta MLX implementada pero **no ejecutada/validada con MLX**: aprobación exacta
vigente, presupuesto/provenance/ventana/preflight/reserva previos; stop_at_millis
solo acorta vida de guardia para respetar expiración. Sin deps nuevas o activación
CLI/servicio. Sandbox deprecated, no protección de datos/XPC o árbol arbitrario;
si MLX requiere subprocess, falla cerrado. Context_tokens es plan, no límite
aplicado por launcher; S4-03 debe verificar ese perfil antes de usar modelos.
No afirmar estabilidad real, cleanup Metal o compatibilidad por estos tests.

## 5. Contrato de búsqueda, lectura y resumen

### Flujo

1. Buscar cuando el usuario lo pida o necesite hechos externos, no en saludos.
2. Registrar resultados con IDs de fuente opacos, título, URL y snippet.
3. Si solo hay portadas, refinar dentro del presupuesto para encontrar artículos.
4. Leer por ID un subconjunto de resultados del run. El host resuelve el ID a URL
   y aplica la política; el modelo no puede sustituir la URL de ese ID.
5. Extraer texto y aportar al modelo únicamente la porción explícitamente admitida.
6. Responder en el idioma del usuario: síntesis primero, citas junto a afirmaciones
   y fuentes después. Distinguir texto leído de snippets; no afirmar lectura total.

MVP: solo URLs de resultados del run actual. Leer una URL pegada directamente,
seguir enlaces dentro de páginas o rastrear sitios requiere una ampliación de
scope posterior; no asumir autorización universal de navegación.

### Red y datos no confiables

- HTTPS, puerto 443, GET; sin userinfo, cookies, credenciales, login, proxy de
  entorno, ejecución JS o acceso a sesión Safari. Sin POST ni descargas persistentes.
- Bloquear loopback, IPs privadas/reservadas/no globales, link-local, multicast,
  metadata cloud y representaciones alternativas IPv4/IPv6/mapeadas.
- Resolver bajo deadline, verificar todas las direcciones y fijar el destino
  validado durante la conexión conservando TLS/SNI y certificado del hostname.
  No permitir una segunda resolución sin validar: prueba explícita de rebinding.
- Redirects manuales limitados: revalidar esquema/host/puerto/DNS/IP en cada salto;
  nunca redirigir a red local o hacer downgrade a HTTP. Sin retries ocultos.
- Admitir solo HTML y texto plano con MIME/decodificación comprobados. PDF,
  vídeo, archivos, paywalls, páginas que requieren JS o anti-bot: no soportados,
  estado explícito, sin bypass ni browser autenticado como fallback.
- Scripts, estilos, navegación y formularios no son contenido a ejecutar. Texto,
  títulos, fechas y etiquetas de una página son datos no confiables. Inyecciones
  no pueden ordenar llamadas, conceder permisos o elevarse a mensajes de sistema.
- No devolver HTML activo a la UI. Texto escapado y enlaces HTTPS validados,
  sin `innerHTML` con contenido de fuentes. No seguir URLs encontradas en el cuerpo.

### Presupuestos iniciales del MVP (por petición)

Son cotas de diseño a probar; no mediciones del servicio actual. Los límites
existentes más estrictos prevalecen. Subirlos exige revisión y nueva evidencia.

| Recurso | Cota |
|---|---|
| Búsquedas / resultados | 3 búsquedas; 5 resultados cada una |
| Páginas intentadas | 3 IDs únicos; cuenta también el fallo |
| Concurrencia de lectura | 1 |
| Redirects | 2 por página; máximo 9 GET contando los iniciales |
| Red total búsqueda+lectura | 45 s; cada operación usa el tiempo restante |
| DNS / conexión / lectura de página | 3 s / 3 s / 10 s, sin superar plazo global |
| Body recibido y body descomprimido | Cada uno ≤1 MiB/página y ≤3 MiB/run para lecturas |
| Cabeceras | 64 KiB/respuesta; 576 KiB/run para lecturas |
| Texto extraído | 20000 caracteres/página; 30000 acumulados |
| Texto de fuentes enviado a inferencia | 8000 caracteres/fuente; 16000 acumulados, además del presupuesto de tokens |

Los bytes de búsqueda conservan su cap actual y se contabilizan aparte. Contar
bytes/cabeceras de intentos fallidos y redirects, no solo los de respuestas 200.
Lectura incremental acotada: no acumular un body ilimitado para rechazarlo después.
Rechazar bombas de descompresión; ambas cotas son independientes. Al exceder
transporte: fallo de página, no documento completo. Al recortar texto extraído o
seleccionado: indicar `text_truncated` y porción suministrada; no ocultarlo.
El límite de caracteres no sustituye al presupuesto real de contexto.

### Calidad y fallos parciales

- «Búsqueda correcta» significa transporte/resultados; «lectura correcta» exige
  cuerpo extraído; «resumen útil» se evalúa por contenido y fuentes. Nunca
  convertir el número de enlaces o HTTP 200 en una puntuación de veracidad.
- En noticias, separar fecha de consulta, publicación, actualización y suceso.
  Fechas ausentes/contradictorias se declaran; un título con «hoy» no las valida.
- Si solo quedan portadas/snippets: síntesis limitada a lo disponible y etiqueta
  clara, o «no pude verificar noticias concretas». No inventar actualidad.
- Citas solo a fuentes realmente entregadas al modelo; validar IDs y enlaces en
  el host. Esa validación no demuestra soporte semántico de cada afirmación.
- Fuentes incompatibles: atribuir discrepancias, sin resolverlas por votación falsa.
  Si no hay texto útil, informar del fallo, no rellenar con titulares inventados.
- Cualquier respuesta solo con enlaces a una solicitud de resumen se registra
  como tarea no resuelta, aunque se muestre un resultado parcial legítimo.
- Resultados de lectura deben distinguir al menos éxito, bloqueo de política,
  timeout, límite, HTTP no aceptado, formato no soportado y sin texto útil. No
  reutilizar texto de otra página o run para maquillar un fallo.

## 6. Métricas e histórico local

Actualización S4-04: [ADR0030](decisions/0030-lector-en-proceso-rust.md) congela
el contrato interno del lector y sustituye el helper propuesto por una librería
en la instancia Rust configurada, sin ejecución por rutas en cada lectura.
[Frames y límites](../contracts/reader-ipc/v0.1-draft.1/README.md), codecs Rust/
Python y compatibilidad con fixtures sintéticas probados offline; no hay todavía
endpoint de lectura, transporte seguro, extracción o integración con el chat.
Identidad/deadline/cancelación quedan resueltos como responsabilidades del diseño,
no como controles operativos ya implementados. S4-05 debe probarlos antes de
activación. No afecta OpenAI/nativo, permisos, perfiles activos o historial.

Adaptar el esquema de evidencia existente antes de usarlo para sesiones largas;
los siguientes requisitos no están implementados por enumerarlos aquí:

Incremento S4-01 (2026-10-25): `seiya-observe::session_evidence` implementa solo
almacenamiento/índice sintéticos acotados y fixtures, con tipos estrictos y pruebas
de publicación atómica. [Formato y alcance](../contracts/benchmarks/chat-session/v0.1.0/README.md).
No registra eventos automáticos del chat, autentica hashes, acepta datos reales,
calcula calidad o sustituye la instrumentación prevista en el resto de esta sección.

- Manifiesto inmutable por experimento: ID, modo sintético/real, perfil, hashes,
  versiones, hardware, parámetros efectivos y consentimiento/alcance de la prueba.
- Eventos acotados: tiempos de cola/carga/búsqueda/lectura/inferencia/cleanup,
  resultados y errores, presión, RSS propio y muestras disponibles. Tokens de
  entrada/salida y tasas solo cuando existen contadores comparables; TTFT `null`
  si el transporte bufferizado no permite medirlo.
- Memoria Metal/MLX/footprint y presupuesto: valor+unidad+procedencia o
  `null`+motivo; no sumar RSS con métricas solapadas como si fueran RAM total.
- Resumen: completed/partial/aborted, causa, limpieza confirmada, páginas
  intentadas/leídas, citas válidas, rúbrica de utilidad y límites encontrados.
  No equiparar generación completada con objetivo del usuario satisfecho.
- Índice local reconstruible y comparación solo de perfiles/workloads compatibles.
  Validar cotas y escrituras atómicas antes de crear histórico; sin SQLite o
  servicio extra obligatorio por decisión documental.
- Máximo propuesto 32 MiB de evidencia por experimento y 256 MiB por campaña,
  con límites de eventos/muestras existentes. Al agotarse, cierre parcial; no
  borrar evidencia anterior automáticamente. No retención ilimitada en RAM.
- Por defecto guardar métricas/IDs/estados, no conversaciones personales,
  cookies, claves, razonamiento interno ni cuerpos completos de páginas. URLs y
  consultas pueden ser sensibles: excluirlas del histórico ordinario; su captura
  para un experimento requiere consentimiento y redacción. Fixtures sintéticas
  pueden guardarse completas. Sin telemetría o entrenamiento automático.

## 7. Puertas de aceptación

1. **Offline:** todas las fixtures de seguridad, presupuesto, cola/cuarentena,
   recuperación, compatibilidad y persistencia pasan; tests normales sin red,
   modelos, Safari o datos personales.
2. **Perfil real de memoria:** tres sesiones aisladas, cada una de al menos 10
   turnos y 20 minutos incluyendo 5 minutos de idle/reanudación, mismo checkpoint
   y perfil fijado. Sin paradas por presión, OOM, huérfanos, pérdida de mensajes o
   escapes del presupuesto. Cleanup y rechazo de nuevo trabajo probados aparte
   con fallos inyectados. Si no pasa, perfil no aprobado; no bajar umbrales.
3. **Lectura offline:** fixtures HTML/texto, portadas, fechas, MIME falso,
   inyección, redirects, SSRF/rebinding, límites y compresión. Cero acceso a
   destinos prohibidos y cero acciones privilegiadas inducidas.
4. **Síntesis:** suite congelada de 10 tareas con fuentes sintéticas conocidas;
   rúbrica de resumen, pertinencia, atribución y límites. Al menos 8/10 útiles,
   cero citas inventadas o afirmaciones críticas sin soporte en la revisión.
   Publicar fallos y denominador; no presentar la rúbrica como detector infalible.
5. **Web real autorizada:** conjunto fijo de fuentes públicas, evidencia separada
   de lectura y modelo. Después, 3 sesiones combinadas bajo el perfil aprobado y
   las mismas cotas de duración/turnos. Incluir fallo de una página y límite de
   contexto; revisar resúmenes/citas manualmente y volver a medir memoria.
6. **Entrega:** opt-in, rollback ensayado, UI real en Safari, historial conservado,
   limitaciones visibles. Sin promover por defecto ni sustituir Python sin petición.

Comparaciones de optimizaciones: mismos workloads, recursos y contadores; primero
resultados descriptivos, luego comparación controlada. S3-05 mantiene su propio
ABBA Python/Rust: estas sesiones no lo completan ni evalúan R1 por sustitución.
Pruebas reales necesitan autorización vigente y exclusión; este contrato no la
concede, no cierra aplicaciones ni reinicia servicios.
