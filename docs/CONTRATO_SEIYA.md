# Contrato técnico de Seiya / Pegassus

**Versión:** `1.0-draft.1` · **Fecha:** 2026-10-19 · **Estado:** propuesta de
arquitectura y requisitos de aceptación. No constituye una lista de funciones
ya disponibles ni promete aprobación en tiendas, rendimiento o compatibilidad
de un modelo sin pruebas. Cambios incompatibles requieren otra versión.

## 1. Identidad y objetivos

- **Seiya:** aplicación, proveedor y futura familia de binarios.
- **Pegassus:** identidad pública del asistente/modelo; ID API `pegassus`,
  referencia OpenCode `seiya/pegassus`. No cambiar a Pegasus/Phi/Gemma en la UI
  como si fueran la identidad comercial.
- Una experiencia compartida de chat, sesiones, adjuntos, fuentes, herramientas
  y permisos. Diferencias de plataforma se anuncian, no se ocultan.
- Desktop: permitir propuestas independientes, revisión y consenso por turnos.
- Mobile: empezar con un modelo local pequeño; capacidad remota solo por elección.
- Offline significa inferencia y herramientas locales sin red. Búsqueda web,
  sincronización de correo y ejecución remota requieren red.
- No afirmar que votos equivalen a tests ni que un tool call equivale a éxito.

## 2. Estado frente al objetivo

| Componente | Estado actual | Objetivo |
|---|---|---|
| Coordinador | Python; secuencial, mayoría absoluta | Núcleo Rust con adaptadores |
| Tools OpenCode | Ejecutadas por OpenCode | Mantener integración externa |
| Tools web | Búsqueda de fragmentos | Broker nativo común y fetch seguro |
| Chat | HTML/JS con historial local | UI compartida empaquetada + accesibilidad |
| Modelos | MLX/Ollama configurados aparte | Adaptadores y evaluación por dispositivo |
| Generación de imágenes | Ensayo CLI aislado; broker/API/UI Rust opt-in probados con fixtures, apagados por defecto | Ejecutor admitido, permiso/recursos y flujo web real validado |
| Visión/audio | API Seiya text-only | Adjuntos y motores compatibles, probados |
| Apps/Mail/pantalla | No implementado | Adaptadores macOS con permisos |
| iOS | No implementado | Prototipo en iPhone 15 Pro Max |
| Seguridad | Loopback, Host/Origin, auth opcional | Broker con grants, auditoría e idempotencia |
| Streaming | SSE bufferizado | Eventos reales y cancelación propagada |
| Sesiones | Chat en localStorage; historial de jobs/PNG Rust opt-in con snapshots y recuperación offline probada; OpenCode externo | Persistencia transaccional compartida y sincronización |

`seiya-contracts` solo aporta tipos, parsing estricto y comprobaciones básicas
de selección de herramientas; no es un servidor ni un broker completo.
`seiya-server` añade un camino experimental opt-in de un worker, guardas HTTP,
cola y chat JSON/SSE bufferizado, con fixtures y dos smoke tests reales aislados
OpenCode/Gemma. No sustituye Python ni implementa sandbox, almacenamiento de
sesiones o permisos del SO. Alcance y rollback: `OPERACION_RUST_EXPERIMENTAL.md`.

## 3. Capas y límites de responsabilidad

```text
UI común (chat, fuentes, permisos, estados)
              │ protocolo de sesión versionado
Núcleo Rust (sesiones, scheduler, contexto, políticas, broker)
              ├── adaptador de inferencia local
              ├── adaptador de inferencia remota autorizada
              └── adaptadores de herramientas por plataforma
                    macOS / Linux / Windows / iOS
```

1. La UI no ejecuta código generado ni interpreta prosa como una orden.
2. El modelo propone llamadas estructuradas; el broker decide y valida.
3. Los adaptadores se encargan de permisos reales, recursos y errores del SO.
4. Los runtimes hacen inferencia. No reimplementar sus kernels por portar el
   coordinador a Rust; medir el coste antes de optimizar.
5. Se mantienen las responsabilidades de OpenCode cuando es el cliente:
   sus herramientas, permisos y sesiones no se duplican ni se ejecutan otra vez.

## 4. Tecnología y portabilidad

