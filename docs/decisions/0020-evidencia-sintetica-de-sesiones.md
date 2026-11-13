# ADR 0020 · evidencia de sesiones antes del runtime

2026-10-25 · S4-01. Contrato interno nuevo `chat-evidence/0.1.0`, sin cambios de
API OpenAI/nativa, del runner ABBA ni del bridge web (S4-04 sigue pendiente).

## Decisión

- Congelar diez turnos de memoria y diez casos de síntesis con fuentes ficticias
  en `contracts/benchmarks/chat-session/v0.1.0/`. Ningún caso abre red/modelos.
- Módulo Rust `seiya_observe::session_evidence`, opt-in y separado de `runner`.
  Documento tipado estricto: manifiesto, eventos y resumen, sin campos de prosa,
  URLs, prompts, consultas, rutas, secretos o razonamiento. El workload sintético
  se conserva aparte, no se copia en cada experimento.
- Esta versión acepta solo `synthetic`; no permite etiquetar datos como campaña
  real ni como prueba de modelo. Perfil/checkpoint/runtime/host se referencian por
  hashes declarados por el productor, validados en formato, no autenticados por
  este módulo. Las métricas conocidas llevan origen sintético; desconocidas
  llevan valor null, motivo y origen unavailable. Nuevos modos/probes reales
  requieren extensión versionada y tests antes de integrarse en S4-02/03.
- Guardar un JSON autocontenido por experimento evita inconsistencias entre tres
  archivos escritos por separado. Publicación atómica por hard-link desde temporal
  privado sincronizado: destino nuevo, nunca overwrite; lectores no ven JSON a
  medias. Los temporales de una interrupción cuentan y se reportan, no se borran.
- Directorio nuevo privado, IDs UUID canónicos, archivos regulares sin symlinks,
  lectura acotada, máximo32 experimentos/128 eventos por experimento,256 KiB por
  archivo y8 MiB agregados incluyendo temporales. Cotas más estrictas que el
  contrato general. Índice reconstruido en memoria desde documentos válidos;
  no otro archivo mutable/DB que pueda divergir ni cache ilimitado.
- Un único escritor propietario por directorio nuevo. No es sandbox contra otro
  proceso hostil del mismo usuario que sustituya ancestros concurrentemente; no
  abrir almacenes ajenos o compartidos. macOS usa O_NOFOLLOW en lectura de archivos.
  Fallos de publicación/lectura se declaran, sin eliminar evidencia anterior.

## Alternativas y migración

Reutilizar logs del runner tal cual puede guardar prompts/respuestas y confundir
ABBA con sesiones: rechazado. JSON libre/extensiones desconocidas permitirían
filtrar contenido: rechazado. SQLite/daemon/telemetría añaden dependencias y no son
necesarios para32 documentos: pospuestos. No generalizar a real por un flag booleano.

No integra chat operativo, UI o historial Safari, ni calcula mejora LLM. No carga
modelos, reintenta errores de conexión o mide memoria real. Índice comparable solo
si modo, workload y los cuatro hashes coinciden; esto no es prueba estadística ni
verificación del contenido de esos hashes. S3-05/S3-06 permanecen bloqueadas.
