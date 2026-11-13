# Lector web · integración experimental S4-07

**Implementado y probado con fixtures; no activado en el servicio habitual.**
S4-08 añadió prueba real aislada de3 páginas públicas (3/3 transporte/extracción
OK), [evidencia](../contracts/benchmarks/web-reader/v0.1.0/README.md). No hay
validación de calidad de Gemma, noticias, Safari o sesiones sostenidas. Nuevas
pruebas de red/modelos siguen requiriendo autorización y sus gates.

## Componentes

- S4-04: frames internos0.1-draft.1 y codecs Rust/Python.
- S4-05: HTTPS443 público, DNS/IP fijada, TLS/SNI con raíces WebPKI, parser
  HTTP1 acotado y compresión rechazada. Rutas read/cancel solo mediante constructor
  Rust de librería `app_with_reader`, no CLI/router normal.
- S4-06: tokenizer HTML5 conservador (no DOM, layout, JS o navegador), texto
 20000/30000 y selección8000/16000 puntos Unicode. Fechas no verificadas; ninguna
 instrucción de una página es permiso. [Límites de Rust](../crates/seiya-server/src/reader/README.md).
- S4-07: `reader_profile.BrowserReader` integra `reader_flow.run`, búsqueda con
 deadline compartido, `ReaderClient`, selección autenticada Rust y presupuesto
 **estimado** antes de cada inferencia. Configuración frontend opt-in, estado y
 cancel por run; UI muestra pasos observados y fallos parciales, sin tokens falsos.

Solo `/api/chat` del perfil usa este flujo; `/v1` conserva tools externas de
OpenCode, sin búsqueda/lectura duplicadas. Un worker fast, un slot durante todo
el run. El modelo propone IDs del run, no URLs libres; respuesta estructurada
con síntesis y citas a IDs cuyo texto llegó al prompt. El host resuelve los enlaces
desde su registro de búsqueda. Validar el ID no demuestra soporte de una afirmación.

El perfil no adapta esquemas ni historial para hacerlos caber: cuando el estimador
rechaza contexto, falla con `context_exhausted` y conserva el borrador. El coste
es bytes UTF-8 de JSON completo+64 por mensaje/tool+salida+margen. Incluye sistema,
historial, resultados y esquemas, pero **no es tokenización ni cota universal**.
`web.context_budget.tokenizer_verified=false` conserva esta limitación. El contexto
candidato de4096 y salida≤256 no son un perfil real aprobado o configurado en MLX.

## Perfil separado y operación

[python-profile.example.json](../contracts/reader-ipc/v0.1-draft.1/python-profile.example.json)
es un ejemplo sintético que no se ejecuta directamente: sustituir ruta del modelo
por el checkpoint autorizado y validado; conservar una copia del perfil original.
`web_reader` exige fast/un modelo y contexto/salida/margen explícitos. El endpoint
es HTTP numérico127.0.0.1 con puerto y path exactos, sin proxy, DNS o redirects.
La clave de Rust solo se obtiene de la variable indicada por `api_key_env`;
`SEIYA_API_KEY` no vacía es obligatoria para el frontend. No guardar claves en
JSON, conversación, logs o binarios. Configurar un perfil no arranca Rust o MLX.

La instancia Rust del lector se construye expresamente con `app_with_reader`
y una `Reader` elegida por el host. No hay flag CLI de activación automática;
la entrega empaquetada corresponde a S4-10. No ejecutar otro coordinador contra
el runtime compartido ni arrancar un perfil real sin preflight/presupuesto/lease.
Esta guía no autoriza consultas de Internet, modelos, instalación o reinicios.

## Protocolo browser y cancelación

Todas las rutas aplican Host/Origin y clave frontend. `/api/reader/runs` recibe
`{}` y devuelve ID/token opacos, generados por servidor. El navegador los mantiene
en memoria, nunca en localStorage; se envían en `reader_run` solo a `/api/chat`.
`/api/reader/status` y `/api/reader/cancel` reciben la misma referencia. Registro
de hasta128 runs, TTL10 minutos para inactivos, sin expulsar trabajo activo.
Un run se consume una vez: no se reejecuta tras refresh, cancelación o fallo.