- Lógica nueva independiente del SO: Rust. Binarios específicos por target,
  no un único ejecutable que funcione indistintamente en todos los sistemas.
- Contratos serializables en JSON; integración nativa por C ABI o UniFFI se
  elegirá mediante una decisión técnica probada. No prometer un FFI ya existente.
- UI: compartir componentes web cuando sea viable. Evaluar un contenedor
  multiplataforma con WKWebView en iOS y un bridge mínimo autenticado. La
  tecnología concreta del contenedor aún no está decidida.
- iOS/macOS: Swift/Objective-C donde lo exijan APIs de Apple y la inferencia.
- MLX/Metal y llama.cpp son candidatos; soporte de arquitectura/tokenizador,
  visión, cuantización y tool parser se valida por cada modelo y versión.
- Evitar Python/Node como requisito de la app final móvil. El prototipo desktop
  sí sigue necesitando Python hasta que la migración se complete.
- Compilación no garantiza confidencialidad: no incrustar secretos, no depender
  de ofuscación para autorización. El código objeto puede analizarse.

## 5. Plataformas y modos de ejecución

| Plataforma | Inferencia objetivo | Herramientas y límites |
|---|---|---|
| macOS | Local single/swarm, remoto opcional | Documentos autorizados; apps y Mail con permisos |
| Linux | Local single/swarm, remoto opcional | Documentos; procesos aislados y limitados |
| Windows | Local single/swarm, remoto opcional | Adaptadores propios, no scripts macOS |
| iOS | Local single; remoto explícito | Sandbox, Archivos/Fotos elegidos; sin shell desktop |

- Dispositivo inicial móvil: iPhone 15 Pro Max. No deducir RAM utilizable de la
  RAM total; medir límites reales de la app, presión, thermal y jetsam.
- Phi multimodal es candidato, no obligación. Comenzar evaluando modelos
  cuantizados pequeños; no empaquetar el enjambre desktop en el teléfono.
- Gemma descargado en el Mac declara visión/audio; el runtime Seiya actual
  no los expone. Tener pesos multimodales no valida un pipeline multimodal.
- `auto`: el host elige entre modos **locales** autorizados según capacidades
  y recursos. Nunca sube datos a otro equipo como fallback silencioso.
- `remote`: usuario elige host emparejado y entiende qué datos se enviarán.
- El catálogo anunciado es efectivo: intersección entre implementación, runtime,
  hardware, permisos, política y destino. Una función planeada no se anuncia.

## 6. Experiencia de usuario compartida

- Enviar por botón o Enter; Shift+Enter crea nueva línea.
- Vaciar el compositor al enviar; no borrar un nuevo borrador al recibir respuesta.
- Mostrar indicador de actividad y tiempo real. No simular tokens, porcentaje
  de progreso ni razonamiento privado. Si no hay streaming, decirlo.
- Mostrar actividad observable: búsqueda completada, permiso pendiente, cola,
  ejecución de tests, revisión y fallo. No inferir pasos que no ocurrieron.
- Historial con ID estable, nuevo chat, títulos editables en una fase posterior,
  selección, eliminación confirmada, fuentes y adjuntos asociados.
- La nueva UI debe rehidratar sesiones y eventos tras reconectar. Un refresh no
  debe provocar reenvío automático de una acción no idempotente.
- Búsqueda disponible automáticamente cuando la política permita red; informar
  de que puede enviar consultas. No pedir activar una casilla inexistente.
- Mostrar «En este iPhone», «En este Mac» o «En Mac remoto X». No ocultar el lugar
  de ejecución, modelo efectivo ni envío de datos fuera del dispositivo.
- Etiquetas, navegación por teclado, lector de pantalla y reduced-motion.
- No mostrar «puedo ver/leer/controlar» sin capacidad efectiva y permiso vigente.

## 7. Sesiones, persistencia y memoria

- IDs opacos estables para conversación, petición, mensaje, run, asset y tool call.
- Destino futuro: SQLite local con transacciones y migraciones versionadas.
  Credenciales en Keychain/almacén del SO, nunca junto al historial ni logs.
- Persistir mensajes confirmados, borradores, estado de runs, llamadas/resultados,
  permisos concedidos, metadatos de fuentes y puntos de recuperación.
