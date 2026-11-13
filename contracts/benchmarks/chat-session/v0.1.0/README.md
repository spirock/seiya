# Workload y evidencia de sesiones · 0.1.0

Estado: fixtures y almacenamiento sintéticos, sin ejecución de chat/modelos.
Contrato ejecutable de evidencia: tipos/validación de
`crates/seiya-observe/src/session_evidence.rs`, con `deny_unknown_fields` y
ejemplo `evidence.json`. No es el wire de Seiya/OpenAI ni una JSON Schema publicada.

- `workload.json`: diez mensajes consecutivos para memoria de conversación y diez
  casos de síntesis. Fuentes ficticias, sin URLs ejecutables. No puntuar por match
  literal de expected_facts: son rúbrica de revisión, no respuestas universales.
  Estos datos no ejecutan búsqueda/lectura ni configuran su política; los fallos,
  texto parcial e inyección tienen que ser inyectados por el futuro harness.
- `evidence.json`: ejemplo de métricas sintéticas; no memoria o rendimiento real.
  `schema_version=chat-evidence/0.1.0`, `workload_version=chat-workload/0.1.0`.
  Hashes son referencias declaradas por productor confiable; no se hashea un
  modelo por deserializarlos. Cambiar fixtures/versiones cambia comparabilidad.
- `supervisor-status.json`: snapshot interno versión1 del núcleo supervisor
  inyectado, no documento Evidence ni estado HTTP público. Estados stopped frente
  a quarantined, reaping/lease separados. No mezclar con model-campaign/calibration
  ni declarar cierre de procesos reales. Binding de runtime nativo y bridge pendientes.
- `native-lease-status.json`: Status versión2 con lease_origin explícito. Lock
  macOS real en fixture propia, pero runtime/pressure/reaping inyectados y
  model_inference false. No acredita Child ni es health HTTP o modelo real.
- `native-child-status.json`: Status versión3 de helper nativo auditado sin fork,
  Child original/lease real y presión inyectada. Solo evidencia de ese helper,
  no MLX, árbol arbitrario o ventana de contexto. No es health HTTP público.
- `native-pressure-status.json`: Status versión4 del mismo helper con lectura
  read-only sysctl macOS y lease real. No capacidad de carga o limpieza MLX.
- `restricted-runtime-status.json`: Status5 de perfil macOS con fork/spawn
  prohibidos y socket del Child original verificado. Fixture model_inference
  false; origen MLX muestra null/no observado. No es health ni compatibilidad MLX.
- `value:null` requiere `missing_reason` no nulo y `origin:unavailable`.
  Un valor disponible requiere `origin:synthetic`, sin missing_reason. Unidades
  en el nombre de métrica (`*_bytes`, `*_millis`, `*_tokens`); cero no es ausencia.
- `generation_completed` no implica `task_resolved`; null significa no evaluada.
  `outcome:completed` requiere cleanup confirmado, no calidad aprobada.
  `sources_available` incluye snippets; `valid_citations` cuenta IDs únicos de
  fuentes suministradas, no páginas leídas ni afirmaciones verificadas.
- Sin campos libres para prompts, URLs, consultas, textos, rutas o secrets. Extras
  se rechazan. Esto minimiza filtración accidental, no autentica al productor.
- Writer en directorio nuevo privado; 32 documentos,128 eventos/documento,
  256 KiB/archivo,8 MiB totales. Temporales incompletos consumen cuota y se indican
  en índice; nunca se borran automáticamente. Un único writer propietario.
- `rebuild_index` devuelve un índice derivado en memoria, ordenado por UUID, sin
  afirmar orden temporal. Solo comparar si workload, modo y hashes de perfil,
  checkpoint, receta de runtime y host coinciden. No genera estadísticas falsas.
  `Store::read_index` permite reconstrucción explícita tras reiniciar, en lectura
  sola y sin reabrir writer; rechaza directorios compartidos/symlinks finales.

Workload congelado SHA-256:
`a470a4e946200399b7c0f1ec624ecc3d476a71fd89e934fc28dd036959e299bc`.
Un guard FNV-1a en tests detecta cambios accidentales (no es autenticación).

Sin eventos automáticos del servicio, integración con runner ABBA, telemetría,
entrenamiento o nuevos permisos. S4-02/03 integrarán readiness/probes reales con
otra revisión versionada. Véase [ADR 0020](../../../../docs/decisions/0020-evidencia-sintetica-de-sesiones.md).