Cancelar antes del upload impide iniciar el run. Durante búsqueda se espera al
adaptador acotado; durante lectura se envía cancel a Rust; durante inferencia
no hay cancel nativo probado: conserva el slot hasta que termine el runtime/plazo
y rechaza su respuesta tardía.202 reconoce solicitud, **no cleanup**. Pérdida de
TCP por sí sola no detiene el handler: deadline/cuarentena son respaldo.
La UI no cierra la tarea al pulsar Cancelar ni anuncia GPU liberada.

Errores inciertos bloquean nuevas lecturas del cliente y pueden dejar Rust en
cuarentena. No retries automáticos/reinicio/fallback; recuperación explícita del
operador después de comprobar recursos. No usar otra instancia como prueba de
limpieza. La interfaz conserva mensajes, fuentes y borrador bajo `seiya.chats.v1`;
al refrescar no reenvía una petición pendiente ni guarda tokens de run/API keys.

## Selección y seguridad del render

`/internal/reader/v0.1/select` exige clave y frame acotado; ejecuta selector Rust
sobre request/response originales y IDs. Rechaza fuentes ajenas, duplicadas o de
error. Los ejemplos/esquemas están en
[contrato interno](../contracts/reader-ipc/v0.1-draft.1/README.md).
Texto y títulos se renderizan con nodos de texto/textContent, nunca innerHTML.
Las fuentes no son HTML sanitizado para incrustar activamente. No ejecutar JS,
links, permisos o acciones propuestos en el texto leído. UI muestra estados de
lectura y truncamiento, no fechas inventadas o porcentaje de progreso.

## Evidencia offline y lo no probado

`cargo test -p seiya-server --test reader_browser --locked` ejecuta10 casos del
workload congelado: frontend HTTP Python → cliente autenticado → router Rust →
transporte en memoria/extractor/selector → modelo falso. Búsqueda y DNS/TLS de
páginas son simulados; ambos servidores escuchan solo en puertos efímeros propios.
El test usa Python -I/-S y no importa MLX, abre perfiles de navegador o lee archivos
personales. Certificados TLS sintéticos solo en otro test de memoria, no Internet.
El peer sintético envuelve cada fuente en article/p y repite dos veces su texto
para superar la heurística80 caracteres; no modifica el workload congelado ni
pretende evaluar extracción de esos snippets como si fueran páginas reales.
El caso de segunda página fallida usa HTTP503 simulado, no un timeout de Internet.

Rúbrica de esta prueba: IDs/citas correlacionados, contenido tool no sistema,
portada/ausencia/fallo de página explícitos, respuesta de referencia preservada,
contadores/slots/errores comprobados por tests separados. Las respuestas esperadas
son **escritas de antemano y emitidas por un modelo falso**. Eso no es10/10 síntesis
útiles, prueba semántica o garantía contra inyección. `usefulness=not_evaluated`,
calidad real `null`; utilidad≥8/10 y cero citas críticas sin soporte quedan a S4-09.

`test_reader_profile.py` verifica guardas, contexto sin truncar, registro acotado,
cancelación antes de upload/durante inferencia, no reejecución y aislamiento /v1.
`tests/ui_history.cjs` prueba render seguro, draft/refresh, cancel visible sin liberar
busy y no persistir referencias. Es DOM simulado Node, **no Safari ni iOS real**.
`test_reader_client.py` cubre framing/deadline y bloqueo sin retries con peer HTTP
sintético. No se afirma eficacia en todos los sitios o compatibilidad real de Gemma.

## Rollback

Detener únicamente el perfil experimental si el operador lo ha arrancado,
retirar `web_reader` de su copia y volver al prototipo de snippets, con su límite
visible. No cambiar origen localhost/127.0.0.1 ni borrar historial, modelos o claves.
Nada en esta tarea instala, promociona o reinicia el servidor habitual.
