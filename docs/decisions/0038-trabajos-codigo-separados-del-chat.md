# ADR0038 · trabajos de código separados del chat

2026-11-11. Alcance: implementación y avance en web local. No incluye
una carga concurrente nueva, activar perfiles reales, descargar pesos o ejecutar
comandos/archivos generados.

## Decisión

Broker Rust opt-in, por defecto un auxiliar activo y hasta4 esperando, independiente del lock
del interlocutor Python. Sesión-capacidad opaca emitida por host, jobs con UUID,
idempotencia por request_id, estados derivados del ejecutor y resultado acotado.
Solo genera **borradores descargables**, no escribe en workspace ni ejecuta tests,
shell o herramientas. No porcentajes/etapas inferidos del texto del modelo.

Alternativas: consenso actual bloquea el turno y no cumple; añadir threads al
coordinador Python mezcla política compartida y no resuelve ownership; lanzar
varios modelos sin admisión contradice la política existente. Broker separado
con ejecutor/gate inyectables elegido, núcleo nuevo en Rust. No cambia /v1 del
perfil normal o identidad pública pegassus. El perfil jobs es browser-only:
/v1/chat/completions devuelve409 en ese perfil para no eludir la cuarentena.
Frontend opcional autenticado y tools de chat submit/list/status; cancelación
solo por acción UI explícita. Modelo no elige endpoints, rutas o permisos.

Pool configurable1–4 auxiliares (4 esperas), validado como conjunto por Admission;
no parámetro de request/modelo. Cálculo puro de capacidad candidata desde reservas
disjuntas de host/interlocutor/auxiliares, nunca suma RSS/Metal ni autoriza carga
por tener32GiB. Un host con más RAM puede admitir más si presupuesto, presión y
runtime lo permiten. Esta versión no implementa autoadmisión de hardware real.

Estados: queued, generating, cancelling, cancelled, completed, failed, blocked,
quarantined. Cancelación activa es solicitud, no fin: resultado tardío se descarta.
Timeout/error incierto bloquea el auxiliar y admisión de inferencia del perfil,
sin retry/reset HTTP; no liberar GPU ficticiamente. Recuperación humana fuera de
esta API tras comprobar runtime. Gate antes de cada ejecución, denegado por
defecto; una condición JSON o salida de modelo nunca acredita memoria/concurrencia.

Sesiones/job registry en memoria y acotados (32 sesiones/64 jobs totales, no
evicción de trabajos activos, reinicio pierde registros). Prompt≤8192 bytes,
artefacto≤65536, frame≤16384, plazo job≤180s incluida cola. Capacidad llena falla
sin sobrescribir. Idempotencia vive tanto como sesión/proceso, no promete exactly
once tras reinicio. Claves/sesiones no van a prompts, logs o localStorage.

HTTP loopback con Host/Origin/bearer obligatorio y cotas antes de ejecución. UI
consulta estados independientemente del envío de chat; permite cancelar/obtener
borrador por acción explícita. No mezcla historial de otros chats; versión inicial
no recupera capacidades al refrescar: informa pérdida de enlace, no reenvía.

## Operación y compatibilidad

Vista previa aislada con ejecutores/interlocutor **simulados**, banner inequívoco,
sin acceso a modelos/documentos/red externa. Puede arrancarse en8010 solo si está
libre, sin reiniciar servicios. No es validación Gemma/Qwen simultáneos. La ruta
real exige integrar admisión/ownership del par y prueba específica autorizada;
no adoptar modelos que el usuario tenga cargados en Ollama.

Reader y jobs se configuran por separado en este incremento; no mezclar dos
bucles de herramientas hasta tener contrato combinado. Rollback: quitar perfil
jobs/omitir constructor Rust; preservar configuración e historial anteriores.
No migración de conversaciones, instalación o cambio de proveedor/OpenCode.
