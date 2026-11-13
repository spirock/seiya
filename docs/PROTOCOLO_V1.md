# Protocolo Seiya · `1.0-draft.1`

## Estado y compatibilidad

Este es el contrato propuesto del núcleo compartido, no la API actual publicada.
Las rutas siguientes **no existen aún**; nunca anunciarlas como operativas.
Durante la migración siguen funcionando `/v1/models`, `/v1/chat/completions`,
`/health`, `/api/chat` y `/` tal como documenta `swarm-app/README.md`.

No añadir campos nuevos a peticiones OpenCode sin revisar su soporte; el
adaptador mantiene `model: pegassus`, herramientas estructuradas y errores HTTP.
No ejecutar en el host herramientas que el cliente externo se encarga de ejecutar.

## API nativa propuesta

| Operación | Ruta objetivo | Resultado |
|---|---|---|
| Negociar | `GET /native/v1/capabilities` | Capacidades efectivas y versión |
| Crear run | `POST /native/v1/runs` | 202 con run ID tras validación |
| Consultar run | `GET /native/v1/runs/{id}` | Estado y resultado persistidos |
| Cancelar | `POST /native/v1/runs/{id}/cancel` | Idempotente, no borra conversación |
| Eventos | `GET /native/v1/runs/{id}/events` | SSE con cursor/replay |
| Resolver permiso | `POST /native/v1/approvals/{id}` | Decisión humana validada |
| Subir adjunto | `POST /native/v1/assets` | Asset ID tras validar límites |
| Historial | `GET /native/v1/conversations` | Páginas con cursor |
| Conversación | `GET /native/v1/conversations/{id}` | Mensajes y runs paginados |
| Eliminar | `DELETE /native/v1/conversations/{id}` | Cancela pendientes y elimina datos propios |

Una app embebida puede usar el mismo contrato por FFI sin escuchar un puerto.
El transporte no determina ni sustituye la autenticación/autorización.

## Petición de run

El esquema normativo de esta revisión está en
[`session-request.schema.json`](../contracts/v1/session-request.schema.json).
El crate Rust representa la misma forma, con comprobaciones semánticas básicas.

```json
{
  "contract_version": "1.0-draft.1",
  "request_id": "request-001",
  "conversation_id": "conversation-001",
  "execution": "auto",
  "privacy": "local_only",
  "messages": [{"role": "user", "content": [{"type": "text", "text": "Resume este documento"}]}]
}
```

- IDs opacos de 1–128 caracteres; el host asigna IDs persistentes, preferiblemente
  UUID. Nunca interpolarlos como rutas o comandos.
- La petición no contiene permisos concedidos ni confirmaciones autoemitidas.
- Una petición idéntica con mismo `request_id` devuelve el run previo. Mismo ID
  con contenido diferente: 409 `idempotency_conflict`.
- `execution`: `auto`, `single`, `swarm`, `remote`. El host comprueba capacidades.
- `privacy`: `local_only` bloquea búsqueda/remoto y herramientas de red;
  `network_allowed` no concede por sí solo acceso a archivos, correo ni envíos.
- Los ejemplos son del protocolo nuevo. No equivalen al comportamiento actual
  de la UI Python, que informa de búsqueda automática y no implementa ese enum.
- Mensajes públicos: solo `user`/`assistant`; sistema y resultados de herramientas
  los inserta el host. Máximo inicial 100 mensajes, 256 KiB de payload JSON.
- Imágenes por `asset_id`, sin ruta del host ni URL elegida por el modelo.
  En el runtime actual no están soportadas.
- `remote` necesita además un destino emparejado seleccionado por el host;
  hasta definir ese flujo, el núcleo rechaza ese modo sin configuración local.

## Negociación de capacidades

Campos de capacidades: `contract_version`, `platform`, `supported_execution`,
`input_modalities`, `tools`, límites de payload/contexto/salida y catálogo de
modelos efectivos. No incluir secretos ni rutas privadas de pesos.