- Contenido sensible: no telemetría por defecto, exportación explícita, borrado
  de sesión y adjuntos propios asociados. No borrar archivos originales del usuario.
- La eliminación debe cancelar acciones pendientes antes de eliminar registros.
  Los backups del SO pueden conservar copias; no prometer borrado forense.
- Integridad: escritura atómica, controles de tamaño, versión y errores visibles.
- El prototipo actual usa localStorage sin cifrar y no se sincroniza. Migración
  futura con consentimiento; no borrar su almacenamiento antes de verificar importación.
- Los modelos reciben solo el contexto necesario, no todo el histórico ni toda
  la carpeta autorizada. Datos de tools no equivalen a instrucciones de sistema.

## 8. Contexto, scheduler y cancelación

- Presupuesto = contexto efectivo menos reserva de salida, sistema, esquemas,
  adjuntos y margen. Reportar estimación y uso real separadamente.
- Anunciar límites medidos y documentar lo no probado. El perfil actual 32K/2048
  tiene ensayo de ~19K, no validación de toda la ventana ni estabilidad prolongada.
- Elegir herramientas relevantes y limitar outputs antes de compactar. Compacción
  no puede reducir esquemas/instrucciones fijas ni arreglar un runtime incompatible.
- Detectar bucles: si dos compactaciones automáticas consecutivas no permiten
  avance significativo, parar con `context_exhausted`; no resumir indefinidamente.
- Default desktop conservador: una inferencia activa y dos esperas; espera máxima
  120 s. En iOS empezar con una activa y una espera. Valores futuros configurables.
- Una llamada de herramienta que espera aprobación no debe mantener ocupado el
  recurso GPU; reanudar con checkpoint y política revisada.
- Cancelación por run/conversación. Propagar al runtime cuando sea posible; si
  no, estado `cancelling` hasta terminar. No liberar GPU ficticiamente ni iniciar
  otro modelo sobre una generación huérfana.
- Dar prioridad a una tarea solicitada por el usuario frente a títulos/resúmenes
  auxiliares. No ejecutar trabajo auxiliar no necesario en un enjambre.
- Presupuestos por run: pasos, tiempo, búsquedas, bytes y tokens. Al agotarse,
  reportar resultados parciales y límite alcanzado; no declarar éxito completo.

### 8.1. Memoria adaptativa en Apple Silicon y reutilización selectiva

Seiya incorporará un gestor de recursos en Rust, con adaptadores macOS/Metal y
del runtime. Se recuperarán de Colibri **ideas, contratos y, solo cuando sea
apropiado, fragmentos concretos**, no su motor completo. La selección, procedencia,
estados, métricas y pruebas están definidos en
[MEMORIA_ADAPTATIVA.md](MEMORIA_ADAPTATIVA.md), que forma parte de este contrato.

Prioridad: presupuesto medido, un modelo residente en equipos ajustados, descarga
por inactividad, cachés/contexto acotados y modalidades bajo demanda. El streaming
de expertos desde disco se estudiará después y solo para modelos MoE compatibles.
El Gemma E4B actual no está configurado como MoE: no se promete descargar pesos
arbitrarios porque no participen en una operación concreta.

Esta sección es un requisito futuro, no una capacidad del servicio actual.
No se restaura código, descarga un modelo o cambia el runtime al aprobar el contrato.

## 9. Enjambre desktop

1. Propuestas independientes y candidatos anónimos con orden rotado.
2. Revisiones mediante llamadas estructuradas y mayoría absoluta verificable.
3. Votos inválidos = abstenciones. Sin mayoría, error explicativo o decisión
   explícita del usuario; nunca inventar votos o ejecutar una propuesta por defecto.
4. Un solo modelo residente a la vez en equipos ajustados. Verificar descarga
   antes de cambiar; detenerse si falla.
5. Para cambios de código, fase futura: plan → propuesta de patch → ejecución
   aislada de tests → revisión del diff y evidencia → aprobación → aplicación.
6. Votar una acción no sustituye su autorización. Ningún voto concede permisos.
7. Tool calls del cliente OpenCode no se ejecutan en Seiya. El perfil actual usa
   un worker también para títulos/resúmenes para evitar recargas y colisiones.

## 10. Herramientas: reglas comunes