El ejemplo `capabilities.example.json` es un dispositivo hipotético, no una
descripción del prototipo que corre hoy. La UI debe mostrar error explicativo
si el host no implementa una capacidad solicitada; sin fallback remoto tácito.

Cada herramienta ofertada tiene nombre, versión, esquema, efectos, plataformas,
límites, política de confirmación y estado real de implementación. El catálogo
objetivo completo no es el catálogo ofertado a un modelo en una petición.

## Argumentos y resultados de herramientas

`tool-arguments.schema.json#/$defs/<nombre>` define los argumentos propuestos.
El host selecciona la referencia, nunca el modelo. Defaults que el adaptador
debe aplicar explícitamente: búsqueda 5 resultados; listados de archivos 50;
lectura 65536 bytes desde offset 0; fetch 1 MiB; Mail lista 5 y lectura 32768
caracteres; captura dimensión máxima 1536; inspección 100 nodos; proceso 30 s.
El schema solo limita valores, no aplica esos defaults ni concede acceso.

| Tool | Entrada esencial | `data` exigido al adaptador antes de aceptación |
|---|---|---|
| `web_search` | query, limit | results: título/URL/snippet/fecha opcional y proveedor |
| `web_fetch` | URL, max_bytes | URL final, MIME, texto extraído, fecha fetch, truncated |
| `files_list` | directory_id, cursor, limit | entries con IDs, nombre, tipo, cursor siguiente |
| `files_read` | file_id, offset_bytes, max_bytes | texto o asset_id, MIME, sha256 y truncated |
| `files_write` | file_id, expected_sha256, content | hash anterior/nuevo y bytes; conflicto si cambió |
| `apps_open` | bundle_id | bundle_id efectivo y estado de apertura comprobado |
| `mail_list_messages` | account_id, mailbox_id, limit | IDs, remitente, asunto, received_at, sin cuerpos |
| `mail_read_message` | account_id, message_id, max_chars | remitente, asunto, fecha, cuerpo texto, truncated |
| `mail_send_message` | draft_id, expected_sha256 | resultado/proveedor e ID de envío verificable |
| `screen_capture` | window_id, max_dimension | asset_id, tamaño, ventana y instante |
| `ui_inspect` | window_id, limit | snapshot_id, nodos con IDs/roles/acciones soportadas |
| `ui_action` | snapshot_id, target_id, action, text si aplica | estado antes/después y verificación del objetivo |
| `shell_run` | workspace_id, executable, arguments, timeout_seconds | exit_code, stdout/stderr limitados, timed_out |

IDs de archivos/cuentas/ventanas/drafts son handles de recursos autorizados del
host, no rutas libres ni identificadores que concedan acceso por conocerlos.
`files_write` de esta revisión modifica un archivo existente con precondición;
crear archivos nuevos requiere otro contrato, no se simula un ID inexistente.
Una lectura binaria devuelve asset, no texto recodificado arbitrariamente.

`tool-result.schema.json` exige resultado o error, nunca ambos; `untrusted:true`.
Límite inicial de resultado 64 KiB. Los esquemas específicos completos de `data`
se cerrarán con cada adaptador antes de anunciarlo; no son validados por el crate
actual. Schema válido no acredita que un correo se envió o una app se abrió.

## Estados de un run

```text
accepted → queued → generating → waiting_approval → executing_tool
                      ↑                           │
                      └───────────────────────────┘
              → reviewing → completed
              → cancelling → cancelled
              → failed
```

- Las transiciones opcionales se omiten si no ocurren. Cada paso tiene límites.
- `completed`, `cancelled`, `failed` son terminales. Un run nuevo tiene otro ID.
- `waiting_approval` persiste target y digest de argumentos. La aprobación solo
  vale para ese digest, acción, recurso y caducidad; no para llamadas futuras.
- Un proceso interrumpido no convierte herramientas en exitosas: estado
  `interrupted` en el intento, con recuperación definida por idempotencia.
- El host debe poder cancelar un run aunque no haya UI conectada.

## Eventos y presentación

Envelope de evento: `contract_version`, `event_id`, `sequence`, `run_id`,
`conversation_id`, `type`, `created_at` (UTC RFC3339), `payload`.

Tipos objetivo:

- `run.accepted`, `run.queued`, `run.started`.
- `assistant.delta`: texto de respuesta generado realmente; nunca cadena de
  pensamiento privada o relleno simulado. `assistant.message` para respuesta final.
- `tool.proposed`, `tool.approval_required`, `tool.started`, `tool.completed`,
  `tool.failed`. Separar propuesta de efecto ejecutado.
- `source.available`: fuentes verificadas por herramienta y procedencia.
- `context.compaction_started`, `context.compaction_completed`, con tamaños y
  causa, no contenido privado del resumen en telemetría.
- `run.completed`, `run.failed`, `run.cancelled`.

Secuencia monótona por run, evento persistido antes de publicar, replay desde
`Last-Event-ID`. Deduplicar por event ID; nunca reejecutar tools al reproducir UI.
Heartbeat de conexión no significa avance del modelo. Un cursor eliminado exige
resincronizar snapshot, no inventar eventos perdidos.

## Errores

Forma objetivo: `error.code`, `error.message`, `request_id`, `run_id` cuando
exista, `retryable`, `retry_after_ms` opcional. Mensajes sin contenido de usuario,
prompts, credenciales, rutas privadas o respuestas de terceros.

| Código | HTTP objetivo | Recuperación |
|---|---|---|
| `invalid_request`, `unsupported_version` | 400 | Corregir petición |
| `unauthorized` | 401 | Autenticación válida |
| `permission_denied` | 403 | No reintentar para evadir permisos |
| `not_found` | 404 | Revisar ID/retención |
| `idempotency_conflict`, `file_conflict` | 409 | Resolver conflicto explícitamente |
| `payload_too_large` | 413 | Reducir payload |
| `unsupported_capability`, `context_exhausted` | 422 | Otra modalidad/menos contexto |
| `queue_full` | 429 | Backoff limitado y cancelable |
| `runtime_unavailable`, `model_unload_failed` | 503 | Diagnóstico sin fallback oculto |
| `deadline_exceeded` | 504 | Estado parcial y acción segura |

Un error asíncrono usa `run.failed` y conserva el run consultable; no puede
cambiar el HTTP 202 ya aceptado. El adaptador `/v1` actual conserva sus códigos
existentes hasta una migración publicada y probada con OpenCode.

## Política y auditoría

El modelo no elige su política. El registro del host ofrece herramientas;
el broker valida argumentos con su esquema completo, recursos, grants,
confirmación e idempotencia antes de invocar adaptadores.

El helper Rust inicial solo comprueba nombre anunciado, plataforma, efectos
concedidos y requisito de confirmación. **No es el broker completo ni basta
para ejecutar herramientas reales de forma segura.** No valida todavía URLs,
rutas, scopes, DNS, timeouts o argumentos específicos por herramienta.

Registro local mínimo: IDs, actor, herramienta, recurso minimizado, decisión,
duración, tamaños y estado. Cuerpos de correo, imágenes, claves y comandos con
datos sensibles no se registran por defecto. Exportar diagnóstico exige revisión.

## Evolución

- Versiones exactas mientras sea draft; no interpretar versiones desconocidas.
- Cambios compatibles dentro de una versión estable solo tras definir reglas de
  extensiones. Los tipos actuales rechazan campos desconocidos por seguridad.
- Fixtures compartidos entre Rust/UI/adaptadores; pruebas de golden serialization.
- La implementación Python no se declara compatible con este protocolo hasta
  disponer del adaptador y pasar su suite. No cambiar formatos de chats actuales
  durante una limpieza del repositorio.