- Registro versionado del host; JSON Schema de argumentos y resultados.
- Nombre exacto permitido, call ID único, argumentos de objeto y límites
  comprobados antes de permisos o efectos. Rechazar extras desconocidos.
- Normalizar rutas y comprobar el recurso resuelto después de symlinks; impedir
  escapes, traversal y carreras. Permiso de una carpeta no autoriza todo el disco.
- Restricciones del SO no se eluden con Rust, scripts o instrucciones al modelo.
- Resultados estructurados con estado, procedencia y error; no marcar `completed`
  antes de comprobar el efecto. No ejecutar texto/JSON encerrado en Markdown.
- Retry de una lectura transitoria puede ser seguro; envío/borrado/clic/ejecución
  no se repite automáticamente. ID de idempotencia y resultado persistente.
- Grants por usuario, recurso y acción, con caducidad y revocación. Deny prevalece.
- El registro, permisos, confirmaciones y destino son del host, no campos
  editables con los que el modelo pueda autoconcederse acceso.

## 11. Catálogo objetivo por dominio

El catálogo de `contracts/v1/tools.json` es una especificación, **no un registro
de herramientas implementadas**. No debe cargarse al prompt como oferta real.

### Web

- `web_search`: consulta limitada y resultados con URL, título, snippet y fecha
  cuando exista. No presentar snippets como contenido completo verificado.
- `web_fetch`: GET limitado, extracción de texto y enlaces, sin ejecutar scripts.
  Validar HTTPS/HTTP según política, host, resolución DNS, IPs reservadas,
  puertos, credenciales y cada redirect. Bloquear loopback, red privada, metadata,
  rebinding y esquemas no web. Límites de tiempo, bytes y descompresión.
- Fuente maliciosa no puede ordenar leer correo/archivos o enviar datos.
- Noticias: buscar fechas, refinar consultas y citar artículos. Si no se puede
  verificar actualidad, reconocerlo sin inventar titulares.

Concreción pendiente de implementación para chat estable y lectura web:
[contrato de aceptación v0.1-draft.1](CONTRATO_CHAT_ESTABLE_Y_WEB.md). Establece
admisión/continuidad, presupuestos de lectura, citas, evidencia y pruebas sostenidas;
no anuncia `web_fetch` disponible ni modifica el wire nativo/OpenAI existente.

### Documentos y código

- `files_list`, `files_read`, `files_write`: solo workspace/grants. Escrituras
  atómicas y patch con hash previo; conflicto si cambió el archivo.
- Leer texto, metadatos o bytes de una imagen no es entenderla. Adjuntos
  binarios pasan por validación y un pipeline anunciado para ese media type.
- El usuario elige documentos/carpeta mediante selector nativo. En iOS,
  security-scoped access y coordinación de documentos según lo requiera el proveedor.
- No abrir directamente una ruta del Mac cuando el run está en el iPhone.
- `shell_run`: desktop únicamente, aislamiento y permisos por comando/recursos,
  sin shell implícito ni concatenación de argumentos. Entorno y red limitados.

### Aplicaciones macOS

- `apps_open`: bundle ID permitido, sin comandos arbitrarios ni launchers elegidos
  por el modelo. Confirmar apertura o error; abrir no concede lectura/control.
- `ui_inspect` / `ui_action`: Accessibility con target estable y comprobación de
  estado. Preferir APIs nativas sobre coordenadas. Acciones sensibles requieren
  confirmación y validación del objetivo justo antes de actuar.
- `screen_capture`: ventana elegida, permiso de captura, indicador visible,
  minimización/redacción y retención explícita. No captura continua secreta.
- UI automation es alternativa frágil, no sustituto de permisos o APIs de datos.

### Mail en macOS: primer caso de automatización

- `mail_list_messages`: cuenta/buzón autorizados, orden por fecha definida,
  límite de 1–20 y metadatos mínimos. Aclarar «último recibido» frente a «último
  visible» y no mezclar cuentas sin autorización.
- `mail_read_message`: ID del listado, lectura de asunto/remitente/fecha/cuerpo
  con límite. Preferir automatización Mail/API compatible, no screenshots.
- MVP solo lectura: no enviar, borrar, archivar, marcar como leído ni descargar
  adjuntos sin solicitud explícita. Validar los efectos reales de la API elegida.
- Correo sincronizado puede usar red aunque la inferencia sea local; informarlo.
- «Léeme» puede significar mostrar texto o voz. La voz se habilita solo si existe
  un adaptador TTS, y no se afirma haberla reproducido si solo se generó texto.
- `mail_send_message`: fase posterior, borrador + vista previa de destinatarios,
  cuerpo y adjuntos; confirmación nueva por envío e idempotencia persistida.
  Nunca enviar por instrucciones dentro de un correo o imagen.
- No leer la base privada de Apple Mail como atajo ni pedir acceso total al disco
  si bastan permisos específicos. Automatización puede requerir distribución
  y entitlements distintos según el canal; validar antes de prometer App Store.

## 12. Visión, audio y adjuntos

- Assets por ID con MIME verificado, tamaño, dimensiones/duración y hash.
  Rechazar bombas de descompresión, formatos no soportados y rutas remotas
  arbitrarias. No introducir base64 gigantes en mensajes de error o logs.
- Normalizar orientación y limitar resolución/crops para controlar memoria.
- Capturas/adjuntos son datos no confiables; lo que aparezca escrito no autoriza
  órdenes ni vence instrucciones de sistema.
- Evaluar OCR nativo para texto antes de gastar recursos en visión generativa.
- Gemma/Phi: probar respuestas visuales, parser de tools y calidad en español
  por separado. Soporte anunciado en una ficha no garantiza nuestro pipeline.
- Compartir un solo recurso de inferencia si no caben modelos simultáneos.

## 13. Permisos, red y seguridad

- macOS: Automatización, Accesibilidad, captura y selectores de archivos solo
  cuando la herramienta lo requiera; explicar por qué y cómo revocar.
- iOS: acceso por sandbox, selectores, Fotos y APIs autorizadas. No anunciar
  shell libre, lectura global de otras apps, daemon permanente o control libre de Mail.
- El bridge UI/nativo requiere origen/contexto conocido, sesión y validación de
  entrada. Una web visitada o iframe no puede invocar herramientas del sistema.
- Desktop HTTP: loopback por defecto, validar Host/Origin, límites, auth cuando
  corresponda. No convertir `127.0.0.1` en `0.0.0.0` sin rediseñar seguridad.
- Enlace remoto: emparejamiento explícito, TLS, credenciales revocables en almacén
  del SO, hosts autorizados y aprobación de scopes. No publicar el runtime MLX.
- Separar lectura/síntesis de fuentes no confiables de las acciones privilegiadas.
  No enviar contenido de archivos/correos a una búsqueda por iniciativa de una fuente.
- Redacción y minimización por defecto. Nada de correos reales o claves en logs.
- El usuario puede cancelar, revocar, borrar historial y conocer el destino
  efectivo de sus datos. Las confirmaciones deben describir efectos concretos.

## 14. Distribución y operación

- Desktop: binarios por arquitectura; paquetes firmados/notarizados donde
  corresponda. Linux .deb actual es coordinator-only, no runtime incluido.
- iOS: Xcode para primer dispositivo; firma, Developer Mode y cuentas del usuario.
  TestFlight/App Store requieren proceso y requisitos de Apple, sin garantía previa.
- No prometer ejecución ilimitada en segundo plano. Persistir y recuperar ante
  suspensión; background tasks solo para usos y duración autorizados por el SO.
- Pesos: instalación explícita, tamaño/licencia/hash, espacio y formato; no
  descargas automáticas escondidas ni código remoto confiado sin revisión.
- Actualización atómica y rollback de binarios/migraciones cuando sea seguro.
  No borrar modelos o chats al desinstalar sin elección separada del usuario.
- APIs de hardware/SO y licencias se revisan por versión de release; «último iOS»
  no es un requisito reproducible. Registrar versiones concretas ensayadas.

## 15. Aceptación y límites de este documento

La matriz, etapas y evidencias exigidas están en
[ENTREGA_Y_MIGRACION.md](ENTREGA_Y_MIGRACION.md). El protocolo propuesto y su
compatibilidad están en [PROTOCOLO_V1.md](PROTOCOLO_V1.md).

Este contrato no autoriza acceso a cuentas/correo ni descargas o instalaciones.
Especificar una función no la implementa: cada capacidad debe pasar pruebas,
concesión de permisos y anuncio efectivo antes de quedar disponible al modelo.
